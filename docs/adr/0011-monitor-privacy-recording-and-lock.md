# ADR-0011 — Monitor privacy gate, recording and lock source

**Status:** Proposed (decisions taken by Claude under the owner's Phase 3 go-ahead, D-25; the owner may overturn at sign-off)
**Date:** 2026-11-16
**Changes:** Architecture §5.2 / §8.2 (C1 sockets and paths), security.md §8 (C1 boundary), PRD OQ-05 (closed in W7), ADR-0007 item 3 (lock source)
**Evidence:** to be filled in by the phase report: lock-source probe, anonymiser property tests, performance JSON in `reports/bench/`.

---

## Context

Phase 3 builds C1. ADR-0007 fixed the sensor sources but left the lock source open. PRD FR-MON-10, FR-MON-12 and FR-PRV-01/02 say *that* C1 honours pause and exclusion and anonymises recordings, but not *how* an excluded window looks on the stream, what a pause does to the recorder, or what the anonymiser replaces.

## Decision

The ten decisions in `docs/plans/phase-03-monitor-plan.md` §1 stand as this ADR's decision text. In short: an excluded window becomes a redacted focus event; pause emits nothing and is followed by a resume snapshot; the stream queue drops oldest and counts; `--record` always anonymises with a per-run discarded key; the dump format lives in `neuroos-proto`; replay can serve a stream socket; no `procfs`/`notify` crates; `cosmic-protocols` gets a narrow GPL exception in `deny.toml`; lock state comes from the session bus or a Wayland observer chosen by a probe, and stays `unknown` (nudges hold) if neither works.

## Consequences

C3 sees "focus ended, nothing known" instead of a gap. Paused time is truly absent from every sink. Dumps are safe to commit but cannot be de-anonymised, even by the owner. The lock decision is final only after the probe result is added here.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Silence for excluded windows | Stretches the previous window's span over private time. |
| Raw dumps plus a later anonymise step | A raw recording on disk is exactly what rules §8.7 forbids. |
| `procfs`/`notify` crates | Extra dependencies in an isolated part for code that is small and easier to property-test by hand. |
