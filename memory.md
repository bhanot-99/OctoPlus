# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-17 |
| Project stage | **Phase 5 (Knowledge and conversation, C5) built, gate green, ready to merge** from branch `phase-5` (2026-11-17); KPI-12 grading and KPI-1 recordings are still the owner's. Phase 4 signed off and merged. |
| Current phase | Phase 6 — Nudge engine |
| Current story | P5 work packages W0–W16 in `docs/plans/phase-05-knowledge-plan.md` |
| Overall progress | 6 / 11 phases done (Phase 5 awaiting merge) |
| Health | 🟦 Phase 6 started |
| Next milestone | M4 — knowledge (C5) |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | Phase 6 plan W0→W10 (`docs/plans/phase-06-nudges-plan.md`) |
| Builder / Checker | agy builds small pure modules and child skills, Claude builds orchestrator, assembler, kernel skills; the other side checks (D-23, D-21 carried over) |
| Branch | `phase-6` |
| Files being changed | `crates/neuroos-knowledge/**`, `neuroos-common` (`[knowledge]` config), `neuroos-proto` (`AskRequest.at_ns`), `neuroos-kernel` skills, `neuroos-skill`, `neuroosctl profile` and `ask` |
| Next concrete step | W1–W3 pure modules (agy: rules, evaluator, policy), W4 storage and kernel additions (Claude). |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | ✅ | 2026-11-14 | 2026-11-15 | `reports/phase-00-foundation.md` |
| 1 | Walking skeleton | ✅ | 2026-11-15 | 2026-11-16 | `reports/phase-01-skeleton.md` |
| 2 | Safety kernel (C6) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-02-kernel.md` |
| 3 | Monitor (C1) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-03-monitor.md` |
| 4 | Storage (C3) | ✅ | 2026-11-16 | 2026-11-17 | `reports/phase-04-storage.md` |
| 5 | Knowledge and conversation (C5) | ✅ (merge pending) | 2026-11-17 | 2026-11-17 | `reports/phase-05-knowledge.md` |
| 6 | Nudge engine | 🟦 | 2026-11-17 | — | `reports/phase-06-nudges.md` |
| 7 | Voice (C2) | ⬜ | — | — | `reports/phase-07-voice.md` |
| 8 | Fetcher and search (C7) | ⬜ | — | — | `reports/phase-08-fetcher.md` |
| 9 | Integration, security verification, soak | ⬜ | — | — | `reports/phase-09-integration-soak.md` |
| 10 | Packaging and release | ⬜ | — | — | `reports/phase-10-release.md` |

Legend: ⬜ Not started · 🟦 In progress · 🟨 In gate review · ✅ Done · 🟥 Blocked

---

## 4. Current Phase Board

**Phase 6 — Nudge engine** (Phase 5 board: `reports/phase-05-knowledge.md`)

