# octoplus-monitor

OctoPlus Context Monitor (C1, Phase 3).

## Purpose
Collects raw desktop context and machine health and streams it to C3 `octoplus-storage`: focused window and fullscreen state, idle, media players, CPU and memory, disk, battery, temperature, the focused window's process tree and (P1) activity in configured folders. C1 never judges or filters: the **only** things that stop or change an event are *pause* and the *exclusion list* (AB-5, `privacy.rs`).

Each sensor is a supervised task with its own health row (see `octoplusctl status`); a failing sensor never stops the others. Design: Architecture §6.2 "Inside C1", ADR-0007 (sources), ADR-0011 (privacy gate, recording, lock).

## Sensors
| Sensor | Source | Requirement |
| :--- | :--- | :--- |
| `focus` | COSMIC `zcosmic_toplevel_info_v1` over the compositor socket | FR-MON-01 |
| `idle` | `ext_idle_notifier_v1` (respects idle inhibitors) | FR-MON-02 |
| `media` | MPRIS on the **session** bus only | FR-MON-03 |
| `resources` | `/proc/stat`, `/proc/meminfo`, `/proc/*/statm` | FR-MON-04 |
| `disk` | `/proc/self/mountinfo` + `statvfs` | FR-MON-05 |
| `battery` | `/sys/class/power_supply` (no UPower) | FR-MON-06 |
| `thermal` | `/sys/class/hwmon` | FR-MON-07 |
| `proc` | `/proc` (matched to the focused app; the pid is a hint) | FR-MON-08 |
| `folders` | inotify on `[[monitor.folders]]` only | FR-MON-11 |

Session lock state is **not** observable inside C1's allowed surface on COSMIC (ADR-0011): no `LockChanged` is emitted and the nudge policy holds on "unknown".

## Sockets
- `monitor/stream/stream.sock`: `MonitorSubscribe` → a stream of `MonitorEvent`. Only the storage unit may connect. A new subscriber first receives the latest state of each kind (not during a pause). A reader that falls far behind is cut loose with a retryable error.
- `monitor/control/control.sock`: `PauseRequest` (0 = until resumed) and `ResumeRequest` → `PrivacyState`. Resuming makes the sensors re-emit their state.
- `monitor/health/health.sock`: scraped by `octoplus-healthd`.

## Config keys (`[monitor]`, unknown keys are fatal)
`cpu_interval_s`, `disk_interval_s`, `battery_interval_s`, `thermal_interval_s`, `idle_timeout_s`, `queue_capacity`, `exclude_apps`, `exclude_title_substrings`, `drop_default_exclusions`, `record_keep_app_ids`, and `[[monitor.folders]]` (`path`, `kind` = `git` | `notes` | `calendar`). Defaults are the PRD §5.2 values. No folder is watched by default.

## Recording
`octoplus-monitor --record <file>` writes an anonymised dump (always: there is no raw mode). Replay with `octoplusctl replay <dump> [--speed N] [--json] [--serve <socket>]`. Runbook: `docs/runbooks/record-monitor-session.md`.

## Sandbox paths
- Landlock read-only: `/proc`, `/sys`, configured folders, config. Read-write: own socket folders (and the folder of `--record`).
- Unit: compositor socket and session bus **read-only**; never the system bus or the accessibility bus; network disabled (`PrivateNetwork=true`).

## How to run alone
```bash
# Defaults ($XDG_RUNTIME_DIR/octoplus), no sandbox unit:
target/debug/octoplus-monitor
# Custom runtime directory, with a recording:
target/debug/octoplus-monitor --runtime-dir /tmp/test-octoplus --record /tmp/session.dump
# In its real sandboxed unit, with a live check:
scripts/p3-live-check.sh
```
