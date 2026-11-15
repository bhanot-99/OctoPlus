# neuroos-storage

Semantic-NeuroOS Storage Engine (C3).

## Purpose
Provides vector and text search (hybrid query) over indexed desktop events, notes, and activity. Ingests events from C1 `neuroos-monitor`.

## Sockets
- `storage/storage/storage.sock`: Serves hybrid retrieval queries (`QueryHybrid` -> `HybridResult`).
- `storage/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
- `deadlines.storage_query_ms`: Deadline for hybrid search queries (default 150 ms).
- `ipc.max_frame_bytes`: Maximum frame payload (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer checks and health RSS).
- Landlock read-write:
  - Own socket directory (`%t/neuroos/storage/`).
  - Persistent storage directory (`~/.local/share/neuroos/storage`).
- Network: Disabled via systemd `PrivateNetwork=true` and Landlock TCP denial.

## How to Run Alone
```bash
# Run with defaults:
target/debug/neuroos-storage

# Run with custom runtime directory for tests:
target/debug/neuroos-storage --runtime-dir /tmp/test-neuroos
```