| Story | Title | Status | Builder | Checker |
| :--- | :--- | :--- | :--- | :--- |
| P6-S01 | Told when disk, battery, memory or temperature needs attention | Ready | agy (rules, evaluator), Claude (engine) | Claude |
| P6-S02 | Not told again and again | Ready | agy (evaluator), Claude | Claude |
| P6-S03 | Not interrupted at a bad moment | Ready | agy (policy), Claude (context) | Claude |
| P6-S04 | Nudges reveal nothing private by default | Ready | Claude | agy review |
| P6-S05 | Say "not now" or switch a nudge off | Ready | Claude | Claude |
| P6-S06 | A nudge can offer help; nothing happens unless I reply | Ready | Claude | agy review |
| P6-S07 | Every nudge and its outcome is logged | Ready | Claude | Claude |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Anything architectural also gets an ADR.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-17 | D-32 | Owner started Phase 6: "you are already connected with agy, so understand your role and agy's role and start executing phase 6 as planned". Plan: `docs/plans/phase-06-nudges-plan.md`. Claude's design calls under that go-ahead (state in C3's nudge log, paused inferred from own pauses and stale data, quiet hours by machine time zone, episode semantics, two disk rules, upcoming-event rule and notification buttons not built) are open to overturn at sign-off. | 0014 (Proposed) |
| 2026-11-17 | D-31 | Owner: "go with your recommendation" on the open Phase 5 items, then "make a final commit and make this branch ready to merge". Recommendations taken: **accept the router as is** (BUG-070: right ~81 % on unseen wording, 100 % on fixtures, a miss falls to chat or "couldn't work out", never to a wrong action; an embedding router is a later option); cleanup and web stay out of the action route until Phase 8; lexical fact relevance, the provisional cutoffs 0.50/0.72 and the 48-token focus budget stand; ADR-0013 accepted. **Still the owner's:** grading `reports/p5-kpi12-transcript.md`, and KPI-1 on real recordings at integration (D-26). | 0013 (Accepted) |
| 2026-11-17 | D-30 | While finishing Phase 5 under D-29, Claude also changed other parts, all open to overturn at sign-off: **C4 now enforces a grammar** (it had none; BUG-063), the **kernel** gained notes/calendar folder roots, a per-skill write limit and Landlock grants for the notes folders, the **unit generator** binds them into C6's unit, ADR-0005's focus budget grew 32 → 48 tokens, `system.cleanup` and `web.*` are not callable from the action route, a window title counts as untrusted. KPI-13/KPI-2 first-run numbers on four fresh sets are recorded as measured (75–81 % and 88–98 %), not tuned away. Review by agy: 10 findings, 8 fixed, 2 documented as design. | 0013 (Proposed) |
| 2026-11-17 | D-29 | Owner started Phase 5: "you are already connected with agy, so understand your role and agy's role and start executing phase 5 as planned". Plan: `docs/plans/phase-05-knowledge-plan.md`. Claude's design calls under that go-ahead (pure router with fixtures, byte-based token estimate, control-token neutraliser, kernel-kind profile/forget skills, facts saved only on the user's word, fixed status and lookup lines, additive `AskRequest.at_ns`) are open to overturn at sign-off. | 0013 (Proposed) |
| 2026-11-16 | D-28 | Owner started Phase 4 and went to sleep: "create a new branch, elaborate each step, start executing with Claude and agy, finish the whole phase by morning". Answers given first: **download the embedding model and runtime** (approved), **synthetic dumps** for the noise gate until real ones exist. Plan: `docs/plans/phase-04-storage-plan.md`; Claude's design calls under the go-ahead (own WordPiece tokenizer instead of the `tokenizers` crate, forget scrubs backups at once, spool format v1, additive proto fields `LIFECYCLE_JOB_PURGE` and `HealthSignals.locked_known`, budgets restated from measurement) are open to overturn at sign-off. | 0012 (Proposed) |
| 2026-11-16 | D-27 | Owner signed off Phase 3: ADR-0011 accepted as written; session lock state stays "unknown" in v1.0 (no logind call, C1's sandbox is not widened); `cosmic-protocols` GPL exception accepted. Branch cleaned for merge (fake clock behind `test-support`, test-only hook removed, unused `subtle` dropped). | 0011 (Accepted) |
| 2026-11-16 | D-26 | Owner: all real-use testing (the Phase 3 recordings: ≥ 8 h in ≥ 3 dumps + 1 held-out, and the live fullscreen/compositor-restart checks) is done once the whole project is built and connected, so problems show up with everything working together. Phase 3's "dumps recorded" exit criterion is therefore deferred to the integration phase, not dropped. Consequence: Phase 4's noise gate (KPI-8) can only be exercised on synthetic dumps until then. | — |
| 2026-11-16 | D-25 | Owner started Phase 3: "create a new branch, elaborate each step, start executing with full coordination of Claude and agy". Plan in `docs/plans/phase-03-monitor-plan.md`; Claude's design calls under that go-ahead (redacted focus for excluded windows, pause = emit nothing + resume snapshot, drop-oldest stream queue, always-anonymised `--record`, dump format in `neuroos-proto`, no `procfs`/`notify` crates, narrow GPL exception for `cosmic-protocols`, lock source by probe) are open to overturn at sign-off. | 0011 (Accepted) |
| 2026-11-16 | D-24 | Owner signed off Phase 2 ("approve all three, raise cap to 128 MiB"): ADR-0010 accepted; `target-lexicon` licence exception approved; C6 hard limit 64 → 128 MiB, dialog allowance +25 → +55 MiB (PRD §6.2, ADR-0010 item 13). | 0010 (Accepted) |
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
| 2026-11-16 | Risk | **C6 memory with the dialog open, real unit:** cgroup reached the 64 MiB hard limit (peak 64, `memory.events max` 865, no OOM): GTK library page cache is charged to C6 and reclaimed under the cap. Kernel process RSS 5 MiB; dialog process RSS 49.8 MiB. PRD budget "+25 MiB while a dialog is open" is not met by RSS. Owner raised C6 MemoryMax to 128 MiB (D-24). | Claude | Closed 2026-11-16 |
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
| 2026-11-16 | Owner signed off Phase 2 (D-24); `phase_2` fast-forwarded into `main`. Post-merge cleanup: kernel README and crate docs describe the real Phase 2 kernel; READMEs for confirm, skill, sandbox; C5 README names `llm.generate`; code comments cite BUG-041/042/043 instead of review finding ids. | P2 close | Claude / Claude | Phase 3 when the owner says |
| 2026-11-16 | Phase 2 built and gated. agy: registry/tiers/limiter, audit log + CLI, notifications, injection corpus, BUG-036/039 fixes, independent review of Claude's code (4 of 5 findings accepted, BUG-040..043). Claude: dialog, tokens, keys, skill child + seccomp, skills, dispatch, recovery, units, install; real-unit bugs BUG-037/038, gate bug BUG-044. Capability check p99 0.032 ms; C6 hits its 64 MiB cap with the dialog open → owner raised it to 128 MiB. `just ci` ALL GREEN (297 Rust, 62 C++, cov 84.8 % / 78 %, ASan/TSan). Owner signed off (D-24). | P2-S01…S07 | agy + Claude / Claude + agy (review) | Phase 3 when the owner says |
| 2026-11-16 | Branch cleanup (dead code, wrong BUG/ADR references, crate READMEs state real Phase 1 behaviour, profraw clutter). Owner signed off Phase 1, ADR-0009 accepted (D-22); merged to `main`. | P1 close | Claude / Claude | Phase 2 when the owner says |
| 2026-11-16 | Phase 1 gate. Soak 1 failed on healthd RSS (BUG-030), fixed, soak 2 PASS (360 asks, 0 restarts, all parts ≤ 2 %). Bench in the real C4 unit (89 / 11.9 tok/s, peak 2,788 MiB); cold start 2.6 s. Fixed BUG-030…035 (rolling window, world-writable dev install, bench pipefail, E2E cancel never cancelling, stop→start race, SC2 fetcher check). Owner installed C7; reboot needed for group `neuroos`; C7 kill-and-recover 494 ms DOWN / 2,468 ms back. `just ci` ALL GREEN (160 Rust, 62 C++, cov 82.3 % / 78.6 %, ASan/TSan). Report written. | P1 gate | Claude / Claude | Owner sign-off, ADR-0009, merge |
| 2026-11-15 | Post-sign-off cleanup: fixed BUG-011…015 (S-04 harness reports only measured verdicts, real DNS query; S-05 probe no longer records titles/app ids; probes use rustix/std instead of raw FFI; S-01 total no longer double-counts; S-02 run-time label); removed unused locals, duplicate config-env literal, empty placeholder dirs, build leftovers; README status updated. `just ci` green. | cleanup | Claude / Claude | Owner merges; Phase 1 |
| 2026-11-15 | Phase 0 signed off. Chat grading (delegated): Qwen3 4.42, Phi 4.00, SmolLM3 3.45. GPU half by Claude: Vulkan prefill ≈ 12–14×, decode ≈ 4×, TTFT 295 ms, VRAM 3,010 MiB, quality same, test-backend-ops 22,362/22,362; works under full baseline incl. MDWE with only nvidia0+nvidiactl. Power sensor unusable. | P0-S08 | Claude / Claude | Owner merges; Phase 1 |
| 2026-11-17 | Phase 4 finished: C3 built (db, ingest filter, embedder, hybrid search, spool, lifecycle, forget), real model runs in the sandboxed unit, NFR-PERF-04 measured 19 ms p50 / 22 ms p99 at the cap, KPI-8 gate on synthetic dumps, `just ci` green, agy review 9 findings (8 fixed, F8 left minor). | P4-S01…S09 | Claude + agy / Claude |
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
