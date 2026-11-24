# Phase 20 plan — Final polish, optimisation and high-level security testing

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24 (D-63). **Branch (when started):** `polish-security`.
**Needs:** Phases 12 to 19 built; runs on the Mac (Phase 15 done).

## 1. The goal

Make OctoPlus fast, calm and trustworthy enough to live with every day, and prove the security claims with tests
an outsider would run. Nothing new is added here; things are made better and checked.

## 2. Polish and optimisation

| Area | Target (to be confirmed on the Mac hardware) | How it is measured |
| :-- | :-- | :-- |
| Answer speed | First sound under 3 s median for a spoken answer (run 2 baseline: probe median 0.8 s, p99 4.1 s) | The voice baseline script, the soak probe |
| Startup | Stack ready in under 20 s after login; OctoPit window under 1 s | Timed scripts |
| Memory | Each part inside its budget as resident plus swapped; total under a stated cap for the Air's memory | The soak, `phys_footprint` on the Mac |
| Battery and heat | Idle draw and fan-free sustained use on an Air; background work paused on battery below a level | Energy log over a one-hour idle and a one-hour use run |
| Storage | Search under 150 ms at the 30,000-chunk cap; backups and GC invisible to the user | Existing perf tests, re-run on the Mac |
| Look and feel | Consistent spacing and motion, light and dark, accessibility checklist (keyboard, screen reader, contrast, reduced motion) | The OctoPit UX acceptance list |
| Voice quality | Echo and barge-in tested in a real room; Companion timing feels natural (not too chatty) | The owner's trial notes and the KPI recordings |
| Errors | Every failure has a plain message and a next step; nothing shows a stack trace | A fault-injection pass over each part |
| Install and update | Drag-and-drop install, first-run, update and uninstall leave no stray files | A clean macOS user account test |

## 3. High-level security testing

The aim is to be as hostile to our own system as a careful stranger would be.

| Test | What it proves |
| :-- | :-- |
| **Threat model refresh** | `security.md` re-read against the new parts: cloud path, encrypted storage, OctoPit settings, Companion memory. A written list of assets, doors and what protects each |
| **Egress verification** | A packet capture over a full day plus an active session: the only network traffic comes from the fetcher and goes to the allowed hosts; zero bytes in local-only mode |
| **Privacy gate red team** | Canary secrets (fake keys, names, addresses, health words) planted in memory and in the screen log; thousands of generated questions; none of the canaries may appear in any cloud request. The gate's tests are run against several redaction attacks (encodings, splitting, paraphrase) |
| **Prompt injection** | Hostile web pages, documents and file names try to make the assistant act, leak memory or change settings; nothing may run without the approval dialog |
| **Sandbox escape** | Each part tries forbidden file, socket and network access from inside; all fail with the expected error |
| **IPC fuzzing** | The existing fuzz targets run for hours on frames, requests and settings writes; new targets for the new calls |
| **Storage encryption review** | Key handling, nonce and salt use, backup and restore, what a stolen disk or a copied database file reveals; an independent reader checks the design |
| **Dependency and supply chain** | `cargo deny` and audit clean, pinned model hashes verified, a software bill of materials, a reproducible build check, signed release |
| **Local attacker** | Another user account and a running non-sandboxed app try to read data or the sockets; macOS permission prompts reviewed |
| **Independent review** | The independent review still owed since Phase 9 is done by someone who did not build the code (a person, or a fresh Claude session with no memory of the work, run on the diff) |

Findings are numbered like the earlier audit (BUG-nnn), ranked S1 to S4, and every S1 and S2 is fixed or accepted
in writing by the owner before release.

## 4. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P20-S01 | Performance pass on the Mac: profile, fix the top five costs, re-measure | Targets in §2 met or the gap explained |
| P20-S02 | Battery, heat and idle behaviour; pause rules | The one-hour energy runs pass |
| P20-S03 | UX polish pass and accessibility fixes | UX acceptance list all green |
| P20-S04 | Threat model refresh and the test plan | Reviewed by the owner |
| P20-S05 | Egress capture, privacy red team, injection suite | Report with numbers, no canary leaks |
| P20-S06 | Fuzzing, sandbox escape, local attacker tests | Report; findings logged |
| P20-S07 | Encryption review and supply-chain checks | Review notes; checks clean |
| P20-S08 | Independent review and fixes for S1 and S2 | Re-test of each fix |
| P20-S09 | The release soak: 24 h on the Mac (the PRD length), with swap recorded | PASS verdict, report in `reports/` |
| P20-S10 | Release candidate: signed build, install test on a clean account, release notes | Owner signs off |

## 5. Exit criteria

- No open S1 or S2 finding. All targets in §2 met, or each gap accepted in writing.
- The 24 h soak passes on the Mac. Egress capture shows no unexpected traffic.
- The owner has lived with it for two weeks (the trial) and signs off. Tag the release.
