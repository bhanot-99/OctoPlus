# Phase 5 build plan — Knowledge and Conversation (C5, text only)

**Branch:** `phase-5` · **Stories:** P5-S01…S08 (`phases.md` §8) · **Report:** `reports/phase-05-knowledge.md`
**Started:** 2026-11-17 on the owner's go-ahead ("you are already connected with agy, so understand your role and agy's role and start executing phase 5 as planned"), recorded as D-29 in `memory.md` (R0-12).
**Roles:** agy builds small, closed, pure modules, the skill programs and the corpora. Claude builds the orchestrator, the prompt assembler, the kernel-side skills and everything that needs judgement. The other side checks each package (rules §2). Claude commits; agy never does. agy prompts follow `agy-needs-bounded-tasks`: exact signatures, a closed file list, no exploring, a time box.

Requirements: `PRD.md` FR-KNO-01…10, FR-CHT-01…06, FR-PRO-01…05, FR-KER-10, KPI-1/2/12/13, NFR-PERF-03/05. Mechanisms: `Architecture.md` §9.2 (layout), §11 (router). Numbers are config defaults or named constants citing the PRD; none is repeated here.

---

## 0. Design in one page

```
AskRequest (text, origin, at_ns) ─► ROUTER (pure, AB-13) ─► one of: ACTION · DESKTOP · GENERAL · NUDGE_REPLY
                                         │
   DESKTOP: deictic? ─► C3 QueryFocusHistory(at_ns)  ┐ in parallel, hard deadline
            evidence ─► C3 QueryHybrid(text)         ┘ → cutoff + dedupe → Evidence block
   GENERAL: personal facts (relevance) + freshness cues → lookup offer
   ACTION : GBNF grammar built from the registry ─► C4 ─► validate call ─► C6 Dispatch
                                         │
   PROMPT ASSEMBLER (pure): persona | facts | focus | history+question | evidence   (ADR-0005 budgets)
       every non-template string: control tokens neutralised; tainted evidence wrapped; taint = union
                                         │
   C6 Evaluate(llm.generate, union taint) ─► C4 Generate (stream) ─► AnswerChunk … AnswerDone(route, taint)
                                         │
   after the turn: C3 StoreInteraction (question, answer summary, taint); session history updated
```

Rules that shape everything:
- **The model never chooses the route** (Architecture §11). Routing is a pure function of text, session state and the nudge window; one generation per turn.
- **Taint is the union of everything in the prompt** and is sent to C6 for the generate check and for every dispatch. Nothing clears it (R0-4). A missing taint on C3 evidence is an error and that evidence is dropped, never treated as clean.
- **Outside text is inert.** Anything that is not our own template (evidence, titles, facts, history, the question itself) passes the control-token neutraliser; tainted evidence is also wrapped in an untrusted-document block that the text itself cannot close.
- **A skill call is a request, never an order.** The action route only builds a grammar-limited call; C6 decides. Facts are saved only on the user's word (R0-8): a model-proposed fact is a question to the user, not a write.
- **C5's own compute stays under NFR-PERF-05**: the router, deictic parse, assembler and neutraliser are allocation-light pure functions, measured with a benchmark.
- **Down parts degrade, they do not hang** (R-AB-10, rules §6.6): C4 down → status line within 250 ms; C3 down or slow → answer without evidence, flagged; C6 down → refuse (fail closed).

## 1. Decisions Claude takes under the go-ahead (ADR-0013 "Proposed", open to overturn at sign-off)

