# ADR-0005 — Language model, prompt layout, latency and memory targets

**Status:** Accepted 2026-11-15 under the owner's standing approval to finish Phase 0 (D-18); confirmed by the owner's Phase 0 sign-off the same day. Chat confirmation done (decision 6).
**Date:** 2026-11-15
**Changes:** PRD §6.1 NFR-PERF-02, NFR-PERF-03 · PRD §6.2 C4 and Total rows · Architecture §9.1, §9.2 · `models/manifest.toml` (only the chosen model stays) · ADR-0008 (GPU not measured)
**Evidence:** spike S-02 (P0-S08). Part A: `reports/spikes/s02a-pin-20261115.{json,md}` (pins, perplexity, backend tests). Part B: `reports/spikes/s02-bakeoff-20261115.{json,md}` and `s02-raw/` (agy) and **`reports/spikes/s02-verification-claude.md`** (checker's rescore and re-measurement — read first; it corrects agy's memory result). Held-out sets: `tests/eval/s02/` (committed in `7b41c17` before any model ran). Harness: `tests/eval/s02-harness/harness.py`, `scripts/bakeoff-s02.sh` (`just bakeoff-s02`).

---

## Context

C4 runs one local language model for grounded answers, honest "I don't know"s, skill calls under a grammar and everyday chat (PRD §5.8). The model must fit the C4 cap of 3,000 MiB (PRD §6.2), have an acceptable licence, and be fast enough on the CPU alone (ADR-0008 decision 1). Four candidates were approved (D-17) and run on upstream llama.cpp b12389, 4-bit, CPU, temperature 0, same system prompt.

Results (rescored and re-measured by the checker; repacking off = the shipped setting):

| | Qwen3-4B-Instruct-2507 | Phi-4-mini-instruct | SmolLM3-3B | BitNet b1.58 2B4T |
| :--- | :--- | :--- | :--- | :--- |
| Perplexity at pin time | 15.55 | 18.33 | 19.56 | 101.23 — **excluded** |
| Grounded answers (20) | **20** | 19 | 19 | — |
| Honest "don't know" (10) | **9** | 7 | 3 | — |
| Skill calls right (20) / parsed | **7** / 20 | 3 / 18 | 3 / 19 | — |
| Prefill / decode, 8 threads (tok/s) | 90.0 / 11.9 | 89.7 / 12.2 ¹ | 98.6 / 15.8 ¹ | — |
| Peak RSS, one turn, `-c 2048` | 2,770 MiB | 2,732 MiB | 2,081 MiB | — |
| Licence | Apache-2.0 | MIT | Apache-2.0 | MIT |

¹ agy's run with repacking on; Qwen3 was re-measured with it off (prefill 74 → 90 tok/s). Phi and SmolLM3 were not re-measured for speed, since they lose on quality.

Two findings changed the plan:

- **Memory:** llama.cpp's default weight repacking keeps a second copy of the weights, so every model looked over the cap (Qwen3 4,440 MiB). With repacking off, Qwen3 uses 2,770 MiB and is *faster* at prefill.
- **Latency:** CPU prefill is about 11.5 ms per token. The provisional layout (Architecture §9.2) leaves ~384 tokens uncached per turn. Computed: 384 × 11.5 ms = 4.4 s prefill + first sentence (12 tokens × 86.5 ms ≈ 1.0 s) + transcription 0.15 s + speech 0.5 s ≈ **6.1 s**, against the NFR-PERF-02 target of 3.0 s. The 3.0 s target cannot be met on this CPU with this layout and any of these models.

## Decision

1. **Model: Qwen3-4B-Instruct-2507, Q4_K_M** (unsloth GGUF, revision `a06e946…`, SHA-256 `3605803b…`, Apache-2.0). It wins every automatic test and fits the cap. It is the only model left in `models/manifest.toml`; the other pins stay in the part A report for re-runs.
2. **C4 runtime settings:** llama.cpp b12389, CPU, 8 threads, weight repacking **off** (`--repack 0`), context 2,048, batch 512, the persona block's KV state kept and reused between turns.
3. **Prompt layout, 384 tokens** (replaces the provisional 512):

   | Block | Tokens | Cached between turns |
   | :--- | :--- | :--- |
   | Persona, rules and skill list | 128 | yes — always first |
   | Personal facts | 48 | usually |
   | Focus context | 32 | no |
   | Session history (incl. question) | 48 | partly |
   | Evidence | 128 | no |

   About 208 tokens are uncached per turn. Truncation rules are unchanged (exact, never splitting the untrusted-document wrapper). Phase 2 must check that the persona block, *including skill descriptions with argument names*, fits 128 tokens. If it does not, the extra goes into the cached part, which costs memory, not latency.
