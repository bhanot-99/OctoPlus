# ADR-0006 — Build order

**Status:** Proposed (written 2026-11-14 in Phase 0; the order was approved by the owner as D-08)
**Date:** 2026-11-14
**Changes:** phases.md §2 · Architecture §15 #13–15
**Evidence:** docs/lessons-from-v1.md §1 and §4

---

## Context

The prototype left integration to a late phase, built storage before it had real input to test against, and added safety checks after features. Each caused rework or bugs found only at the end.

## Decision

Phases run in this order (phases.md §2), one at a time (R0-12):

0. Foundation and spikes — measure the riskiest assumptions before building on them.
1. Walking skeleton — every part runs as a sandboxed service from the start, with real health.
2. Safety kernel (C6) — every later feature is born behind the gate.
3. Monitor (C1) — produces the real recordings storage is tested on.
4. Storage (C3).
5. Knowledge and conversation (C5, text only).
6. Nudge engine.
7. Voice (C2).
8. Fetcher and search (C7) — the only networked part comes last, behind a finished gate.
9. Integration, security verification, 24 h soak.
10. Packaging and release.

The owner may direct work on a later phase; that direction is written in memory.md first.

## Consequences

- Integration problems show up in Phase 1 rather than Phase 9.
- Voice, the most visible feature, arrives late; text conversation is usable earlier through `neuroosctl`.
- The order is a plan, not a contract: a spike result that breaks an assumption reopens it with a new ADR.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Voice first (as in the prototype) | Showed progress early but left safety, storage and integration to be retrofitted. |
| Storage before monitor | Storage's noise gate would be tuned on invented data. |
| Fetcher earlier | Puts the only networked part in place before the gate that controls it. |
