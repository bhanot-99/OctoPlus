# ADR-0011 — Monitor privacy gate, recording and lock source

**Status:** Proposed (decisions taken by Claude under the owner's Phase 3 go-ahead, D-25; the owner may overturn at sign-off)
**Date:** 2026-11-16
**Changes:** Architecture §5.2 / §8.2 (C1 sockets and paths), security.md §8 (C1 boundary), PRD OQ-05 (closed in W7), ADR-0007 item 3 (lock source)
**Evidence:** `reports/phase-03-monitor.md`: lock-source probe (below), `tests/privacy_test.rs` and `tests/anonymise_test.rs` (property tests with negative controls), the live run in the real unit (`scripts/p3-live-check.sh`), `reports/bench/p3-monitor-20261116T155509Z.json`, `just ci` green.

---

## Context

Phase 3 builds C1. ADR-0007 fixed the sensor sources but left the lock source open. PRD FR-MON-10, FR-MON-12 and FR-PRV-01/02 say *that* C1 honours pause and exclusion and anonymises recordings, but not *how* an excluded window looks on the stream, what a pause does to the recorder, or what the anonymiser replaces.

## Decision

The ten decisions in `docs/plans/phase-03-monitor-plan.md` §1 stand as this ADR's decision text. In short: an excluded window becomes a redacted focus event; pause emits nothing and is followed by a resume snapshot; the stream queue drops oldest and counts; `--record` always anonymises with a per-run discarded key; the dump format lives in `neuroos-proto`; replay can serve a stream socket; no `procfs`/`notify` crates; `cosmic-protocols` gets a narrow GPL exception in `deny.toml`; lock state comes from the session bus or a Wayland observer chosen by a probe, and stays `unknown` (nudges hold) if neither works.

### Lock-source probe (2026-11-16, reference machine, COSMIC session)

| Candidate | Result |
| :--- | :--- |
| Session bus `org.freedesktop.ScreenSaver` (owner: `cosmic-idle`) | Introspected: only `Inhibit` and `UnInhibit`. No `GetActive`, no `ActiveChanged`. Cannot observe lock. |
| Session bus `org.gnome.ScreenSaver` | Not present. |
| Session bus `com.system76.CosmicSession` | Only `Exit` and `Restart`. |
| Wayland observer protocol | None. `ext_session_lock_v1` is the *locker's* protocol (held by `cosmic-greeter`), not an observer. |
| logind `LockedHint` (system bus, `loginctl show-session`) | Readable, currently `no`; not shown to flip on a real lock (locking the owner's screen was not done). Needs the system bus, which ADR-0002 and ADR-0007 keep out of every unit. |

**Result:** no verified lock source exists inside C1's allowed surface. Lock state stays **unknown** (`LockChanged` is not emitted without a real source) and the nudge policy holds on unknown, as ADR-0007 item 3 already says. Reopening this needs an owner decision to allow a read-only, single-property logind call, which would widen C1's sandbox (R0-3), so it is listed for sign-off rather than done.

### Additions made while building (Claude, 2026-11-16, same go-ahead)

- **Late subscribers start from the latest state** of each kind (focus, idle, lock, media per player, resources, disk per mount, battery, thermal per sensor) so a reconnecting C3 is not blind. Nothing is handed out while paused, and live state (focus, idle, lock, media) is forgotten across every pause and resume because the gate cannot tell what changed while it was dropping events; the sensors re-emit it on resume (agy review F2).
- **The process sensor learns the focused app from events that passed the gate**, so an excluded app is never looked up (agy review F1).
- **Idle is "no input for the timeout"**: the protocol's version-2 input notification, which ignores idle inhibitors, because a playing video otherwise hides idleness (BUG-055, measured on the real session); version 1 is the fallback.
- **One disk sample per filesystem**, except Btrfs, where (device, root) is the identity (BUG-051).
- **`FileActivity.path` is absolute**; the anonymiser hashes every non-structural component.

## Consequences

C3 sees "focus ended, nothing known" instead of a gap. Paused time is truly absent from every sink. Dumps are safe to commit but cannot be de-anonymised, even by the owner. The lock decision is final only after the probe result is added here.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Silence for excluded windows | Stretches the previous window's span over private time. |
| Raw dumps plus a later anonymise step | A raw recording on disk is exactly what rules §8.7 forbids. |
| `procfs`/`notify` crates | Extra dependencies in an isolated part for code that is small and easier to property-test by hand. |
