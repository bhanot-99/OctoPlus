# octoplus-kernel

OctoPlus Safety Kernel (C6). Design: ADR-0010; Architecture §8.2–§8.5; security.md §4.4–4.5, §8.

## Purpose
Decides, asks, runs and records every action (FR-KER-01…10).

- **Evaluate:** classifies a capability as `SAFE`, `REVIEW` or `DANGEROUS` from the compiled-in manifests in `skills/*.toml`. Taint `EXTERNAL_UNTRUSTED` raises `SAFE` to `REVIEW`; tiers are never lowered. Disabled skills (`shell.exec`) and unknown capabilities get `PermissionDenied`; a missing or malformed taint gets `InvalidRequest`. `llm.generate` is answered `SAFE` for any valid taint and can never be dispatched (ADR-0010 item 11).
- **Dispatch:** `ATTEMPT` is written before anything happens. For `REVIEW`/`DANGEROUS` there is one dialog at a time, at most 6 requests per rolling minute, and a 300 s refusal after a denial. Your Allow becomes an HMAC-SHA256 token over action id, nonce, capability, argument hash, verdict and expiry (120 s); the nonce is single-use and the token is re-checked against exactly what runs, just before it runs. A crashed, killed, silent or non-zero-exit dialog is never an Allow. The skill then runs in a `octoplus-skill` child (or, for `privacy.pause`, in C6 itself), and `RESULT` is written after.
- **Notify:** notifications are rendered only from compiled-in templates (numbers, no user text) and sent over the session bus.
- **Audit:** hash-chained JSON lines, one actor thread owns both files.
- **Recovery:** on start, every `ATTEMPT` in the current log without a `RESULT` gets `RESULT EXPIRED` (its token died with the old process).

## Sockets
- `kernel/kernel/kernel.sock`: `EvaluateRequest`, `DispatchRequest`, `NotifyRequest`, `AuditTailRequest`, `AuditVerifyRequest`, `EgressListRequest`.
- `kernel/health/health.sock`: scraped by `octoplus-healthd`; includes the `action_log` probe.
- Client of: `monitor/control` (`privacy.pause`; voice pause arrives in Phase 7) and the fetcher socket.

## Children
- `octoplus-confirm` (the dialog) and `octoplus-skill` (one per action), both found next to the `octoplus-kernel` binary. Their stderr is C6's (journal; codes only).

## Audit and egress storage
- Folder `~/.local/share/octoplus/audit/` (mode `0700`): `actions.jsonl` (`ATTEMPT`/`RESULT`) and `egress.jsonl`, mode `0600`.
- Every record carries `prev_hash` (SHA-256); the first line uses 64 zeros. Rotation at 10 MB, 5 copies kept, the chain continues across files. An unfinished last line (short write) is cut on open (BUG-036).

## Keys
- `~/.config/octoplus/hmac.key`: 32 bytes, mode `0600`, created by `scripts/install-dev.sh`. C6 refuses to start if it is missing, the wrong size, or readable by others.

## Config keys
- `kernel.review_per_minute` (default 6), `kernel.denial_cooldown_s` (default 300).
- `kernel.cleanup_build_folders`: absolute folders `system.cleanup` may empty, besides the trash and `~/.cache` (default empty). The installer turns them into a unit drop-in (`octoplus-deploy --kernel-dropin`).
- `deadlines.kernel_check_ms` (default 50 ms), `deadlines.kernel_dispatch_ms` (default 155,000 ms: covers a whole approval).
- `ipc.max_frame_bytes` (default 1 MiB).

## Sandbox
C6's policy is the union of what it and its children need (ADR-0010 item 3); each child narrows itself further.
- Unit binds: the bin folder, `config.toml`, `hmac.key` (read-only); the audit folder, Wayland, session bus, at-spi, `~/.cache`, the trash, build folders (read-write). `MemoryMax=128M` (PRD §6.2; the dialog is ~50 MiB). `SystemCallFilter=seccomp` so children can install their own filter (BUG-038).
- Landlock read-only: `/usr`, `/etc`, `/var/cache/fontconfig`, `hmac.key`, the bin folder. Read-write: the audit folder, `~/.cache`, the trash, build folders, own sockets.
- Network: `PrivateNetwork=true`, `RestrictAddressFamilies=AF_UNIX`, Landlock TCP denial.

## How to run alone
```bash
target/debug/octoplus-kernel
target/debug/octoplus-kernel --runtime-dir /tmp/test-octoplus
```
