# octoplus-fetcher

OctoPlus Network Fetcher (C7).

## Purpose
The only part allowed network access (system unit, own user `octoplus-fetcher`). Fetching and search come in Phase 8.

**Phase 1 (skeleton):** no network code is linked; every fetch request returns the error `UNSUPPORTED_VERSION`. Its sockets are group-shared with `octoplus` (ADR-0009) so C6 and healthd can connect. Install with `sudo bash scripts/install-fetcher.sh`, then `sudo systemctl enable --now octoplus-fetcher`.

## Sockets
- `/run/octoplus-fetcher/fetch/fetch.sock`: Accepts `FetchRequest` from C6 `octoplus-kernel`.
- `/run/octoplus-fetcher/health/health.sock`: Scraped by `octoplus-healthd`.

## Config Keys
- `deadlines.fetch_ms`: Deadline for network fetches (default 15,000 ms).
- `ipc.max_frame_bytes`: Maximum frame payload (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer credentials and health RSS).
- Landlock read-write:
  - Own socket directory (`/run/octoplus-fetcher/`).
  - Spool directory (`/var/spool/octoplus-fetcher`).

## How to Run Alone
```bash
# Run with defaults (/run/octoplus-fetcher):
/opt/octoplus/bin/octoplus-fetcher --allow-uid 1000

# Run in test environment with custom runtime directory:
target/debug/octoplus-fetcher --runtime-dir /tmp/test-fetcher --allow-uid 1000
```