| # | Decision | Why |
| :--- | :--- | :--- |
| 1 | **Router signals are word-boundary patterns over normalised text** (lower-case, punctuation folded), kept in one data table so fixtures and code share it. Order is the Architecture §11 order. No model call. | AB-13, KPI-13 |
| 2 | **Deictic words** (`this`, `that`, `it`, `here`, "what I was doing/looking at") resolve against `QueryFocusHistory(at_ns)`. Typed turns use "now" as the time of speech; voice (Phase 7) will pass `t_speech_start`. `AskRequest` gets one additive field `at_ns` (0 = now). | FR-KNO-01 |
| 3 | **Token counting is a conservative estimate** (UTF-8 bytes ÷ a config ratio, rounded up) because C5 holds no tokenizer; budgets are exact on the estimate and never over on real tokens. C4 reports real counts, and the eval harness checks the estimate's margin. | FR-KNO-05, ADR-0005 |
| 4 | **Control-token neutraliser** removes the model's reserved markers (`<\|…\|>` family, `<untrusted_document>` tags, role words at line start after a marker) by replacing the opening character with a visible look-alike that the tokenizer does not treat as special; applied to every non-template string; idempotent. | FR-KNO-06, SC corpus |
| 5 | **Session history** keeps the last N turns (config) as `question / short answer`, newest kept first when trimming; it ends when the follow-up window closes (config) and is dropped on pause. History text is C5-produced but contains user and model text, so it is neutralised and carries the union taint of its turns. | FR-KNO-09 |
| 6 | **Lookup offer** (FR-CHT-03): freshness cues (`latest`, `best`, `price`, `news`, version numbers, "today"…) append a fixed offer sentence; until Phase 8 a "yes" is answered with the fixed line "I can't look things up yet." and no dispatch happens. | FR-CHT-03/04 |
| 7 | **Profile, memory.forget are kernel-kind skills** that call C3 with the kernel's own peer identity (C3 already allows profile writes and forget only from the kernel unit). `notes.search`, `notes.append`, `calendar.read` are child skills confined to config folders. | Phase 4 decision 7, FR-KER-05 |
| 8 | **Fact proposals:** C5 never saves a fact from model output. It can ask "want me to remember that?" and a following "yes" routes to `profile.remember`. | R0-8, FR-PRO-01 |
| 9 | **Status answers** for a down part are fixed sentences keyed by which part is down, sent as ordinary chunks with `route` set and taint empty. | FR-KNO-10 |
| 10 | **Additive contract changes only:** `AskRequest.at_ns`; envelope entries for the profile messages C5 and `neuroosctl` already need if absent. Nothing removed or renumbered. | R0-11 |

## 2. Work packages, in order

Each: owner · files · what to build · how tested · done when. File lists are closed: nobody touches another package's files.

### W0 — Kickoff (Claude)
This plan, ADR-0013 (Proposed), `memory.md` (D-29, board P5-S01…S08), `[knowledge]` config block (`neuroos-common`), additive proto, module skeleton in `neuroos-knowledge/src/{router,deictic,evidence,sanitize,tokens,prompt,session,chat,action,facts,turn,status}.rs` as empty documented modules. Commit.

### W1 — Turn router (agy) · `router.rs`, `tests/fixtures/router.jsonl`, `tests/router_test.rs`
`route(input: &RouteInput) -> RouteDecision`, pure. `RouteInput { text, nudge_window_open: bool, retrieval_best_score: Option<f64> , cutoff: f64 }`. `RouteDecision { route, deictic: bool, time_ref: bool, action_hint: Option<ActionHint> }`. Order: nudge reply → action → desktop/memory → general. Fixture file of ≥ 120 labelled lines (JSON: text, expected route) covering all four routes plus tricky near-misses ("remember" in chat vs. an action, "it" in "is it going to rain", "forget" as a plain word). Tests: every fixture, boundary cases, proptest that it never panics and never returns ACTION for text with no action word, snapshot of the decision table. **Done when:** fixtures pass and are the KPI-13 set.

### W2 — Sanitiser and wrapper (agy) · `sanitize.rs`, `tests/sanitize_test.rs`, `tests/fixtures/control_tokens.txt`
`neutralise(&str) -> String` (idempotent, never longer than 2× input, keeps ordinary text byte-identical), `wrap_untrusted(&str) -> String` (neutralises first, then wraps; the wrapped body cannot contain the closing tag in any case/spacing/unicode-lookalike form), `contains_control_token(&str) -> bool`. Corpus v1 ≥ 60 lines (ChatML, Llama, Gemma, Qwen tool/think tags, fake closing tags, nested wrappers, zero-width and homoglyph tricks, huge repeats). Tests: table, proptest idempotence, "after neutralise `contains_control_token` is false", negative control. **Done when:** the control-token corpus is inert.

### W3 — Token budget and truncation (agy) · `tokens.rs`, `tests/tokens_test.rs`
`estimate(&str, ratio) -> u32`; `truncate_to(&str, budget_tokens, ratio) -> &str` on a char boundary, exact (the result never exceeds the budget, and one more char would). Tests: table, proptest over unicode, never splits a char, monotonic.

### W4 — Deictic and time parse (agy) · `deictic.rs`, `tests/deictic_test.rs`
`parse(text) -> DeicticInfo { deictic: bool, anchor: Anchor }` where `Anchor` ∈ `Now | Earlier(Duration) | Today | Yesterday | LastHour | None`; pure, word-boundary, ≥ 60 fixture lines (KPI-2 set). Tests as W1.

### W5 — Evidence selection (agy) · `evidence.rs`, `tests/evidence_test.rs`
`select(evidence: Vec<Evidence>, cutoff, max) -> Selected` : drops evidence with missing taint (counted), below cutoff, duplicates by chunk id and by normalised text, keeps best-first order, returns the taint union and a `dropped` breakdown. Tests: table, proptest (union ⊇ each kept item, no duplicates, order stable).

