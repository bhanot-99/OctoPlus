# neuroos-monitor

Semantic-NeuroOS Context Monitor (C1).

## Purpose
Will collect desktop activity (window focus, idle, system metrics, media playback) and stream it to C3 `neuroos-storage`, with pause and resume privacy control (Phase 3).

**Phase 1 (skeleton):** `stream` accepts a subscription and keeps it open but sends no events yet; `control` handles pause and resume and returns the privacy state.

## Sockets
- `monitor/stream/stream.sock`: Accepts `MonitorSubscribe`, streams `MonitorEvent` frames to C3.
- `monitor/control/control.sock`: Accepts `PauseRequest` and `ResumeRequest`, returns `PrivacyState`.
- `monitor/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
Uses standard `[ipc]` and `[deadlines]` configuration sections:
- `deadlines.control_ms`: Deadline for control requests (default 250 ms).
- `ipc.max_frame_bytes`: Maximum IPC frame size (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer credentials and health RSS scraping).
- Landlock read-write: Own socket directory (`%t/neuroos/monitor/`).
- Network: Disabled via systemd `PrivateNetwork=true` and Landlock TCP ruleset.

## How to Run Alone
```bash
# Run with defaults ($XDG_RUNTIME_DIR/neuroos):
target/debug/neuroos-monitor

# Run with custom runtime directory for testing:
target/debug/neuroos-monitor --runtime-dir /tmp/test-neuroos
```
