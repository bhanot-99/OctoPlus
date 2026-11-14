# ADR-0008 — Optional GPU acceleration for inference (C4)

**Status:** Accepted by the owner 2026-11-14 (D-13). The backend and the enable decision are fixed after spikes S-02 and S-04.
**Date:** 2026-11-14
**Changes:** PRD §2.2, §4, §5.8, §6.2, §11 · Architecture §8, §14, §15, §17 · security.md §9 · phases.md §3, §14

---

## Context

The v2.0 baseline runs everything on the CPU and lists GPU acceleration as a v1.0 non-goal. The reference machine also has a discrete GPU: NVIDIA GeForce RTX 3050 Mobile, 4,096 MiB VRAM, driver 595.104.02 (read with `nvidia-smi` on 2026-11-14).

Prefill speed is the largest part of the voice wait (Architecture §9.1) and the main risk to the product's feel (PRD R-01). A GPU is the strongest available lever on it.

Owner direction: use the GPU only as an improvement to the designed project — speed — with no drawbacks to what is already designed.

## Decision

1. **The CPU path stays the baseline.** Every v1.0 requirement, budget and target is met on the CPU alone. No pass/fail rule depends on the GPU. Minimum hardware does not change.
2. **GPU offload is an optional accelerator for C4 only.** It is off by default, turned on in config, and uses the same model file and the same prompt layout as the CPU path. Voice, embedding and every other part stay on the CPU.
3. **No-regression rule.** The GPU path ships enabled on the reference machine only if all of these are measured and true. If one fails, the GPU path stays off and nothing else changes.
   - With the GPU off, the system is the same as the baseline: same units, same sandbox, same budgets.
   - With the GPU on, prefill and decode are faster than the CPU path, and answers pass the same quality checks.
   - No sandbox control is dropped without a named replacement control (rule R0-3). The device exception applies to C4 only and only while the GPU path is enabled.
   - RAM budgets in PRD §6.2 are unchanged. VRAM gets its own budget.
   - The backend's licence is acceptable and recorded.
4. **Automatic fallback.** C4 picks the path at start and before loading the model. It uses the CPU when the GPU is missing, the driver fails, free VRAM is below the VRAM budget, or the machine is on battery (default; configurable). The path in use is reported in health. A GPU failure during a turn ends the stream with an error frame and C4 restarts on the CPU.
5. **Backend chosen by measurement in S-02.** Candidates: llama.cpp's Vulkan backend (open source, no new proprietary package) and its CUDA backend (needs the proprietary CUDA toolkit: an install the owner must approve, and a licence outside the allowlist). Vulkan is preferred if its speed is close.
6. **Sandbox mechanism proven in S-04.** The spike finds the smallest set of device nodes and driver files C4 needs, and which hardening stanzas (`PrivateDevices`, `MemoryDenyWriteExecute`, the syscall filter) the driver conflicts with, if any.

## Consequences

- **Good:** faster answers on machines with a supported GPU; a second lever for R-01; no change for machines without one.
- **Cost that cannot be made zero:** with the GPU on, C4 talks to a large proprietary kernel driver. That is a wider attack surface than the CPU path (new residual risk RR-10). It is limited to C4, which has no network, no home access and no user data on disk, and it does not exist when the GPU is off.
- **More work:** two inference paths to build, benchmark and test; S-02 and S-04 each grow; CI on hosted runners can only cover the CPU path, so the GPU path is tested on the reference machine.
- **VRAM is shared.** Other programs use the card (2,053 MiB of 4,096 MiB was in use by another process when this was written). The VRAM budget must leave room, and the fallback handles the rest.
- **4 GB limits the model size on the GPU** to roughly a 3–4B model at 4-bit. This matches the baseline's small-model plan; it does not allow a much larger model.
- **Battery and heat:** GPU use is AC-only by default. S-02 also measures whether holding the model in VRAM keeps the GPU awake when idle.
- Answers on the two paths may differ in wording, because the arithmetic differs slightly. Quality is checked on both.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Stay CPU-only for v1.0 | Leaves the largest speed lever unused on the machine the product is built for. |
| Require a GPU | Breaks goal G-7 and the minimum hardware; no fallback when the card is busy or on battery. |
| Pick a larger model that only works with the GPU | Makes the GPU a requirement in practice and breaks the CPU baseline. |
| Use the GPU for voice and embedding too | Those parts already meet their targets on the CPU; more driver exposure for little gain. Revisit by demand. |

---

## Addendum 2026-11-15 — S-04 result (evidence, not a change of decision)

Measured in user-scope units (`reports/spikes/s04-verification-claude.md` §5):

- `DevicePolicy=`/`DeviceAllow=` are **not enforced in user scope**; they cannot be the C4 device control.
- The working mechanism keeps `PrivateDevices=yes` and binds only NVIDIA nodes into the private `/dev` with `BindPaths=`. Enumeration needs `/dev/nvidia0` and `/dev/nvidiactl`; the AMD iGPU stays hidden. `MemoryDenyWriteExecute=yes` and the syscall filter did not stop enumeration.
- Still open for S-02: whether real compute (llama.cpp Vulkan, shader compilation) works with `MemoryDenyWriteExecute=yes`, and whether it needs `/dev/nvidia-uvm` or `/dev/dri/renderD129`. Decision point 6 is answered only when that is measured.