### W6 — Session history (agy) · `session.rs`, `tests/session_test.rs`
`Session::new(cfg)`, `push(turn, now)`, `expire(now)`, `render(budget) -> (String, TaintFlags)`; time passed in, fake-clock tested. Newest turns kept first under the budget; expired after the window; `clear()` on pause.

### W7 — Prompt assembler (Claude) · `prompt.rs`
Builds the five blocks of ADR-0005 in order, each truncated to its budget, stable prefix first and measured, evidence wrapped, everything non-template neutralised, returns `Prompt { text, stable_prefix_chars, taint }`. Property tests: never over total budget; wrapper never split; output taint ⊇ union of inputs; persona block identical across turns (cache prefix).

### W8 — General chat rules (agy) · `chat.rs`, `tests/chat_test.rs`
`freshness_cue(text) -> bool`, `professional_topic(text) -> Option<Topic>` (health/legal/money), `LOOKUP_OFFER`, `NO_LOOKUP_YET`. Fixture ≥ 60 lines each way. Pure.

### W9 — Facts relevance (agy) · `facts.rs`, `tests/facts_test.rs`
`pick(facts: &[Fact], question: &str, budget_tokens) -> (Vec<&Fact>, TaintFlags)` : token-overlap relevance with stop words removed; deterministic; never exceeds the budget; includes nothing when nothing overlaps; neutralised rendering.

### W10 — Action route (Claude) · `action.rs`
GBNF built from the registry's argument schemas (only SAFE/REVIEW skills; DANGEROUS and disabled never appear), parse and strictly validate the model's JSON (unknown skill, unknown key, wrong type, too long → rejected), then `Dispatch` with the turn's union taint. Property tests: no output of the validator names a skill outside the list; round-trip of every valid call. The "yes" to a proposal routes to `profile.remember`.

### W11 — Remaining skills (Claude kernel side, agy child side)
- Claude: kernel-kind `profile.remember`, `profile.forget`, `memory.forget` in `kernel_skills.rs` calling C3; `implemented = true` in the manifests; registry and tier tests.
- agy: `notes.search`, `notes.append`, `calendar.read` as child programs in `neuroos-skill` (config folders only, size and count caps, UTF-8 only, no symlink following; escape tests in the style of `tests/escape.rs`).

### W12 — Orchestrator (Claude) · `turn.rs`, `status.rs`, `lib.rs`
Replaces the Phase 1 handler. Router → parallel C3 queries under a hard deadline → assembler → C6 evaluate → C4 stream → `AnswerDone` with route and union taint → store interaction → session update. Status lines for down parts. Cancellation as before. Health: histograms for route, assemble and C5-own compute (NFR-PERF-05), counters per route and per drop reason.

### W13 — `neuroosctl profile` and `ask` (Claude) · `neuroosctl/src/profile.rs`, `ask.rs`
`profile list / add / remove` (FR-PRO-04; `add` is the user's own typed word, so it is `USER_PROVIDED`), `ask` prints the route and taint with `--explain`.

### W14 — Evaluation harness and corpora (Claude harness, agy corpora)
Harness `tests/eval/p5/` : KPI-13 (router fixtures), KPI-2 (deictic fixtures against a seeded C3), KPI-1 and KPI-12 (question files written **before any tuning**, held-out set frozen with a commit hash, transcripts saved for the owner to grade at real deadlines). agy writes: injection corpus v1 (≥ 80 hostile evidence/title/fact/question strings with a stated forbidden outcome each) and the control-token corpus (W2).

### W15 — Tests across levels (Claude)
Integration with real C3 + C6 and a scripted stand-in for C4 (`neuroosctl ask` for each route); fault injection (C4 down ≤ 250 ms, C3 slow → flagged answer without evidence, C6 down → refused); security (injection corpus: zero unauthorised dispatches, zero unconfirmed saved facts; control-token corpus inert); performance bench for NFR-PERF-05 (p99) and NFR-PERF-03 (needs real C4: owner-approved run).

### W16 — Independent review, gate, report (agy review, Claude gate)
Read-only agy review of the whole phase diff; Claude verifies every finding in code, fixes, reruns `just ci`; `reports/phase-05-knowledge.md`, `memory.md`, `phases.md` ticks, ADR-0013 stays Proposed until the owner signs off.

## 3. What needs the owner (listed once, at the end)

- **KPI-1 and KPI-12 grading** on the held-out sets, transcripts supplied (needs the real model running).
- **Sign-off** and ADR-0013 acceptance; merge to `main`.
- Any model run long enough to need approval (the real C4 for latency numbers).
