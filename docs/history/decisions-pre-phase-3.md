# Historical Decisions (D-01 through D-25)

Archived from `memory.md` to keep the active status document under 200 lines per `rules.md` §11.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-16 | D-25 | Owner started Phase 3: "create a new branch, elaborate each step, start executing with full coordination of Claude and agy". Plan in `docs/plans/phase-03-monitor-plan.md`; Claude's design calls under that go-ahead (redacted focus for excluded windows, pause = emit nothing + resume snapshot, drop-oldest stream queue, always-anonymised `--record`, dump format in `octoplus-proto`, no `procfs`/`notify` crates, narrow GPL exception for `cosmic-protocols`, lock source by probe) are open to overturn at sign-off. | 0011 (Accepted) |
| 2026-11-16 | D-24 | Owner signed off Phase 2 ("approve all three, raise cap to 128 MiB"): ADR-0010 accepted; `target-lexicon` licence exception approved; C6 hard limit 64 → 128 MiB, dialog allowance +25 → +55 MiB (PRD §6.2, ADR-0010 item 13). | 0010 (Accepted) |
| 2026-11-16 | D-23 | Owner started Phase 2: "Builder: you and agy - full coordination". Owner named no build folders → OQ-07 closed as trash + `~/.cache`, build folders by config only. Claude's design calls under that go-ahead, open to overturn at sign-off: compiled-in skill manifests, `kernel`-kind skills (privacy.pause), C6 sandbox = union of its children, dialog with cairo renderer, new limits NFR-SEC-09/10. | 0010 (Proposed) |
| 2026-11-16 | D-22 | Owner signed off Phase 1 ("signoff - approved") and accepted ADR-0009 on Claude's recommendation; `phase_1` merged to `main`. | 0009 |
| 2026-11-16 | D-21 | Owner: Claude may build any piece itself when agy struggles or needs too many rounds; finish Phase 1 unattended overnight; fetcher sudo steps and re-login in the morning. | — |
| 2026-11-15 | D-20 | Owner started Phase 1 with agy as builder, on one branch `phase_1`. Claude's design calls (owner may overturn at sign-off): one folder per socket, socket map as code, units generated from it, runtime-folder unit, dev install under `~/.local/opt/octoplus`, C4 hash check by `sha256sum` in ExecStartPre; C++ parts take sockets and peer units as flags from their unit file. | 0009 (Proposed) |
| 2026-11-15 | D-19 | Owner signed off Phase 0 and delegated the blind chat grading to Claude (Qwen3 best, 4.42/5). After the owner installed `spirv-headers`: Vulkan backend chosen; GPU path enabled on the reference machine (AC only), C4 binds `/dev/nvidia0` + `/dev/nvidiactl`, VRAM budget 3,100 MiB. | 0005, 0008 |
| 2026-11-15 | D-18 | Under the owner's standing approval ("everything approved", finish Phase 0): model Qwen3-4B-Instruct-2507 Q4_K_M, repacking off; 384-token layout; CPU targets NFR-PERF-02 ≤ 4.5 s p50 / 6.0 s p95, NFR-PERF-03 ≤ 3.0 s p50 (3.0 / 2.0 s become GPU-path goals); C4 2,800 MiB, total 3,795 MiB. GPU path stays off until measured. Owner may overturn at sign-off. | 0005, 0008 addendum |
| 2026-11-15 | D-17 | Owner approved spike S-02 as proposed: pinned llama.cpp + BitNet b1.58 2B4T, Qwen3-4B-Instruct-2507, Phi-4-mini-instruct, SmolLM3-3B (Q4_K_M; ~7–8 GB, hash-pinned), 1–2 h CPU + Vulkan runs, no CUDA. Chat set graded blind by the owner. | 0005 (after S-02) |
| 2026-11-15 | D-16 | Owner accepted ADR-0001, 0002, 0003, 0004, 0006, 0007. | 0001–0004, 0006, 0007 |
| 2026-11-14 | D-15 | Owner restored the roles: agy builds, Claude plans/checks/commits. P0-S01…S06 were built by Claude alone while agy was off (owner's instruction) and get a second check by agy. | — |
| 2026-11-14 | D-14 | Owner removed `archive/v1-docs/` from the working tree; the documents stay in git history (`9ec1ac7`). | — |
| 2026-11-14 | D-13 | GPU is an optional speed-up for inference only. The CPU path stays the baseline; nothing already designed may get worse. | 0008 (accepted 2026-11-14) |
| 2026-11-14 | D-12 | Code lives locally and on GitHub as `OctoPlus`, in a newly created repository. | — |
| 2026-11-14 | D-11 | Owner approved the v2.0 documents as the baseline. | 0001 |
| 2026-11-14 | D-01 | Build on the v2.0 documents; they are the single baseline. | 0001 |
| 2026-11-14 | D-02 | The product is a private laptop buddy: it speaks first (nudges), chats generally, and remembers facts the user asks it to. | — |
| 2026-11-14 | D-03 | Web lookups are allowed only as one approved search or link at a time, with the exact text shown and editable. No cloud model. | — |
| 2026-11-14 | D-04 | Storage is one SQLite database with an exact in-memory vector scan — to be confirmed by spike S-01. | 0004 |
| 2026-11-14 | D-05 | Generated text streams over the ordinary socket; no shared memory. | 0003 |
| 2026-11-14 | D-06 | Knowledge graph, graph view, Python worker and background distillation are out of v1.0. Two languages: Rust and C++. | — |
| 2026-11-14 | D-07 | The language model is chosen by a measured bake-off on upstream llama.cpp. | 0005 (after S-02) |
| 2026-11-14 | D-08 | Build order: spikes → walking skeleton → safety kernel → monitor → storage → knowledge → nudges → voice → fetcher → integration → release. | 0006 |
| 2026-11-14 | D-09 | Nudges are template text and numbers, and the nudge engine never dispatches a skill. | — |
| 2026-11-14 | D-10 | Roles: agy builds, Claude checks and commits, the owner approves. | — |
