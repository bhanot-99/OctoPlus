# Phase 5 — Knowledge and Conversation (C5, text only): Report

> **Update 2026-11-17 (BUG-070 fixed, D-34):** the router was generalised and re-measured on four blind sets; KPI-13 on unseen wording is now about 94 % (first run on the last set), not 80 %. The numbers below are the Phase 5 sign-off state.

**Date:** 2026-11-17 · **Branch:** `phase-5` · **Builder:** Claude (engine, assembler, action route, kernel and C4 changes, child skills), agy (router, sanitiser, token budget, deictic parse, evidence selection, session, facts, chat rules, injection corpus) · **Checker:** Claude on every agy module (own probe sets, found and fixed gaps in each); agy on Claude's code (read-only review, 10 findings)
**Status:** Built, gate green (§7), owner decisions taken 2026-11-17 (D-31: recommendations accepted, BUG-070 accepted as is, ADR-0013 accepted). **Ready to merge.** Still open for the owner: grade KPI-12; KPI-1 at integration (D-26)

## 1. Summary

Typed questions now go through the real turn engine. A pure router picks one of four routes. A question about your activity is grounded in what C3 stored: the window you were looking at, evidence above a cutoff, no duplicates, every piece wrapped as data and counted in the taint sent to C6. A general question gets an honest answer, with a fixed offer to look it up when it depends on fresh facts. The buddy keeps a short session for follow-ups, remembers facts only when you say so (and asks first when you merely state one), and can pause, forget, search notes, add to a note, read the calendar and report disk use. All of it was run on the real stack (real C3, C4, C6 and sandboxed skill children).

Three things are **not** where the plan wanted them, in order of weight:

1. **KPI-13 on wording the router has not seen is about 80 %, not 90 %** (§4). It is 100 % on the fixtures, which is how the PRD defines the KPI, so the box can be ticked as written, but the honest estimate for new phrasing is the first-run number. Options are in §6; the owner decides (BUG-070).
2. **KPI-1 (answers about recorded activity) is not measured**: it needs real recordings, which the owner deferred to integration (D-26). The harness and the way to run it are ready.
3. **KPI-12 is run but not graded**: 40 questions, real stack, transcript in `reports/p5-kpi12-transcript.md`, grading is the owner's.

The real stack also found two things the fakes could not: C4 had **no grammar support at all** (FR-INF-05 was never built, so a skill request came back as free-form JSON), and the first grammar C5 generated was malformed. Both are fixed and covered (BUG-063, BUG-064).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P5-S01 | "This" means the window I was looking at | Done | agy (parse), Claude (wiring) | Claude | `deictic_test`, `turn_test::this_means_the_window_at_the_time_asked`, `a_minute_ago_...`; KPI-2 §4 |
| P5-S02 | Grounded answers; a status answer when a part is down | Done | agy (selection), Claude | Claude | `evidence_test`, `turn_test` (grounding, no-taint drop, C3 down, C3 slow, C4 down) |
| P5-S03 | Every turn goes down the right route | Done, KPI-13 caveat | agy (router), Claude (generalised) | Claude | `router_test` (140 fixtures), `eval_heldout`; §4 |
| P5-S04 | Right taint to C6; outside text cannot break the prompt | Done | agy (sanitiser), Claude (assembler) | Claude, agy review | `sanitize_test`, `prompt_test`, `turn_test` taint and injection tests |
| P5-S05 | Follow-ups without repeating | Done | agy | Claude | `session_test`, `turn_test::a_follow_up_...` |
| P5-S06 | Sensible, honest everyday answers | Done, ungraded | agy (rules), Claude | owner (KPI-12) | `chat_test`, `reports/p5-kpi12-transcript.md` |
| P5-S07 | Remembers facts I ask it to, uses them when relevant | Done | agy (relevance), Claude (kernel, CLI) | Claude, agy review | `facts_test`, `kernel_skills_test`, `profile_test`, `turn_test` offer tests |
| P5-S08 | Pause, forget, search notes, add a note | Done (cleanup and web deferred, §6) | Claude | agy review | `action_test`, `notes_calendar` (incl. real sandboxed child), real-stack runs §5 |

## 3. Exit criteria

