# Soak run 1 (12 h): storage and voice memory "growth", root causes and fixes

Run: 2026-11-23 21:05 IST to 2026-11-24 09:14 IST, stopped at the 12 h mark (D-61). Verdict was
FAIL on two checks: storage RSS growth 22.8 % and voice RSS growth 578.8 %. Evidence is in
`reports/bench/soak-run1-2026-11-24-first-12h/`. No crashes, no restarts, no MemoryMax hits.

## Finding 1: the soak measured resident memory only (affects both)

The kernel here swaps idle memory to compressed RAM (zram, `vm.swappiness` = 180). A part that
sits idle looks small in `VmRSS` and then "grows" when its pages come back. Proof from the live
processes right after the run:

| Part | VmRSS | VmSwap | Real footprint |
| :-- | --: | --: | --: |
| storage | 160 MiB | 122 MiB | 282 MiB |
| inference | 2,774 MiB | 50 MiB | 2,824 MiB |
| voice (fresh restart) | 735 MiB | 0 MiB | 735 MiB |

**Fix:** `scripts/soak-24h.sh` now also records `swap_mib` per sample and judges the footprint
(resident + swapped). Older CSVs without the column still analyse.

## Finding 2: voice "578 % growth" was not growth

Voice RSS: 42 MiB (start) -> 24 MiB for hours -> one step to 431 MiB at about 3.5 h -> exactly
430.9 MiB flat for the next 8 h of continuous simulated turns. A leak does not stay flat to the
tenth of a MiB. A fresh voice process holds 735 MiB, all of it model memory (Kokoro fp32,
whisper tiny, VAD, wake word), loaded at start. The low early readings were those pages swapped
out. The step was them coming back. Real footprint is about 735 MiB against the 800 MiB budget:
inside it, with about 8 % headroom.

**Fix:** the measurement fix above. No voice code change is justified by the data.

## Finding 3: storage had a real, bounded cost (ONNX Runtime arena)

The storage part's embedder (`OnnxEmbedder`) used ONNX Runtime's default CPU memory arena and
memory pattern. Inputs vary in length, so the arena grows to a high-water mark and keeps it.
Measured with `crates/neuroos-storage/examples/embed_mem.rs` (600 rounds of 1 to 16 texts of 1 to
120 words, real model):

| Setting | After load | After 100 rounds | After 600 rounds |
| :-- | --: | --: | --: |
| default (old) | 227.7 MiB | 304.9 MiB | 307.8 MiB |
| lean (arena and memory pattern off) | 218.7 MiB | 232.6 MiB | 231.8 MiB |

About 76 MiB less, flat. Single short queries (the real case) take 12.1 ms per call in both
settings. Heavy batches of long texts are about 39 % slower with lean (0.93 s against 0.67 s per
round), which only affects bulk ingest. Vectors are identical to the last digit (checked across
two processes). The voice part already uses the same setting for Kokoro.

**Fix:** `OnnxEmbedder::load` now uses the lean session (`load_tuned(.., lean)` keeps the old
behaviour for measuring).

## What is not proven

Storage RSS also rose through the run (127 to 156 MiB resident) while swapped pages came back, so
run 1 cannot say whether the footprint itself drifted. Run 2 (clean, with swap recorded) answers
that.
