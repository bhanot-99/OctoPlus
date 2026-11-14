# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-14 |
| Project stage | **Not started.** Documents v2.0 are the approved baseline (owner sign-off 2026-11-14). |
| Current phase | Phase 0 — Foundation and spikes (not started) |
| Current story | — |
| Overall progress | 0 / 11 phases |
| Health | ⬜ Nothing built yet |
| Next milestone | M0 — proven foundations |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | — |
| Builder / Checker | — / — |
| Branch | — |
| Files being changed | — |
| Next concrete step | Create the repository (local + GitHub `Semantic-NeuroOS`, see D-12) and start **P0-S01**, beginning with installing and verifying the toolchains. |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | ⬜ | — | — | `reports/phase-00-foundation.md` |
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
| P0-S01 | `just ci` green on a clean clone; hosted CI green | Backlog | — | — |
| P0-S02 | Contracts usable from Rust and C++ | Backlog | — | — |
| P0-S03 | Socket server and client library | Backlog | — | — |
| P0-S04 | Health endpoint with real status | Backlog | — | — |
| P0-S05 | Network-crate ban in the build | Backlog | — | — |
| P0-S06 | Model fetch and verify | Backlog | — | — |
| P0-S07 | Spike S-01: storage scan | Backlog | — | — |
| P0-S08 | Spike S-02: model bake-off | Backlog | — | — |
| P0-S09 | Spike S-03: streamed frames | Backlog | — | — |
| P0-S10 | Spike S-04: unit hardening and socket isolation | Backlog | — | — |
| P0-S11 | Spike S-05: health and context sensors | Backlog | — | — |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Anything architectural also gets an ADR.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-14 | D-12 | Code lives locally and on GitHub as `Semantic-NeuroOS`, in a newly created repository. | — |
| 2026-11-14 | D-11 | Owner approved the v2.0 documents as the baseline. | 0001 (to write) |
| 2026-11-14 | D-01 | Build on the v2.0 documents; they are the single baseline. | 0001 (to write) |
| 2026-11-14 | D-02 | The product is a private laptop buddy: it speaks first (nudges), chats generally, and remembers facts the user asks it to. | — |
| 2026-11-14 | D-03 | Web lookups are allowed only as one approved search or link at a time, with the exact text shown and editable. No cloud model. | — |
| 2026-11-14 | D-04 | Storage is one SQLite database with an exact in-memory vector scan — to be confirmed by spike S-01. | 0004 (after S-01) |
| 2026-11-14 | D-05 | Generated text streams over the ordinary socket; no shared memory. | 0003 (after S-03) |
| 2026-11-14 | D-06 | Knowledge graph, graph view, Python worker and background distillation are out of v1.0. Two languages: Rust and C++. | — |
| 2026-11-14 | D-07 | The language model is chosen by a measured bake-off on upstream llama.cpp. | 0005 (after S-02) |
| 2026-11-14 | D-08 | Build order: spikes → walking skeleton → safety kernel → monitor → storage → knowledge → nudges → voice → fetcher → integration → release. | 0006 (to write) |
| 2026-11-14 | D-09 | Nudges are template text and numbers, and the nudge engine never dispatches a skill. | — |
| 2026-11-14 | D-10 | Roles: agy builds, Claude checks and commits, the owner approves. | — |

---

## 6. Open Questions

Mirror of [PRD.md](PRD.md) §12 plus anything blocking the current phase. Close in both places together.

| ID | Question | Default | Needed by | Status |
| :--- | :--- | :--- | :--- | :--- |
| — | Should the GitHub repository be private or public? | Private | Phase 0 start | **Open — owner** |
| — | Candidate model list for spike S-02 | Proposed at the start of the spike | P0-S08 | Open |
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
| 2026-11-14 | Watch | Toolchains, system packages and models must be installed and verified on the reference machine. | Claude | Open — first task of P0-S01 |

---

## 8. Environment Facts (reference machine)

To be verified in P0-S01. Nothing below is confirmed yet.

| Item | Expected | Verified |
| :--- | :--- | :--- |
| CPU | AMD Ryzen 7 5800H, 8C/16T, AVX2 | — |
| RAM | 16 GB | — |
| OS / desktop | Pop!_OS 24.04 LTS, COSMIC on Wayland | — |
| Kernel | ≥ 6.7 | — |
| Toolchains | Rust (pinned), GCC, CMake, Ninja, just, protoc | — |
| System packages | PipeWire dev files, espeak-ng, GTK4 dev files, protobuf, spdlog | — |
| Disk encryption | On (recommended) | — |

---

## 9. Session Log

Newest first. One row per work session.

| Date | Summary | Stories | Builder / Checker | Next step |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-14 | Wrote the v2.0 baseline: PRD, Architecture, security, rules, phases, design, README, lessons. No code. | Planning | — / Claude | Owner sign-off, then P0-S01 |

---

## 10. Update Protocol

1. **Session start:** read §1, §2, §4, §7; read the current phase in [phases.md](phases.md); state the story.
2. **While working:** keep §2 accurate.
3. **Story done:** set it Done in §4 with builder and checker; update §1.
4. **New decision, question or blocker:** add to §5, §6 or §7 at once.
5. **Session end:** add a row to §9, set the next step in §2, push.
6. **Phase gate:** verify every exit criterion, write the report, set the phase ✅ in §3, reset §4 for the next phase.
