# Phase 12: the Companion (Laila): Report

**Date:** 2026-11-25 · **Branch:** `phase-12` (not merged) · **Builders:** agy (C-S00 documents, C-S01 core, C-S02 turn machine and pipeline, C-S07a tuning) and Claude (C-S03 to C-S08, all checks, every commit) · **Checker:** Claude read every diff and re-ran every check; agy's reports were never taken as proof · **Owner decisions:** `memory.md` D-77 (defaults), D-78 (progress), and on 2026-11-25 "take full control after agy's last report" and "the 24 h soak and the one-week trial are not part of this phase"
**Status:** Built and green. Waiting for the owner's sign-off and merge, and for the owner-only items in section 7.

## 1. Summary

Laila is a companion who talks, jokes and remembers, by voice first. She chats on her own 2 to 4 times an hour when the owner is free, keeps listening without the wake word during a chat, writes short model-made lines in a persona the owner can edit, follows "that was funny" and "not funny", keeps about 4 in 10 of the things the owner says about themselves (never secrets, card numbers, health or other people's private matters), and can be steered by voice or from a new OctoPit page. Her words only ever go to speech: nothing she or the model writes can act on the computer (DR-1). The full gate `just ci` ran to the end and printed **ALL GREEN**.

## 2. Stories

| Story | What | Built by | Evidence |
| :--- | :--- | :--- | :--- |
| C-S00 | Rule changes (R0-6 retired, R0-8 reworded), DR-1, ADR-0019 accepted, persona file format, Laila's default file | agy, Claude fixes | `rules.md` 2.1, `docs/design/persona-format.md`, `deploy/persona/laila.toml` |
| C-S01 | Companion core: persona loader, mode machine, guards, Kabir gate, dependence guard, repeat guard, prompt, session memory, reply filter, engine, chat route, `ask --companion`, `octoplusctl chat` | agy, Claude fixes | 100 scripted conversations: Kabir in 0.1 % of turns, heart mode 53 %, the gentle break line at most once a day |
| C-S02 | Open-mic sessions: turn machine, earcons, stop and guest phrases, directedness, pipeline wiring | agy, Claude fixes | 258 C++ tests; release build (-O3 -Werror) found a GCC false alarm, fixed |
| C-S03 | Pulse scheduler, runner, `SpeakCompanion`, quiet listening session, spoken controls | Claude | 200 simulated days: 2.03 talks per free hour, longest free gap 155 min, no rule broken; an owner who ignores everything gets about 8 a day against 22 |
| C-S04 | Humour and manners | Claude | jokes after two "not funny": a third fewer; after four: none; no joke told twice over 200 days (542 repeats stopped) |
| C-S05 | Memory of the owner | Claude | share kept 0.44 to 0.48 over 6000 statements for every mix; 23 excluded sentences never kept in 2000 tries each; live: kept through the real kernel, audited |
| C-S06 | OctoPit Companion page, status and switches, session line | Claude | 6 core tests, 2 gateway tests, 3 handler tests, screenshots in both themes (`reports/phase-12-octopit/`); `scripts/check-octopit.sh` ok |
| C-S07 | Speed | Claude | first piece p50 1.81 s to 1.21 s, p95 5.30 s to 1.92 s (`reports/bench/companion-*.json`) |
| C-S08 | Safety verification | Claude | see section 4 |
| ~~C-S09~~ | Soak and trial | | Not part of this phase (owner decision 2026-11-25) |
| C-S10 | Hinglish | | Not run: needs the owner's recordings; materials in `docs/design/hinglish-spike.md` and `docs/design/hinglish-recording-script.md` |

## 3. Numbers

- **Gate:** `just ci` ALL GREEN: format, build, clippy `-D warnings`, clang-format and clang-tidy, `cargo machete`, `cargo deny`, netban, OctoPit check, 1174 Rust tests, 253 C++ tests, coverage (Rust 84.8 % lines, C++ 82 %), AddressSanitizer with UndefinedBehaviorSanitizer, ThreadSanitizer.
- **Speed (text path, real local model, 20 casual prompts):** first piece p50 1.21 s, p95 1.92 s (target about 1.5 and under 3); whole reply p50 2.8 s; replies about 17 to 37 words. The voice path adds speech to text and text to speech and has not been measured (the microphone is not available on this machine right now).
- **One thing the release build caught that the debug build did not:** a GCC 13 `-Wstringop-overflow` false alarm at `-O3 -Werror` in the turn machine.

## 4. Safety evidence (C-S08)

| Control | Proof |
| :--- | :--- |
| Her speech has no path to actions (DR-1) | `companion_isolation_test` scans her code for any dispatch or fact-store name; `whatever_the_model_writes_the_companion_acts_on_nothing`: 160 turns of hostile model output (skill JSON, role markers, "delete my files", "approve the dialog"): only owner-worded `profile.remember` calls from the memory rule reached the kernel, never model text |
| Chance memory never keeps what it should not | `nothing_excluded_is_ever_kept_by_chance`, ambient, question and crisis tests; kept facts are the owner's own sentence under `USER_PROVIDED` taint, one audited kernel call each |
| No personal text in logs (R0-10) | `companion_log_scan_test` over every `info!`, `warn!`, `error!`; live: a test word said in chat appeared in no service journal; the audit log shows `ATTEMPT` and `RESULT` for each kept fact |
| Hostile stored facts and persona text cannot break a prompt | `a_hostile_stored_fact_cannot_break_the_prompt` (exactly 3 role markers), prompt tests |
| Only C5 may make C2 speak a companion line | the socket map, and live: the running voice service answered a stranger with `PermissionDenied: peer rejected` (`tests/live_companion_line.rs`, run with `--ignored`) |
| Open microphone stores no audio | the voice service has no file-writing code and earcons are made in memory; its live process held no writable file; the full file-watch proof (`scripts/voice-sc-checks.sh`) was not run because it needs a working microphone |

## 5. Deviations from the plan, and honest limits

1. `per_day_cap` is 40, not 24: 24 contradicted "2 to 4 an hour" over about 12 free hours.
2. "Is this meant for her?" is a word-list check. The planned extra step (ask the model) was not built. Its test set (70 owner lines, 70 TV and room lines, 100 % both ways) was written together with the check, so it proves the examples, not a real room.
3. Kept facts are marked with the text prefix `(noticed) `, not a database column: a schema change would have locked the owner's real data to this build.
4. OctoPit has no persona editor: the page says where `persona.toml` is. The Forget button opens the approval dialog (as a spoken "forget that" does) and waits for it.
5. Pulse and humour counters are kept in memory only: they restart with the service.
6. During live testing I created 3 test facts (the word "PURPLEWALRUS"). One was forgotten at 16:10 through the approval dialog (probably the owner); ids 398 and 399 remain. Removing them needs the dialog, so they were left.
7. The small local model still sometimes invents things; a filter drops lines that claim a shared past with nothing to back it, but it cannot catch every false statement.
8. The microphone service reports no microphone on this machine, so open-mic chats and her self-started talks were proven with fakes and with C2's own tests, not live.

## 6. Where things are

Code: `crates/octoplus-knowledge/src/companion/`, `src/turn/companion_*.rs`, `src/turn/chance_memory.rs`; C++: `cpp/octoplus-voice` (turn machine, pipeline, earcons, phrases); OctoPit: `crates/octopit-core/src/companion.rs`, `crates/octopit/src/pages/companion.rs`; contract: `proto/octoplus/v1/{knowledge,voice,envelope}.proto` (additive only). Documents: ADR-0019, `security.md` section 4 and 8, `design.md` 9.6 and 12, `PRD.md` FR-COM-01 to 11, `docs/design/persona-format.md`, `docs/runbooks/companion-trial.md` (optional, owner-started).

## 7. Owner items

1. Read this report, sign off, and merge `phase-12` into `main`.
2. Give the helpline numbers or trusted people for the fixed crisis text (`crisis.contacts` in `persona.toml`; none were invented).
3. Say whether the voice memory budget (836 of 800 MiB) is accepted; open-mic sessions were not measured against it.
4. Try the microphone side in a real room (open-mic chat, a self-started talk, guest mode, TV in the background) and listen to the "Mm.", "Oh.", "Hm.", "Ah." sounds.
5. Remove the 2 leftover test facts (ids 398 and 399) from OctoPit's Companion page or `octoplusctl profile remove`.
6. Hinglish test, 24 h soak and trial week: only if and when you decide.
