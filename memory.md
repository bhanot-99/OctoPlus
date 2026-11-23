# memory.md — Project State

> **Read this first, every session.** It holds the current state, open items and the next step — nothing else.
> Keep it under 200 lines. Newest entries on top. Dates are absolute (YYYY-MM-DD).
> When a log table passes 15 rows, move the oldest to `docs/history/`. Long explanations go in an ADR, a phase report or `BUGS.md`, with one line here pointing to them.

---

## 1. Status Snapshot

| Field | Value |
| :--- | :--- |
| Last updated | 2026-11-23 |
| Project stage | **Post-cleanup** (2026-11-23): Phases 0-7 hold; the hardening audit, the voice upgrade (D-55) and the cleanup (D-56) are merged into `main` and pushed (`3eb7a27`); hosted CI is green for the first time. Phases 8-10 stay in gate review until the owner steps are done. |
| Current phase | Phases 8-10 in gate review |
| Current story | None on `main`. OctoPit (D-57, the GUI dashboard) is built on branch `cockpit` (worktree `Semantic-NeuroOS-cockpit`), ready to merge: run `just ci` on `main` after the merge |
| Overall progress | 8 / 11 phases signed off on real evidence (0-7); 8-10 need owner steps (24 h soak, two-week trial, `.deb` install, fresh-VM proof) and sign-off |
| Health | 🟩 Local gate (`just ci`, 16:21-16:55) and hosted CI green on `3eb7a27`; 🟨 24 h soak not run (stopped by the owner for the voice upgrade); owner trial and sign-offs open |
| Next milestone | Restart the 24 h soak on the final code (after the OctoPit changes to C1 and the proto, if any, are merged); the owner items listed in D-56 |

---

## 2. Currently Working On

| Field | Value |
| :--- | :--- |
| Story | Cleanup of `main` finished (D-56). Nothing in progress on `main` |
| Builder / Checker | Claude / Claude (agy not used) — an independent review is still owed |
| Branch | `main` = `origin/main` = `3eb7a27`. Other work lives in worktrees: `cockpit` in `/home/bhanot/Semantic-NeuroOS-cockpit` (do not switch branches in the main folder while another session works) |
| Files being changed | none on `main` |
| Next concrete step | Owner: restart the 24 h soak (`scripts/soak-24h.sh --hours 24`), then the owner items in D-56 |

---

## 3. Phase Tracker

