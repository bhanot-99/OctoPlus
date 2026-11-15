# S-02 part B — checker's verification (Claude)

**Date:** 2026-11-15 · **Checks:** agy's part B run (`reports/spikes/s02-bakeoff-20261115.{json,md}`, raw files in `s02-raw/`, harness `tests/eval/s02-harness/harness.py`) · **Machine:** reference machine (Ryzen 7 5800H, 15 GiB RAM, governor `performance`, AC power), llama.cpp b12389 CPU build · **Read this file first**: it corrects agy's memory result and adds the measurements ADR-0005 needs.

## 1. Quality scores — confirmed

I rescored every raw answer with my own scorer written from `tests/eval/s02/README.md` (not agy's code). The results match agy's exactly:

| Model | Grounded (20) | Honesty (10) | Skill calls (20) | Skill output parsed |
| :--- | :--- | :--- | :--- | :--- |
| Qwen3-4B-Instruct-2507 | 20 | 9 | 7 | 20 |
| Phi-4-mini-instruct | 19 | 7 | 3 | 18 |
| SmolLM3-3B | 19 | 3 | 3 | 19 |

Two caveats about the test, not the harness:

- **The skill-call test is weaker than intended.** The system prompt (`tests/eval/s02/system_prompt.txt`, written by Claude) lists no skills and no argument names. The grammar forces a valid skill name, but the model has to guess which skill and which argument keys from the names alone (e.g. it called `memory.forget` for "remember my bike lock code"). The ranking is still informative; the absolute score is not. The rules forbid editing a held-out set after the run, so this is recorded, not fixed. Phase 2 must put skill descriptions and argument names into the cached persona block and re-run this set.
- **SmolLM3 answered in "thinking" mode** (it writes `<think>…</think>` by default; 9 of its 20 chat answers contain it, and it used up the token limit on several honesty items). It was run under the same rules as the others, so the score stands, but it is not SmolLM3's best setting.

## 2. Memory — agy's "all three fail the cap" is wrong for the shipped setting

agy measured peak RSS with llama.cpp's default **weight repacking on**. Repacking builds a second, CPU-friendly copy of the weights in ordinary memory while the memory-mapped file pages are still resident, so RSS counts the weights roughly 1.6 times. I reproduced agy's numbers and then measured with repacking off (`-nr`), same prompt, `-c 2048 -b 512`, 32 new tokens, `/usr/bin/time -v`:

| Model | Repacking on (default) | Repacking off |
| :--- | :--- | :--- |
| Qwen3-4B-Instruct-2507 | 4,440 MiB (agy: 4,441) | **2,770 MiB** |
| Phi-4-mini-instruct | 3,948 MiB (agy: 3,948) | 2,732 MiB |
| SmolLM3-3B | 3,350 MiB (agy: 3,350) | 2,081 MiB |

`--no-mmap` failed to start (exit 1) on this build and was not used.

## 3. Speed — repacking off is also faster

`llama-bench`, Qwen3, 8 threads, `-b 512`, 3 repetitions:

| Repacking | Prefill pp512 (tok/s) | Decode tg128 (tok/s) |
| :--- | :--- | :--- |
| on | 73.95 ± 0.26 | 11.46 ± 0.02 |
| off | **90.02 ± 0.31** | 11.88 ± 0.14 |

agy's 8-thread figures (77.2 prefill, 12.0 decode, repacking on) agree within the run-to-run spread. 16 threads is slower than 8 for every model (agy's numbers), so C4 uses 8.

**Prefill after a cached prefix** (what C4 does each turn: the persona block is reused), Qwen3, repacking off, `-d 192`:

| Uncached part | Prefill (tok/s) | ms/token |
| :--- | :--- | :--- |
| 80 tokens | 77.41 ± 0.89 | 12.9 |
| 320 tokens | 86.74 ± 0.72 | 11.5 |
| decode at depth 192 | 11.56 ± 0.09 | 86.5 |

**Time to first token** on the real grounded item g01 (268 prompt tokens including the system prompt, nothing cached), repacking off, 3 runs: 3,029 / 3,058 / 3,137 ms. The same question with no system prompt (73 tokens): 986 ms. agy's 3,572 ms was with repacking on.

## 4. Not measured

- **GPU (Vulkan):** the Vulkan build needs the `spirv-headers` package. Installing it needs the owner's sudo password, which Claude does not have (checked: `sudo -n` asks for a password). Every GPU item in S-02 is "not measured".
- **The owner's blind chat grading:** the answers are on a private grading page; the result is a confirmation check for ADR-0005 (see there).

## 5. Harness issues found (BUG-015)

- The report's "Total Run Time: 1.53 s" is the final pass that only re-read cached raw files, not the measurement time (about 50 minutes by agy's own log).
- The report says apt installs are "strictly unapproved"; in fact the owner approved the S-02 work, and the blocker is the sudo password.
- Weight repacking was left at the default. Fixed by Claude: the harness now passes `-nr` / `--repack 0` everywhere, so a re-run measures the shipped setting.
