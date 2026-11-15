# Phase 0 — Foundation and Spikes: Report

**Date:** 2026-11-15 · **Commit:** see §5 · **Builders:** Claude (P0-S01…S06, while agy was off at the owner's instruction), agy (fixes and all spikes) · **Checker:** agy (independent review of P0-S01…S06), Claude (every agy result re-run or re-measured)
**Status:** Signed off by the owner 2026-11-15

## 1. Summary

Phase 0 set out to build a repository where anything can be built, tested and sandboxed from day one, and to answer the five riskiest questions by measurement. The foundation is complete and green locally and on hosted CI. All five spikes are done and ADR-0001…0008 are accepted. The model bake-off chose Qwen3-4B-Instruct-2507 and showed that the CPU cannot reach the original 3-second voice target; the CPU targets were restated (ADR-0005). The GPU half (Vulkan on the RTX 3050) is 12–14× faster at prefill with the same quality and works inside the hardened sandbox, so the GPU path is enabled on AC power (ADR-0008 addendum).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P0-S01 | `just ci` green on a clean clone; hosted CI green | Done | Claude | agy + Claude | `just ci` ALL GREEN on every branch; hosted CI runs green from `p0/review-fixes` onward |
| P0-S02 | Contracts usable from Rust and C++ | Done | Claude | agy + Claude | `crates/neuroos-proto/tests/contract.rs`: every Envelope body round-trips byte-identically; negative control fails it |
| P0-S03 | Socket server and client library | Done | Claude (+ agy fixes) | agy + Claude | Rust and C++ IPC, 45 + 41 tests, ASan/UBSan/TSan clean, cross-language stream CT; BUG-001…007 fixed |
| P0-S04 | Health endpoint with real status | Done | Claude | agy + Claude | `neuroos-health`, `libneuroos/health`; STARTING → worst subsystem; probes at scrape time |
| P0-S05 | Network-crate ban | Done | Claude | agy + Claude | `deny.toml`; `scripts/check-netban.sh` fails when the ban is removed |
| P0-S06 | Model fetch and verify | Done | Claude (+ agy fixes) | agy + Claude | `scripts/fetch-models.sh`, 16-check test, tampered file is fatal |
| P0-S07 | S-01 storage scan | Done | agy | Claude (rerun) | ADR-0004; `reports/bench/s01-*` |
| P0-S08 | S-02 model bake-off | Done | agy (CPU) + Claude (pins, GPU) | Claude (rescored, re-measured) | ADR-0005, ADR-0008 addendum; `reports/spikes/s02-*`, read `s02-verification-claude.md` and `s02-gpu-claude.md` first |
| P0-S09 | S-03 streamed frames | Done | agy | Claude (rerun) | ADR-0003; `reports/bench/s03-*` |
| P0-S10 | S-04 unit hardening, socket isolation | Done | agy (+ Claude) | Claude (re-measured) | ADR-0002; `reports/spikes/s04-*` |
| P0-S11 | S-05 health and context sensors | Done | agy | Claude (rerun) | ADR-0007; `reports/spikes/s05-*` |

Shared libraries built (phases.md §3.1 item 6): `neuroos-common`, `-proto`, `-ipc`, `-health`, `-taint`, `-sandbox`, `-testkit`, and `cpp/libneuroos` with the same IPC and health behaviour.

## 3. Exit criteria

| Criterion (phases.md §3.4) | Met? | Evidence |
| :--- | :--- | :--- |
| `just ci` green on a clean clone; hosted CI green | Yes | local gate on every commit; hosted CI (format, lint, build, tests, deny) green |
| Contracts generate and pass round-trip tests | Yes | contract test, both directions |
| `neuroos-ipc` coverage gate met; every decision in framing and peer checking tested | Yes | Rust 90–96 % lines overall (ipc ≈ 95 %), C++ 96 %; table-driven framing and peer-policy tests in both languages |
| Spikes S-01…S-05 complete; ADR-0001…0008 accepted | Yes | 0001–0004, 0006, 0007 accepted by the owner 2026-11-15; 0008 2026-11-14 with S-04 and S-02 addenda; 0005 accepted 2026-11-15 under the owner's standing approval (D-18) |
| PRD §6 updated from spike results | Yes | NFR-PERF-04, -11 measured; NFR-PERF-02/03 restated for CPU; C4 budget measured; total restated (ADR-0005) |
| Models fetched and verified | Yes | `just fetch-models --verify-only`: Qwen3-4B, SHA-256 checked; manifest holds only the chosen model |
| Phase report written; owner sign-off | Report yes; sign-off pending | this file |

## 4. Measurements (headline; full conditions in each ADR and JSON)

| What | Result | Target | Source |
| :--- | :--- | :--- | :--- |
| Token frame C4 → C5 added latency | p99 ≈ 0.58 ms (both runs, plain and hardened unit) | ≤ 1 ms | ADR-0003 |
| Cancel to end frame | p99 < 0.1 ms | — | ADR-0003 |
| C3 hybrid query at 30,000 chunks, warm | ≈ 13 ms p50, ≈ 16 ms p99 (pessimistic harness) | ≤ 25 / ≤ 50 ms | ADR-0004 |
| Same, with a writer committing | p99 40–47 ms | ≤ 50 ms | ADR-0004 (Phase 4 risk) |
| C3 RSS without embedding model | ≈ 205 MiB steady, 250 MiB peak | 240 MiB incl. model | ADR-0004 |
| Sensor reads | 0.25–14 ms per read; all work inside the hardened unit | — | ADR-0007 |
| Model quality, Qwen3-4B (held-out sets) | grounded 20/20, honesty 9/10, skill calls 7/20 (grammar output always parsed) | — | ADR-0005 |
| Model speed, CPU, 8 threads | prefill ≈ 87–90 tok/s, decode ≈ 11.6–11.9 tok/s | — | ADR-0005 |
| C4 RSS, one turn | 2,770 MiB (weight repacking off) | ≤ 3,000 MiB cap | ADR-0005 |
| Voice wait on CPU (computed from measured rates) | ≈ 4.2 s | was 3.0 s; now ≤ 4.5 s p50 (CPU) | ADR-0005 |
| GPU path (Vulkan, RTX 3050) | prefill ≈ 12–14×, decode ≈ 4×, first token 295 ms, VRAM 3,010 MiB; same quality; works in the hardened unit | voice wait ≈ 1.3 s computed, inside the 3.0 s goal | ADR-0008 addendum |

## 5. Gate run

`just ci` on 2026-11-15 on branch `p0/s08-spike-s02` (the tip of the Phase 0 branch stack), with the S-02 work in place: **ALL GREEN, exit 0**. It covers format, lint (clippy and clang-tidy), cargo-deny with the network ban, Rust tests, the model-fetch test, coverage (Rust 90.0 % lines, gate 80 %; C++ 96.0 %, gate 70 %), and C++ tests under ASan/UBSan and TSan (41/41 each). The commit is the one that adds this report; hosted CI runs on push. The owner merges the stack into `main` (fast-forward to this branch).

## 6. Deviations, bugs and descoped items

- P0-S01…S06 were built by Claude alone while agy was unavailable (owner's instruction, D-15); the independent check came afterwards (agy review, 10 findings, 9 real, all fixed and re-verified).
- Bugs: BUG-001…016 all fixed. BUG-011…015 were in spike harnesses (no product code) and were fixed in the post-sign-off cleanup: S-04 hard-coded verdicts, probe raw FFI, S-05 title hashes, S-01 double-counted total, S-02 harness run-time label.
- agy's reports overstated or invented results four times (R-09, four S-04 claims, missing `just ci` in S-01, wrong GPU conclusion). Every result in an ADR was re-measured by Claude.
- `archive/v1-docs/` removed at the owner's request (D-14).
- **S-02 deviations:**
  - The GPU (Vulkan) half first waited on `spirv-headers` (it needs the owner's password). The owner installed it, and Claude then measured it.
  - The skill-call test prompt had no skill descriptions (Claude's test-design gap), so the 7/20 is a lower bound. It is re-run in Phase 2.
  - agy's memory result ("every model over the cap") was an artefact of weight repacking (BUG-015), corrected by Claude's re-measurement.
- **Owner decisions taken under standing approval (D-18):** model, 384-token layout, and new CPU latency targets (NFR-PERF-02 ≤ 4.5 s p50, NFR-PERF-03 ≤ 3.0 s p50). The owner may overturn these at sign-off.
- **Blind chat grading** was delegated by the owner to Claude. Qwen3 scored best (4.42 of 5, against Phi 4.00 and SmolLM3 3.45), so ADR-0005 stands (ADR-0005 decision 6).
- **GPU half done after the owner installed `spirv-headers`** (see §4 and the ADR-0008 addendum). Built and measured by Claude, not agy, to finish quickly.

## 7. Decisions and ADRs

| ADR | Title | Status |
| :--- | :--- | :--- |
| 0001 | Record decisions; v2.0 documents are the baseline | Accepted 2026-11-15 |
| 0002 | Unit mode and socket-map enforcement | Accepted 2026-11-15 |
| 0003 | IPC: sockets, framing, streaming | Accepted 2026-11-15 |
| 0004 | Storage: SQLite, FTS5, exact scan | Accepted 2026-11-15 |
| 0005 | Model, prompt layout, latency and memory targets | Accepted 2026-11-15 (D-18) |
| 0006 | Build order | Accepted 2026-11-15 |
| 0007 | Health and context sensor sources | Accepted 2026-11-15 |
| 0008 | Optional GPU acceleration for C4 | Accepted 2026-11-14; S-04 and S-02 addenda: Vulkan, GPU path enabled on AC |

## 8. Next phase: risks and first steps

- **Concurrent read/write latency in C3** (ADR-0004): Phase 4 mitigations and a p99 ≤ 50 ms test.
- **System D-Bus** hidden in every unit (ADR-0002); C1 uses `/sys` and Wayland only (ADR-0007).
- **GPU path:** measured and enabled. Phase 2 must test the VRAM-short fallback and the restart on a mid-turn GPU failure; power is not measurable with `nvidia-smi` on this laptop.
- **Latency (PRD R-01):** the CPU voice wait is ≈ 4.2 s. Phase 2 measures it end to end against the restated targets.
- **Memory headroom:** 3,795 of 4,000 MiB is budgeted; llama.cpp must run with weight repacking off.
- Phase 1 (walking skeleton) first steps: unit files generated from the socket map with the ADR-0002 additions; each part a thin real service with health; `neuroosctl status`.

## 9. Sign-off

- [x] Owner sign-off — owner (bhanot-99), 2026-11-15 ("sign off approved")