4. **Latency targets, CPU path** (computed from the measured rates; measured end to end in Phase 2):
   - NFR-PERF-02, end of speech to first answer audio: **≤ 4.5 s p50, ≤ 6.0 s p95** (208 × 12 ms ≈ 2.5 s + 1.0 + 0.15 + 0.5 ≈ 4.2 s).
   - NFR-PERF-03, typed question to first token: **≤ 3.0 s p50**.
   - The original 3.0 s and 2.0 s stay as the **GPU-path goals** (ADR-0008), not yet measured. The preamble (NFR-PERF-01) covers the wait in both cases.
5. **Memory:** C4 budget **2,800 MiB**, hard limit (`MemoryMax`) **3,220 MiB** (+15 %). The other parts' budgets add up to 995 MiB, so the **total is 3,795 MiB, within ≤ 4,000 MiB**. Little headroom remains; C2 (620 MiB, label P) and C3 (restated in Phase 4) are the parts to watch.
6. **Chat quality confirmation — done.** The owner delegated the blind grading to Claude ("do binding grade"). Claude scored all 60 answers (1–5 on helpful, honest, safe) from the answers file before opening the key; the labels for c01–c04 had been seen earlier, so those four are not fully blind. Result (`reports/spikes/s02-chat-grades-20261115.json`):

   | Model | Helpful | Honest | Safe | Overall |
   | :--- | :--- | :--- | :--- | :--- |
   | **Qwen3-4B** | **4.20** | **4.25** | 4.80 | **4.42** |
   | Phi-4-mini | 3.05 | 4.15 | 4.80 | 4.00 |
   | SmolLM3-3B | 2.10 | 3.65 | 4.60 | 3.45 |

   Qwen3 is first, so the choice stands. Weak spot for every model: c15 (ibuprofen with blood-pressure pills), where each called it "generally safe" before adding a caveat. Phase 2 adds this item and similar medicine-interaction questions to the safety tests (KPI-12).
7. **Skill calls are a Phase 2 acceptance test.** With the grammar, Qwen3's output always parsed (20/20). The low score (7/20) came from a test prompt with no skill descriptions (verification §1). Phase 2 re-runs `tests/eval/s02/skill_calls.jsonl` with the real persona block. Proposed pass mark: **≥ 18/20 right skill and argument keys** (owner to confirm at sign-off).

## Consequences

- One ~2.5 GB model file. RSS fits; there is no room for a second resident model in C4.
- The voice wait on CPU is about 4–4.5 s, not 3 s. The product's feel (PRD R-01) now depends on the preamble and on the GPU path (ADR-0008), which S-02 could not measure.
- The 384-token layout carries less evidence (128 tokens, about one or two chunks). C5's ranking quality matters more; Phase 4 checks that answers stay grounded with this budget.
- 16 threads is slower than 8 on this CPU, so C4 pins 8 and leaves the other cores to C2 and C3.
- Not measured: the GPU path; quality at the 384-token layout (S-02 used short single-turn prompts); a long soak of RSS growth; Qwen3 with a full skill list in its prompt.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| SmolLM3-3B (smaller, ~30 % faster decode) | Honesty 3/10 under the same prompt (thinking mode on by default); the saving does not reach the 3.0 s target anyway. Could be re-tried with thinking off if latency becomes the bigger problem. |
| Phi-4-mini-instruct | Lower on every quality test, same speed and memory as Qwen3. |
| BitNet b1.58 2B4T | Failed the pin-time correctness check (perplexity 101 vs 15–20). |
| Keep the 512-token layout | ≈ 6.1 s voice wait on CPU. |
| Keep repacking on | +1.6 GB RSS (over the cap) and slower prefill on this build. |

## Addendum 2026-11-22: 8-bit K cache

Measured on the installed stack (8 fixed questions, C4 restarted for each set, greedy decoding). At `-c 2048` the f16 cache peaked at 2,788 MiB (2,813 MiB under mixed load), over the 2,800 MiB budget.

| KV cache | Peak RSS | First answer chunk | Answers |
| :--- | --: | :--- | :--- |
| f16 / f16 (before) | 2,788 MiB | 1.0–2.75 s | — |
| q8_0 / q8_0, flash attention on | 2,651 MiB | about +0.4 s each (+18 %) | identical, 8/8 |
| **q8_0 K / f16 V, flash attention off (chosen)** | **2,734 MiB** | **about +0.08 s each (+3 %)** | **identical, 8/8** |

An 8-bit V cache needs flash attention, which slowed prefill on this CPU. The chosen setting leaves 66 MiB of headroom; the 24 h soak re-checks it under load. Pinned by `local_ContextParamsStoreTheKCacheAtEightBits`.
