# neuroos-fetcher

Semantic-NeuroOS Network Fetcher (C7).

## Purpose
The sole network-capable part in Semantic-NeuroOS (system scope). In Phase 1, network fetching is disabled and stubs return `UNSUPPORTED`. Zero network code is linked or executed.

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
