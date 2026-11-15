# ADR-0002 — Unit mode and socket-map enforcement

**Status:** Accepted by the owner 2026-11-15
**Date:** 2026-11-15
**Changes:** Architecture §5.2, §8.1, §8.2 (three additions marked "S-04") · ADR-0008 (GPU mechanism, see the addendum there)
**Evidence:** spike S-04 (P0-S10): `reports/spikes/s04-hardening-20261114.{json,md}` (agy's harness run), `reports/spikes/s04-verification-claude.md` (checker's re-measurement — read this first; it corrects four claims in agy's report), tests `local_unit_isolation.rs` and `local_user_unit.rs` in `crates/neuroos-testkit`.

---

## Context

Architecture §8.1 hardens every part with a systemd baseline, and §5.2 enforces the socket map by mount namespaces: each unit sees an empty runtime root with only its own and its allowed peers' folders bound in. Decision #12 put everything except the fetcher in **user-scope** units, which inherit the desktop session's sockets. S-04 had to show that user scope actually honours these stanzas on the reference machine (Pop!_OS 24.04, systemd 255.4, kernel 7.2.15), and whether peer-unit checks work.

Measured (details in the evidence files):

1. **Every §8.1 stanza is applied in user scope.** All but two have a visible effect when started with and without the stanza; `ProtectClock` and `RestrictRealtime` are applied but make no difference for an unprivileged user (those powers are already denied). `systemd-analyze security --user` scores the baseline 1.5 ("OK") against 9.8 unhardened.
2. **`ProtectHome=tmpfs` also hides `/run/user/<uid>`.** Runtime folders must be bound back with `BindPaths=`; `ReadWritePaths=` fails with `226/NAMESPACE`.
3. **systemd 255's `@system-service` does not include the Landlock syscalls.** A part that restricts itself (NFR-SEC-04) is killed with SIGSYS on `landlock_create_ruleset` unless they are added.
4. **DNS leaks through the system D-Bus.** With the full baseline, `org.freedesktop.resolve1` still resolves names over `/run/dbus/system_bus_socket`, which bypasses `PrivateNetwork=` and the resolver-socket block. Hiding the system bus socket closes it.
5. **Socket-folder isolation works:** a unit given `TemporaryFileSystem=<root>` + `BindPaths=<root>/a <root>/b` connects to B's socket; a unit given only `<root>/c` gets `ENOENT` — it cannot even see B.
6. **Peer-unit checks need `/proc` in Landlock.** A server whose Landlock rules allow only `/proc/self` gets EACCES reading `/proc/<peer pid>/cgroup`, so a unit-checking server would reject every peer.
7. **`DevicePolicy=`/`DeviceAllow=` are not enforced in user scope** (no device-cgroup control without privileges). Only `PrivateDevices=yes` hides devices; specific nodes can be bound back with `BindPaths=`.
8. TCP, UDP (AF_INET/AF_INET6), raw/packet sockets and the resolver socket all fail inside the baseline unit while Unix sockets in the unit's own folder work (BUG-008 test, runs in the local gate).

## Decision

1. **User-scope units for every part except C7** (Architecture decision #12 stands).
2. The §8.1 baseline stands, with three additions:
   - `SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self`.
   - `InaccessiblePaths=-/run/dbus/system_bus_socket` for every part that does not need the system bus. A part that needs it (if C1 must, for a sensor S-05 cannot get from `/sys`) must say so in its unit and in security.md, and still has no network.
   - Runtime paths under `/run/user/<uid>` are always exposed with `BindPaths=`, never `ReadWritePaths=`.
3. **Socket-map enforcement (AB-7) uses `TemporaryFileSystem=%t/neuroos` + `BindPaths=`** of the unit's own folder and each allowed peer's folder, exactly as Architecture §5.2. Every unit file's bind list must match its row of the socket map, and Phase 1 adds the test that proves the other folders are unreachable.
4. **Peer checks are a second layer.** The mount namespace is the primary control. Servers check `SO_PEERCRED` UID always; servers that also check the peer's unit get read-only `/proc` in their Landlock rules. (Reading `/proc/<pid>` after `SO_PEERCRED` has a PID-reuse window; acceptable for a second layer, revisit with pidfds if a part ever relies on the unit check alone.)
5. **Device access:** `PrivateDevices=yes` everywhere; where a part needs a specific device (C2 audio is via the PipeWire socket, not device nodes; C4's optional GPU), the nodes are bound in with `BindPaths=` and listed in the part's ADR. `DeviceAllow=` is not relied on in user scope.
6. `just spike-s04` stays as the re-runnable check; the `local_*` tests run in every local gate.

## Consequences

- The isolation the design assumed is available without root, with three concrete fixes now written into Architecture §8.1/§8.2.
- The D-Bus finding removes a real DNS path for every part; C1's sensor choices (S-05) must avoid the system bus where `/sys` works.
- A part's sockets are invisible to units outside its row of the socket map, so a compromised part cannot even see sockets it was not given.
- agy's harness hard-coded "works" for stanzas without a behaviour check and printed two unmeasured findings (BUG-011). Fixed 2026-11-15: the harness now reports only what it checks, and its re-run (`reports/spikes/s04-hardening-20261115.*`) agrees with the checker's file. The D-Bus and peer-unit findings still rest on `s04-verification-claude.md`.
- Two stanzas (`ProtectClock`, `RestrictRealtime`) add nothing observable here; they stay as defence in depth.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| System-scope units with a dedicated user per part | Loses the desktop session's Wayland, PipeWire and session-bus sockets; needs root to install and run. |
| Rely on UID + unit peer checks instead of mount namespaces | All parts share one UID; a unit check alone has the PID-reuse window. Mount namespaces make other sockets invisible. |
| `DeviceAllow=` for the GPU | Not enforced in user scope (measured). |
