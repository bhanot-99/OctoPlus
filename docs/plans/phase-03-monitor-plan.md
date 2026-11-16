# Phase 3 build plan — Monitor (C1)

**Branch:** `phase-3-monitor` · **Stories:** P3-S01…S07 (`phases.md` §6) · **Report:** `reports/phase-03-monitor.md`
**Started:** 2026-11-16 on the owner's go-ahead ("start executing phase 3, full coordination of Claude and agy"), recorded as D-25 in `memory.md` (R0-12).
**Roles:** agy builds the sensors and the replay command; Claude builds the foundation, privacy layer, recorder and anything agy loops on (owner rule: over 2 fix rounds, Claude takes over); the other side checks every work package (rules.md §2). Claude commits; agy never does.

This file explains *how* each step is built. Requirements live in `PRD.md`, mechanisms in `Architecture.md`, source choices in ADR-0007. Numbers (intervals, caps) are config defaults or named constants that cite the PRD ID; none is repeated here.

---

## 0. Design in one page

```
 sensors (one supervised task each)          privacy gate              bus                    subscribers
 focus ─┐                                                              ┌─ bounded queue ─► C3 (stream.sock)
 idle  ─┤                                                              ├─ bounded queue ─► recorder (--record)
 lock  ─┤  SensorEvent{observed_at_ns,…}   exclusion + pause   fan-out ┤   each queue: drop counter
 media ─┼───────────────────────────────►  (the ONLY filter,  ───────► │
 cpu/mem┤                                   AB-5, FR-MON-10)           └─ health: events_emitted, dropped
 disk, battery, thermal, proc tree, folders
```

Rules that shape everything:
- **C1 never judges** (AB-5, FR-MON-09). The only things that stop an event are *pause* and the *exclusion list*, both in one module (`privacy.rs`) that sits between sensors and the bus. No sensor knows about privacy; no other code drops events.
- **Every timestamp is `u64` UTC ns** (AB-8), taken at the moment of observation, not at send.
- **No lock across `.await`; sensors never block the bus** (AB-11). Blocking work (Wayland dispatch, `/proc` walks) runs on its own thread or `spawn_blocking`; results cross a bounded channel.
- **A dead sensor is a degraded sensor, not a dead monitor.** Each sensor is supervised: on failure it restarts with jittered backoff (AB-10, reuse `neuroos_ipc::backoff`), its health subsystem turns DEGRADED with a fixed detail string, and the others keep running.
- **Window ids are ours.** Compositor toplevel handles are per-connection; C1 assigns a never-reused `u64` per window. On compositor restart every open window gets a close (an empty-focus event) before any new id appears (FI test).
- **Personal text stays out of logs** (R0-10): no title, app id, path or media tag at `info` or above.

## 1. Decisions Claude takes under the go-ahead (open to owner overturn at sign-off; ADR-0011 "Proposed")

