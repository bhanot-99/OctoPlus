# Phase 7 — Voice (C2) and spoken nudges: Report

**Date:** 2026-11-18 · **Branch:** `phase-7` · **Builder:** Claude (pipeline, engines, service, C5 side, unit, tools, measurements), agy (ring buffer, sentence splitter, VAD gate, turn state machine, spoken lines, WAV reader, word error rate, Silero and openWakeWord adapters) · **Checker:** Claude (independent probes of every agy piece), agy (read-only review of Claude's code)
**Status:** Built and working on the real stack; gate result in §7. Owner-only items left (§8): your voice on the KPIs, the voice choice, the 4 h soak, a trial, sign-off, merge. Design calls are D-35 and ADR-0015 (Proposed), open to overturn.

## 1. Summary

You can now talk to it. Say "hey Jarvis" and a question; it hears you (voice activity, wake word, speech to text, all on this machine, nothing written to disk), answers out loud sentence by sentence while it is still thinking, and you can talk over it to stop it. After it has spoken you can answer without the wake word for a few seconds. A nudge can now be spoken at a good moment, and "yes, show me" by voice asks for the offered skill under the normal tiers. All of it was run on the real installed units with the real C5 and C4, using virtual microphones and speakers so nothing was audible and nothing real was touched.

Honest caveats, in order of weight:
1. **KPI-3, KPI-4 and KPI-5 are not measured on your voice and your room.** The tool exists (`neuroos-voice-eval`), and on synthetic voices it gives 0 of 8 wake phrases missed and a 2 % word error rate, but that is not your KPI.
2. **Echo cancellation is available but not switched on** (`scripts/voice-echo-cancel.sh enable`, then two lines in `voice.conf`; plumbing verified, acoustic effect is yours to judge). Barge-in uses a higher voice threshold while it speaks so its own voice coming back through the microphone does not stop it. Loud speakers next to the microphone can still defeat that; headphones or the system's echo cancel are the remedy.
3. **The 4 h soak is yours to start.** A 25-minute run (13 turns) kept memory bounded (it moves between 284 and 503 MiB as glibc gives memory back) and the service never restarted. Resident memory right after load (about 590 MiB) is close to the 620 MiB budget, and the unit's cgroup touches its 768 MiB limit while loading because model files count as page cache.
4. **The live nudge reply by voice was proven with a lowered threshold and no quiet hours** (a temporary rule file), because a real disk below 90 % never fires; the journey itself is the one UJ-2 describes.
5. **Wake-word model licence:** the stock "hey jarvis" model is non-commercial (CC-BY-NC-SA-4.0), fine for your own use, a limit on any distribution (PRD R-06).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P7-S01 | The wake word starts listening; nothing is transcribed otherwise | Done | Claude (pipeline), agy (VAD gate) | Claude | `VoicePipeline.SpeechWithoutTheWakeWordIsNeverTranscribed`, `TheWakeWordModelRunsOnlyWhileSomeoneIsSpeaking`, `LocalEndToEnd.SpeechWithoutTheWakePhraseIsIgnored` |
| P7-S02 | An instant preamble, then the answer as it is generated | Done | Claude | Claude | `ReplyIsSpokenAsItStreamsInOneVoice...`, `a_spoken_question_gets_a_preamble_then_speakable_sentences` (C5), `local_a_voice_command_gets_a_preamble_then_speakable_sentences` (real C5 and C4), `scripts/voice-real-stack.sh` |
| P7-S03 | Speaking over it stops it at once | Done | Claude, agy (turn machine) | Claude, agy review | `LocalEndToEnd.SpeakingOverALongAnswerStopsItAndCancelsTheRequest` (flush 0.036 ms), `ABargeInWhileAwaitingTheAnswerDropsWhatWasAlreadyQueued` |
| P7-S04 | A reply without the wake word | Done | Claude | Claude | `FollowUpNeedsNoWakeWordAndIsMarkedAsSuch`, `AnswerIsSpokenAndAFollowUpNeedsNoWakeWord`, the nudge journey |
| P7-S05 | Every word in one consistent voice | Done, voice choice pending | Claude | Claude | one Kokoro engine for answers, preambles, nudges and status clips; a failing engine plays a cached clip and restarts, never another voice (`AFailingSpeechEngineSaysSoOnceInTheCachedVoice`); `scripts/voice-listening-test.sh` for you |
| P7-S06 | The buddy can speak a nudge, only at a good moment | Done | Claude (C2 and C5) | Claude, agy review | six engine tests (`a_good_moment_with_voice_ready_speaks_the_nudge...`, microphone busy, turn in progress, speaker failure falls back to the notification), `scripts/voice-nudge-check.sh` (UJ-2 by voice on the real stack) |
| P7-S07 | Cheap while I am silent | Done | Claude | Claude | idle CPU 2.3 % of one core with a real microphone (budget < 3.5 %); the wake model runs only while speech is detected; one 0.1 ms voice-activity step per 32 ms frame |

## 3. Measurements (release build, real units, this machine)

| What | Target | Measured |
| :--- | :--- | :--- |
| NFR-PERF-01 preamble: request to first sound | ≤ 50 ms p95 | under 0.25 ms (a cached clip; the service's own histogram bucket) |
| NFR-PERF-08 barge-in flush | < 1 ms | 0.036 ms (real virtual speaker) |
| NFR-PERF-09 transcription of a 3 s utterance | ≤ 150 ms | 67 ms median, 4 threads (1 thread 165 ms) once the audio window is sized to the utterance; 337 ms before |
| End of your question to the first sound | — | 0.60–0.83 s (includes the 0.6 s of silence that ends your sentence) |
| First spoken sentence of an answer | — | about 1.5 s after the command reaches C5 (C4's generation) |
| Idle CPU of C2, real microphone | < 3.5 % of one core | 2.3 % |
| C2 resident memory | 620 MiB steady, 480 idle; limit 768 | 560 MiB after one turn of each engine (was 702 before the memory work), about 590 MiB right after load; the kernel swaps idle pages out under pressure |
| Cost of listening | — | voice activity 0.105 ms and wake word 0.51 ms per 32 ms frame |
| KPI-4 / KPI-5 on 8 synthetic voices | ≤ 5 % / ≤ 12 % | 0 of 8 missed / 2.1 % (first run 56 %: lost first words, a pause after the wake phrase, a looping decoder; all fixed, BUG-083) |

## 4. Bugs found (BUGS.md)
Real-stack testing and the evaluation tool found the most: BUG-076 (a repeated question could get an empty answer, since Phase 2; S2), BUG-082 (the first nudge after a start could be shown instead of spoken), BUG-083 (lost first words, pause after the wake phrase, looping decoder), BUG-084 (Landlock rule on one file), BUG-085 (the unit could not see the audio socket). agy's review of Claude's code found BUG-077 (shutdown use-after-free), 078 (no flush when barge-in comes before the first audio), 079 (a spoken test line ignored the busy microphone), 080 (speech engine restart doubled model memory), 081 (unbounded sentence buffer). All fixed with tests.

## 5. What was built
`cpp/neuroos-voice` (pipeline with six seams, ring buffer, VAD gate, turn machine, spoken lines, transcript clean-up, `voice.conf`, link to C5, the real engines: Silero, openWakeWord, whisper.cpp, Kokoro with espeak-ng, PipeWire source, sink and microphone-use probe; tools `neuroos-voice-eval` and `neuroos-voice-say`); C5 (`VoiceCommand`, speech chunks, spoken nudges, `nudges test --voice`); `neuroosctl voice status`; C2 unit (models, settings, the audio socket, no `RestrictRealtime`); a Landlock rule for a single file; the `voice` presets and installer; C4's cache fix. Scripts: `voice-real-stack.sh`, `voice-nudge-check.sh`, `voice-sc-checks.sh`, `voice-soak.sh`, `voice-listening-test.sh`.

## 6. Security evidence
No audio on disk (`voice-sc-checks.sh`: no audio file appears, no writable descriptor), capture strictness (a nonexistent microphone creates no stream), no spoken nudge while another application holds the microphone (probe, C5 policy, C2 backstop), no approval by voice (a transcript is text through the same turn engine). See `security.md` rows and ADR-0015.

## 7. Gate
`just ci` stages all green on the final code: format check, lint (clippy, clang-format, clang-tidy including the voice engines in the `voice` tree, shellcheck, machete), tests (777 Rust, 190 C++ plus the local real-stack tests), deny, coverage, and the ASan/UBSan and TSan runs. The stages were run in pieces after fixes, not as one uninterrupted `just ci`. Three things flaked or failed on the way and were fixed or recorded: SC proof 3 caught C2 reading the install folder (narrowed), the kill-and-recover baseline now waits for C2 to finish loading, a TSan race in a test fake. Known flaky under load: BUG-086 (cancel timing), BUG-087 (an ipc test, once).

## 8. Owner to do
1. ~~**Choose the voice** (OQ-04)~~ Done 2026-11-18: `af_heart` (D-37). Original step: `scripts/voice-listening-test.sh`, then `voice = <name>` in `~/.config/neuroos/voice.conf`.
2. ~~**Confirm the wake phrase** (OQ-01)~~ Done 2026-11-18: "hey jarvis", stock model (D-37). Original step: "hey jarvis" with the stock model, or name another stock phrase.
3. **KPI-3, KPI-4, KPI-5 on your voice:** record your command set (WAV, one file per command, a `manifest.tsv` of `file<TAB>what you said after the wake phrase`) and 8 hours or more of ordinary room audio; run `neuroos-voice-eval MODELS commands manifest.tsv` and `... ambient FILES`.
4. **The 4 h soak:** `scripts/voice-soak.sh 4` (virtual devices, nothing audible; it samples memory and CPU every 5 minutes).
5. **A day of use**, then sign-off and merge. The trial also closes the nudge-by-voice question for OQ-03.
