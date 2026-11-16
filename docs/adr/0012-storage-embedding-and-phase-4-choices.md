# ADR-0012 — Storage: embedding stack and Phase 4 choices

**Status:** Proposed (taken by Claude under the owner's Phase 4 go-ahead, D-28; the owner may overturn at sign-off)
**Date:** 2026-11-16
**Changes:** PRD §6.2 (C3 budget restated from measurement), PRD §10 (embedding model pinned), `proto/neuroos/v1/storage.proto` (two additive fields), `models/manifest.toml` (three entries, new role `runtime`), Architecture §7 (spool format v1, per-message peer checks)
**Evidence:** `reports/phase-04-storage.md` and `reports/bench/p4-*.json` (filled in as measured)

---

## Context

Phase 4 builds C3: filtered ingest, one SQLite database, exact vector scan (ADR-0004) and the single embedding model (FR-STO-03). ADR-0004 left the model, its memory cost and the cost of the embedding call open. The owner approved downloading the model and runtime before going to sleep (2026-11-16). Cargo-deny and R0-1 constrain the Rust stack: `fastembed` pulls `hf-hub` and an HTTP client; `tokenizers` 1.0.0-rc.4 pulls `ptr_hash`, which brings in `xxhash-rust` (BSL-1.0, not on the allowlist) and the unmaintained `fxhash` (RUSTSEC-2025-0057).

## Decision

The ten decisions in `docs/plans/phase-04-storage-plan.md` §1 stand as this ADR's decision text, with this change to decision 1: **no `tokenizers` crate either.** The model's tokenizer is BERT WordPiece (lower-casing, accent stripping, punctuation split, `##` continuations), about 150 lines read from the pinned `tokenizer.json`, tested against known token ids. `ort` is used with `load-dynamic` and without its download or TLS features; its tree contains no HTTP or TLS crate (checked with `cargo tree`), and `cargo deny` passes without a new exception.

Pinned: `bge-small-en-v1.5` (MIT; repo commit `5c38ec7c…`, ONNX sha256 `828e1496…` equals the repo's LFS oid), its `tokenizer.json`, and `onnxruntime-linux-x64-1.31.1` (MIT, sha256 `c849e36a…`).

## Consequences

- C3 loads about 130 MB of weights on top of the vector matrix, so its RSS rises above the Phase 1 estimate; the real number goes into PRD §6.2 and may raise `MemoryMax` (decision 10).
- The embedding call is on every ingested chunk and every query; its latency is measured and reported separately from the scan (NFR-PERF-04).
- Without the library or model C3 still starts: it ingests text, serves full-text search and reports DEGRADED with `stale_index`.
- A hand-written tokenizer is more code to own but removes a licence exception and an unmaintained dependency.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| `fastembed` | Brings `hf-hub` and an HTTP client into a part that must have no network code (R0-1). |
| `tokenizers` crate | Fails cargo-deny (BSL-1.0, unmaintained `fxhash`); either an exception or an ignore entry would need the owner's approval. |
| `ort` with `download-binaries` | Downloads at build time from an unpinned CDN (R0-9). |
| llama.cpp embeddings in C4 | C3 must own the embedding model (AP-3, FR-STO-03). |
| A smaller, quantised model | None is published for this revision; revisit if the measured memory does not fit. |
