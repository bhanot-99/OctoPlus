# Phase 3 — Monitor (C1): Report

**Date:** 2026-11-16 · **Branch:** `phase-3-monitor` · **Builders:** Claude (foundation, privacy gate, bus, all sensors, recorder, anonymiser, dump format, units, tooling); agy (`neuroosctl replay`) · **Checker:** Claude for agy's work (re-ran every check, read the diff); agy for Claude's work (independent review, 9 findings, all valid, all fixed)
**Status:** ✅ Signed off 2026-11-16 (recordings deferred to integration, D-26)

## 1. Summary

**In plain words.** NeuroOS can now see what you are doing and how the machine is doing, and it does it quietly and safely.

1. It notices **which window has focus**, its title, and whether it is fullscreen; when you go **idle**; and what **media** is playing.
2. It samples **CPU and memory** (and which program uses the most), **disk space**, **battery**, and **CPU temperature**.
3. It looks at the **processes** of the program you are using, and (if you list folders) **file activity** in your git repositories, notes and calendar files.
4. **Apps you exclude never leave the monitor**, and neither does anything while you have **paused** it. Password managers and private browser windows are excluded by default.
5. You can **record** a session for testing. The recording is scrambled before it is written: titles, song names and file names become meaningless codes, and the key is thrown away.

It runs as its own sandboxed service, uses about 7 MiB of memory and about half a percent of one CPU core, and shows each sensor's health in `neuroosctl status`.

**Technically.** `neuroos-monitor`: `clock`, `sensor` (trait, `SensorOut`), `supervisor` (jittered backoff, per-sensor health), `bus` (bounded drop-oldest queues, latest-state snapshot for late subscribers, the `Gate` trait), `privacy` + `exclusion` (the only filter, AB-5), `stream`, `anonymise`, `record`, and `sensors/{cosmic, idle, mpris, resources, disk, battery, thermal, proc, folders, wl}`. `neuroos-proto::dump` (file format). `neuroosctl replay` and subsystem rows in `neuroosctl status`. `[monitor]` config. Unit: compositor socket and session bus read-only, `/sys` read-only via Landlock, folder drop-in. ADR-0011 (proposed). Tooling: `scripts/{record-session,check-dump,bench-p3,p3-live-check}.sh`, `neuroos-monitor-tail`, runbook `docs/runbooks/record-monitor-session.md`.

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P3-S01 | Focus changes with app, title, pid, timestamps | Done (pending sign-off) | Claude | agy review | `sensors_wayland_test` (Tracker incl. proptest, pacer, restart); real compositor `local_` test; live: focus with `fullscreen_known` seen in the real unit |
| P3-S02 | Idle, lock and media events | Done, lock by design "unknown" (pending sign-off) | Claude | agy review | idle + media tests; MPRIS on a private bus with a fake player; **lock: no verified source inside C1's surface (ADR-0011, §7)** |
| P3-S03 | CPU, memory, disk, battery, temperature | Done (pending sign-off) | Claude (agy stalled, §7) | agy review | `sensors_system_test` (fixtures, negative controls, proptest, real-machine test) |
| P3-S04 | Focused window's process tree | Done (pending sign-off) | Claude | agy review | `sensors_proc_test`; conservative pid matching (empty id, truncated names, ambiguity) |
| P3-S05 | Excluded apps and paused periods never leave C1 | Done (pending sign-off) | Claude | agy review (2 S1 findings fixed) | `privacy_test` property test + negative control + socket test |
| P3-S06 | Record, anonymise, replay | Done (pending sign-off) | Claude (dump, anonymiser, recorder), agy (replay) | Claude / agy review | `dump.rs` tests, `anonymise_test`, `replay_test`; real 45 s recording in the real unit validated |
| P3-S07 | (P1) File activity for configured folders | Done (pending sign-off) | Claude | agy review | `sensors_folders_test` (real inotify) |

## 3. Exit criteria

| Criterion (phases.md §6.4) | Met? | Evidence |
| :--- | :--- | :--- |
| Every sensor emits correct events on the reference machine and reports its own health | **Yes**, lock excepted | Real unit, 12 s subscriber (`scripts/p3-live-check.sh`): battery, disk, focus, idle, media, process tree, resources, thermal all arrived; every sensor has a health row; the idle sensor armed on the real compositor. Closed ADR-0007 gaps: **battery estimate on battery power** (a real 4,461 s estimate while discharging), **fullscreen** (a real fullscreen window reported `fullscreen = true`). **Idle transition measured on the real session** (3 s timeout, no input: `idled` arrived after exactly 3.0 s; this also found BUG-055). Not exercised live: a fullscreen *flip* (tested in the state machine and with fake compositor messages; see §7) |
| Performance and RSS measured in the shipped unit | **Yes** | §4 |
| Exclusion, pause and the anonymiser proven by property tests | **Yes** | `tests/privacy_test.rs`, `tests/anonymise_test.rs`, each with a negative control (a leaky gate and a bypassed anonymiser are caught by the same checks) |
| Dumps recorded; the held-out dump set aside and never used for tuning | **No — needs the owner's real use** | Tooling and runbook ready (`docs/runbooks/record-monitor-session.md`); a 45 s smoke recording proved the whole path in the real unit and was deleted |
| OQ-05 closed. Phase report; owner sign-off | OQ-05 yes · report yes · sign-off pending | PRD §10: no folder watched by default; the user lists them |

