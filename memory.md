# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-15 |
| Project stage | **Phase 0 in gate review.** All 11 stories Done; awaiting owner sign-off and merge. |
| Current phase | Phase 0 — Foundation and spikes (2026-11-14 → 2026-11-15, gate review) |
| Current story | — (Phase 0 report `reports/phase-00-foundation.md` awaits sign-off) |
| Overall progress | 0 / 11 phases |
| Health | 🟨 Foundation built, all five spikes done, ADR-0001…0008 accepted; GPU path not measured (needs `spirv-headers`) |
| Next milestone | M0 — proven foundations |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | Phase 0 gate |
| Builder / Checker | — / owner |
| Branch | `p0/s08-spike-s02` is the tip of the stacked branches; nothing merged to `main` yet. Owner merges (fast-forward `main` to the tip). |
| Files being changed | — |
| Next concrete step | Owner: sign off Phase 0, grade the blind chat page (confirms ADR-0005), optionally install `spirv-headers` for the GPU half. Then Phase 1. |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | 🟨 | 2026-11-14 | — | `reports/phase-00-foundation.md` |
| 1 | Walking skeleton | ⬜ | — | — | `reports/phase-01-skeleton.md` |
| 2 | Safety kernel (C6) | ⬜ | — | — | `reports/phase-02-kernel.md` |
| 3 | Monitor (C1) | ⬜ | — | — | `reports/phase-03-monitor.md` |
| 4 | Storage (C3) | ⬜ | — | — | `reports/phase-04-storage.md` |
| 5 | Knowledge and conversation (C5) | ⬜ | — | — | `reports/phase-05-knowledge.md` |
| 6 | Nudge engine | ⬜ | — | — | `reports/phase-06-nudges.md` |
| 7 | Voice (C2) | ⬜ | — | — | `reports/phase-07-voice.md` |
| 8 | Fetcher and search (C7) | ⬜ | — | — | `reports/phase-08-fetcher.md` |
| 9 | Integration, security verification, soak | ⬜ | — | — | `reports/phase-09-integration-soak.md` |
| 10 | Packaging and release | ⬜ | — | — | `reports/phase-10-release.md` |

Legend: ⬜ Not started · 🟦 In progress · 🟨 In gate review · ✅ Done · 🟥 Blocked

---

## 4. Current Phase Board

**Phase 0 — Foundation and spikes**

