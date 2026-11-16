# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-16 |
| Project stage | **Phase 2 in progress** (started 2026-11-16 on branch `phase_2`). |
| Current phase | Phase 2 — Safety kernel (C6) |
| Current story | P2-S01/S02 (agy, T1); P2-S03 dialog (Claude) |
| Overall progress | 2 / 11 phases |
| Health | 🟦 Phase 2 building |
| Next milestone | M2 — safety kernel |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | P2-T1 (agy): registry, schemas, tier evaluator, taint escalation, rate limiter. In parallel Claude built `neuroos-confirm` (dialog). |
| Builder / Checker | agy builds, Claude checks (D-23); Claude builds pieces itself where faster (D-21 carried over) |
| Branch | `phase_2` (one branch for the phase, as in Phase 1) |
| Files being changed | `crates/neuroos-kernel`, `crates/neuroos-confirm`, `crates/neuroos-common/src/config.rs`, `ui/tokens/` |
| Next concrete step | Check T1; then T2 audit log, T3 keys and tokens, T4 skill runner and skills, T5 notifications, T6 kernel wiring and units. CPU governor is `powersave` (needs sudo to set `performance`) before PF measurements. |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | ✅ | 2026-11-14 | 2026-11-15 | `reports/phase-00-foundation.md` |
| 1 | Walking skeleton | ✅ | 2026-11-15 | 2026-11-16 | `reports/phase-01-skeleton.md` |
| 2 | Safety kernel (C6) | 🟦 | 2026-11-16 | — | `reports/phase-02-kernel.md` |
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

**Phase 2 — Safety kernel (C6)** (Phase 1 board: `reports/phase-01-skeleton.md` §2)

| Story | Title | Status | Builder | Checker |
| :--- | :--- | :--- | :--- | :--- |
| P2-S01 | C5 asks whether a capability is allowed and gets a fast answer | In Progress | agy | Claude |
| P2-S02 | Anything influenced by web content asks the user first | In Progress | agy | Claude |
| P2-S03 | Clear approval dialog; approval cannot be faked | In Progress (dialog built) | Claude (dialog) | agy review |
| P2-S04 | Skills run in a sandbox with only the access they need | Backlog | — | — |
| P2-S05 | Every action has ATTEMPT and RESULT; tampering is detectable | Backlog | — | — |
| P2-S06 | Disk report and confirmed cleanup of an allowlisted set | Backlog | — | — |
| P2-S07 | Notifications show only fixed wording with numbers | Backlog | — | — |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Anything architectural also gets an ADR.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-16 | D-23 | Owner started Phase 2: "Builder: you and agy - full coordination". Owner named no build folders → OQ-07 closed as trash + `~/.cache`, build folders by config only. Claude's design calls under that go-ahead, open to overturn at sign-off: compiled-in skill manifests, `kernel`-kind skills (privacy.pause), C6 sandbox = union of its children, dialog with cairo renderer, new limits NFR-SEC-09/10. | 0010 (Proposed) |
| 2026-11-16 | D-22 | Owner signed off Phase 1 ("signoff - approved") and accepted ADR-0009 on Claude's recommendation; `phase_1` merged to `main`. | 0009 |
| 2026-11-16 | D-21 | Owner: Claude may build any piece itself when agy struggles or needs too many rounds; finish Phase 1 unattended overnight; fetcher sudo steps and re-login in the morning. | — |
| 2026-11-15 | D-20 | Owner started Phase 1 with agy as builder, on one branch `phase_1`. Claude's design calls (owner may overturn at sign-off): one folder per socket, socket map as code, units generated from it, runtime-folder unit, dev install under `~/.local/opt/neuroos`, C4 hash check by `sha256sum` in ExecStartPre; C++ parts take sockets and peer units as flags from their unit file. | 0009 (Proposed) |
| 2026-11-15 | D-19 | Owner signed off Phase 0 and delegated the blind chat grading to Claude (Qwen3 best, 4.42/5). After the owner installed `spirv-headers`: Vulkan backend chosen; GPU path enabled on the reference machine (AC only), C4 binds `/dev/nvidia0` + `/dev/nvidiactl`, VRAM budget 3,100 MiB. | 0005, 0008 |
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
| — | GPU backend: Vulkan or CUDA (CUDA needs an owner-approved install) | Vulkan if its speed is close | P0-S08 | **Closed 2026-11-15 — Vulkan, measured (ADR-0008 S-02 addendum)** |
| OQ-01 | Buddy name and wake phrase | "hey jarvis" | Phase 7 | Open |
| OQ-02 | Search provider | Compare in Phase 8 | Phase 8 | Open |
| OQ-03 | Quiet hours and default nudges | 22:00–08:00; all on, break reminder as notification | Phase 6 | Open |
| OQ-04 | Which voice | `af_heart` | Phase 7 | Open |
| OQ-05 | Notes folder and calendar source | `~/Notes`; local `.ics` files | Phase 3 | Open |
| OQ-06 | Personal install or public release | Personal | Phase 10 | Open |
| OQ-07 | Cleanup allowlist | Trash, `~/.cache`, named build folders | Phase 2 | **Closed 2026-11-16 (D-23, ADR-0010)** — config list, empty by default |

