# ADR-0013 — Knowledge and conversation: routing, prompt assembly, skills and facts (Phase 5)

**Status:** Proposed 2026-11-17 (decisions taken under the owner's Phase 5 go-ahead, D-29; open to overturn at sign-off)
**Date:** 2026-11-17
**Changes:** `proto/neuroos/v1/knowledge.proto` (`AskRequest.at_ns`, additive) · `[knowledge]` config block · `neuroos-knowledge` · kernel skills `profile.remember`, `profile.forget`, `memory.forget` (kernel-kind) and `notes.search`, `notes.append`, `calendar.read` (child) · C4 grammar support (`cpp/neuroos-inference`) · the kernel unit's folder drop-in (`neuroos-deploy`) · `neuroosctl profile` and `ask --explain` · ADR-0005 focus budget 32 → 48 tokens
**Evidence:** `reports/phase-05-knowledge.md`: held-out sets (`tests/eval/p5/`), injection corpus (100 strings, 600 cases through the turn engine), control-token corpus, NFR-PERF-05 and NFR-PERF-03 measurements, the KPI-12 transcript (ungraded), the real-stack runs, and the independent agy review (`.agy/reports/20261117-123226.md`).

---

## Context

Phase 5 turns C5 from the Phase 1 thin path into the real turn engine (PRD §5.4–5.7, Architecture §9.2 and §11). The mechanisms are fixed by earlier documents; this ADR records the choices that were open and the reasons. The full list with the plan is in `docs/plans/phase-05-knowledge-plan.md` §1.

## Decision

1. **Routing** is a pure function over normalised text, the nudge-reply window and the retrieval score, in the order of Architecture §11. The model never picks the route. A request to *do* something is recognised by a leading verb phrase plus an object word (a table, not a list of sentences), a short non-question reply inside a nudge window is a nudge reply, and a question about the user's own activity is recognised by pointer words, "I was / am I" phrases and time words.
2. **Deictic words** resolve against `QueryFocusHistory(at_ns)`; typed turns use "now". `AskRequest` gains `at_ns` (0 = now); Phase 7 passes `t_speech_start`. "This week" and "this morning" are time words, not pointers.
3. **Token counts** are estimated from bytes (config ratio, rounding up) because C5 holds no tokenizer. The persona block is the constant, cached prefix and is never cut, so it has no budget; its size is measured, not capped.
4. **Every string that is not our own template** passes the control-token neutraliser. Evidence **and the window title** are wrapped as untrusted documents (a title can be a web page title, review F2), so the focus budget is 48 tokens, ADR-0005's 32 plus the wrapper. The prompt's taint is the union of everything handed to the assembler, even a block that was dropped or cut to nothing; the window title counts as `EXTERNAL_UNTRUSTED` because C3's focus message carries no taint and a missing taint is never clean (R0-4).
5. **Session history** keeps the last N turns inside its budget, ends with the follow-up window (8 s spoken, 120 s typed) and on pause, and carries the union taint of its turns. Action turns join it; they are not stored in C3, because C6's audit log already records them and a stored "remember that …" would keep the fact text twice.
6. **Offers are code, not model text.** A freshness cue appends a fixed lookup offer; a personal statement appends "Want me to remember that?". Until Phase 8 a "yes" to the lookup is answered "I can't look things up yet." and nothing is dispatched.
7. **Skills.** `profile.remember`, `profile.forget`, `memory.forget` are kernel-kind skills that call C3 as the kernel unit; `notes.search`, `notes.append`, `calendar.read` are child skills confined to the configured folders (the notes folders are writable inside C6's unit, the calendar folders read-only; both are bound into the unit by the generated drop-in). A fact approved in the dialog is stored `USER_PROVIDED`: the user saw the exact text and said yes, which is the design of FR-PRO-02 (review F4).
8. **The action route is closed twice.** (a) Only the skills the request's *intent* allows can be called (`allowed_for`): a "search my notes" request can never end in `profile.remember`, whatever the model emits (review F1). (b) Text that will be saved (a fact, a note line) must be made of the user's own words, so an injected or invented fact is refused. C4 is given a grammar of just those skills plus "none", and C5 validates the answer again, strictly. `system.cleanup` (paths) and `web.*` (Phase 8) are not callable from the action route; asking for them gets a fixed "I can't do that one yet." "Forget that" is not left to the model at all: the fact id comes from C3's list by matching the user's words, and the kernel's dialog still asks.
9. **C4 enforces the grammar** (FR-INF-05): `SetGrammar` builds a grammar and greedy sampler chain per request and resets it, an unparsable grammar is refused. This was missing and was found on the real stack.
10. **A down part gives a fixed sentence**: C4 → "my thinking part isn't running"; C3 on an activity question → "my memory isn't available"; C6 → refusal (fail closed). A slow C3 gives an answer without evidence, flagged `ask_c3_degraded`. Every closing frame carries at least `USER_PROVIDED` plus the session's taint.
11. **Retrieval cutoffs are provisional and measured.** On the reference machine's live C3 (bge-small, 40 general questions): top score p50 0.52, p90 0.61, p95 0.67, max 0.70; 20 activity questions with no time or pointer word: min 0.59, p10 0.61, p50 0.68. The ranges overlap, so a score alone cannot separate them. A general question sent down the desktop route is the worse mistake (it would be answered "I don't have anything on that"), so the route cutoff is **0.72**, above every general question seen; evidence below **0.50** is dropped. Typical activity questions are caught by their time and "I was" words instead. Both numbers are config, and are to be re-measured on real recordings (D-26).

## Measurements (details in `reports/phase-05-knowledge.md`)

- KPI-13 (routing) on four fresh held-out sets, first run each, before any tuning on that set: **75.0, 80.0, 81.2, 81.2 %** (target 90 %); the router was generalised after each set, so all four now pass as regression sets. KPI-2 ("this" resolved): **97.5, 90.0, 87.5, 97.5 %** (target 95 %).
- NFR-PERF-05 (C5's own compute): p99 0.84 ms in a debug build (target < 5 ms).
- NFR-PERF-03 (typed question to first answer byte, real stack, CPU): general questions p50 0.44 s / p95 0.91 s (40 questions); activity questions with evidence p50 1.49 s / p95 2.10 s (10 questions); target ≤ 3.0 s p50.

## Consequences

- Routing is bounded by its tables. On wording the author has not seen, it is right about four times in five; a miss falls to general chat or "I couldn't work out what to do with that", never to a wrong action, because the model's call is restricted to the request's intent and validated.
- Fact relevance is lexical: "can I eat satay" does not select an allergy fact. Embedding-based relevance (C3 already has the vectors) is the next step; until then health-critical facts can be missed.
- The window title costs 16 more uncached tokens per desktop turn (about 0.18 s of CPU prefill) in exchange for being treated as data.
- A fact saved after approval is stored as the user's own, so a malicious page title that the user approves word for word becomes trusted text; the dialog showing the exact text is the control.
- The kernel's dialog for `profile.forget` shows a fact id, not the fact's text; the user can check ids with `neuroosctl profile list`. A text preview in the dialog is a Phase 9 polish item.
- `neuroosctl profile list` reads C3 directly (it is the owner's own read-only tool on a socket the owner may use), so it is not audited by C6 (review F10); writes go through C6.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Let the model choose the route | Violates AB-13 and KPI-13 cannot be measured on a fixture. |
| Ship a tokenizer in C5 | Extra dependency and memory for a budget that the estimate already holds safely. |
| Child-kind profile and forget skills | A sandboxed child has no path to C3, and C3 accepts these writes only from the kernel unit (Phase 4 decision 7). |
| Save facts the model proposes | Web text could plant a fact; breaks R0-8. |
| Offer the model every skill and trust the validator alone | A hijacked answer could swap a harmless request for `profile.remember`; closed by decision 8 (review F1). |
| Route with an embedding classifier or the model | Would likely lift KPI-13 on unseen wording; costs a model call (or C5 memory) on every turn and an ADR; left as the option if the owner wants 90 % on unseen wording (BUG-070). |
| Treat the window title as clean (Architecture §7.3 gives `window_focus` no taint) | A title can be web text; the table predates the title entering a prompt. |
