# ADR-0001 — Record decisions; documents v2.0 are the baseline

**Status:** Accepted by the owner 2026-11-15
**Date:** 2026-11-14
**Changes:** rules.md §3.6 · memory.md §5
**Evidence:** owner approval of the v2.0 documents, 2026-11-14 (memory.md D-11)

---

## Context

The project restarted from scratch on 2026-11-14. An earlier prototype taught us that decisions spread across a growing state file contradicted each other and the other documents (docs/lessons-from-v1.md §1). The rebuild needs one baseline and one place where each decision and its reasons live.

## Decision

1. The v2.0 documents — PRD, Architecture, security, rules, phases, design, README, memory — are the single baseline. Each fact has one home (rules.md §11).
2. Any decision that changes architecture, a contract, security posture, a dependency, a budget or a target is recorded as an ADR in `docs/adr/`, numbered from 0001, using `docs/adr/_TEMPLATE.md` (Context, Decision, Consequences, Alternatives).
3. An ADR is Proposed until the owner accepts it. memory.md §5 holds a one-line entry pointing to it.
4. Accepted ADRs are not edited in substance; a later ADR supersedes them.
5. ADRs that rest on a measurement name it: what, on what machine, for how long, and the commit.

## Consequences

- Reasons survive after the conversation that produced them.
- Small overhead per decision; decisions that do not change any of the items in (2) need only a memory.md line.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Decisions only in memory.md | It grew to 151 KB and contradicted itself in the prototype. |
| Decisions in commit messages | Hard to find, no review step, no superseding. |