## 4. Measurements

Conditions: reference machine, **governor `performance`** (set by the owner), release build installed in the real sandboxed unit, the owner working normally (load average about 4.2 at the start), 10 minutes, 60 samples at 10 s, binary sha256 `5cb81bdc…`, commit `6956183`. The first run (before the review fixes) is kept and marked superseded. JSON: `reports/bench/p3-monitor-20261116T155509Z.json`.

| Measure | Result | Target | Notes |
| :--- | :--- | :--- | :--- |
| NFR-PERF-07 event capture, observation → bus enqueue | **p99 ≤ 0.5 ms** (254 events: 239 ≤ 0.1 ms, 10 ≤ 0.25 ms, 5 ≤ 0.5 ms) | < 1.5 ms p99 | Histogram bucket upper bound, so an upper bound. The PRD's projection of 0.008 ms is **not** reached for every sensor: the slower events are D-Bus (media) and focus with the pid lookup |
| RSS in the real unit | 6.3–7.2 MiB (min 6,635,520 B, max 7,585,792 B) | 30 MiB target, 48 MiB hard | |
| Idle CPU | 0.50 % of one core (cgroup CPU time over wall time) | all parts together ≤ 3 % | includes real focus, media and D-Bus activity |
| RSS growth | **not asserted**: 10 minutes is shorter than the curve needs (rules.md §9.3) | — | the hour-long recording sessions log RSS once a minute (`<label>.metrics.jsonl`) |
| `systemd-analyze security` | 1.5 OK | — | same as C6 |

An earlier version showed 25 ms p99 in the real unit: scan-based sensors stamped the observation before the `/proc` walk (BUG-047). Found by the live check, fixed.

## 5. Gate run

`just ci` on the reference machine, run to the end, 2026-11-16, commit `1891420` (Claude; later commits are documents only).

| Step | Result |
| :--- | :--- |
| fmt-check, clippy `-D warnings`, clang-format/tidy, shellcheck, machete | pass |
| Rust tests (incl. `local_*`: SC proofs 1–3 on the new unit stanzas, real compositor, private D-Bus, real inotify) | **414 / 414** (+1 manual test run on its own with `--run-ignored`: the real idle transition) |
| C++ tests (debug) | 62 / 62 |
| `cargo deny` + network-ban probe | pass (new narrow exception: `cosmic-protocols` GPL-3.0-only, ADR-0007 R-07, owner accepted) |
| Coverage | Rust lines **85.41 %** (gate 80 %); C++ lines 78.7 % (gate 70 %) |
| ASan+UBSan, TSan | 59 / 59 each |
| Result | **ALL GREEN** |

