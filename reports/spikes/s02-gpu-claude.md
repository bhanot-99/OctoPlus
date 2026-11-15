# S-02 GPU half — Vulkan on the RTX 3050 (Claude)

**Date:** 2026-11-15 · **Builder and measurer:** Claude (the owner installed `spirv-headers`; agy was not used, for speed, at the owner's direction to finish the phase) · **Model:** Qwen3-4B-Instruct-2507 Q4_K_M (ADR-0005) · **Build:** llama.cpp b12389, `just build-llama --vulkan-only` (`cpp/build/llama-vulkan`), success in 154 s · **GPU:** NVIDIA GeForce RTX 3050 Laptop, 4,096 MiB, driver 595.104.02; Vulkan also sees the AMD Radeon iGPU (RADV), which is not used.

## 1. Speed (`llama-bench`, `-dev Vulkan1 -ngl 99 -b 512`, whole model on the GPU)

| Test | GPU (tok/s) | CPU, repacking off (tok/s) | Speed-up |
| :--- | :--- | :--- | :--- |
| Prefill pp512 | 1,248.95 ± 7.30 | 90.02 | ≈ 14× |
| Prefill 320 after a 192-token cached prefix | 1,040.85 ± 6.46 | 86.74 | ≈ 12× |
| Prefill pp80 | 782.59 ± 1.16 | 77.41 | ≈ 10× |
| Decode tg128 | 50.30 ± 0.27 | 11.88 | ≈ 4.2× |
| Decode at depth 192 | 49.71 ± 0.16 | 11.56 | ≈ 4.3× |

**Time to first token**, real grounded item g01 (268 tokens, nothing cached), 3 runs: 294.7 / 295.1 / 296.1 ms (CPU: ≈ 3,030–3,140 ms). The first run after a build took 1,649 ms for 213 tokens while the driver built its shader pipelines; later runs were warm.

## 2. Memory (one turn, `llama-completion -c 2048 -b 512 -ngl 99`, 64 new tokens)

- **VRAM peak:** 3,010 MiB (sampled every 100 ms with `nvidia-smi`; 1 MiB before the run). About 1 GB of the 4 GB stays free for other programs.
- **Host RSS peak:** 2,616 MiB (the model file is read through the page cache to upload it). Inside the C4 budget of 2,800 MiB.

## 3. Quality

The grounded (20) and honesty (10) sets were run on the GPU with the same prompt and settings and scored with the checker's scorer: **grounded 20/20, honesty 9/10, the same as on the CPU.** 21 of the 30 answers are byte-identical to the CPU answers; the others differ in wording, as expected from different float arithmetic. Raw file: `reports/spikes/s02-raw/llm-qwen3-4b-instruct-2507-q4km/gpu-quality-claude.json`.

## 4. Sandbox: compute inside the hardened unit (ADR-0008 decision 6)

`systemd-run --user` with the full Architecture §8.1 baseline plus the S-04 additions: `PrivateNetwork`, `ProtectSystem=strict`, `ProtectHome=tmpfs`, `PrivateDevices=yes`, `MemoryDenyWriteExecute=yes`, the `@system-service` + Landlock syscall filter, `RestrictAddressFamilies=AF_UNIX`, no capabilities, and the system bus and resolver hidden. The repository is bound read-only.

| Devices bound into the private `/dev` | `MemoryDenyWriteExecute` | Result |
| :--- | :--- | :--- |
| none (negative control) | yes | `ggml_vulkan: No devices found` — no GPU reachable |
| `/dev/nvidia0`, `/dev/nvidiactl`, `/dev/nvidia-uvm` | yes | works: pp512 1,256.07, tg32 51.06 |
| **`/dev/nvidia0`, `/dev/nvidiactl`** | **yes** | **works: pp512 1,253.64, tg32 50.92** |
| `/dev/nvidia0`, `/dev/nvidiactl` | no | works: pp512 1,253.45, tg32 50.83 |

- `/dev/nvidia-uvm` is **not** needed.
- Inside the unit only the NVIDIA GPU is visible (the AMD iGPU's render node is not bound), so it is device `Vulkan0` there.
- With `MemoryDenyWriteExecute=yes` the driver logs `JIT session error: Permission denied` once and falls back. Speed is the same as without the stanza, so nothing is lost.
- A real answer generated inside the unit (rice question) is byte-identical to the same answer outside the unit.

## 5. Not measured

- **Power:** `nvidia-smi` reports a constant 6.10–6.11 W on this laptop, in P0 under full load as well as idle. The sensor does not report real draw here, so idle and load power are **not measured**. Battery impact is handled by the existing rule: the GPU path is off on battery by default (ADR-0008 decision 4).
- **VRAM-short behaviour:** the fallback when less than the VRAM budget is free is a C4 feature (Phase 2 test), not a llama.cpp property.
- **Backend operator tests on Vulkan:** see §6.

## 6. Backend correctness (`test-backend-ops -b Vulkan1`)

**22,362 / 22,362 tests passed** on `Vulkan1` (the RTX 3050), exit 0, compared against the CPU backend. Operators the card does not support (bf16 types; the card reports `bf16: 0`) are skipped as "not supported". llama.cpp runs those on the CPU, and the chosen Q4_K_M model does not use bf16.
