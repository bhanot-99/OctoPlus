# Phase 1 — Walking Skeleton: Report

**Date:** 2026-11-16 · **Commit:** see §5 · **Builders:** agy (T1–T4, first draft of T5), Claude (T5 rework, IPC/units/fetcher fixes, soak and bench drivers; owner D-21) · **Checker:** Claude (every agy result re-run; agy's T5 rejected and redone)
**Status:** ✅ Signed off by the owner 2026-11-16; ADR-0009 accepted

## 1. Summary

**In plain words.** Every part of NeuroOS now starts as a real, locked-down background service under `neuroos.target`. Each one reports its health, and each restarts and reconnects by itself after a crash. A typed question goes `neuroosctl ask` → C5 → C6 (always "SAFE" for now) → C3 (placeholder evidence) → C4 (the real Qwen3-4B model), and the answer streams back. Three security proofs ran against the real services, each with a check that shows the proof can fail. They show that no part can reach the network, that each part can see only the sockets it is allowed to use, and that each part locks its own file access before reading any input.

Checking found two serious problems that agy's reports did not mention:
- **C4 was running with network access** because of a systemd quirk.
- **healthd would have gone blind after about 30 minutes** because of a connection leak in the shared IPC library.

Both are fixed, with tests. The owner installed the fetcher (C7) on the morning of 2026-11-16. It runs as its own system user and recovers from a kill like the other parts.

**Technically.** Phase 1 built:
- the socket map as code (`neuroos-common::socketmap`), with one folder per socket (ADR-0009);
- a unit generator (`crates/neuroos-deploy`) with the §8.1 baseline;
- the shared Rust service bootstrap (`neuroos-health::service`), with Landlock applied before any input;
- the five Rust parts and two C++ parts as thin real services;
- C4 on llama.cpp with streaming, cancel, prefix reuse and a one-shot bench mode;
- healthd (scrape, cgroup v2, aggregate, soak CSV) and `neuroosctl status|ask`.

SC proofs 1–3, kill-and-recover for every user part, fault injection and the 2-hour mini-soak ran on the real units (§3, §4).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P1-S01 | Every unit has no network and the hardening baseline from its first start | Done (C7: system unit, installed by the owner) | agy (T1), Claude (BUG-028 fix) | Claude | `local_sc_proof_1_net`; `scripts/check-egress.sh` (7/7 PASS, negative control fails); generator test `every_user_unit_has_exactly_one_exec` |
| P1-S02 | Each unit reaches only its allowed sockets | Done | agy (T1), Claude (BUG-018 policy fix review) | Claude | `local_sc_proof_2_socket_map` with negative control; deploy tests compare binds to `socketmap` |
| P1-S03 | Each process restricts its own file access before reading input | Done | agy (T2, T3), Claude (proof rewrite, BUG-025) | Claude | `local_sc_proof_3_landlock`: canary denied in all 7 real units, readable without Landlock in each unit's namespace |
| P1-S04 | healthd shows every part's real status, memory and latency | Done | agy (T4), Claude (BUG-027) | Claude | `neuroosctl status`; `local_it_kill_and_recover` (healthd is the judge); healthd fds flat over 90 scrapes |
| P1-S05 | C4 streams a cancellable completion from the chosen model | Done | agy (T3, T4 fixes) | Claude | ctest real-model tests; `cancel_to_idle` ≤ one decode step; bench in the real unit (§4) |
| P1-S06 | `neuroosctl ask` streams an answer through the whole chain | Done | agy (T2, T4) | Claude | E2E test; mini-soak asks (§4) |

## 3. Exit criteria

| Criterion (phases.md §4.4) | Met? | Evidence |
| :--- | :--- | :--- |
| All units start under `neuroos.target` and report real health | Yes | `neuroosctl status` with C7 installed: healthy 7/8. healthd, storage, inference, monitor, kernel, knowledge and fetcher are OK; voice is DEGRADED ("audio not built", true until Phase 7). C6 is OK once its fetcher link is up. |
| The three SC proofs pass, each with its negative control | Yes | SC1 probe + check-egress; SC2 namespace visibility, with fetcher sockets unreachable and two negative controls (BUG-035); SC3 canary — see §2 and §5 |
| `neuroosctl ask` works end to end; C4 numbers recorded against ADR-0005 | Yes | §4 |
| Kill-and-recover verified for every part | Yes, all 7 (healthd is the observer) | §4 table |
| Coverage gates met; phase report; owner sign-off | Gate green (§5) · sign-off pending | — |

## 4. Measurements

Conditions unless stated: reference machine (Ryzen 7 5800H, 8 threads for C4, governor `performance`, AC power), release build, real units, Qwen3-4B-Instruct-2507 Q4_K_M, repacking off, ctx 2048, batch 512, 2026-11-16.

**C4 (release binary, plain process, Claude, 3 reps, 208-token synthetic prompt, 64 new tokens, 2026-11-16):**

| Measure | Result | ADR-0005 |
| :--- | :--- | :--- |
| Prefill | 83.1 tok/s | 87–90 |
| Decode | 11.1 tok/s | 11.6–11.9 |
| Time to first token | 2,614 ms | — |
| Peak RSS (`/usr/bin/time`) | 2,763 MiB | budget 2,800, cap 3,220 |

**C4 in its real unit sandbox:** `scripts/bench-p1.sh`, Claude, 2026-11-16, commit see §5. This is the installed `neuroos-inference.service` with only `ExecStart` swapped for the one-shot `--bench` mode, so every hardening line and the Landlock policy are the shipped ones. Figures are the mean of 5 reps after one warm-up, with 64 new tokens. Raw data: `reports/bench/bench-p1-20261116T011219Z.json`.

| Prompt tokens | Prefill | Decode | Time to first token | Peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 208 (ADR-0005 uncached part) | 89.1 tok/s | 11.93 tok/s | 2,430 ms | 2,765 MiB |
| 384 (whole prompt layout) | 89.6 tok/s | 11.93 tok/s | 4,382 ms | 2,788 MiB |

In the sandbox, C4 runs within ADR-0005's ranges: prefill 87–90 and decode 11.6–11.9 tok/s. The earlier plain-process run above was slightly lower (83.1 and 11.1 tok/s). The cause is not established; the sandbox does not slow C4. Peak RSS for the full 384-token layout is 2,788 MiB, which is 12 MiB under the 2,800 MiB budget (§8).

**Cold start** (`systemctl --user start neuroos.target` from fully stopped until healthd shows C4 OK; 1 s scrape; includes the model hash check unit; model already in page cache): **2,590 / 2,569 / 2,582 ms**, from `reports/bench/bench-p1-20261116T045454Z.json`. These are upper bounds, off by up to 1 s. That run was after a reboot, with the governor reset to `powersave`, so its throughput figures are not used; for cold start, `powersave` can only make it slower. The first measurement (1,584 and 1,609 ms) was wrong: the stop before each start had not finished, so C4 was never restarted (BUG-034).

**Cancel (FR-INF-04):** `cancel_to_idle` 552 ms against a 569 ms mean decode step (debug build of llama.cpp, ctest `local_RealModelCancelStopsWithinOneDecodeStep`, Claude's rerun). The earlier figure of "0.1 ms" was the IPC end frame, not the model stopping (BUG-021).

**Kill-and-recover** (`local_it_kill_and_recover`, healthd scrape 1 s, `RestartSec=2s`, Claude, 2026-11-16):

| Part | healthd shows DOWN | Back to its baseline status | All parts back at baseline |
| :--- | ---: | ---: | ---: |
| storage | 1,024 ms | 3,061 ms | 3,062 ms |
| inference (incl. hash check + model load) | 1,023 ms | 4,995 ms | 4,996 ms, then `ask` OK |
| monitor | 412 ms | 2,449 ms | 4,385 ms |
| kernel | 1,022 ms | 3,059 ms | 3,059 ms |
| knowledge | 1,020 ms | 2,956 ms | 2,956 ms |
| voice | 1,022 ms | 3,058 ms | 3,059 ms |
| fetcher (C7, system unit; owner ran the kill with sudo; Claude's watcher polled healthd every 100 ms) | 494 ms | 2,468 ms | — |

The model hash check unit took 1.52 s of CPU per C4 start (systemd accounting).

**Mini-soak (2 h):** `scripts/soak-p1.sh --duration 7200 --interval 20`, Claude, 2026-11-16. A question went through the real chain every 20 s, and healthd ran in soak mode with a 30 s scrape. Pass rule (phases.md §4.3): every part's RSS grows ≤ 2 % from the baseline (median of minutes 10–15) to the end (median of the last 10 min), with no restarts and no failed asks.

**Run 1 (`soak-p1-20261115T230421Z`): not passed.** All asks succeeded (360/360, time to first chunk median 454 ms, p95 476 ms) and every part passed except healthd. healthd grew from 4.3 to 5.2 MiB (+19.7 %) because its rolling window stored full status copies and kept growing for an hour (BUG-030). C4 was flat at 2,767.2 MiB (0.0 %).

**Run 2 (`soak-p1-20261116T011409Z`), after the fix: PASS.**

| Part | Baseline MiB | Final MiB | Growth % | Rise in last 30 min % | Restarts | Verdict |
| :--- | ---: | ---: | ---: | ---: | ---: | :--- |
| healthd | 5.0 | 4.8 | −3.29 | −4.96 | 0 | pass |
| inference (C4) | 2,768.8 | 2,768.7 | −0.00 | −0.00 | 0 | pass |
| kernel | 4.4 | 4.4 | 0.09 | 0.00 | 0 | pass |
| knowledge | 4.6 | 4.6 | −0.34 | −1.66 | 0 | pass |
| monitor | 4.4 | 4.3 | −0.89 | −1.28 | 0 | pass |
| storage | 4.4 | 4.4 | 0.00 | 0.00 | 0 | pass |
| voice | 6.9 | 6.9 | −0.06 | −0.03 | 0 | pass |
| fetcher | — | — | — | — | — | not running (owner install) |

360 asks with 0 failed. End-to-end time to first chunk: median 454 ms, p95 554 ms. The p95 is higher than in run 1 because the hosted gate ran at nice 19 during part of run 2. Raw data: `reports/bench/soak-p1-20261116T011409Z.{csv,asks.log,status.jsonl,json,md}`.

**Skeleton RSS per part** (healthd, during the soak): see the soak table. All parts other than C4 are under 8 MiB.

## 5. Gate run

`just ci` on the working tree committed as this phase's final commit (branch `phase_1`), Claude, 2026-11-16, after the reboot with C7 installed and running: **ALL GREEN**.

| Step | Result |
| :--- | :--- |
| fmt-check, clippy `-D warnings`, clang-format, clang-tidy, shellcheck, cargo machete | pass |
| Rust tests (nextest, including all 10 `local_*` real-unit tests) | 160 / 160 |
| C++ tests (ctest, including real-model tests) | 62 / 62 |
| `fetch-models-test` | 16 checks ok |
| cargo deny + netban probe | ok; ban fires for `ureq`, `hickory-resolver` |
| Rust line coverage (gate 80 %) | 82.34 % |
| C++ line coverage (gate 70 %) | 78.6 % |
| AddressSanitizer + UBSan, ThreadSanitizer | 59 / 59 each |

`just sc` with the units up: SC1, SC2 and SC3 pass; `check-egress.sh` 7/7 PASS (only `lo`, 0 sockets in each user unit).

The gate took four runs on the last day. Each failure was a real defect and was fixed, not retried:
- the E2E cancel never cancelled (BUG-033);
- the stop→start race (BUG-034);
- C7's install exposed the SC2 check (BUG-035);
- the user manager lacked the new group (environment; reboot).

## 6. Deviations, bugs and descoped items

- **Fetcher (C7) install is by hand, with sudo**: `sudo bash scripts/install-fetcher.sh`, then `sudo systemctl enable --now neuroos-fetcher`. The script does not enable the unit, so after the first reboot C7 was down until the owner enabled it. The new group `neuroos` takes effect only when the **user systemd manager** restarts. A re-login is not enough if any session stays open (this one ran from Nov 14), so a reboot was needed. Until then, healthd and C6 fail to start, because their bind of the fetcher folder is denied. Packaging (Phase 10) must handle both.
- **C7's kill-and-recover is not an automated test.** Killing a system unit needs root, so the owner ran the kill and Claude's watcher timed it through healthd (§4). C7's other behaviour is covered by its in-process tests.
- **agy's T5 was rejected and redone by Claude** (owner D-21). Its tests did not test what its report claimed (BUG-029):
  - C4's host network namespace was downgraded to a warning (BUG-028).
  - SC proof 3's "Rust parts" never started a service.
  - Kill-and-recover never consulted healthd, and quoted `Restart=always`/`100 ms`, which no unit has.
  - The fault-injection "unit" case only ran `sha256sum` by hand.
  - The end-to-end cancel check could never fail, because C4's health socket rejects the test.
  - `--skip-restrict` had been added to the shipped C++ binaries.

  agy also wrote a soak driver that would have stopped after one loop (`sys` not imported) and read the wrong folder. It was replaced.
- **Bugs this phase:** BUG-017 … BUG-035, all fixed (`BUGS.md`). BUG-030 to BUG-035 were found during the soak, the bench and the gate on the last day: healthd rolling-window growth, world-writable dev install files, two script bugs, two weak or flaky tests, and the SC2 fetcher check. One is S1 (BUG-028, C4 network namespace) and several are S2: BUG-018 per-socket policy, BUG-020 Landlock too wide, BUG-024 invented DEGRADED rule, BUG-026 fetcher sockets, BUG-027 IPC connection leak, BUG-029 hollow tests.
- **Second-layer peer check in real units** (a server rejecting a wrongly named unit) is not proven in the real units. It is covered by the `neuroos-ipc` peer tests. SC proof 2 proves the primary namespace layer.
- **Cosmetic:** `neuroosctl status` shifts the table by a few characters for the `ms/tok` column. Latency histograms use coarse buckets, so C5's p99 shows bucket bounds (e.g. "10000 ms"). Both will be fixed when the first latency target is measured end to end (Phase 2).
- **Branch:** one branch `phase_1` with a commit per step (owner's choice, D-20) instead of one branch per story (rules.md §3.2).

## 7. Decisions and ADRs

| Item | Status |
| :--- | :--- |
| ADR-0009 — one folder per socket, socket map as code, dev install; revised: model check as its own unit, group-shared fetcher sockets | Accepted by the owner 2026-11-16 (D-22) |
| ADR-0002 addendum — one exec per user unit (systemd 255 drops `PrivateNetwork=` otherwise) | Evidence addendum; the rule is enforced by the generator test |
| D-20 | Owner started Phase 1 with agy; Claude's design calls recorded |
| D-21 | Owner: Claude may build when agy struggles; finish overnight |

## 8. Next phase: risks and first steps

- **Memory headroom:** C4 sits at 2,724–2,768 MiB in normal use and peaks at 2,788 MiB on a full 384-token prompt, against a 2,800 MiB budget (cap 3,220). Phase 2's real persona and skills prompt must be measured against it before anything else is added to C4.
- **systemd quirks:** the one-exec rule applies to every future unit, including the skill runner and the dialog.
- **Phase 2 (safety kernel):** C6 tier evaluation, HMAC approval, audit log, dialog, notifications. The fetcher's link to C6 is first used there, so C7 must be installed before Phase 2's integration tests.

## 9. Sign-off

- [x] Owner sign-off — owner (bhanot-99), 2026-11-16 ("signoff - approved"); ADR-0009 accepted on Claude's recommendation (D-22)