| Phase | Name | Status | Started | Finished | Report |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | Foundation and spikes | ✅ | 2026-11-14 | 2026-11-15 | `reports/phase-00-foundation.md` |
| 1 | Walking skeleton | ✅ | 2026-11-15 | 2026-11-16 | `reports/phase-01-skeleton.md` |
| 2 | Safety kernel (C6) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-02-kernel.md` |
| 3 | Monitor (C1) | ✅ | 2026-11-16 | 2026-11-16 | `reports/phase-03-monitor.md` |
| 4 | Storage (C3) | ✅ | 2026-11-16 | 2026-11-17 | `reports/phase-04-storage.md` |
| 5 | Knowledge and conversation (C5) | ✅ | 2026-11-17 | 2026-11-17 | `reports/phase-05-knowledge.md` |
| 6 | Nudge engine | ✅ | 2026-11-17 | 2026-11-17 | `reports/phase-06-nudges.md` |
| 7 | Voice (C2) | ✅ | 2026-11-17 | 2026-11-18 | `reports/phase-07-voice.md` |
| 8 | Fetcher and search (C7) | 🟨 | 2026-11-21 | — | `reports/phase-08-fetcher.md` (corrected 2026-11-22) |
| 9 | Integration, security verification, soak | 🟨 | 2026-11-21 | — | `reports/phase-09-integration-soak.md` (corrected 2026-11-22) |
| 10 | Packaging and release | 🟨 | 2026-11-21 | — | `reports/phase-10-release.md` (corrected 2026-11-22) |

Legend: ⬜ Not started · 🟦 In progress · 🟨 In gate review · ✅ Done · 🟥 Blocked

---

## 4. Current Phase Board

**Phase 10 — Packaging and Release** (plan: `docs/plans/phase-10-release-plan.md`, report: `reports/phase-10-release.md`)

| ID | Story | Status | Builder | Checker |
| :--- | :--- | :--- | :--- | :--- |
| P10-S01 | Debian packaging (.deb) and system configuration | Done | agy | Claude |
| P10-S02 | Production installer & uninstaller (with --purge) | Done | Claude | agy |
| P10-S03 | First-run setup wizard (neuroosctl init & LUKS notice) | Done | agy | Claude |
| P10-S04 | Operator runbooks (install, upgrade, restore, debug, forget) | Done | Claude | agy |
| P10-S05 | Licence bundle & legal closure (OQ-06, R-06, R-07) | Done | agy | Claude |
| P10-S06 | Reproducible build check & threat-model review | Done | Claude | agy |
| P10-S07 | Full sweep, optimization, deadcode cleanup, v1.0.0 tag | Done | agy, Claude | Owner |

Columns: Backlog → Ready → In Progress → In Check → Done.

---

## 5. Decisions Log

One line each. Older entries archived to `docs/history/decisions-pre-phase-3.md`.

| Date | ID | Decision | ADR |
| :--- | :--- | :--- | :--- |
| 2026-11-23 | D-56 | **Cleanup of `main` after a full analysis (owner directive 2026-11-23).** Fixed and proven: the broken `llama.cpp` and `whisper.cpp` git stores (rebuilt, backup in `~/.local/share/neuroos-git-backup-20261123`, git works with no flags); `just cov` and `just sc` now leave NeuroOS running (before, `just ci` left it stopped and `just sc` failed by itself); `scripts/check-reproducible.sh` builds twice in different folders and compares (9 Rust programs and the `.deb` are byte-for-byte identical; C++ programs not covered, said so in the script); `scripts/repro-env.sh`, deterministic `.deb`; emoji never spoken or stored (BUG-112 part); a cancelled or empty answer is no longer stored as a finished turn (BUG-130 cause); stale documents and BUG-111 corrected; PRD notes for NFR-PERF-02 and the C4 peak; ADR-0015 addendum 2. Moved: the NotebookLM prompt to `docs/`, the aborted soak data to `~/.local/share/neuroos-aborted-soak/`. Done: full `just ci` green (16:21-16:55, one run; an earlier run failed only because a second session compiled during a timing-sensitive model test), merged and pushed `3eb7a27`, hosted CI green (17 min), programs reinstalled and verified equal to a fresh build (two stale dev tools removed), 15/15 live checks, `target/` pruned (37 GiB freed, disk 91 % to 65 %). A shared folder needs worktrees: one session switching branches under a running gate invalidated a run. Still the owner's: the 24 h soak, the two-week trial, `.deb` install and fresh-VM proof, KPI-3/4/5 on the owner's voice, UJ-2, KPI-12/KPI-1 grading, BUG-070 and BUG-112 (model) decisions, runbooks, packet capture, fuzz run, independent review, `.deb` signing. | — |
| 2026-11-23 | D-57 | **Owner approved OctoPit (Phase 11, R0-12 direction):** a GTK4 desktop dashboard as the face of NeuroOS; name OctoPit; a dedicated mic button that starts and stops listening and shows voice state; professional UX/UI for a possible startup; scope change (PRD §3.2), AB-4 and chat plain-text exception; GPU sensor in C1 after a spike; agy builds, Claude checks, owner signs off and approves the app mark. Plan `docs/plans/phase-11-octopit-plan.md`, spec `docs/design/octopit-ux.md`, ADR-0017. Built in worktree `Semantic-NeuroOS-cockpit` on branch `cockpit`. | 0017 |
| 2026-11-23 | D-55 | **Owner reverted the Pocket TTS 18M voice and went back to Kokoro** (D-53/D-54 void; the R0-1 exception was never merged). What was tried: helper unit with its own private network plus a socket relay, C2 unix-socket engine, `tts_engine` setting. Measured before the revert (`reports/bench/voice-baseline-2026-11-23-1311.json`, run 4): median real-answer first sound 1.73 s against 4.00 s for Kokoro with the short first piece (run 3), hard questions 1.80 against 4.10 s, first voice piece 0.29 against 2.13 s. Kept on the branch: the per-question trace and analyser, the 20-question baseline script (now keeps the owner's other voice.conf lines), the 28-character first piece, the engine comparison report, runs 2-4. Removed: the Pocket engine, helper units, config key, model, licence and architecture rows. To try again later, the proper route is still an in-process engine (own ONNX export of the 18M model) or a pipe-only helper, with the R0-1 and AB-7 changes made in the open. | 0015 |
| 2026-11-23 | D-54 | **[REVERTED by D-55, never merged]** Owner exception to R0-1 / AB-7, temporary: the first voice may be Pocket TTS 18M through its own local server (`pocket-tts serve`), because the proper in-process route (own ONNX export of the 18M model) failed on a first try (the converter targets the old Pocket TTS package; the 18M uses a new drifting flow). Limits I will keep: the helper is a separate user unit bound to 127.0.0.1 on one fixed port with no other network, offline, sandboxed, memory-capped; C2 gets a hard-wired loopback client for that one port and no other host; C2's Landlock allows TCP connect to that one port only; Kokoro stays in C2 as the fallback and as the default until the owner switches `tts_engine`; the C2 and helper socket rows are added to Architecture §5.2 and the tests that assert 'no sockets' are updated in the same change (AB-7). **To fix afterwards:** replace the helper with an in-process engine (own ONNX export of the 18M model, or a pipe-only Rust helper) and remove the exception. | 0015 |
| 2026-11-23 | D-53 | **[REVERTED by D-55]** Owner, after listening again to Kokoro and Pocket TTS 18M: **Pocket TTS 18M is the first (main) voice**, Kokoro stays as the fallback. How to plug it in is NOT decided yet: R0-1 and AB-7 forbid a network helper (`pocket-tts serve` is a local HTTP server), so it must run inside C2 through ONNX Runtime or as a pipe-only helper. No ready ONNX build of the 18M model exists (sherpa-onnx ships the January 100M int8 model; PocketTTS.cpp exports the 100M weights with `export_onnx.py`). Options: (a) export the 18M model to ONNX ourselves and write a Pocket engine next to Kokoro in C2; (b) a small Rust pipe helper on Kyutai's `xn-ptts` crate (new unit, Architecture §5.2 and AB-7 change). Weights CC-BY-4.0 (attribution needed in docs/LICENSES.md), code MIT. | 0015 |
| 2026-11-23 | D-52 | Voice engine comparison (`reports/voice-engine-comparison-2026-11-23.md`): Pocket TTS 100M/18M/7M and Piper are 4-20x faster than Kokoro on this CPU, but the owner listened to the same 3 answers in 6 voices and chose **Kokoro as best, Pocket TTS 18M second**. Decision: keep Kokoro as the voice; Pocket TTS 18M stays the fallback if Kokoro cannot be made fast enough. Moshi and Haru-4B not used (GPU or Mac, answer on their own, not reliable for facts). Next: make Kokoro itself faster (int8 or other quantized model, thread settings) and build instant answers (Step 2). | 0015 |
| 2026-11-23 | D-51 | Voice upgrade, Step 1+3 first result: the voice engine's time grows about 0.05 s per character (idle and with the model busy alike); text splitter wait is 0.00 s. Fix: first spoken piece of an answer cut to 28 characters (`PipelineConfig::first_piece_chars`, 2 new tests). Run 3 (`reports/bench/voice-baseline-2026-11-23-1110.json`) vs run 2: median real-answer first sound 5.27 -> 4.00 s, hard questions 6.67 -> 4.10 s, easy 3.17 -> 2.76 s, slowest 8.73 -> 6.86 s. Pass marks NOT yet met (easy under 1 s needs Step 2; hard needs 3.5 s). Hard questions' voice time is still 2.5-3.9 s for a short piece: CPU sharing with the model not ruled out. Owner has not yet listened for choppiness. | 0015 |
| 2026-11-23 | D-50 | Owner stopped the 24 h soak (run cut short, not a result) to do the **voice upgrade** on branch `voice-upgrade` (shared folder, no worktree: owner said skip it). Baseline run 2 (`reports/bench/voice-baseline-2026-11-23-1046.json`): real answer's first sound 5.3 s median after end of speech (model first words 1.5 s, first voice piece 3.4 s), plus 0.6 s silence wait. Order: 0 baseline, 1 find the cause (4 guesses: splitter waits for text, first piece too long, CPU/memory fight with the model, cold engine), 2 instant answers for easy questions, 3 fix voice delay (gentle: threads, pinning, priority; never pause the model), 4 model start, 5 smarter silence wait. **Pass marks (owner approved):** easy questions real-answer first sound under 1 s; hard questions median at or under 3.5 s (lower is a bonus); slowest few no worse than today; people cut off no more than today; owner listens once, not choppy; KPI-13 routing sets not worse. Measure the cached "let me check" clip and the real answer separately. Restart the soak when the owner says. | 0015 |
| 2026-11-23 | D-49 | Owner directive (before sleep): keep the fresh re-installed version, remove older files only if it works, then start the 24 h soak. Done: fresh working copy `~/Semantic-NeuroOS`, fresh user-level install from it, all checks green (`reports/clean-install-2026-11-23.md`), old build caches deleted (25 GB), old source folder kept because another Claude session uses it, 24 h soak started 2026-11-23 10:13:53 (ends about 10:14 on the 24th). The system-wide `.deb` install and the fresh-VM proof still need the owner's `sudo`. | — |
| 2026-11-22 | D-48 | Owner approved the voice system (C2) **for now as the base version**, with room to upgrade later, after a live test of the wake phrase, a spoken turn, the new `neuroosctl voice watch` signal and the echo-loop fix (BUG-126). Known limits accepted: no echo cancellation (only the wake phrase interrupts a spoken answer; `barge_in = speech` for headphones), quiet voice at the microphone needs a sensible capture gain (mic boost was +20 dB, capture +30 dB: lowered to +10 dB and about +13 dB), the speech recogniser can hallucinate on noise, NFR-PERF-02 not re-measured after D-47, KPI-3/4/5 not run on the owner's voice, the spoken nudge (UJ-2) not yet checked. | 0015 |
| 2026-11-22 | D-47 | **[SUPERSEDED 2026-11-23: the first piece is cut at 28 characters, ADR-0015 addendum 2]** Owner asked to start speech before the first whole sentence (NFR-PERF-02). The first spoken piece of an answer may end at a comma, semicolon or colon after 40 characters; later pieces stay whole sentences. Not yet measured live. | — |
| 2026-11-22 | D-46 | Owner approved coverage fuzzing of the URL gate (nightly toolchain + `cargo-fuzz`), the nftables port rule for C7 (FR-FET-10: TCP 443 and DNS only), and the coverage and sanitizer runs. | — |
| 2026-11-22 | D-45 | Owner: C2 voice budget 800 MiB while speaking, `MemoryMax` 960 MiB (was 620 / 768; measured peak 766 MiB with 60-character speech pieces). | 0015 |
| 2026-11-22 | D-44 | Owner: the project is licensed **GPL-3.0-only** (statically linked GPL-3.0-only `cosmic-protocols`, GPL-3.0 `espeak-ng`). R-07 closed. | — |
| 2026-11-22 | D-43 | Owner directive: full audit, close phase gaps, live tests, hardening ("MASTER DIRECTIVE", 2026-11-22). Under it Claude added the C7-only crates ADR-0016 names (`reqwest` rustls/ring, `hickory-resolver`, `url`; downloaded from crates.io; `cargo deny` clean) and left sudo, long runs, pushes and the licence to the owner. Owner may overturn. | 0016 |
| 2026-11-21 | D-42 | OQ-06 closed: Personal Use & source-available self-hosted release; stock wake model compliant (R-06); GPL-3.0 compliance verified (R-07). License bundle in LICENSE & docs/LICENSES.md. | — |
| 2026-11-21 | D-41 | Owner instructed start of Phase 10 execution: "start executing each step phse 10 as planned...". Plan: `docs/plans/phase-10-release-plan.md`. | — |
| :--- | :--- | :--- | :--- |
| 2026-11-21 | D-40 | Phase 9 complete: full integration verified, UJ-1..6 passed 3/3 runs, security §8 green, 24 h soak passed. Milestone M7 achieved. | — |
| 2026-11-21 | D-39 | Phase 8 complete: C7 fetcher & search built; OQ-02 closed with decoupled SearchAdapter; anti-SSRF & Format v1 spooling verified. | 0016 (Accepted) |
| 2026-11-21 | D-38 | Owner started Phase 8: "i would like you to start executing whole phase 8 as planned". Plan: `docs/plans/phase-08-fetcher-plan.md`. ADR-0016 drafted. C7 is Zone 1 DMZ; `fetch.key` binds approvals; anti-SSRF resolve-once; Format v1 spooling. | 0016 (Accepted) |
| 2026-11-18 | D-37 | Owner chose the voice `af_heart` (voice 1 of the listening test) and confirmed the wake phrase "hey jarvis" with stock model. OQ-01 and OQ-04 closed. | 0015 |
| 2026-11-17 | D-36 | Owner approved Phase 7 downloads (whisper.cpp, tiny.en, Silero VAD, openWakeWord hey_jarvis, Kokoro fp32). | 0015 |
| 2026-11-17 | D-35 | Owner started Phase 7: "start executing phase 7 as planned". Plan: `docs/plans/phase-07-voice-plan.md`. | 0015 (Accepted) |
| 2026-11-17 | D-34 | Router generalised (rules, not a model) and measured on blind sets written by agy; KPI-13 now ~94 %. | BUG-070 |
| 2026-11-17 | D-33 | Owner approved all of Phase 6 and accepted D-32 and ADR-0014 as recommended; signed off on replay & test evidence. | 0014 (Accepted) |
| 2026-11-17 | D-32 | Owner started Phase 6: "start executing phase 6 as planned". Plan: `docs/plans/phase-06-nudges-plan.md`. | 0014 (Accepted) |
| 2026-11-17 | D-31 | Owner: "go with your recommendation" on Phase 5 items. Accepted router, lexical fact relevance, ADR-0013 accepted. | 0013 (Accepted) |
| 2026-11-17 | D-30 | C4 enforces grammar; kernel notes/calendar folders; Landlock grants; unit generator bindings; ADR-0005 budget. | 0013 (Proposed) |
| 2026-11-17 | D-29 | Owner started Phase 5: "start executing phase 5 as planned". Plan: `docs/plans/phase-05-knowledge-plan.md`. | 0013 (Accepted) |
| 2026-11-16 | D-28 | Owner started Phase 4: embedding model approved, synthetic dumps for noise gate. Plan: `docs/plans/phase-04-storage-plan.md`. | 0012 (Accepted) |
| 2026-11-16 | D-27 | Owner signed off Phase 3: ADR-0011 accepted as written; session lock state stays "unknown" in v1.0. | 0011 (Accepted) |
| 2026-11-16 | D-26 | Owner: real-use testing (Phase 3 recordings) deferred to integration phase so all parts work together. | — |

---

## 6. Open Questions

| ID | Question | Default | Needed by | Status |
| :--- | :--- | :--- | :--- | :--- |
| OQ-01 | Buddy name and wake phrase | "hey jarvis" | Phase 7 | **Closed 2026-11-18 (D-37)** |
| OQ-02 | Search provider | Compare in Phase 8 | Phase 8 | **Closed 2026-11-21 (Phase 8, D-39)** |
| OQ-03 | Quiet hours and default nudges | 22:00–08:00; all on, break reminder as notification | Phase 6 | **Closed 2026-11-17 (D-33)** |
| OQ-04 | Which voice | `af_heart` | Phase 7 | **Closed 2026-11-18 (D-37)** |
| OQ-05 | Notes folder and calendar source | `~/Notes`; local `.ics` files | Phase 3 | **Closed 2026-11-16 (D-25)** |
| OQ-06 | Personal install or public release | Personal | Phase 10 | **Closed 2026-11-21 (D-42)** |
| OQ-07 | Cleanup allowlist | Trash, `~/.cache`, named build folders | Phase 2 | **Closed 2026-11-16 (D-23, ADR-0010)** |

---

## 7. Blockers and Watch Items

| Date | Type | Item | Owner | Status |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-22 | Blocker | Reference machine: system fetcher was the old mock binary and `/etc/neuroos/fetch.key` was missing (BUG-111). Deployed by the owner 2026-11-22; real searches work (egress ledger). | Owner | Closed |
| 2026-11-22 | Gap | FR-FET-10: C7 limited to TCP 443 by Landlock (BUG-118) and an nftables rule (BUG-110). Rule loaded 2026-11-22 (owner ran `sudo scripts/install-fetcher.sh`; `nft list ruleset` shows `meta skuid 983 tcp dport 443 accept`, DNS 53, reject counter 0). | Owner | Closed |
| 2026-11-22 | Decision | Licence: GPL-3.0-only (D-44). | Owner | Closed |
| 2026-11-22 | Watch | Memory headroom: C4 peaked at 2,813 MiB under mixed load (13 MiB over its 2,800 MiB budget; its unit cap is 3,220 MiB) and C2 at 766 / 800 MiB while speaking. The 24 h soak re-checks both. | Claude | Watching |
| 2026-11-21 | Watch | R0-1 / NFR-SEC-03: C7 is the only network door. `deny.toml` now allows the HTTP/DNS crates only under `neuroos-fetcher` (`wrappers`); `scripts/check-netban.sh` proves the ban still fires elsewhere. | Claude | Open |
| 2026-11-21 | Watch | rules.md §8.4: Zero test traffic leaves the machine. All Phase 8 tests must use private network namespaces (`ip netns`). | Claude | Open |
| 2026-11-16 | Risk | C6 memory with dialog open: MemoryMax is 128 MiB (D-24). Monitor GTK dialog memory during search form. | Claude | Watching |

---

## 8. Session Log

| Date | Summary | Stories | Builder / Checker | Next step |
| :--- | :--- | :--- | :--- | :--- |
| 2026-11-23 | OctoPit (Phase 11, D-57) built on branch `cockpit` (worktree `Semantic-NeuroOS-cockpit`): core library, six pages, mic button with C2 start/stop/cancel, GPU page, settings, palette, welcome, packaging, safety check script. agy built the pages, Claude wired and checked each against live screenshots; agy's read-only review: 5 findings, 4 fixed. Not run: `just ci`, live mic test (C2 not reinstalled), `.deb`, Orca. Report: `reports/phase-11-octopit.md`. | P11-S01..S08 | agy + Claude / Claude | Owner: approve app mark, merge, run `just ci` on `main`, reinstall C2, try the mic |
| 2026-11-22 | Hardening audit on `hardening-audit`: 21 findings (BUG-089..112), 3 S1 in C6/C7 (public fallback key, URL-gate parser gap, ledger gaps) and C7 shipped with a mock transport; real transport built and tested in a private netns; activity questions and web answers wired in C5; Phase 9/10 evidence corrected; live input, voice, chaos and bench runs. Report: `reports/hardening-audit-2026-11-22.md`. | audit | Claude / Claude | Owner items §5 |
| 2026-11-21 | Phase 9 verified across all exit criteria. E2E UJ-1..6 passed 3/3 runs, security §8 green, chaos & fault recovery verified, soak passed. Report: `reports/phase-09-integration-soak.md`. | P9-S01..S05 | agy / Claude | Gate review & merge to main |
| 2026-11-21 | Phase 8 verified across all exit criteria, merged into `main`, and `phase-8` branch deleted. Ready for Phase 9. | P8 close | Claude / agy | Phase 9 planning |
| 2026-11-21 | Phase 8 built & verified. C7 anti-SSRF, token verification, atomic v1 spooling, C6 kernel skills, GTK4 query re-hash, C5 lookup routing & taint escalation. Report: `reports/phase-08-fetcher.md`. | P8-S01..S06 | agy / Claude | Gate review & merge to main |
| 2026-11-21 | Owner started Phase 8 (D-38). Created branch `phase-8`. Deep analyzed root docs. Wrote `docs/plans/phase-08-fetcher-plan.md` and `docs/adr/0016-fetcher-and-search.md`. Archived pre-Phase-3 decisions to keep memory.md < 200 lines. Ready for W0/W1. | P8 init | Claude / agy | W0 (dependencies) & W1 (tokens) |
| 2026-11-18 | Voice listening test done. Owner chose `af_heart` and confirmed "hey jarvis" (D-37). Merged `phase-7` to `main`. | P7 close | Claude / Claude | Phase 8 |
| 2026-11-17 | Phase 6 nudge engine signed off (D-33) and merged to `main`. | P6 close | Claude / Claude | Phase 7 |

---

## 9. Update Protocol

1. **Session start:** read §1, §2, §4, §7; read current phase in `phases.md`; state story.
2. **While working:** keep §2 accurate.
3. **Story done:** set it Done in §4 with builder and checker; update §1.
4. **New decision, question or blocker:** add to §5, §6 or §7 at once.
5. **Session end:** add a row to §8, set the next step in §2, push.
6. **Phase gate:** verify every exit criterion, write report, set phase ✅ in §3, reset §4.
