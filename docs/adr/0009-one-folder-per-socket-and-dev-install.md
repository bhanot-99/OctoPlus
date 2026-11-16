# ADR-0009 — One folder per socket, socket map as code, development install

**Status:** Proposed (decided by Claude while building Phase 1; for the owner to accept or overturn at the Phase 1 sign-off, D-20)
**Date:** 2026-11-15
**Changes:** Architecture §5.2 (socket paths), §7.1 (dev install prefix), §13 (runtime-folder unit) · security.md §8 (socket-map proof)
**Evidence:** ADR-0002 finding 5 (a bound folder exposes everything inside it); systemd `BindPaths=` semantics (a missing source is skipped with `-`, or fails the unit without it)

---

## Context

ADR-0002 enforces the socket map by giving each unit an empty runtime root and binding in only the folders of the peers it may call. Architecture §5.2 put all of a part's sockets in one folder (`monitor/stream.sock` and `monitor/control.sock`, plus every part's `health.sock`). Binding that folder into a client exposes every socket in it, so:

- C3, which may use only `monitor/stream.sock`, would also see `monitor/control.sock`.
- healthd, which may use only health sockets, would see every socket of every part.
- C5, which may use `kernel/kernel.sock`, would also see C6's health socket.

The peer-unit check (ADR-0002 decision 4) would still refuse those connections, but that is the second layer. The design says the mount namespace is the primary control and a part "cannot open a socket it cannot see". That is not true with shared folders.

A second problem: a client's `BindPaths=` source must exist when the client starts. If the server has not created its folder yet, the bind is skipped (with `-`) and the client never sees the socket, even after the server starts. Without `-`, the client fails to start.

Third: user parts run with `ProtectHome=tmpfs`, and the reference machine has no root-owned install yet (packaging is Phase 10). Phase 1 needs real units without asking for root on every rebuild.

## Decision

1. **One folder per socket.** Every socket lives at `<part>/<name>/<name>.sock` under the runtime root, for example `monitor/stream/stream.sock`, `monitor/control/control.sock` and `<part>/health/health.sock`. The fetcher's are `/run/neuroos-fetcher/fetch/fetch.sock` and `/run/neuroos-fetcher/health/health.sock`.
2. **A server binds its whole part folder. A client binds exactly the socket folders in its row** of the socket map. healthd binds each `<part>/health`.
3. **The socket map is code.** It lives in `neuroos-common::socketmap`. The unit files are generated from it (`just units`), and so are the peer policies and the security probe's expectations. A test fails if the committed unit files drift from the generator.
4. **The runtime tree is created before any part starts**, by a oneshot `neuroos-runtime.service` that every part requires. Servers never delete their folder, only a stale socket file.
5. **Fetcher folders are hidden** from every user unit except C6 (binds `fetch/`) and healthd (binds `health/`).
6. **Development install** (until Phase 10 packaging): binaries and hard-linked models under `~/.local/opt/neuroos/{bin,models}`, exposed read-only in each unit with `BindReadOnlyPaths=`. The fetcher alone is installed under `/opt/neuroos/bin` by `scripts/install-fetcher.sh`, run by the owner with sudo, because a system user cannot read `/home`.
7. **C4's model hash is checked by its own oneshot unit**, `neuroos-model-check.service` (`sha256sum --check` against the manifest hash, FR-INF-01), which C4 `Requires=` and is ordered after. It has no `RemainAfterExit`, so every C4 start, including an automatic restart, runs the check again. No hash code is written in C++. A wrong or missing model fails the check, and C4's process never starts. *(Revised 2026-11-16: the first version used `ExecStartPre`, which drops `PrivateNetwork=` in user scope; see ADR-0002 addendum.)*
8. **Fetcher sockets are group-shared**: the fetcher's folders are created by tmpfiles (`2770 neuroos-fetcher:neuroos`) and its sockets are `0660`, so C6 and healthd (in group `neuroos`) can connect. The server never creates these folders; it refuses them unless they are its own and give nothing to others (`SocketAccess::Group`).

## Consequences

- Socket reach is exact at the namespace layer: a part cannot even see a socket outside its row. Phase 1's probe test checks every pair.
- Paths are one level deeper. All path logic sits in one module, so callers do not change.
- The dev install puts user-owned binaries in the user's home. Any program running as the user could replace them, which a root-owned `/opt/neuroos` would prevent. This is accepted for development only. Phase 10 installs to root-owned `/opt/neuroos` and changes one prefix constant in the generator.
- Using the fetcher's folders needs the user to be in the `neuroos` group, which takes effect after the next login.
- The hash check reads the 2.5 GB model on every C4 start: measured ≈ 1.5 s (systemd reports 1.52 s CPU for the check unit). C4 is back about 5 s after a kill (2 s restart delay + check + load), against about 3 s for the other parts (Phase 1 kill-and-recover test).

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Keep shared folders and rely on peer-unit checks | Makes the second layer the only layer, with the PID-reuse window ADR-0002 notes. |
| Bind single socket files instead of folders | A restarted server creates a new socket inode, so the client's bind points at the dead one. |
| Install to `/opt/neuroos` now | Needs root for every rebuild during development. |
| Hash the model in C4 with a C++ SHA-256 | No allowlisted C++ crypto library, and hand-rolled crypto is forbidden (rules.md §5). |
