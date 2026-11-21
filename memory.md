# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-21 |
| Project stage | **Phase 8 merged into main** (2026-11-21). Milestone M6 achieved. Ready for Phase 9. |
| Current phase | Phase 8 complete; next is Phase 9 (Integration, security verification, 24 h soak) |
| Current story | Phase 8 merged and branch deleted. Ready for Phase 9 planning. |
| Overall progress | 8 / 11 phases done (Phase 8 completed) |
| Health | 🟩 All phases 0-8 complete and verified on main; 772 tests green |
| Next milestone | Phase 9 — Integration, security verification, 24 h soak |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | Phase 8 close & transition to Phase 9 |
| Builder / Checker | agy & Claude |
| Branch | `main` |
| Files being changed | `memory.md` |
| Next concrete step | Prepare and plan Phase 9 (Integration, security verification, soak). |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | ✅ | 2026-11-14 | 2026-11-15 | `reports/phase-00-foundation.md` |
| 1 | Walking skeleton | ✅ | 2026-11-15 | 2026-11-16 | `reports/phase-01-skeleton.md` |
| 2 | Safety kernel (C6) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-02-kernel.md` |
| 3 | Monitor (C1) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-03-monitor.md` |
| 4 | Storage (C3) | ✅ | 2026-11-16 | 2026-11-17 | `reports/phase-04-storage.md` |
| 5 | Knowledge and conversation (C5) | ✅ | 2026-11-17 | 2026-11-17 | `reports/phase-05-knowledge.md` |
| 6 | Nudge engine | ✅ | 2026-11-17 | 2026-11-17 | `reports/phase-06-nudges.md` |
| 7 | Voice (C2) | ✅ | 2026-11-17 | 2026-11-18 | `reports/phase-07-voice.md` |
| 8 | Fetcher and search (C7) | ✅ | 2026-11-21 | 2026-11-21 | `reports/phase-08-fetcher.md` |
| 9 | Integration, security verification, soak | ⬜ | — | — | `reports/phase-09-integration-soak.md` |
| 10 | Packaging and release | ⬜ | — | — | `reports/phase-10-release.md` |

Legend: ⬜ Not started · 🟦 In progress · 🟨 In gate review · ✅ Done · 🟥 Blocked

---

## 4. Current Phase Board

**Phase 8 — Fetcher and search (C7)** (plan: `docs/plans/phase-08-fetcher-plan.md`, report: `reports/phase-08-fetcher.md`)

| ID | Story | Status | Builder | Checker |
| :--- | :--- | :--- | :--- | :--- |
| P8-S01 | Approved link fetched, extracted, tainted spooling into C3 | Done | agy, Claude | Claude |
| P8-S02 | Approved search, editable GTK4 query, C5 answer from results | Done | agy, Claude | Claude |
| P8-S03 | Anti-SSRF engine, DNS pinning, redirect re-validation | Done | agy | Claude |
| P8-S04 | Cryptographic tokens with `fetch.key`, peer validation | Done | agy, Claude | Claude |
| P8-S05 | Request hygiene: zero cookies, generic headers, stateless | Done | agy | Claude |
| P8-S06 | Egress ledger `egress.jsonl`, `neuroosctl egress list` | Done | Claude | agy review |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Older entries archived to `docs/history/decisions-pre-phase-3.md`.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-21 | D-39 | Phase 8 complete: C7 fetcher & search built; OQ-02 closed with decoupled SearchAdapter; anti-SSRF & Format v1 spooling verified. | 0016 (Accepted) |
| 2026-11-21 | D-38 | Owner started Phase 8: "i would like you to start executing whole phase 8 as planned". Plan: `docs/plans/phase-08-fetcher-plan.md`. ADR-0016 drafted. C7 is Zone 1 DMZ; `fetch.key` binds approvals; anti-SSRF resolve-once; Format v1 spooling. | 0016 (Accepted) |
| 2026-11-18 | D-37 | Owner chose the voice `af_heart` (voice 1 of the listening test) and confirmed the wake phrase "hey jarvis" with stock model. OQ-01 and OQ-04 closed. | 0015 |
| 2026-11-17 | D-36 | Owner approved Phase 7 downloads (whisper.cpp, tiny.en, Silero VAD, openWakeWord hey_jarvis, Kokoro fp32). | 0015 |
| 2026-11-17 | D-35 | Owner started Phase 7: "start executing phase 7 as planned". Plan: `docs/plans/phase-07-voice-plan.md`. | 0015 (Accepted) |
| 2026-11-17 | D-34 | Router generalised (rules, not a model) and measured on blind sets written by agy; KPI-13 now ~94 %. | BUG-070 |
| 2026-11-17 | D-33 | Owner approved all of Phase 6 and accepted D-32 and ADR-0014 as recommended; signed off on replay & test evidence. | 0014 (Accepted) |
| 2026-11-17 | D-32 | Owner started Phase 6: "start executing phase 6 as planned". Plan: `docs/plans/phase-06-nudges-plan.md`. | 0014 (Accepted) |
| 2026-11-17 | D-31 | Owner: "go with your recommendation" on Phase 5 items. Accepted router, lexical fact relevance, ADR-0013 accepted. | 0013 (Accepted) |
| 2026-11-17 | D-30 | C4 enforces grammar; kernel notes/calendar folders; Landlock grants; unit generator bindings; ADR-0005 budget. | 0013 (Proposed) |
| 2026-11-17 | D-29 | Owner started Phase 5: "start executing phase 5 as planned". Plan: `docs/plans/phase-05-knowledge-plan.md`. | 0013 (Accepted) |
| 2026-11-16 | D-28 | Owner started Phase 4: embedding model approved, synthetic dumps for noise gate. Plan: `docs/plans/phase-04-storage-plan.md`. | 0012 (Accepted) |
| 2026-11-16 | D-27 | Owner signed off Phase 3: ADR-0011 accepted as written; session lock state stays "unknown" in v1.0. | 0011 (Accepted) |
| 2026-11-16 | D-26 | Owner: real-use testing (Phase 3 recordings) deferred to integration phase so all parts work together. | — |

