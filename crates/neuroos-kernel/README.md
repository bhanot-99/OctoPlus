# neuroos-kernel

Semantic-NeuroOS Safety Kernel (C6).

## Purpose
Enforces capability tiers, taint-based escalation, in-house argument schema validation, review rate limiting, cryptographic action audit logging, egress ledger tracking, and action dispatch gating (Phase 2, FR-KER-01, FR-KER-02, FR-KER-03, FR-KER-06, FR-KER-07, FR-KER-08; Architecture §8.3, §8.5; security.md §4.4, §8).

Answers `EvaluateRequest` with accurate safety tiers (`SAFE`, `REVIEW`, `DANGEROUS`), escalates clean `SAFE` skills to `REVIEW` under `EXTERNAL_UNTRUSTED` taint, rejects disabled skills (`shell.exec`) and unknown capabilities with `PermissionDenied`, rejects missing or malformed taint flags with `InvalidRequest`, and tracks evaluation health metrics.

Maintains tamper-evident hash-chained JSON-lines audit logs (`actions.jsonl` and `egress.jsonl`) managed by a dedicated actor thread with synchronous disk flushing and automatic log rotation.

## Sockets
- `kernel/kernel/kernel.sock`: Accepts `EvaluateRequest`, `AuditTailRequest`, `AuditVerifyRequest`, and `EgressListRequest`.
- `kernel/health/health.sock`: Scraped by `neuroos-healthd`. Includes `action_log` probe.

## Audit and Egress Storage
- Directory: `<data_dir>/audit/` (`%h/.local/share/neuroos/audit/` in production, permissions `0700`).
- Files:
  - `actions.jsonl`: Action audit log (`ATTEMPT` and `RESULT` records, permissions `0600`).
  - `egress.jsonl`: Outbound network egress ledger (permissions `0600`).
- Integrity: Every record is SHA-256 hash-chained with `prev_hash`. The first line uses 64 zeros.
- Rotation: Rotates when file reaches 10 MB (`ROTATE_BYTES = 10 * 1024 * 1024`), retaining 5 copies (`.1`..`.5`). Chain continues uninterrupted across rotated files.
- Actor: Dedicated OS thread (`kernel-audit-log`) processes disk I/O off the Tokio async pool via a bounded channel (bound 64).

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
