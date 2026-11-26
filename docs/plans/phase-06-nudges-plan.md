# Phase 6 build plan — Nudge Engine (inside C5)

**Branch:** `phase-6` · **Stories:** P6-S01…S07 (`phases.md` §9) · **Report:** `reports/phase-06-nudges.md`
**Started:** 2026-11-17 on the owner's go-ahead ("you are already connected with agy, so understand your role and agy's role and start executing phase 6 as planned"), recorded as D-32 in `memory.md` (R0-12).
**Roles:** agy builds the three pure modules (rule loader, rule evaluator, policy gate) against types Claude fixes first, plus fixtures and corpora. Claude builds the engine task, C3/C6/C5 wiring, `neuroosctl nudges`, the replay harness and all tests above unit level, and checks every agy module with an independent probe set. agy then reviews Claude's code read-only. Claude commits. agy prompts follow `agy-needs-bounded-tasks`.

Requirements: `PRD.md` FR-NUD-01…12, NFR-PERF-10, KPI-9, KPI-10, OQ-03; mechanisms `Architecture.md` §6.3 and §10; wording `design.md` §8. Numbers are config or named constants citing the PRD.

---

## 0. Design in one page

```
C3 QueryHealthSignals (every 30 s) ─► signals builder ─► readings ─► EVALUATOR (pure) ─► candidates
                                                                          │  hysteresis · hold_for · cooldown · trend
 user context (pause, quiet hours, turn, media, fullscreen, away) ──────► POLICY GATE (pure) ─► speak │ notify │ hold │ drop
                                                                          │  rate limits · quiet mode · merge of held
                              C6 NotifyRequest(template id + numbers) ◄───┘   (voice arrives in Phase 7)
                              C3 StoreNudgeLog(rule, outcome)           ◄───┘   every nudge and what became of it
 user reply (yes / no / not now / stop) ─► turn engine ─► offered skill dispatched ONLY here, under the normal tiers
```

Rules that shape everything:
- **A nudge never acts (R0-6, FR-NUD-10).** The engine module tree has no import of the action route, `DispatchRequest` or evidence types; a test greps for it. The offered skill is remembered as a *name* and dispatched only by the turn engine when the user's reply says yes. Update 2026-11-25: R0-6 retired by ADR-0019; the no-action property remains as DR-1.
- **Lines are templates and numbers (FR-NUD-04, FR-NUD-12).** C5 sends C6 a template id and numbers; C6 renders. No model text, no titles, no file names.
- **Both decision functions are pure and take time as input (AB-13).** Recorded dumps replay through them at any speed (KPI-10 is proven that way).
- **State lives in C3's nudge log**, not in a file C5 cannot write: cooldowns, snoozes and on/off switches are rebuilt at start by folding the log (`QueryNudgeLog`). Switches are never deleted by retention.
- **A down part idles the engine, it does not guess.** C3 unreachable → no nudges, health DEGRADED. A clock jump resets hold timers instead of firing a burst.
- **Voice is not here yet.** `voice_available = false` until Phase 7: every "speak" decision is downgraded to a notification; the gate itself, with `voice_available = true`, is what the replay proves.

## 1. Decisions Claude takes under the go-ahead (ADR-0014 "Proposed")

| # | Decision | Why |
| :--- | :--- | :--- |
| 1 | Rule file `nudges.toml` (design.md §8.5 format); defaults compiled in; a user file overrides fields of a default rule by id, may add whole new rules, and unknown keys are fatal. Read **before** the sandbox is applied (like the kernel reads config), not bound into the unit. | rules §5, FR-NUD-02 |
| 2 | **Two rules for disk** (`disk_nearly_full` at 90 %, `disk_critical` at 95 %, urgent) instead of an escalation field; the fill-rate rule projects hours-to-full by linear fit over the last hour of free-space samples (needs ≥ 10 min of data and a real rate). | FR-NUD-03, simplicity |
| 3 | **Episode semantics:** a rule fires once per episode: armed → value past `fire_at` for `hold_for` → fires → disarmed until the value passes `rearm` (or the reading disappears). Cooldown additionally spaces episodes. | FR-NUD-02/06 |
| 4 | **Silent hold reasons** follow design.md §8.3 exactly; "limits reached" applies to spoken nudges only; urgent nudges that would be held for pause, quiet hours or a busy microphone become a *silent* notification. Held nudges merge into one "While you were away" line. | FR-NUD-05/06/07 |
| 5 | **"Paused" is read two ways**: the pauses C5 itself dispatches, and stale monitor data (no sample for 3× its interval). C5 may not talk to C1's control socket (socket map), so C1's own pause cannot be asked. | AB-7, FR-PRV-01 |
| 6 | **Quiet hours** use the machine's time zone, loaded before the sandbox (`jiff`), default 22:00–08:00 (OQ-03). | OQ-03 |
| 7 | **Defaults (OQ-03):** all rules enabled except the break reminder, which is delivered as a notification only; important and urgent want voice (downgraded to a notification until Phase 7), info is a notification. | OQ-03 |
| 8 | **Reply window** for text is 120 s after a notification (config), "not now" snoozes 4 h, "stop" disables until `neuroosctl nudges enable`; replies are recognised by the existing router `NudgeReply` route, the *effect* is decided by the engine. | FR-NUD-09 |
| 9 | **Additive contract changes:** `QueryNudgeLog`/`NudgeLogEntries` (C3), outcomes `SNOOZED`, `DISABLED`, `ENABLED`, `ENGINE_IDLE`; C6 templates `nudge.while_away` and `nudge.test`. Nothing removed. | R0-11 |
| 10 | **NFR-PERF-10 is measured from the sample reaching C3:** tick 30 s plus the rule's own hold time. C1 samples disk every 60 s, so "disk full to notification" is bounded by that sample, which is C1's budget, not C5's. | NFR-PERF-10 |
| 11 | **Not built in v1.0 of this phase:** the upcoming-calendar-event rule (P1 in the PRD) and notification action buttons ("Not now" is a text reply). | PRD P1 |