| Criterion | Met? | Evidence |
| :--- | :--- | :--- |
| KPI-2 and KPI-13 met | **Partly.** On the fixtures: yes (100 %). On four fresh sets, first run each: KPI-13 75.0 / 80.0 / 81.2 / 81.2 %, KPI-2 97.5 / 90.0 / 87.5 / 97.5 % | §4, BUG-070 |
| KPI-1 and KPI-12 graded by the owner on held-out sets at real deadlines; transcripts saved | **Not yet.** KPI-12: transcript saved, ungraded. KPI-1: needs real recordings (D-26) | `reports/p5-kpi12-transcript.md`, `scripts/eval-p5.py` |
| Injection and control-token corpora pass | Yes | 100 hostile strings × 6 hijacking model answers = 600 cases through the whole engine; 85 control-token lines in every prompt slot; negative controls |
| Performance targets measured | Yes | NFR-PERF-05 p99 0.84 ms (debug build); NFR-PERF-03 first byte p50 0.44 s general, 1.49 s with evidence (target 3.0 s) |
| Phase report; owner sign-off | Report: yes. Sign-off: pending | this file |

## 4. Measurements

**Routing and "this" (KPI-13, KPI-2).** Each set was written without looking at the router's tables for it, run once, and recorded; the router was then generalised from that set's misses (a table of verb phrase + object word instead of phrase lists, a rule for short non-question replies in a nudge window, "I was / am I" activity phrases), and a *new* set was written. Sets A–D are now regression sets and pass 100 %, which says nothing about new wording.

| Set | KPI-13 first run (target 90 %) | KPI-2 first run (target 95 %) |
| :-: | :-: | :-: |
| A | 75.0 % (60/80) | 97.5 % (39/40) |
| B | 80.0 % (68/85) | 90.0 % (36/40) |
| C | 81.2 % (65/80) | 87.5 % (35/40) |
| D | 81.2 % (65/80) | 97.5 % (39/40) |

The estimate for unseen wording is therefore routing ≈ 80 % and "this" ≈ 93 %. The plateau across four sets, despite generalisation after each, says a phrase-based router has reached its limit; the misses are mostly short nudge replies and unusual wording of skill requests. A wrong route is mild by design: the model's call is limited to the request's intent and validated, so a miss ends in general chat or "I couldn't work out what to do with that", never in a wrong action.

**Latency (NFR-PERF-03, typed question to first answer byte, real stack, CPU path).** General questions, 40 asked: p50 0.44 s, p95 0.91 s, whole answer p50 5.1 s. Activity questions with evidence, 10 asked: p50 1.49 s, p95 2.10 s. Target ≤ 3.0 s p50. The stable persona prefix stays in C4's cache, so only the question and evidence are prefilled.

**C5's own compute (NFR-PERF-05).** Route, parse, selection, assembly with neutralising, 2,000 turns, debug build: p50 0.76 ms, p99 0.84 ms (target < 5 ms).

**Memory.** C5 4 MiB resident on the real unit (budget 60 MiB, limit 96 MiB).

**Retrieval scores on the real C3** (bge-small, templated chunks; ADR-0013 decision 11): 40 general questions top score p50 0.52, p95 0.67, max 0.70; 20 activity questions without cue words: min 0.59, p50 0.68. The ranges overlap, so the desktop-route cutoff is 0.72 (above every general question seen) and evidence under 0.50 is dropped. **Provisional**: measured on one machine's live data, to be repeated on real recordings.

## 5. Tests and real-stack runs

| Level | What | Result |
| :--- | :--- | :--- |
| UT / PT | router, deictic, evidence, tokens, sanitiser, session, facts, chat, action table, grammar well-formedness, prompt budgets, wrapper integrity, taint union, read-only result sentences | 112 tests in `neuroos-knowledge`; proptests for no-panic, idempotence, budgets, taint ⊇ union |
| IT | the whole engine against fake C3, C4, C6 (every route, offers, follow-ups, cancel, real service start); kernel skills against a fake C3; `neuroosctl profile` against fake C3 and C6; the three child skills in a **real** Landlock and seccomp child | all pass |
| IT (real stack) | `neuroosctl ask` for every route on real C3, C4, C6 and real skill children; audit log shows `ATTEMPT` and `RESULT` for `privacy.pause` and `system.disk_report` | pass after BUG-063/064; `calendar.read` failed honestly (no calendar folder configured) |
| PF | NFR-PERF-05 and NFR-PERF-03, §4 | met |
| FI | C4 down → status line inside 350 ms (250 ms control deadline + margin); C3 down → status line for activity questions, answer without evidence otherwise; C3 slow (600 ms) → answer without evidence; C6 down or refusing → no generation | pass |
| SC | injection corpus 600 cases; control-token corpus; hostile window titles; skills cannot escape (symlinks, `..`, size limits, read-only invocation cannot write) | pass |
| AT | KPI-13/KPI-2 on fixtures and held-out sets; KPI-12 transcript | §4; KPI-12 awaits the owner |

C4 (C++): 3 new tests (grammar reaches the engine and does not leak to the next request; unparsable grammar refused without generating; **real model** obeys a grammar against a hostile prompt, then returns to free text).

## 6. Decisions and open items for the owner

