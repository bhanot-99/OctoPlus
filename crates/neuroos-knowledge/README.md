# neuroos-knowledge

Semantic-NeuroOS Knowledge and Nudge Engine (C5).

## Purpose
Orchestrates turn processing, grounding, evidence assembly, safety evaluation, inference streaming, and nudges.
In Phase 1, implements the thin `AskRequest` walking skeleton path:
1. Calls C6 `EvaluateRequest` to verify capability permission.
2. Calls C3 `QueryHybrid` for context evidence (fails soft if C3 is down).
3. Builds ChatML prompt with persona, evidence wrapped in `<untrusted_document>`, and stable prefix length.
4. Streams `GenerateRequest` to C4, streaming `TokenChunk` -> `AnswerChunk` back to client, ending with `AnswerDone`.
5. Propagates client cancellation upstream to C4.
6. Returns fast retryable UNAVAILABLE error when C4 is down, and records latency in `ask_first_token`.

## Sockets
- `knowledge/knowledge/knowledge.sock`: Serves `AskRequest` to `neuroosctl` and C2 `neuroos-voice`.
- `knowledge/health/health.sock`: Scraped by `neuroos-healthd`.

## Config Keys
- `deadlines.generation_ms`: Generation deadline (default 30,000 ms).
- `deadlines.kernel_check_ms`: Safety check deadline (default 50 ms).
- `deadlines.storage_query_ms`: Evidence query deadline (default 150 ms).
- `ipc.max_frame_bytes`: Maximum frame payload (default 1 MiB).

## Sandbox Paths
- Landlock read-only: `/proc` (for peer checks and health RSS).
- Landlock read-write: Own socket directory (`%t/neuroos/knowledge/`).
- Network: Disabled via systemd `PrivateNetwork=true` and Landlock TCP denial.

## How to Run Alone
```bash
# Run with defaults:
target/debug/neuroos-knowledge

# Run with custom runtime directory:
target/debug/neuroos-knowledge --runtime-dir /tmp/test-neuroos
```
