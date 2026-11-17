# neuroos-knowledge

Semantic-NeuroOS Knowledge and Conversation engine (C5). Phase 5 (text); the nudge engine arrives in Phase 6.

## Purpose
One typed (later spoken) turn in, one answer stream out. Order of work (`src/turn.rs`):
1. A pending offer ("look it up?", "remember that?") is answered first, by a plain yes or no.
2. The pure router (`router.rs`) decides action, desktop answer or general chat. The model never chooses.
3. **Action**: the skills the request's intent allows (`action::allowed_for`) are offered to C4 as a grammar, the answer is validated strictly (`action::parse_call`), saved text must be the user's own words, and the call goes to C6 as one `DispatchRequest`. "Forget that" picks the fact from C3's list.
4. **Otherwise**: evidence, the focused window and personal facts are fetched from C3 in parallel under one deadline; evidence is selected (`evidence.rs`), facts chosen (`facts.rs`), the prompt assembled (`prompt.rs`, `sanitize.rs`, `tokens.rs`), C6 is asked to allow `llm.generate` for the prompt's union taint, and C4 streams the answer.
5. The turn joins the session (`session.rs`) and is stored in C3.

A part that is down gives a fixed sentence (`status.rs`); C6 down fails closed.

## Sockets
- `knowledge/knowledge/knowledge.sock`: `AskRequest` from `neuroosctl` and C2.
- `knowledge/health/health.sock`: scraped by `neuroos-healthd`.
- Clients: C6 (evaluate, dispatch), C3 (queries, store), C4 (generate).

## Config (`[knowledge]`, `[deadlines]`)
Evidence count and score cutoffs, the five prompt block budgets, follow-up windows, answer length. Defaults and their sources are in `neuroos-common::config::KnowledgeConfig`; the two score cutoffs are **provisional** (ADR-0013 decision 11).

## Sandbox
Landlock read-only `/proc`; read-write own socket folder; no network (`PrivateNetwork=true`).

## Tests
`cargo nextest run -p neuroos-knowledge`: fixtures for the router, deictic parse, chat rules and evidence; four held-out sets (`tests/eval/p5`); the control-token and injection corpora; the whole engine against fake C3, C4 and C6 (`tests/turn_test.rs`). The real stack is exercised with `scripts/eval-p5.py`.