| Story | Title | Status | Builder | Checker |
| :--- | :--- | :--- | :--- | :--- |
| P0-S01 | `just ci` green on a clean clone; hosted CI green | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S02 | Contracts usable from Rust and C++ | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S03 | Socket server and client library | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S04 | Health endpoint with real status | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S05 | Network-crate ban in the build | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S06 | Model fetch and verify | Done | Claude | agy (review 2026-11-14) + Claude (verified fixes) |
| P0-S07 | Spike S-01: storage scan | Done | agy | Claude (independent rerun) |
| P0-S08 | Spike S-02: model bake-off | Done (CPU; GPU not measured) | agy (+ Claude pins, part A) | Claude (rescored; re-measured RSS and speed, corrected agy's memory result) |
| P0-S09 | Spike S-03: streamed frames | Done | agy | Claude (independent rerun) |
| P0-S10 | Spike S-04: unit hardening and socket isolation | Done | agy (+ Claude early findings) | Claude (re-measured; corrected 4 claims) |
| P0-S11 | Spike S-05: health and context sensors | Done | agy | Claude (independent rerun; redacted personal data) |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Anything architectural also gets an ADR.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-15 | D-18 | Under the owner's standing approval ("everything approved", finish Phase 0): model Qwen3-4B-Instruct-2507 Q4_K_M, repacking off; 384-token layout; CPU targets NFR-PERF-02 ≤ 4.5 s p50 / 6.0 s p95, NFR-PERF-03 ≤ 3.0 s p50 (3.0 / 2.0 s become GPU-path goals); C4 2,800 MiB, total 3,795 MiB. GPU path stays off until measured. Owner may overturn at sign-off. | 0005, 0008 addendum |
| 2026-11-15 | D-17 | Owner approved spike S-02 as proposed: pinned llama.cpp + BitNet b1.58 2B4T, Qwen3-4B-Instruct-2507, Phi-4-mini-instruct, SmolLM3-3B (Q4_K_M; ~7–8 GB, hash-pinned), 1–2 h CPU + Vulkan runs, no CUDA. Chat set graded blind by the owner. | 0005 (after S-02) |
| 2026-11-15 | D-16 | Owner accepted ADR-0001, 0002, 0003, 0004, 0006, 0007. | 0001–0004, 0006, 0007 |
| 2026-11-14 | D-15 | Owner restored the roles: agy builds, Claude plans/checks/commits. P0-S01…S06 were built by Claude alone while agy was off (owner's instruction) and get a second check by agy. | — |
| 2026-11-14 | D-14 | Owner removed `archive/v1-docs/` from the working tree; the documents stay in git history (`9ec1ac7`). | — |
| 2026-11-14 | D-13 | GPU is an optional speed-up for inference only. The CPU path stays the baseline; nothing already designed may get worse. | 0008 (accepted 2026-11-14) |
| 2026-11-14 | D-12 | Code lives locally and on GitHub as `Semantic-NeuroOS`, in a newly created repository. | — |
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

---

## 6. Open Questions

Mirror of [PRD.md](PRD.md) §12 plus anything blocking the current phase. Close in both places together.

| ID | Question | Default | Needed by | Status |
| :--- | :--- | :--- | :--- | :--- |
| — | Should the GitHub repository be private or public? | Private | Phase 0 start | **Closed 2026-11-14 — private** (`github.com/bhanot-99/Semantic-NeuroOS`) |
| — | Candidate model list for spike S-02 | Proposed at the start of the spike | P0-S08 | **Closed 2026-11-15 — owner approved 4 candidates (D-17)** |
| — | GPU backend: Vulkan or CUDA (CUDA needs an owner-approved install) | Vulkan if its speed is close | Before Phase 2 latency test | Open — not measured in S-02; needs owner to run `sudo apt install spirv-headers` (ADR-0008 addendum) |
| OQ-01 | Buddy name and wake phrase | "hey jarvis" | Phase 7 | Open |
| OQ-02 | Search provider | Compare in Phase 8 | Phase 8 | Open |
| OQ-03 | Quiet hours and default nudges | 22:00–08:00; all on, break reminder as notification | Phase 6 | Open |
| OQ-04 | Which voice | `af_heart` | Phase 7 | Open |
| OQ-05 | Notes folder and calendar source | `~/Notes`; local `.ics` files | Phase 3 | Open |
| OQ-06 | Personal install or public release | Personal | Phase 10 | Open |
| OQ-07 | Cleanup allowlist | Trash, `~/.cache`, named build folders | Phase 2 | Open |

---

## 7. Blockers and Watch Items

Bugs live in `BUGS.md` (created in Phase 0). This table is for blockers and risks to watch right now.

| Date | Type | Item | Owner | Status |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-15 | Watch | agy's reports can state unmeasured results as fact (TASK 01 R-09, S-04). Every agy claim is re-checked against raw output before it enters an ADR. | Claude | Open |
| 2026-11-15 | Watch | Chat-quality confirmation for ADR-0005: owner's blind grading page (private artifact). If Qwen3 is clearly last, reopen ADR-0005 before Phase 2. | Owner | Open |
| 2026-11-15 | Watch | Memory headroom is thin: total 3,795 of 4,000 MiB. llama.cpp must run with weight repacking off (+1.6 GB otherwise). | Claude | Open |
| 2026-11-14 | Watch | Toolchains installed and verified 2026-11-14 (see §8). Language model fetched and verified 2026-11-15. | Claude | Closed |
| 2026-11-14 | Note | TSan needs `setarch -R` on this kernel (mmap randomisation); wired into the `tsan` preset. | Claude | Closed |

---

## 8. Environment Facts (reference machine)

Verified 2026-11-14 during P0-S01 unless marked.

| Item | Expected | Verified |
| :--- | :--- | :--- |
| CPU | AMD Ryzen 7 5800H, 8C/16T, AVX2 | ✅ `lscpu` |
| RAM | 16 GB | ✅ 15 GiB usable |
| GPU (optional) | NVIDIA RTX 3050 Mobile, 4,096 MiB VRAM | Seen with `nvidia-smi` 2026-11-14, driver 595.104.02 |
| OS / desktop | Pop!_OS 24.04 LTS, COSMIC on Wayland | ✅ OS; desktop not yet checked |
| Kernel | ≥ 6.7 | ✅ 7.2.15; Landlock ABI ≥ 6 (scopes work) |
| Toolchains | Rust (pinned), GCC, CMake, Ninja, just, protoc | ✅ Rust 1.100.0, GCC 13.3, CMake 3.28, Ninja 1.11, just 1.59, protoc 3.21.12, nextest, cargo-deny 0.20.2, llvm-cov, machete, clang-format/tidy 18, gcovr, GoogleTest |
| System packages | PipeWire dev files, espeak-ng, GTK4 dev files, protobuf, spdlog | ✅ PipeWire 1.6.8, libespeak-ng-dev, GTK 4.14, protobuf 3.21, spdlog 1.12, Vulkan dev; systemd 255 |
| Disk encryption | On (recommended) | — |

---

## 9. Session Log

Newest first. One row per work session.

| Date | Summary | Stories | Builder / Checker | Next step |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-15 | S-02 done (CPU): Qwen3-4B wins every automatic test (grounded 20/20, honesty 9/10, skill 7/20 — test prompt lacked skill descriptions); BitNet failed pin-time perplexity. agy's "all models over the 3,000 MiB cap" was weight repacking: off → 2,770 MiB and faster prefill (Claude re-measured). CPU voice wait ≈ 4.2 s with a 384-token layout; 3 s needs the GPU, not measured (spirv-headers). ADR-0005 written; ADR-0008 addendum; PRD/Architecture updated; blind grading page published; Phase 0 report finished. | P0-S08 | agy / Claude | Owner sign-off + merge |
| 2026-11-15 | S-01 done: hybrid query at 30k chunks ≈ 13 ms p50 / 16 ms p99 warm (target 25/50); p99 40–47 ms with a concurrent writer (Phase 4 risk); C3 ≈ 205 MiB steady without embedding model. ADR-0004 proposed. Benches excluded from coverage gate. | P0-S07 | agy / Claude | S-02 (owner) |
| 2026-11-15 | S-05 done: disk/battery/heat from /proc+/sys, window+fullscreen via zcosmic_toplevel_info_v1, idle via ext_idle_notifier_v1, mic-in-use via PipeWire — all work inside the hardened unit with no system bus. Probe leaked title hashes/app ids into reports (BUG-013), redacted before commit. ADR-0007 proposed. | P0-S11 | agy / Claude | S-01 |
| 2026-11-15 | S-04 done: all §8.1 stanzas apply in user scope; 3 additions (landlock syscalls, hide system D-Bus — DNS leak via resolve1, BindPaths for /run/user); DeviceAllow not enforced in user scope → GPU via PrivateDevices + bound NVIDIA nodes. agy's report had 4 unmeasured/wrong claims, re-measured by Claude (BUG-011). ADR-0002 proposed. | P0-S10 | agy / Claude | S-05 |
| 2026-11-15 | S-03 done: token-frame p99 ≈ 0.58 ms (target ≤ 1 ms), cancel p99 < 0.1 ms, reproduced by Claude; ADR-0003 proposed. | P0-S09 | agy / Claude | S-04 |
| 2026-11-15 | agy reviewed P0-S01…S06: 10 findings, 9 real (R-09 rejected as invented). agy fixed 8 (BUG-002…007, 009, 010), Claude verified each; BUG-008 goes to S-04. P0-S01…S06 Done. | P0-S01…S06 | agy / Claude | Spikes |
| 2026-11-14 | Phase 0 started. ADR-0008 accepted. Built repo gate, contracts, Rust+C++ IPC, health, net-ban, sandbox, testkit, model fetch; ADR-0001/0006 drafted. BUG-001 found by hosted CI and fixed. All branches pushed, `just ci` + hosted CI green. | P0-S01…S06 | Claude / Claude (self) | agy second check, then spikes |
| 2026-11-14 | Owner directed optional GPU acceleration (D-13). Drafted ADR-0008 and matching edits to PRD, Architecture, security, phases, README. No code. | Planning | — / Claude | Owner accepts ADR-0008, then P0-S01 |
| 2026-11-14 | Wrote the v2.0 baseline: PRD, Architecture, security, rules, phases, design, README, lessons. No code. | Planning | — / Claude | Owner sign-off, then P0-S01 |

---

## 10. Update Protocol

1. **Session start:** read §1, §2, §4, §7; read the current phase in [phases.md](phases.md); state the story.
2. **While working:** keep §2 accurate.
3. **Story done:** set it Done in §4 with builder and checker; update §1.
4. **New decision, question or blocker:** add to §5, §6 or §7 at once.
5. **Session end:** add a row to §9, set the next step in §2, push.
6. **Phase gate:** verify every exit criterion, write the report, set the phase ✅ in §3, reset §4 for the next phase.