| # | Decision | Why |
| :--- | :--- | :--- |
| 1 | **Excluded window = redacted focus event**, not silence: `FocusChanged{app_id:"", title:"", pid:0, fullscreen_known:false}`. Process tree, media-from-that-app and file events tied to it are suppressed. System-wide samples (CPU, disk…) continue. | C3 needs to know a focus *ended*; silence would stretch the previous window's span over private time. The redacted event carries nothing. |
| 2 | **Pause = nothing is emitted at all** (including samples), sensors keep running so state is fresh. **On resume C1 emits one snapshot** (current focus, idle, lock, media). Pause never reaches the recorder either. | FR-PRV-01 "stops sensing"; a snapshot avoids a blind spot after resume. Keeping sensors alive avoids Wayland reconnect cost. |
| 3 | **Pause timer uses a monotonic clock**; `paused_until_ns` is derived UTC. Expiry emits the resume snapshot. (The Phase 1 stub echoed the duration as the deadline — a bug fixed here.) | Wall-clock jumps must not extend or end a pause. |
| 4 | **Exclusion matches `app_id` (case-insensitive exact, or `prefix*`) and title substrings** for private windows. Defaults: common password managers and private-browsing title markers. User list in `[monitor] exclude_apps`/`exclude_title_substrings`, appended to defaults (defaults cannot be removed by config, only by an explicit `drop_default_exclusions = true`). | FR-PRV-02. Fail safe. |
| 5 | **Stream overflow = drop oldest, count it.** Per-subscriber bounded queue; `dropped_events` is in health and in the next event's side channel (a counter C3 can scrape), never silent. | AB-11, rules §9.7. |
| 6 | **Lock state source** (ADR-0007 left it open): probe order is (a) session-bus `org.freedesktop.ScreenSaver`/`org.gnome.ScreenSaver` `ActiveChanged`, (b) a Wayland observer if the compositor offers one. If neither works on COSMIC, lock stays `unknown` and the nudge policy holds (fail safe). The probe result decides; recorded in ADR-0011 with evidence. | Closes ADR-0007 item 3 without the system bus. |
| 7 | **`--record` always anonymises** (no raw-dump mode exists). Keyed HMAC-SHA256, 32-byte per-run key from `getrandom`, zeroized at the end of the run; placeholders `t_<10 hex>`; the same text maps to the same placeholder inside one run. Replaced: titles, media title/artist, process names, file paths, top-process name. `app_id` is kept only if it is in a built-in public-app list (config-extendable); otherwise hashed. | FR-MON-12, BUG-013. Noise-gate work in Phase 4 needs app classes, so well-known apps stay readable. |
| 8 | **Dump format** `neuroos-proto::dump`: magic + version header, then length-delimited `MonitorEvent` frames, then an explicit end record with event count. Reader rejects truncation and unknown versions. Lives in `neuroos-proto` because both C1 (writer) and `neuroosctl` (reader) need it and AB-1 forbids linking the monitor crate. | AB-1, AB-12. |
| 9 | **`neuroosctl replay <dump>`** prints events as JSON lines at `--speed N` (default real time); `--serve <socket>` instead binds a stream socket and plays the dump to any `MonitorSubscribe` (this is what Phase 4's replay harness and the noise gate use). | P3-S06, Architecture §14 "replay harness". |
| 10 | **No `procfs` / `notify` crates.** `/proc` and `/sys` parsing is hand-written as pure functions over `&str` (property-testable); inotify uses `rustix::fs::inotify`. Fewer dependencies in an isolated part (rules §5). New deps: `wayland-client`, `wayland-protocols`, `cosmic-protocols` (GPL-3.0, ADR-0007 R-07, isolated to C1), `zbus` (already in workspace), `hmac`, `sha2`, `getrandom`, `zeroize` (already in workspace). `deny.toml` gets a narrow GPL exception for `cosmic-protocols` only, with the owner's recorded approval from ADR-0007. | rules §5, ADR-0007. |

## 2. Work packages, in order

Each package lists: owner · files · what to build · how it is tested · done when. "Real" means the shipped binary in its sandboxed unit.

### W0 — Kickoff records (Claude)
`memory.md` (Phase 3 started, D-25, board with P3-S01…S07), this plan, ADR-0011 stub (Proposed). Commit.

### W1 — Foundation (Claude) · files: `neuroos-common/src/config.rs`, `neuroos-monitor/src/{lib,clock,bus,sensor,supervisor,stream}.rs`
1. **Config `[monitor]`** (deny unknown keys, defaults cite PRD): `cpu_interval_s` (FR-MON-04), `disk_interval_s` (-05), `battery_interval_s` (-06), `thermal_interval_s` (-07), `idle_timeout_s` (-02), `queue_capacity`, `exclude_apps`, `exclude_title_substrings`, `drop_default_exclusions`, `folders` (list of `{path, kind}`), `record_keep_app_ids`. Validation rejects zero intervals and a zero queue.
2. **`Clock` trait** (`now_utc_ns()`, `now_mono()`); real and fake implementations. Everything time-dependent takes it (AB-13).
3. **`Sensor` trait** (native async fn in trait, used generically — no `dyn`): `fn name() -> &'static str`; `async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError>`. `SensorOut::emit(event)` stamps nothing: the sensor passes the observation time. `SensorError` is a `thiserror` enum with `retryable`.
4. **Supervisor**: runs a sensor, on `Err` or return marks health `DEGRADED(detail)`, waits jittered backoff 100 ms→10 s (AB-10), restarts; on success of the first event marks `OK`. Per-sensor health subsystem name = sensor name; counters `events.<name>`.
5. **Bus**: `Bus::publish(event)`; `Bus::subscribe() -> Subscription` with its own bounded queue (drop-oldest, counter). Gate is injected as a trait so W2 plugs in without touching the bus.
6. **Stream handler**: `MonitorSubscribe` → loop sending `MonitorEvent` bodies; ends with an explicit end frame on stop or cancel (AB-12); a slow subscriber never blocks the bus (FI).
7. **Tests:** UT for clock, queue overflow ordering, supervisor backoff with a fake clock and a failing fake sensor; IT in-process (Server + Client) for subscribe → events → end frame on stop.
**Done when:** a fake sensor's events reach a real socket client; kill/restart of the fake sensor flips health DEGRADED→OK.

### W2 — Privacy layer (Claude) · files: `privacy.rs`, `control.rs`, `exclusion.rs`
1. `Exclusion::matches(app_id, title) -> bool` (case-insensitive, prefix glob, title substrings) with the default list as a named constant citing FR-PRV-02. Edge cases: empty app id, empty title, mixed case, unicode, very long titles.
2. `PrivacyState` machine: `Running | Paused{until: Option<Instant>}`; `pause(duration)`, `resume()`, `tick(now)`; pause timer expiry; pause while already paused extends/replaces (last request wins).
3. `Gate::admit(event) -> Option<event>`: pause drops everything; exclusion rewrites focus to the redacted event and drops process-tree/file events belonging to the excluded window. Gate also remembers "currently excluded" so a later sample for that window cannot leak.
4. **Control handler**: real `PauseRequest/ResumeRequest` → `PrivacyState` (UTC-derived deadline); emits the resume snapshot request to the sensors via a `watch` channel; a status request is answered by a pause with no change (no new proto message).
5. **Tests:** UT for matching and timer (fake clock); **PT (proptest): for any event sequence and any exclusion list, no event containing an excluded app id/title substring and no event during pause is ever published** — the negative control (rule 8.9) breaks the gate on purpose and asserts the property test fails.
**Done when:** P3-S05 provable with the property test and a socket-level IT where an excluded fake app never appears on the stream.

### W3 — Procfs/sysfs samplers (agy) · files: `sensors/{resources,disk,battery,thermal}.rs`, `sensors/mod.rs` registration line only
Parsers are pure functions over text; file reads are a thin layer on top, with the root path injectable so tests run on fixtures in a tempdir (no real `/sys` needed in unit tests).
1. **resources** (FR-MON-04): CPU % from two `/proc/stat` snapshots (guard wrap-around, zero delta, first sample = no event), memory from `/proc/meminfo` (`MemAvailable`), largest process by RSS from `/proc/*/statm` scan (skip vanished pids; never fail the sample because one pid disappeared). Reports name = `comm` (kernel truncates to 15 chars — handled, not "fixed").
2. **disk** (FR-MON-05): mounts from `/proc/self/mountinfo`, keep user-relevant filesystem types only (skip pseudo, tmpfs, squashfs/loop, overlay, bind duplicates), `statvfs` via `rustix`; one `DiskSample` per mount.
3. **battery** (FR-MON-06): `/sys/class/power_supply/BAT*` + AC; percent, status, `on_ac`; time estimate only when `power_now > 0` (never guessed; ADR-0007); emit on change and at least every `battery_interval_s`. No battery (desktop) → sensor reports OK-but-idle with detail "no battery", emits nothing.
4. **thermal** (FR-MON-07): hwmon with the CPU package sensor chosen by driver name (`k10temp` `Tctl`, `coretemp` `Package id 0`); millidegrees → °C.
5. **Tests:** table-driven UT on fixture text (including malformed, empty, missing fields, huge values); proptest that parsers never panic on arbitrary bytes; a `local_` test reading the real machine once and asserting sane ranges.
**Done when:** each sensor, run standalone through the supervisor, produces correct events on this machine and a fixture-driven negative test fails if the parser is broken.

### W4 — Process tree and pid matching (agy) · files: `sensors/proc.rs`
FR-MON-08, P3-S04. Wayland gives no pid (lessons-from-v1 §5), so the pid is a *hint*:
1. `snapshot(root_pid) -> ProcessTree` from `/proc/<pid>/stat` (pid, ppid, comm) walking children; bounded (node cap constant) so a fork bomb cannot blow the frame; cycle-safe; vanished pids skipped.
2. `match_window(app_id, candidates) -> Option<pid>`: match the window's `app_id` against `comm`, `exe` basename and cmdline[0] (strip `.desktop` / reverse-DNS prefix, flatpak `app/<id>` cgroup, case-insensitive). Edge cases from the phase plan: **empty app id → no match; truncated 15-char `comm` → compare against the truncated prefix of the app name; ambiguous (several equally good) → pick the lowest-start-time ancestor or none, never a guess.**
3. Emitted on focus change (after a short settle delay, config constant) as `ProcessTree`; never for an excluded window (the gate also enforces it).
4. **Tests:** UT with fixture `/proc` trees; proptest on the matcher (never panics, empty app id never matches, result pid is always in the candidate set).

### W5 — Wayland sensors (agy builds, Claude checks) · files: `sensors/{cosmic,idle,lock}.rs`, `Cargo.toml`, `deny.toml`
Base: `tests/spikes/s05-probe/src/main.rs` (723 lines, already talks `zcosmic_toplevel_info_v1` and lists idle/lock globals — reuse its protocol code, not its report printing, BUG-013).
1. **focus/cosmic** (FR-MON-01, P3-S01): connect to `$WAYLAND_DISPLAY`, bind `zcosmic_toplevel_info_v1` + `ext_foreign_toplevel_list_v1`; track toplevels (title, app_id, state: activated, fullscreen); emit `FocusChanged` when the *activated* toplevel changes or its title/fullscreen changes (debounce rapid title churn with a constant, not a sleep); on compositor disconnect close all windows (redacted empty focus) then reconnect with backoff. Blocking dispatch loop runs on a dedicated thread feeding a bounded channel.
2. **idle** (FR-MON-02, P3-S02): `ext_idle_notifier_v1` notification with `idle_timeout_s`; emits `IdleChanged` on idled/resumed. Must handle: `get_idle_notification` vs input-idle variant, seat availability, compositor restart.
3. **lock** (FR-MON-02): implement the source the W5 probe picks (decision 6); until then emit `LockChanged` only when it is *known*.
4. **Landlock/unit:** the Wayland socket is passed to `ServiceBuilder::extra_read_only`; unit binds only `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` read-only (ADR-0007).
5. **Tests:** pure-logic UT on the toplevel state machine (feed it fake protocol events: activate, retitle, fullscreen on/off, close, compositor restart → ids unique and windows closed); `local_` integration against the real compositor: focus changes when the test opens a window, fullscreen flip, idle after the timeout (short timeout in the test), lock state.
**Done when:** ADR-0007's "not exercised" gaps (fullscreen flip, idle/lock events) have measured evidence in the report.

### W6 — Media players (agy) · files: `sensors/mpris.rs`, unit file
FR-MON-03, P3-S02. `zbus` on the **session** bus only (never the system bus): watch `NameOwnerChanged` for `org.mpris.MediaPlayer2.*`, read `PlaybackStatus` and `Metadata` (`xesam:title`, `xesam:artist`), subscribe to `PropertiesChanged`. Player vanishes → emit `STOPPED` and **drop its watcher** (FI: no leaked match rules or tasks; assert the task count returns to baseline). Metadata is untrusted text: length-capped, control characters stripped. Unit gains `BindReadOnlyPaths=%t/bus` (ADR-0007 / Architecture §8.2) — and nothing wider.
**Tests:** UT on metadata parsing (missing keys, `artist` as array/string, huge strings); IT with a small fake MPRIS service on a private `dbus-daemon` (skipped if the daemon binary is absent, run in the local gate); `local_` test with a real player if one is running.

### W7 — Folder sensors, P1 (agy) · files: `sensors/folders.rs`
FR-MON-11, P3-S07, closes OQ-05 (`~/Notes`, local `.ics`). `rustix::fs::inotify` over configured folders only; kinds: git repo (watch `.git/HEAD`, `.git/index`, `refs`), notes (`*.md`/`*.txt`), calendar (`*.ics`). Coalesce bursts (constant window, not a sleep in tests — use the fake clock), cap watches, ignore dotfiles except `.git`, never follow symlinks out of the folder, emit `FileActivity{path, kind}` with the path **relative to the configured root** plus root label. Landlock: configured folders come from config → `extra_read_only`; unit `BindReadOnlyPaths=-%h/Notes` etc. generated from config at install.
**Tests:** tempdir IT (create/modify/remove, burst coalescing, symlink escape denied, watch cap reached → DEGRADED not crash).

### W8 — Recorder, anonymiser, dump, replay (Claude: C1 side · agy: `neuroosctl replay`) · files: `neuroos-proto/src/dump.rs`, `neuroos-monitor/src/{record,anonymise}.rs`, `neuroosctl/src/replay.rs`
1. Dump writer/reader (decision 8) with truncation/version tests and a golden file (`insta`).
2. Anonymiser (decision 7): HMAC-SHA256, per-run key from `getrandom`, `zeroize` on drop; pure `anonymise(event, &Key) -> event`.
3. `--record <file>` CLI flag on the binary (clap); recorder is just another bus subscriber after the gate, so paused/excluded data never reaches the file.
4. `neuroosctl replay` (decision 9), reading via `neuroos_proto::dump`; `--speed`, `--print`, `--serve`.
5. **PT:** the same text yields the same placeholder in one run and a different one in another run; placeholders cannot be reversed or reproduced without the key (test: recompute HMAC with a *wrong* key and assert mismatch; assert the key is not written anywhere — scan the dump bytes for it); dumps contain none of the original strings (fixture with sentinel titles).
6. **Negative control:** disable the anonymiser, confirm the PT fails.

### W9 — Real unit, health, `neuroosctl status` (Claude)
Unit file: exactly the sockets and paths of Architecture §5.2/§8.2 (stream, control, health sockets; `/proc`; `/sys/class/{power_supply,hwmon,thermal}` read-only; Wayland socket; `%t/bus`; configured folders); `MemoryMax` untouched (owner-approved). `neuroos-deploy` generates/validates the unit, `scripts/install-dev.sh` installs it. Health: per-sensor subsystem statuses, `events_emitted`, `dropped_events`, `subscribers`, capture-latency histogram (NFR-PERF-07 measured from observation to enqueue). `neuroosctl status` shows the sensors. Socket-map test (AB-7) proves no other socket is reachable. `systemd-analyze security` score recorded.
**Done when:** `systemctl --user start neuroos-monitor` in the real unit streams real focus events to a `neuroosctl`-style subscriber, and every sensor row in `status` is real.

### W10 — Gate, measurements, report, handoff (Claude, with agy review)
1. **FI:** compositor restart (nested compositor or killing/reconnecting the Wayland connection in a test harness) — ids unique, windows closed; player vanishes; slow subscriber; disk-full sensor input (filled tempfs for the disk sensor); full sensor failure injection (each sensor made to error, supervisor recovers).
2. **PF** (release build, shipped unit, governor recorded — the owner's `performance` sudo step from memory.md §2): NFR-PERF-07 p99 capture latency; RSS over a long idle run (assert only for as long as the run lasted — rule 9.3); idle CPU. JSON to `reports/bench/`.
3. **MS (needs the owner's real use — cannot be done by agents):** `scripts/record-session.sh` + a runbook start a recording unit; the owner works normally for ≥ 8 h across ≥ 3 sessions, plus one separate session reserved as the held-out dump. I check, count and anonymise-verify the dumps; **only anonymised dumps are committed, the held-out one is stored aside (path recorded in the report, never used for tuning).**
4. `just ci` run to the end, coverage for touched modules, `cargo deny check`, security.md §8 update (C1 boundary), Architecture §5.2/§8.2 sync, PRD OQ-05 closed, BUGS.md entries, ADR-0011 finalised, `reports/phase-03-monitor.md`, memory.md.
5. Owner sign-off; owner merges.

## 3. Order of execution and who touches what

| Order | Package | Builder | Checker | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 1 | W0, W1 | Claude | agy review | Foundation first so agy's files are disjoint. |
| 2 | W3 + W2 in parallel | agy (sensors/) · Claude (privacy/control) | Claude · agy | No shared files: agy owns `sensors/*`; Claude owns the rest. |
| 3 | W4, W6, W7 | agy | Claude | One at a time in one agy conversation. |
| 4 | W5 | agy (probe base given) | Claude + a real-compositor run | The riskiest package; if agy loops twice, Claude takes it. |
| 5 | W8 | Claude (C1 side) · agy (replay) | cross-check | |
| 6 | W9, W10 | Claude | agy review | |

## 4. Exit-criteria map

| Exit criterion (phases.md §6.4) | Evidence |
| :--- | :--- |
| Every sensor emits correct events on the reference machine and reports its own health | W3–W7 `local_` tests; `neuroosctl status` rows; report table |
| Performance and RSS measured in the shipped unit | W10.2 JSON in `reports/bench/` |
| Exclusion, pause and anonymiser proven by property tests | W2.5, W8.5 + negative controls |
| Dumps recorded; held-out set aside | W10.3 (owner's real use) |
| OQ-05 closed; report; sign-off | W7, W10.4–5 |
