# neuroos-fetcher

Semantic-NeuroOS Network Fetcher (C7).

## Purpose
The only part allowed network access (system unit, own user `neuroos-fetcher`). Fetching and search come in Phase 8.

**Phase 1 (skeleton):** no network code is linked; every fetch request returns the error `UNSUPPORTED_VERSION`. Its sockets are group-shared with `neuroos` (ADR-0009) so C6 and healthd can connect. Install with `sudo bash scripts/install-fetcher.sh`, then `sudo systemctl enable --now neuroos-fetcher`.

## Sockets
- `/run/neuroos-fetcher/fetch/fetch.sock`: Accepts `FetchRequest` from C6 `neuroos-kernel`.
- `/run/neuroos-fetcher/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
- `deadlines.fetch_ms`: Deadline for network fetches (default 15,000 ms).
- `ipc.max_frame_bytes`: Maximum frame payload (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer credentials and health RSS).
- Landlock read-write:
  - Own socket directory (`/run/neuroos-fetcher/`).
  - Spool directory (`/var/spool/neuroos-fetcher`).

## How to Run Alone
```bash
# Run with defaults (/run/neuroos-fetcher):
/opt/neuroos/bin/neuroos-fetcher --allow-uid 1000

# Run in test environment with custom runtime directory:
target/debug/neuroos-fetcher --runtime-dir /tmp/test-fetcher --allow-uid 1000
```
