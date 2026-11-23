# ADR-0018 — Cloud-first answers behind a Privacy Gate; encrypted storage; local model as fallback

**Status:** Proposed, OPTIONAL and on hold 2026-11-23 (used only if a week of real use on the MacBook shows the local model is not enough). Owner decisions recorded in `docs/plans/phase-12-cloud-first-plan.md` §10 (trust the gate for P1, TPM-only unlock, local model loads on demand for P2, tags yes); work has NOT started and waits for the owner's instruction.
**Date:** 2026-11-23
**Changes (if accepted):** `rules.md` R0-2 (standing approval policy for a defined class of requests), `PRD.md` §3 (cloud answers in scope), `Architecture.md` (C7 provider adapters, C5 router and Privacy Gate, C4 on demand, C3 encryption), `security.md` (provider terms, key handling, residual risks).
**Evidence:** provider terms read on 2026-11-23 (Gemini unpaid tier may be used to improve products and reviewed by humans; Perplexity API documents zero-day retention; Claude has no standing free API tier). Links in the plan.

## Context

The local model is small (2.7 GiB, a wrong answer is common, slowest answers about 4.5 s) and uses most of the memory budget. The owner wants better answers from cloud models while keeping privacy. Free cloud tiers may train on and show submitted text; paid APIs usually do not.

## Decision (proposed)

1. Answers are cloud-first through C7 (the only network part), with the local model (C4, loaded on demand) for offline use and for the class that never leaves.
2. Every question is classified by a local **Privacy Gate** into P0 public, P1 personal-light, P2 stays local. P0 may use any provider; P1 only no-training providers after minimisation and redaction; P2 never leaves.
3. One outbound text per request; it is the text the policy checks, the ledger records and C7 sends (the existing exact-text rule). Standing policies replace per-request dialogs only for P0 (and for P1 if the owner chooses); a daily spend and request cap is enforced.
4. Cloud text is untrusted (taint model unchanged); no action is triggered by it without C6's tiers.
5. Storage is encrypted at rest with a random data key sealed by the TPM (and an optional passphrase); hardware readings are never key material; rotation wraps the key, not the data.
6. Audio never leaves the machine.
7. agy (the Antigravity CLI) is not used as a backend.
8. Privacy is verified by automated tests (canary secrets, ledger equals packet capture, redaction corpus, class tests, injection tests, log scan) in the gate.

## Consequences

Good: much better answers, a lighter local footprint (memory freed while online), a measurable privacy claim, theft-safe storage. Bad: depends on the internet and on provider terms that change; small recurring cost; added latency; a larger attack surface in C7 and in the key handling; more rules to maintain. Cost that cannot be made zero: the provider sees what is sent, so the guarantee is "we control what leaves", not "what the provider does next".

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Free cloud tiers for everything | The free Gemini tier may train on and show text to reviewers: incompatible with personal context |
| Keys derived from hardware readings | Guessable and not recreatable: data would be lost or weakly protected |
| Give the cloud model access to storage | Breaks the central privacy promise |
| agy as the backend | Not a documented stable API; unclear terms; fragile |
| Stay fully local | Answer quality stays the main weakness |
