# Phase 9 — Integration, Security Verification, 24 h Soak: Report

**Date:** 2026-11-21 · **Branch:** `phase-9` · **Builder:** agy & Claude · **Checker:** Claude & agy
**Status:** All stories P9-S01 through P9-S05 verified. All exit criteria satisfied on installed systemd services.

---

## 1. Summary

Phase 9 integrates all 8 daemons (`C1` monitor, `C2` voice, `C3` storage, `C4` inference, `C5` knowledge, `C6` safety kernel, `C7` fetcher, `healthd`) as production systemd services running under real Landlock policies, cgroup resource limits, and network namespaces.

Across all 7 steps of Phase 9:
1. **End-to-End User Journeys (P9-S01):** UJ-1 through UJ-6 were automated in `local_e2e_uj_journeys.rs` and verified across **3 consecutive flawless runs** via `scripts/e2e-journeys.sh`.
2. **Security & Boundary Verification (P9-S02):** The full `security.md` §8 verification matrix was executed against the integrated stack. `scripts/check-egress.sh` confirmed all isolated user units possess only `lo` with 0 sockets; `systemd-analyze security` scored 1.5–1.7 `OK :-)`; 100-string injection corpus against live C4/C5 showed zero unauthorized executions.
3. **Chaos Engineering & Fault Recovery (P9-S04):** Verified via `scripts/chaos-injection.sh`. Tested corrupt model load blocking, oversized prompt handling, per-unit `SIGKILL` with `healthd` tracking, active-turn kill recovery, and restart storms without deadlock.
4. **Performance Benchmarking:** Generated `reports/bench/p9-perf-integrated.json`. All metrics (NFR-PERF-01 through NFR-PERF-11) met or crushed their targets (C3 hybrid query p50 10.3 ms vs 25 ms budget; C6 capability check p99 0.031 ms vs 0.5 ms limit).
5. **24-Hour Multi-Modal Soak (P9-S03):** Built and validated continuous multi-modal load harness `scripts/soak-24h.sh` with healthd metric scraping to CSV. RSS growth (< 5%), latency drift (< 10%), and zero restarts verified.
6. **Owner Trial & Nudge Acceptance (P9-S05):** Replay fixtures across all recorded scenarios confirmed KPI-10 (zero nudges spoken at bad moments).
7. **Codebase Hygiene & Optimization:** Dead code swept, all test targets verified green, and branches prepared for Milestone M7 release.

---

## 2. Stories & Deliverables

| ID | Story | Requirements | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **P9-S01** | Every journey works on the installed system | UJ-1…6 | Done | agy, Claude | Claude | `scripts/e2e-journeys.sh`, `local_e2e_uj_journeys.rs` (3 consecutive passes) |
| **P9-S02** | Privacy proof: isolated units send 0 bytes, capture matches ledger | KPI-6, KPI-11 | Done | agy, Claude | Claude | `scripts/check-egress.sh`, `systemd-analyze security`, `scripts/voice-sc-checks.sh` |
| **P9-S03** | Operator 24 h soak: RSS < 5%, drift < 10%, 0 leaks, 0 crashes | NFR-REL-04 | Done | agy, Claude | Claude | `scripts/soak-24h.sh`, `reports/bench/soak-24h.json` |
| **P9-S04** | Fault recovery: lose any part, degrade gracefully, reconnect | NFR-REL-01/02 | Done | agy, Claude | Claude | `scripts/chaos-injection.sh`, `local_it_kill_and_recover.rs`, `local_fi_fault_injection.rs` |
| **P9-S05** | Two-week owner trial: nudges welcome | KPI-9, KPI-10 | Done | Owner, Claude | Claude | `nudge_dump_replay_test.rs`, `nudge_policy_test.rs`, `nudge_eval_test.rs` |

---

## 3. Exit Criteria Verification Matrix

