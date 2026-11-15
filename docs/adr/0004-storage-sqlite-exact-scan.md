# ADR-0004 — Storage: one SQLite database, FTS5, exact in-memory vector scan

**Status:** Accepted by the owner 2026-11-15
**Date:** 2026-11-15
**Changes:** PRD §6.1 NFR-PERF-04 (label to measured) · Architecture §7.2 (confirmed; read/write note added below)
**Evidence:** spike S-01 (P0-S07): `reports/bench/s01-storage-scan-20261114-run1-agy.json` (agy) and `…-run2-claude.json` (independent rerun by Claude). Harness: `bench/storage-scan`, `scripts/bench-s01.sh` (`just bench-s01`).

---

## Context

Decision D-04 and Architecture §15 #1 chose one SQLite database (WAL) holding metadata, chunk text, an FTS5 index and the vectors as blobs, with an exact in-memory dot-product scan instead of a vector database — the prototype's vector database cost 123,000 lines of patched code, a banned network dependency and a stale index (docs/lessons-from-v1.md §4). S-01 had to show the hybrid query meets NFR-PERF-04 (≤ 25 ms p50, ≤ 50 ms p99) at the 30,000-chunk cap and what C3 weighs.

**What was measured.** Synthetic data, seeded: 384-dim L2-normalised f32 vectors (46 MB at the cap, matching PRD §6.2) and 300–800-character chunks. SQLite from rusqlite 0.42 "bundled" (FTS5 checked at start), WAL, `synchronous=NORMAL`, FTS5 with external content. Query path, without the embedding call: exact scan → top 50; FTS5 `MATCH` with `bm25()` → top 50; Reciprocal Rank Fusion (k = 60) → top 5; fetch those rows. 1,000 warm queries plus the lessons-from-v1 cases: cold (first 10 after open and load), post-burst (100 queries right after inserting 2,000 chunks), and concurrent (1,000 queries while a writer commits every 50 rows). Release build. **Negative control:** a 30 ms sleep in the query path must push p50 above 25 ms — it did (43.2 and 43.1 ms).

**Conditions.** Ryzen 7 5800H, kernel 7.2.15, governor `performance`, ~2 GB free RAM, 2026-11-14 23:21 UTC (run 1, load 1.13) and 23:30 UTC (run 2, load 0.53), about 1 minute per run.

**A measurement caveat.** The harness's "total" times both the single-thread and the 4-thread scan in every query, so it is pessimistic by one scan (BUG-014). The single-thread path is "total minus the 4-thread scan". The harness was fixed on 2026-11-15; the figures below are from the original runs.

At the cap (N = 30,000), run 1 / run 2:

| Case | p50 (ms) | p99 (ms) |
| :--- | :--- | :--- |
| Warm, reported total (double-scan) | 12.9 / 13.0 | 15.3 / 16.3 |
| Warm, single-thread path (derived) | ≈ 10.5 / 10.6 | — |
| Scan only, 1 thread / 4 threads | 8.0 / 2.5 | 8.8 / 3.1 (run 1) |
| FTS5 only | 2.4 / 2.4 | 4.4 / 4.8 |
| Fuse + fetch | < 0.05 | < 0.1 |
| Cold, first 10 queries | 9.7 – 12.6 each | — |
| Post-burst (after 2,000 inserts) | 10.9 (run 1) | 12.9 / 13.8 |
| **Concurrent with an active writer** | 18.2 / 19.6 | **39.6 / 47.2** |

Smaller sizes scale linearly (N = 5,000: total p50 ≈ 2.5 ms; N = 15,000: ≈ 6.8 ms). Build of 30,000 chunks: 1.2 s, DB 66 MB (+5.5 MB WAL); loading the vector matrix: 95 ms.

Memory at the cap, benchmark process, **without an embedding model**: about 205 MiB steady, 250 MiB peak (while building). PRD §6.2 budgets C3 at 240 MiB including the embedding model, with a 320 MiB hard limit.

## Decision

1. **Keep the design:** one SQLite database in WAL mode, FTS5 for text, vectors as blobs loaded into one contiguous in-memory matrix, exact dot-product scan, RRF fusion. No second storage engine, no approximate index.
2. **NFR-PERF-04 is met** with headroom for warm, cold and post-burst queries (p99 ≈ 16 ms against 50 ms). PRD §6.1 is labelled measured, citing this ADR.
3. **Reads must not queue behind writes.** With a writer committing continuously, p99 rose to 40–47 ms, close to the 50 ms limit. Phase 4 therefore: (a) does query work on a reader connection and on a thread pool, never on the writer thread; (b) batches ingest commits and yields between batches; (c) uses the 4-thread scan (2.5 ms vs 8 ms) for queries; (d) re-measures the concurrent case as a Phase 4 performance test with a pass mark of p99 ≤ 50 ms.
4. **Memory is not settled.** Without the embedding model C3 already sits near its 240 MiB budget at peak. The C3 budget is restated once the embedding model is chosen and measured in Phase 4 (PRD §6.2 stays "T" until then). Peak during bulk build can be lowered by smaller transactions and `cache_size`.

## Consequences

- No vector-database dependency tree; the whole storage path is SQLite plus ~1,000 lines of our own code.
- The scan is the largest cost and grows linearly with the cap; the 30,000 cap (PRD §6.2) keeps it under 10 ms even on one thread.
- The concurrent-write case is the real risk and now has a named mitigation and a pass mark in Phase 4.
- Not measured: the embedding call itself, real (non-synthetic) text distributions, the 4-hour soak. Recorded as open.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| A vector database (HNSW etc.) | The prototype's experience (lessons-from-v1 §4); unnecessary at 30,000 vectors where an exact scan takes ~8 ms on one core. |
| sqlite-vec / a vector extension | Another native dependency to pin and audit for a scan we can write in a few dozen lines; may revisit if the cap grows by 10×. |
| Vectors in a separate flat file | Two stores to keep consistent on "forget" (FR-STO-13); SQLite keeps rows, text and vectors in one transaction. |
