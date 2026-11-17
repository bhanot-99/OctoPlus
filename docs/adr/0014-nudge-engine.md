# ADR-0014 — Nudge engine: rules, policy gate, state and delivery (Phase 6)

**Status:** Accepted 2026-11-17 (owner approved all, D-33) (decisions taken under the owner's Phase 6 go-ahead, D-32; open to overturn at sign-off)
**Date:** 2026-11-17
**Changes:** `neuroos-knowledge` (`nudge/`), `proto/neuroos/v1/storage.proto` (`QueryNudgeLog`, four outcomes, additive), C3 (`QueryNudgeLog`), C6 notification templates, `[nudges]` config, `nudges.toml`, `neuroosctl nudges`
**Evidence:** to be filled from `reports/phase-06-nudges.md`.

---

## Context

The buddy should speak first, rarely and kindly (PRD G-2, FR-NUD-01…12, Architecture §10, design.md §8). Voice does not exist yet (Phase 7), so delivery in this phase is a desktop notification rendered by C6 from a template id and numbers. The decisions below are the ones left open; the list with reasons is `docs/plans/phase-06-nudges-plan.md` §1.

## Decision

See the plan §1 (items 1–11); in short: a declarative rule file with compiled-in defaults; two pure functions (evaluator, policy gate) that take time as an input; rules fire once per episode (hysteresis) and respect a cooldown; hold rules exactly as design.md §8.3; state (cooldowns, snoozes, on/off switches) rebuilt from C3's nudge log, with switch rows kept past retention; "paused" inferred from the pauses C5 dispatched and from stale monitor data; the engine never imports the action route or evidence types; the offered skill is a name dispatched only by the turn engine on the user's reply.

## Consequences

- The engine is a pure core (`Core::step`: readings, context and time in, decisions out) shared by the live loop and the replay harness, so the KPI-10 proof runs the same code that runs live (AB-13).
- A nudge cannot act: the engine tree imports no action, dispatch or evidence code (source-scan test), the only text-bearing message it sends is a template id with numbers, and an offered skill runs only when the user replies yes, through the normal tiers.
- State lives in C3's nudge log and is rebuilt at start; switches survive retention and forget. A C3 outage at start leaves the engine idle (reported as degraded) and it retries each tick.
- Cost: held, dropped and answered rows are logged on every tick they occur, so the log grows; restore reads only state rows, so growth does not affect correctness (BUG-073).
- Measured: KPI-10 is zero over 22 scenarios and over the three recorded dumps (178 ticks each, 55 to 81 bad moments, 3 spoken at good ones, none at a bad one). Voice does not exist yet, so speech is proven only in replay with voice available.
- Not built: upcoming-calendar-event rule, notification buttons, call detection and mic-busy (Phase 7).

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Keep rule state in a file C5 writes | C5's sandbox may write only its socket folder; widening it is a security change for no gain, C3 already stores the log. |
| Ask C1 whether it is paused | Not in C5's socket row (AB-7); would need a map change for one bit that stale data already shows. |
| One rule with an escalation field for disk | Two rules are simpler to test and to switch off separately. |
| Let the model word the nudge | Forbidden by FR-NUD-04. |
