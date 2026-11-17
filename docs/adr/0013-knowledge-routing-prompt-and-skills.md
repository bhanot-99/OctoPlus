# ADR-0013 — Knowledge and conversation: routing, prompt assembly, skills and facts (Phase 5)

**Status:** Proposed 2026-11-17 (decisions taken under the owner's Phase 5 go-ahead, D-29; open to overturn at sign-off)
**Date:** 2026-11-17
**Changes:** `proto/neuroos/v1/knowledge.proto` (`AskRequest.at_ns`, additive) · `[knowledge]` config block · `neuroos-knowledge` · kernel skills `profile.remember`, `profile.forget`, `memory.forget` (kernel-kind) and `notes.search`, `notes.append`, `calendar.read` (child) · `neuroosctl profile`
**Evidence:** to be filled from the Phase 5 report (`reports/phase-05-knowledge.md`): router and deictic fixtures (KPI-13, KPI-2), injection and control-token corpora, NFR-PERF-05 benchmark, held-out KPI-1 and KPI-12 transcripts.

---

## Context

Phase 5 turns C5 from the Phase 1 thin path into the real turn engine (PRD §5.4–5.7, Architecture §9.2 and §11). The mechanisms are fixed by earlier documents; this ADR records the choices that were open and the reasons. The full list with the plan is in `docs/plans/phase-05-knowledge-plan.md` §1.

## Decision

1. Routing is a pure function over normalised text, the nudge-reply window and the retrieval score, in the order of Architecture §11. The model never picks the route. Pattern data lives in one table shared by code and fixtures.
2. Deictic words resolve against `QueryFocusHistory(at_ns)`; typed turns use "now". `AskRequest` gains `at_ns` (0 = now); Phase 7 passes `t_speech_start`.
3. C5 estimates tokens from bytes (config ratio, rounding up) because it holds no tokenizer; block budgets are exact on the estimate and conservative on real tokens.
4. Every string that is not our own template passes the control-token neutraliser; tainted evidence is wrapped in an untrusted-document block the text cannot close; the prompt taint is the union of all inputs and goes to C6 for the generate check and every dispatch. C3 evidence with a missing taint is dropped and counted.
5. Session history keeps the last N turns inside its budget, ends with the follow-up window and on pause, and carries the union taint of its turns.
6. Freshness cues append a fixed lookup offer. Until Phase 8 an accepted offer is answered "I can't look things up yet." and nothing is dispatched.
7. `profile.remember`, `profile.forget` and `memory.forget` are kernel-kind skills that call C3 as the kernel unit; `notes.search`, `notes.append`, `calendar.read` are child skills confined to config folders.
8. C5 never saves a fact from model output. A proposal is a question; the user's "yes" becomes `profile.remember`.
9. A down part produces a fixed status sentence within the control deadline; a slow C3 produces an answer without evidence, flagged; a down C6 refuses (fail closed).

## Consequences

- Routing and deictic quality are bounded by the fixtures; a miss falls to general chat with the persona's own "I don't have anything on that." rule, never to a wrong action.
- The byte-based token estimate wastes some budget on code-heavy evidence; the eval harness measures the margin against C4's real counts.
- Facts need the user's explicit word, so the buddy never "learns" silently (R0-8), at the cost of one extra turn for a proposal.
- The lookup offer is a dead end until Phase 8; the fixed line keeps that honest.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Let the model choose the route | Violates AB-13 and KPI-13 cannot be measured on a fixture. |
| Ship a tokenizer in C5 | Extra dependency and memory for a budget that the estimate already holds safely. |
| Child-kind profile and forget skills | A sandboxed child has no path to C3, and C3 accepts these writes only from the kernel unit (Phase 4 decision 7). |
| Save facts the model proposes | Web text could plant a fact; breaks R0-8. |