---

## 6. Open Questions

| ID | Question | Default | Needed by | Status |
| :--- | :--- | :--- | :--- | :--- |
| OQ-01 | Buddy name and wake phrase | "hey jarvis" | Phase 7 | **Closed 2026-11-18 (D-37)** |
| OQ-02 | Search provider | Compare in Phase 8 | Phase 8 | **Closed 2026-11-21 (Phase 8, D-39)** |
| OQ-03 | Quiet hours and default nudges | 22:00–08:00; all on, break reminder as notification | Phase 6 | **Closed 2026-11-17 (D-33)** |
| OQ-04 | Which voice | `af_heart` | Phase 7 | **Closed 2026-11-18 (D-37)** |
| OQ-05 | Notes folder and calendar source | `~/Notes`; local `.ics` files | Phase 3 | **Closed 2026-11-16 (D-25)** |
| OQ-06 | Personal install or public release | Personal | Phase 10 | Open |
| OQ-07 | Cleanup allowlist | Trash, `~/.cache`, named build folders | Phase 2 | **Closed 2026-11-16 (D-23, ADR-0010)** |

---

## 7. Blockers and Watch Items

| Date | Type | Item | Owner | Status |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-21 | Watch | R0-1 / NFR-SEC-03: C7 is the only network door. `cargo deny check` must verify no network crate leaks into C1–C6. | Claude | Open |
| 2026-11-21 | Watch | rules.md §8.4: Zero test traffic leaves the machine. All Phase 8 tests must use private network namespaces (`ip netns`). | Claude | Open |
| 2026-11-16 | Risk | C6 memory with dialog open: MemoryMax is 128 MiB (D-24). Monitor GTK dialog memory during search form. | Claude | Watching |

---

## 8. Session Log

| Date | Summary | Stories | Builder / Checker | Next step |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-21 | Phase 8 verified across all exit criteria, merged into `main`, and `phase-8` branch deleted. Ready for Phase 9. | P8 close | Claude / agy | Phase 9 planning |
| 2026-11-21 | Phase 8 built & verified. C7 anti-SSRF, token verification, atomic v1 spooling, C6 kernel skills, GTK4 query re-hash, C5 lookup routing & taint escalation. Report: `reports/phase-08-fetcher.md`. | P8-S01..S06 | agy / Claude | Gate review & merge to main |
| 2026-11-21 | Owner started Phase 8 (D-38). Created branch `phase-8`. Deep analyzed root docs. Wrote `docs/plans/phase-08-fetcher-plan.md` and `docs/adr/0016-fetcher-and-search.md`. Archived pre-Phase-3 decisions to keep memory.md < 200 lines. Ready for W0/W1. | P8 init | Claude / agy | W0 (dependencies) & W1 (tokens) |
| 2026-11-18 | Voice listening test done. Owner chose `af_heart` and confirmed "hey jarvis" (D-37). Merged `phase-7` to `main`. | P7 close | Claude / Claude | Phase 8 |
| 2026-11-17 | Phase 6 nudge engine signed off (D-33) and merged to `main`. | P6 close | Claude / Claude | Phase 7 |

---

## 9. Update Protocol

1. **Session start:** read §1, §2, §4, §7; read current phase in `phases.md`; state story.
2. **While working:** keep §2 accurate.
3. **Story done:** set it Done in §4 with builder and checker; update §1.
4. **New decision, question or blocker:** add to §5, §6 or §7 at once.
5. **Session end:** add a row to §8, set the next step in §2, push.
6. **Phase gate:** verify every exit criterion, write report, set phase ✅ in §3, reset §4.