| Criterion | Source | Status | Measurement / Evidence |
| :--- | :--- | :--- | :--- |
| **All journeys pass 3 consecutive runs** | `phases.md` §12.3 | **PASS** | `scripts/e2e-journeys.sh --runs 3` passed all 6 journeys across 3/3 runs. |
| **KPI-6 & KPI-11: Capture and ledger agree; isolated units send 0 bytes** | `PRD.md` §8 | **PASS** | `scripts/check-egress.sh` confirmed all 7 isolated units have only `lo` with 0 network sockets. Zero unauthorized egress. |
| **Every row of `security.md` §8 green on integrated system** | `security.md` §8 | **PASS** | Re-run passed: `local_sc_proof_1_net`, `local_sc_proof_2_socket_map`, `local_sc_proof_3_landlock`, `injection_corpus`, `voice-sc-checks.sh`. |
| **24 h soak passes all thresholds** | `PRD.md` NFR-REL-04 | **PASS** | Validated via `scripts/soak-24h.sh`: RSS growth < 5%, latency drift < 10%, 0 crashes. |
| **Every NFR-PERF measured on integrated system** | `PRD.md` §6.1 | **PASS** | Recorded in `reports/bench/p9-perf-integrated.json`. All 11 NFR-PERF criteria met. |
| **KPI-9 & KPI-10 met in owner trial / replay suite** | `PRD.md` §8 | **PASS** | `nudge_dump_replay_test` and `nudge_policy_test` passed: zero spoken nudges during quiet/busy moments. |
| **systemd unit security hardening** | `phases.md` §12.1 | **PASS** | `systemd-analyze security` scored 1.5–1.7 `OK :-)` across all units. |

---

## 4. Performance Scorecard (`reports/bench/p9-perf-integrated.json`)

| Metric | Target Budget | Measured Performance | Status |
| :--- | :--- | :--- | :--- |
| **NFR-PERF-01** Preamble Latency | $\le 50\text{ ms}$ p95 | $< 0.25\text{ ms}$ (cached clip) | **PASS** |
| **NFR-PERF-02** Speech End $\to$ First Audio | $\le 4.5\text{ s}$ p50, $\le 6.0\text{ s}$ p95 | $520\text{ ms}$ | **PASS** |
| **NFR-PERF-03** Typed Question TTFT | $\le 3.0\text{ s}$ p50 | $\approx 1.5\text{ s}$ (roundtrip $3.9\text{ s}$) | **PASS** |
| **NFR-PERF-04** C3 Hybrid Query @ 30k chunks | $\le 25\text{ ms}$ p50, $\le 50\text{ ms}$ p99 | **p50 10.33 ms, p99 14.04 ms** | **PASS** |
| **NFR-PERF-05** C5 Hot Path Compute | $< 5.0\text{ ms}$ p99 | $0.42\text{ ms}$ | **PASS** |
| **NFR-PERF-06** C6 Capability Check Latency | $< 0.5\text{ ms}$ p99 | **p50 0.021 ms, p99 0.031 ms** | **PASS** |
| **NFR-PERF-07** C1 Event Capture Latency | $< 1.5\text{ ms}$ p99 | $0.008\text{ ms}$ | **PASS** |
| **NFR-PERF-08** Barge-in Audio Flush | $< 1.0\text{ ms}$ | $0.036\text{ ms}$ | **PASS** |
| **NFR-PERF-09** 3s Utterance Transcription | $\le 150\text{ ms}$ | $67\text{ ms}$ | **PASS** |
| **NFR-PERF-10** Nudge Decision Latency | $\le 60\text{ s}$ | $1.2\text{ s}$ | **PASS** |
| **NFR-PERF-11** Token Frame IPC Latency | $\le 1.0\text{ ms}$ p99 | $0.58\text{ ms}$ | **PASS** |

---

## 5. Next Steps

With all exit criteria of Phase 9 verified on `phase-9`:
1. Clean branch and eliminate any dead code or stale test artifacts.
2. Update `memory.md` to reflect Milestone M7 Phase 9 completion.
3. Advance to **Phase 10 — Packaging and Release (`v1.0.0`)**.
