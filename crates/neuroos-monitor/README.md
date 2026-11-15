# neuroos-monitor

Semantic-NeuroOS Context Monitor (C1).

## Purpose
Collects desktop activity events (window focus, idle, system metrics, media playback) and streams them to C3 `neuroos-storage`. Provides pause and resume privacy control.

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
