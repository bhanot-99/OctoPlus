# Phase 6 — Nudge Engine: Report

**Date:** 2026-11-17 · **Branch:** `phase-6` · **Builder:** Claude (engine, signals, reply handling, C3/C6/ctl/deploy changes), agy (rules, evaluator, policy gate, replay harness, scenarios) · **Checker:** Claude (independent probes), agy (read-only review of Claude's code)
**Status:** Built, `just ci` green. Owner items left: one-week trial (OQ-03; KPI-9 needs two weeks), sign-off, merge. Design calls are D-32 and open to overturn.

## 1. Summary
C5 now watches disk, battery, memory and temperature (plus a long-session rule) and tells the user through notifications, never at a bad moment and never repeatedly. A nudge never acts: it sends a template id and numbers to C6; if it offers help, only the user's reply ("show me") can run the named skill, under the normal tiers. "Not now" snoozes 4 h, "stop" switches the rule off until `neuroosctl nudges enable`. Every nudge and outcome goes to C3's nudge log, and cooldowns, snoozes and switches are rebuilt from it at start.

Honest caveats:
1. **Voice does not exist (Phase 7)**: no nudge is spoken in reality. "Spoken at a bad moment is zero" is proven in replay with voice made available, not live.
2. **The reply path was not demonstrated on the real stack.** A real nudge fired and was logged, but the machine was idle at the time, so the gate correctly held it (`Hold Away`) and no reply window opened. The reply path (yes/no/not now/stop, window expiry, skill dispatch) is proven by the engine integration tests with fakes, not by a live reply.
3. Not built: upcoming-calendar-event rule, notification buttons; call and mic-busy detection arrive with voice. "Paused" is inferred from pauses C5 dispatched plus stale monitor data.

## 2. Stories
All seven (P6-S01..S07) Done; evidence in `nudge_rules_test`, `nudge_eval_test`, `nudge_policy_test`, `nudge_signals_test`, `nudge_engine_test`, `nudge_isolation_test`, `nudge_replay_test`, `nudge_dump_replay_test`, `nudges_test` (ctl), `query_test`, `kernel_test`, testkit `local_it_nudge_switch_survives_restart`.

## 3. Measurements
- **KPI-10** (spoken at a bad moment): 0 over 22 scenarios; 0 over the three recorded dumps replayed through the real signal builder and core (178 ticks each, 55/69/81 bad moments, 3 spoken at good ones each).
- **KPI-9** (nudges the user keeps on): not measurable yet; needs the owner's two-week trial.
- **NFR-PERF-10** and fault injection (C3 down at start, clock jump): C3-down handled and tested (engine idle, degraded, retries); clock-jump guard covered by evaluator tests. No separate real-stack timing measurement was made.

## 4. Bugs found (BUGS.md)
- BUG-071 (S2): on sockets that also admit `neuroosctl`, the peer's unit was never resolved, so C3 refused every per-message-checked write in the real units (turns were never stored since Phase 4/5). Fixed; real-unit regression test.
- BUG-072 (S3): sandbox hid `nudges.toml`; now bound read-only into the knowledge unit.
- BUG-073..075 (agy review): restore query capped at 5,000 rows could lose cooldowns; enabling a rule kept its snooze; test notifications could sound at a bad moment. All fixed with tests.

## 5. Owner to do
Run the one-week trial and decide OQ-03 defaults; grade nothing else new; sign off and merge. Review D-32 and ADR-0014.
