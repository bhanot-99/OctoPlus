# Phase 4 — Storage (C3): Report

**Date:** 2026-11-17 · **Commit:** see `git log` on `phase-4-storage` · **Builder:** Claude (most of it), agy (vector scan, fusion, spool parser/chunker) · **Checker:** Claude; agy for the read-only review
**Status:** Signed off 2026-11-17

## 1. Summary

C3 is built: one SQLite store (WAL, text index, secure delete), a single writer thread, a reader pool, an exact in-memory vector scan fused with text search, the four-stage ingest filter fed live from C1, spool ingest, lifecycle jobs on systemd timers, and forget/purge that reach the backups. The real embedding model loads inside the sandboxed unit and a real subscriber sees C1 data in C3. Everything is measured with the real model. One thing is not done as the plan wanted: no real recordings exist yet, so KPI-8 ran on synthetic dumps (owner-approved).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P4-S01 | Noise never becomes entities | Done | Claude | agy review | `noise_gate_test`, `filter_test`; negative control per stage |
| P4-S02 | Right domain, right taint | Done | Claude | agy review | `filter_test`, `lifecycle_test` |
| P4-S03 | Relevant chunks within budget | Done | Claude, agy (W2, W3) | Claude | `vectors_test`, `fusion_test`, `query_test`; §4 |
| P4-S04 | Focus at a moment, activity range | Done | Claude | agy review | `query_test`; review F8 left (§6) |
| P4-S05 | Health signals | Done | Claude | agy review | `query_test` |
| P4-S06 | Spool stored untrusted | Done | agy (parser), Claude (ingest) | Claude | `spool_test`, `spool_ingest_test` |
| P4-S07 | Retention, cap, backups | Done | Claude | agy review | `lifecycle_test` |
| P4-S08 | Re-embed without downtime | Done | Claude | agy review | `lifecycle_test`, `embed_test` |
| P4-S09 | Forget and purge | Done | Claude | agy review | `lifecycle_test` (live DB and backups, raw-byte checks), `review_fixes_test` |

## 3. Exit criteria

| Criterion | Met? | Evidence |
| :--- | :--- | :--- |
| Noise gate passes on all dumps | Yes, on **synthetic** dumps only | `noise_gate_test`, `tests/fixtures/monitor-dumps/synthetic-{1,2,3}.dump`; real recordings deferred to integration (owner decision) |
| Latency and RSS within budget, measured per rules §9 | Yes | §4 |
| Crash consistency; backup restore end to end | Yes | `fi_test` (SIGKILL on the real binary), `lifecycle_test` restore tests |
| Forget verified including the next backup | Yes | `lifecycle_test` forget tests scrub both |
| Every ingest-filter and taint decision tested | Yes | `filter_test`, proptests; 540 tests in the workspace |
| Phase report; owner sign-off | Report here; sign-off pending | — |

## 4. Measurements

All with the real model (bge-small-en-v1.5), release build, 30,000 chunks, governor `performance`, load average about 8 (other work was running), 2026-11-17.

| Measure | Result | Target | File |
| :--- | :--- | :--- | :--- |
| Hybrid query, warm (n=300) | p50 19.3 ms, p99 22.0 ms | 25 / 50 | `reports/bench/p4-search-1794856837.json` |
| First queries after a 2,000-chunk burst (n=30) | p99 21.9 ms | 50 | same |
| With a writer committing continuously (n=200) | p50 20.1 ms, p99 23.1 ms | 25 / 50 | same |
| Embedding call alone | p50 4.7 ms | — | same |
| Process RSS, full cap with model | 309 MiB (234 before data) | restated to 320 | same |
| Real unit RSS after start | about 235 MiB; cgroup peak 208 MiB at idle | 320 / 400 MaxMem | `systemctl`, cgroup |
| Soak: 4 h synthetic session at 10× (24.8 min, 6,730 events, 25 samples) | RSS 241 → 244 MiB, growth 1.17 % after minute 5; p99 5.8–8.8 ms | growth < 3 % | `reports/bench/p4-soak-1794861305.json` |
| Test coverage, `neuroos-storage` | 83.85 % lines | project gate | `just ci` |

Caveat: in the soak the chunk cap (lowered to 400) was never reached (135 chunks stored), because the filter keeps few chunks. The cap is tested separately in `lifecycle_test`; the soak does not prove it under load.

## 5. Gate run

`just ci`: ALL GREEN (540 Rust tests, 2 skipped; C++ 59/59), run on the final commit of the branch (after the review fixes), exit 0.

## 6. Deviations, bugs and descoped items

- KPI-8 on synthetic dumps, not recordings (owner-approved; recordings deferred to integration, D-26).
- Disk-full fault injection was attempted with a private tmpfs namespace and not completed; the disk-full behaviour (ingest holds batches, queries served) is covered by a unit test only. Owner may want a real run at integration.
- agy review: 9 findings. F1 (live state survived forget), F2 (in-memory counters resurrected forgotten history), F3 (explicit chunk delete in source_gone), F4 (missing runtime library refused), F5 (min_score no longer drops text-only hits), F6 (reader pool leak on panic), F7 (promoted counters bounded), F9 (stale flag from returned evidence) fixed with tests where practical. **F8 not fixed**: a query for an instant just before the still-open window ignores that window's start edge (minor).
- Bugs: BUG-056, 057, 058 in `BUGS.md`.
- The pipeline IT's entity count depends on how far the second replay got, so its bound is loose (3–200).

## 7. Decisions and ADRs

ADR-0012 (items 1–15), proposed; the owner may overturn at sign-off. C3 budget restated to 320 MiB (hard limit 400 MiB; total 3,875 MiB). NFR-PERF-04 measured and written into the PRD.

## 8. Next phase: risks and first steps

Phase 5 (C5): the knowledge side must treat C3 results by taint and handle `STALE_INDEX`. Real recordings should be run through the KPI-8 gate at integration.

## 9. Sign-off

Owner decisions waiting: ADR-0012 (items 11–15 especially, budget change), synthetic-only noise gate, F8 left open.

- [x] Owner sign-off — owner, 2026-11-17 (ADR-0012 accepted; F8 and synthetic-only gate accepted as listed)
