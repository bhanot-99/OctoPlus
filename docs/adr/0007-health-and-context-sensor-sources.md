# ADR-0007 — Health and context sensor sources

**Status:** Accepted by the owner 2026-11-15
**Date:** 2026-11-15
**Changes:** Architecture §8.1/§8.2 (C1 unit and Landlock rows) · security.md (C1 needs no system bus) · PRD §10 (cosmic-protocols use confirmed)
**Evidence:** spike S-05 (P0-S11): `reports/spikes/s05-sensors-20261114-run1-agy.{json,md}` (agy) and `…-run2-claude.{json,md}` (independent rerun by Claude), probe `tests/spikes/s05-probe` (separate workspace so the GPL crate stays out of the main build), `scripts/spike-s05.sh`. App ids and window-title data were removed from the committed files.

---

## Context

C1 must read machine health (disk, battery, temperature) for the nudges in FR-NUD-03, context for the hold rules in FR-NUD-05 (fullscreen, idle, lock) and C2 must know whether another app is capturing the microphone (FR-VOI-10). S-04 showed the system D-Bus leaks DNS from a sandboxed unit, so sources that avoid the system bus are preferred. S-05 asked which sources work on the reference machine (Pop!_OS 24.04, COSMIC `cosmic-comp 1.0.0`, PipeWire 1.6.8, kernel 7.2.15), plainly and inside a unit with the full §8.1 baseline plus `InaccessiblePaths=-/run/dbus/system_bus_socket`.

Measured, both runs agreeing (latencies are one read, plain / in unit, run 1):

| Signal | Source | Plain | In unit | Latency (µs) |
| :--- | :--- | :--- | :--- | :--- |
| Disk (FR-MON-05) | `/proc/self/mountinfo` + `statvfs` | yes | yes | 258 / 254 |
| Battery, AC (FR-MON-06) | `/sys/class/power_supply/{BAT*,AC*}` | yes | yes | 1430 / 484 |
| CPU temperature (FR-MON-07) | `/sys/class/hwmon` — `k10temp` `Tctl` is the package sensor | yes | yes | 4596 / 4409 |
| Focused window, fullscreen state (FR-MON-01) | Wayland `zcosmic_toplevel_info_v1` (cosmic-protocols) | yes | yes (Wayland socket bound read-only) | 687 / 595 |
| Idle (FR-MON-02) | Wayland `ext_idle_notifier_v1` advertised | yes | yes | — |
| Session lock (FR-MON-02) | `ext_session_lock_manager_v1` and `cosmic_session_lock_layer_manager_v1` advertised | yes | yes | — |
| Mic in use by another app (FR-VOI-10) | PipeWire nodes with `media.class = Stream/Input/Audio` (via `pw-dump` in the spike) | yes; flips to "in use" while a test capture runs | yes (PipeWire socket bound read-only) | 13 942 / 13 727 |

Gaps that were **not** measured: the battery time estimate (the battery reported `power_now = 0` while on AC, so the estimate was correctly "unavailable"; on battery power it is untested); a fullscreen window (none was fullscreen during the runs, so the flag was only seen as false); idle and lock *events* (the protocols are offered, events not exercised); the session-lock event needs a client role check, since `ext_session_lock_manager_v1` is the locker's protocol, not a lock observer.

## Decision

1. **C1 reads disk, battery and temperature from `/proc` and `/sys` only.** No UPower, no system D-Bus. C1's unit hides the system bus like every other part (ADR-0002). If `power_now` is 0 or missing, the battery estimate is reported as unknown, never guessed.
2. **Window, fullscreen and idle come from Wayland:** `zcosmic_toplevel_info_v1` for toplevels and state (COSMIC first, PRD §4), `ext_idle_notifier_v1` for idle. `cosmic-protocols` is GPL-3.0-only; that is the accepted risk R-07 for a personal install and stays isolated in C1. A non-COSMIC fallback (`ext_foreign_toplevel_list_v1`, which has no fullscreen state) is out of scope for v1.0. C1's unit binds only `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` read-only.
3. **Lock state:** read from the session bus or logind is not chosen here; Phase 3 decides between a Wayland source and the user's session bus (not the system bus) and records it. Until then lock state is "unknown", which the nudge policy treats as "hold" (fail safe).
4. **Mic in use is C2's job, from PipeWire directly** (C2 already links libpipewire): watch the registry for `Stream/Input/Audio` nodes that are not C2's own, and report it in `VoiceStatus.mic_in_use_by_other`. Spawning `pw-dump` is a spike shortcut only; at ~14 ms per call it is also too slow to poll.
5. Sensor reads are cheap enough for the PRD intervals (5–60 s) with no caching tricks.

## Consequences

- C1 needs read-only `/proc`, `/sys/class/{power_supply,hwmon,thermal}` and the Wayland socket — matching Architecture §8.2, minus "bus sockets" for the system bus.
- No sensor needs the system D-Bus, so the DNS leak from S-04 is closed for C1 too.
- Window titles and app ids are personal (R0-10): the S-05 probe printed unsalted title hashes and app ids into its report (BUG-013); the committed copies are redacted, and C1's real recorder must use the keyed, discarded-key anonymiser (FR-MON-12).
- Open items for Phase 3 tests: fullscreen flip, idle/lock events, battery estimate on battery power.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| UPower over the system D-Bus for battery | Needs the system bus inside the sandbox, which leaks DNS (ADR-0002); `/sys` gives the same data. |
| `ext_foreign_toplevel_list_v1` only (MIT, no GPL) | Has no fullscreen state, which the nudge hold rule needs (FR-NUD-05). |
| `/proc/asound` for mic use | Shows open PCM devices, not which app; PipeWire knows the app and is what C2 already uses. |