## 2. Work packages, in order

### W0 — Kickoff (Claude)
This plan, ADR-0014, `memory.md` (D-32, board P6-S01…S07), `[nudges]` config, additive proto, `nudge/types.rs` (the shared plain types), module skeleton. Commit.

### W1 — Rule loader (agy) · `nudge/rules.rs`, `tests/nudge_rules_test.rs`, fixtures
`DEFAULT_RULES_TOML` (the v1.0 set), `load(user: Option<&str>) -> Result<Vec<Rule>, RuleError>`: durations ("2m", "24h", "90s"), validation (thresholds ordered for the direction, rearm on the right side of fire, cooldown ≥ 0, template and offered skill exist in a known list), merge by id, unknown keys and duplicate ids fatal. Tests: defaults load and equal the PRD numbers; every field's bad value; merge; hostile TOML.

### W2 — Rule evaluator (agy) · `nudge/eval.rs`, `tests/nudge_eval_test.rs`
`Evaluator::{new, tick, set_enabled, snooze, restore, status}`: episode semantics, hold_for, cooldown, disk fill-rate projection, clock-jump guard (backwards or a forward gap above `max_gap_s` resets hold timers and fires nothing that tick). Tests: boundaries exactly at fire/rearm/hold/cooldown, flapping value, absent reading re-arms, projection numbers, jump cases; property: a rule never fires twice in one episode, never inside its cooldown, never while disabled or snoozed.

### W3 — Policy gate (agy) · `nudge/policy.rs`, `tests/nudge_policy_test.rs`
`Policy::{new, decide, release, held}`: delivery mode per priority and rule, quiet mode, every hold row of design.md §8.3, rate limits (3 per hour, 8 per day, one urgent exception per rule), merge on release, bounded held list. Tests: one table row per situation; property over random context sequences: limits never exceeded, a held nudge is eventually delivered or explicitly dropped for being replaced by a newer one of the same rule and key, never silently lost.

### W4 — Storage and kernel additions (Claude) · C3 `QueryNudgeLog`; C6 templates
Fold query for the log (switches kept past retention), outcome enum additions, `nudge.while_away` and `nudge.test` templates, tests.

### W5 — Engine (Claude) · `nudge/engine.rs`, `nudge/signals.rs`, wiring in `lib.rs`
The 30 s task: signals from C3 (`HealthSignals` → readings, parts-degraded count from C5's own peers and stale monitor data), context assembly, evaluator → policy → C6 notify → C3 log, restore from the log at start, DEGRADED when C3 is unreachable, shared state for replies and the CLI.

### W6 — Replies (Claude) · `turn.rs`
The nudge window opens on delivery; router `NudgeReply` effects: yes → offered skill dispatched under normal tiers, no → dismissed, not now → snooze 4 h, stop → disable; anything else is a new question. Logged.

### W7 — `neuroosctl nudges` (Claude)
`list` (state of every rule), `enable`, `disable`, `test` (deliver one rule's line now through the same path), over the existing knowledge-socket messages.

### W8 — Replay harness and fixtures (Claude harness, agy scenarios)
`neuroos-nudge-replay` binary: dump or synthetic scenario + a context timeline → the pure engine core at any speed → decisions as JSON. Scenarios prove KPI-10 (zero spoken nudges during call, media, fullscreen, pause, quiet hours), clock jumps, flapping signals.

### W9 — Tests across levels (Claude)
Isolation test (no path to evidence or Dispatch from the engine), integration with fake C3 and C6 and then the real stack, fault injection (C3 down → idle and DEGRADED, clock jump), NFR-PERF-10 measurement.

### W10 — Review, gate, report (agy read-only review, Claude gate)

## 3. What needs the owner

- The **one-week trial** (OQ-03, KPI-9 needs two) with the nudges delivered as notifications.
- Sign-off and merge.