---

## 7. Blockers and Watch Items

Bugs live in `BUGS.md` (created in Phase 0). This table is for blockers and risks to watch right now.

| Date | Type | Item | Owner | Status |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-16 | Risk | GTK4 dialog RSS ≈ 66 MiB with the cairo renderer (162 MiB default), outside the unit. PRD allows C6 +25 MiB while a dialog is open, hard limit 64 MiB. Measure inside the C6 unit at the gate; owner decides if over. | Claude | Open |
| 2026-11-15 | Watch | agy's reports can state unmeasured results as fact (TASK 01 R-09, S-04). Every agy claim is re-checked against raw output before it enters an ADR. | Claude | Open |
| 2026-11-15 | Watch | Chat grading done (delegated to Claude): Qwen3 best, 4.42/5. Every model called ibuprofen + blood-pressure pills "generally safe" — add to Phase 2 safety tests. | Claude | Open |
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
| 2026-11-16 | Branch cleanup (dead code, wrong BUG/ADR references, crate READMEs state real Phase 1 behaviour, profraw clutter). Owner signed off Phase 1, ADR-0009 accepted (D-22); merged to `main`. | P1 close | Claude / Claude | Phase 2 when the owner says |
| 2026-11-16 | Phase 1 gate. Soak 1 failed on healthd RSS (BUG-030), fixed, soak 2 PASS (360 asks, 0 restarts, all parts ≤ 2 %). Bench in the real C4 unit (89 / 11.9 tok/s, peak 2,788 MiB); cold start 2.6 s. Fixed BUG-030…035 (rolling window, world-writable dev install, bench pipefail, E2E cancel never cancelling, stop→start race, SC2 fetcher check). Owner installed C7; reboot needed for group `neuroos`; C7 kill-and-recover 494 ms DOWN / 2,468 ms back. `just ci` ALL GREEN (160 Rust, 62 C++, cov 82.3 % / 78.6 %, ASan/TSan). Report written. | P1 gate | Claude / Claude | Owner sign-off, ADR-0009, merge |
| 2026-11-15 | Post-sign-off cleanup: fixed BUG-011…015 (S-04 harness reports only measured verdicts, real DNS query; S-05 probe no longer records titles/app ids; probes use rustix/std instead of raw FFI; S-01 total no longer double-counts; S-02 run-time label); removed unused locals, duplicate config-env literal, empty placeholder dirs, build leftovers; README status updated. `just ci` green. | cleanup | Claude / Claude | Owner merges; Phase 1 |
| 2026-11-15 | Phase 0 signed off. Chat grading (delegated): Qwen3 4.42, Phi 4.00, SmolLM3 3.45. GPU half by Claude: Vulkan prefill ≈ 12–14×, decode ≈ 4×, TTFT 295 ms, VRAM 3,010 MiB, quality same, test-backend-ops 22,362/22,362; works under full baseline incl. MDWE with only nvidia0+nvidiactl. Power sensor unusable. | P0-S08 | Claude / Claude | Owner merges; Phase 1 |
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