1. **BUG-070 — routing on unseen wording ≈ 80 %.** Choose: (a) accept (the router is a fallback to general chat, never to a wrong action; KPI-13 is a fixture KPI in the PRD); (b) spend a later task on an embedding or small-classifier router (needs an ADR; costs memory or a model call per turn); (c) widen the tables again (cheap, but the four sets suggest ~85 % at best).
2. **Cleanup and web are not callable from the action route.** `system.cleanup` takes paths a model must not choose; `web.*` needs Phase 8. "Clean up my disk" gets "I can't do that one yet." `system.disk_report` works. Say if you want cleanup offered by voice with the kernel's own path list.
3. **Fact relevance is lexical** ("can I eat satay" does not select "I am allergic to peanuts"). Safe facts could be always included, or relevance made embedding-based inside C3.
4. **Dialog for `profile.forget` shows a fact id**, not the text.
5. **Provisional cutoffs 0.50 / 0.72** and the 8 s / 120 s follow-up windows.
6. **The window title is treated as untrusted** (taint `EXTERNAL_UNTRUSTED`), so a "what is this window" turn makes the next 120 s of requests for risky skills ask for approval. Conservative on purpose; say if it is too much.
7. **ADR-0013 acceptance**, including the focus budget 32 → 48 tokens (ADR-0005) and the C4 grammar change.
8. **What needs you:** grade KPI-12 (40 answers, fields at the end of each), provide recordings for KPI-1 at integration, sign off and merge.

## 7. Gate

`just ci` was run **to the end and is green** (format, lint with clippy `-D warnings`, clang-format and clang-tidy, shellcheck, machete, tests, dependency policy and network-ban probe, coverage, ASan/UBSan and TSan):

| Step | Result |
| :--- | :--- |
| Rust tests (`nextest`, whole workspace) | 668 passed, 3 skipped (includes the real-unit proofs and the real-stack e2e ask-and-cancel) |
| C++ tests (debug) | 65 of 65 |
| `cargo deny`, network-ban probe | ok |
| Rust coverage (gate ≥ 80 % lines) | 85.5 % lines overall; this phase's files 83–100 % (`turn.rs` 93.7, `router.rs` 97.2, `prompt.rs` 96.7, `action.rs` 96.8, `deictic.rs` 83.2, `notes.rs` 92.3, `calendar.rs` 93.9, `kernel_skills.rs` 96.2) |
| C++ coverage (gate ≥ 70 % lines) | 78.8 % |
| ASan + UBSan, TSan | 61 of 61 each |

The first full run failed at coverage on `local_e2e_ask_and_cancel`: the Phase 1 test retries `neuroosctl ask` until it exits 0, and a down C4 now gives a spoken status line (FR-KNO-10) that exited 0, so the test took the line for an answer. Fixed properly rather than in the test: `AnswerDone` gained `status_only` and `degraded` (additive), and `neuroosctl ask` exits 1 for a status line. A second attempt stopped at the gate's own disk preflight (9.9 GiB free, 10 required); the 11 GB coverage build cache was removed and the whole gate re-run from the start.

## 8. Review

agy read the Claude-built code of this phase (turn engine, assembler, action route, kernel skills, child skills, CLI) with no tools but `cat`/`grep`/`git diff` and reported 10 findings (`.agy/reports/20261117-123226.md`); each was checked against the code. Fixed: F1 (the model could swap the skill; BUG-065), F2 (window title was clean text; BUG-069), F3 (taint lowered on some paths; BUG-066), F5 (open-ended forget; BUG-067), F6 (actions not in the session; BUG-068), F7 (injection test too weak; now six hijacking answers), F8, F9 (BUG-060, BUG-061). By design, documented in ADR-0013: F4 (a fact approved in the dialog is stored as the user's), F10 (`profile list` reads C3 directly). Claude's own probe sets found gaps in every agy module (activity phrases, medicine names, version numbers, question words in fact statements, nudge replies); the fixes are in the same commits.

## 9. Files

`crates/neuroos-knowledge/` (all of `src/`, `tests/`, README), `crates/neuroos-kernel/` (kernel-kind skills, folder roots, per-skill write limit, Landlock grants), `crates/neuroos-skill/` (`notes.rs`, `calendar.rs`), `crates/neuroosctl/` (`profile`, `ask --explain`), `crates/neuroos-deploy/` (kernel folder drop-in), `crates/neuroos-common/` (`[knowledge]`), `proto/neuroos/v1/knowledge.proto` (`at_ns`), `cpp/neuroos-inference/` (grammar), `scripts/eval-p5.py`, `tests/eval/p5/`, `docs/adr/0013-*`, `docs/plans/phase-05-knowledge-plan.md`, `security.md` §8, `BUGS.md` (BUG-059…070).
