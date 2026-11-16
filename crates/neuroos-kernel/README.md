# neuroos-kernel

Semantic-NeuroOS Safety Kernel (C6).

## Purpose
Enforces capability tiers, taint-based escalation, in-house argument schema validation, review rate limiting, and action dispatch gating (Phase 2, FR-KER-01, FR-KER-02, FR-KER-03, FR-KER-08; Architecture §8.3; security.md §4.4).

Answers `EvaluateRequest` with accurate safety tiers (`SAFE`, `REVIEW`, `DANGEROUS`), escalates clean `SAFE` skills to `REVIEW` under `EXTERNAL_UNTRUSTED` taint, rejects disabled skills (`shell.exec`) and unknown capabilities with `PermissionDenied`, rejects missing or malformed taint flags with `InvalidRequest`, and tracks evaluation health metrics.

## Sockets
- `kernel/kernel/kernel.sock`: Accepts `EvaluateRequest` and capability evaluations.
- `kernel/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
- `kernel.review_per_minute`: Maximum review/dangerous requests per rolling 60 s window (default 6).
- `kernel.denial_cooldown_s`: Cooling period in seconds after user denies a request (default 300 s).
- `kernel.cleanup_build_folders`: Absolute filesystem paths allowed for build cleanup (default empty).
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
