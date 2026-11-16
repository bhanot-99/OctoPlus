# neuroos-kernel

Semantic-NeuroOS Safety Kernel (C6).

## Purpose
Will enforce capability tiers, taint checks, approval and action dispatch (Phase 2).

**Phase 1 (skeleton):** answers every `EvaluateRequest` with `tier: SAFE`, rejects a request without `context_taint`, and keeps reconnecting links to C7 and `monitor/control` (DEGRADED while either is down).

## Sockets
- `kernel/kernel/kernel.sock`: Accepts `EvaluateRequest` and capability checks.
- `kernel/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
- `deadlines.kernel_check_ms`: Deadline for safety evaluations (default 50 ms).
- `ipc.max_frame_bytes`: Maximum frame payload (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer unit/UID checks and health RSS).
- Landlock read-write: Own socket directory (`%t/neuroos/kernel/`).
- Network: Disabled via systemd `PrivateNetwork=true` and Landlock TCP denial.

## How to Run Alone
```bash
# Run with defaults:
target/debug/neuroos-kernel

# Run with custom runtime directory:
target/debug/neuroos-kernel --runtime-dir /tmp/test-neuroos
```