Also run: `just sc` (security proofs 1–3: no network, socket map, Landlock canary — all pass for the monitor's real unit) and `scripts/check-egress.sh` (monitor isolated: only `lo`, 0 sockets).

Monitor coverage (Rust lines): `privacy.rs` 97 %, `bus.rs` 95 %, `exclusion.rs` 95 %, `sensors/thermal` 99 %, `disk` 97 %, `battery` 97 %, `resources` 97 %, `proc` 94 %, `folders` 94 %, `cosmic` 93 %, `mpris` 93 %, `dump.rs` 93 %, `record.rs` 90 %, `anonymise.rs` 86 %, `idle.rs` 70 % (the compositor thread body needs a real idle timeout; run once by hand, below), `main.rs` 0 % (startup glue, exercised by the real unit). `neuroosctl replay` shows low in-process coverage because its tests run the built binary, which is not instrumented.

## 6. Bugs and review findings

| ID | Sev | What | Found by |
| :--- | :--- | :--- | :--- |
| BUG-045 | S3 | Phase 1 pause stub echoed the duration as the deadline and never expired | Claude (W2) |
| BUG-046 | S1 | A window focused during a pause was not tracked → process tree could leak after resume | Claude (property test, before it shipped) |
| BUG-047 | S3 | Capture latency 25 ms: observation stamped before the blocking `/proc` walk | Claude (live check) |
| BUG-048 | S1 | Process sensor learned the *unredacted* focused app → an excluded app's tree could be published after a quick focus change | agy review F1 |
| BUG-049 | S2 | Media state remembered across a pause could stay "playing" forever for late subscribers | agy review F2 |
| BUG-050 | S2 | `--record session.dump` (no folder) failed Landlock at start | agy review F3 |
| BUG-051 | S3 | Mount de-duplication dropped Btrfs `/home` (F4); the opposite rule made 5 disk samples of 2 filesystems in the real unit | agy review + live check |
| BUG-052 | S3 | `statvfs` on the async runtime (AB-11) | agy review F5 |
| BUG-053 | S3 | Dead inotify watches never freed | agy review F8 |
| BUG-055 | S2 | Idle never reported while any app held an idle inhibitor (a playing video); now the input-idle notification | Claude (manual real-compositor test) |
| BUG-054 | S4 | Phase 1 test `dropped_clients_release_their_connection` raced under load | gate |
| (no ID) | S3 | F6 resources sensor ended on one bad reading; F7 comment claimed it dropped events (it blocks on purpose, to keep the state machine in step); F9 script `rm -rf` of a whole drop-in folder | agy review, fixed |

The review was a read-only pass by agy over every Phase 3 file; each finding was checked against the code before fixing (all nine were real or a documentation fix) and each S1/S2 has a regression test that failed before the fix.

## 7. Decisions, deviations and what is not done

- **ADR-0011 (proposed, D-25)** — redacted focus for excluded windows; pause emits nothing and is followed by a snapshot; drop-oldest queues; late subscribers receive the latest state (not during a pause); always-anonymised `--record`; dump format in `neuroos-proto`; no `procfs`/`notify` crates; narrow GPL exception for `cosmic-protocols`.
- **Lock state is "unknown".** Probed on COSMIC: `org.freedesktop.ScreenSaver` offers only `Inhibit`/`UnInhibit`, no Wayland observer protocol exists, and logind needs the system bus that every unit hides (ADR-0002/0007). FR-MON-02's lock part is therefore not met in v1.0; the nudge policy holds on unknown (fail safe). Reopening needs your decision (§8).
- **agy stalled on W3** (the four samplers): about 45 minutes of reading and probing, one 7-line file, no code. Cancelled and rebuilt by Claude under the standing "do it yourself if agy loops" instruction. agy then built `neuroosctl replay` in one pass (checked: read the diff, re-ran fmt, clippy, 24 tests) and did the independent review, which was the most useful thing it did.
- **File paths in `FileActivity` are absolute** (the plan said relative). The anonymiser hashes every non-structural component.
- **Not exercised live:** a fullscreen enter/leave and a compositor restart. Their logic is tested against fake compositor messages; the real-compositor test checks the connection, the baseline and health. The idle transition *was* exercised on the real session by a manual test (`cargo nextest run -p neuroos-monitor --run-ignored only -E 'test(manual_)'`), not part of the gate because it needs the user's hands off the keyboard. Reported, not claimed.
- **A configured folder that does not exist at start** is reported DEGRADED and not re-checked until the sensor restarts.
- **Known limits:** one `focus` value per instant (no per-output focus); a window's pid is a best-effort hint (`0` when ambiguous); `check-dump.sh` lists readable app ids for a human to approve, it cannot judge them.

## 8. Owner decisions and actions at sign-off

1. **Record the measurement sessions** (MS): at least 8 hours in total over at least 3 dumps, plus one held-out dump. Runbook: `docs/runbooks/record-monitor-session.md`; start with `scripts/record-session.sh start morning`, and `stop` validates the dump. Recordings are anonymised before they touch disk. Without them the "dumps recorded" exit criterion stays open and Phase 4's noise gate has nothing to test on.
2. **ADR-0011 as a whole**, especially: an excluded window shows as an empty focus event (not silence), a pause emits nothing at all, recordings can never be un-scrambled even by you, and `~/Notes` is a suggestion rather than a default folder.
3. **Lock state.** Accept "unknown" for v1.0, or approve a narrow, read-only logind call (one property) from C1, which would widen its sandbox (R0-3) and needs an ADR.
4. **`cosmic-protocols` GPL-3.0-only exception** in `deny.toml` (already accepted as risk R-07 in ADR-0007; this is the build-time record of it).
5. (Optional) Try the live paths once yourself: enter and leave fullscreen, and leave the machine untouched for a minute with `[monitor] idle_timeout_s = 60`, while watching `scripts/p3-live-check.sh`.

## 9. Sign-off

Owner, 2026-11-16: ADR-0011 accepted as written ("go with your recommend"); lock state stays "unknown" in v1.0; `cosmic-protocols` exception accepted. The real-use recordings are deferred to the integration phase (D-26); the first session was started in the background the same day. Phase 3 signed off; the owner merges `phase-3-monitor`.
