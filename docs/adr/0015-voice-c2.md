# ADR-0015 — Voice (C2): engines, pipeline, protocol, sandbox (Phase 7)

**Status:** Proposed 2026-11-17 (decisions taken under the owner's Phase 7 go-ahead, D-35; open to overturn at sign-off)
**Date:** 2026-11-17
**Changes:** `cpp/neuroos-voice` (pipeline, engines, link to C5, service, tools), `cpp/CMakeLists.txt` and presets (`voice`, `voice-release`), `models/manifest.toml` (voice models), `cpp/third_party/whisper.cpp` (submodule v1.9.5), `crates/neuroos-knowledge` (`VoiceCommand`, speech chunks, spoken nudges), `proto/neuroos/v1/knowledge.proto` (`NudgeTest.speak`), `neuroosctl voice status` and `nudges test --voice`, the C2 unit, `libneuroos` sandbox (a rule on a single file), `scripts/install-dev.sh`, C4's prefix cache (BUG-076, found on the way).
**Evidence:** `reports/phase-07-voice.md`.

---

## Context

The buddy should be talked to hands-free and answer in one natural voice, and it should be able to speak a nudge at a good moment (PRD G-1, FR-VOI-01…12, FR-NUD-05). C2 was a stub that only answered a status question. The v1 lessons (`docs/lessons-from-v1.md` §3) shaped most of the choices below.

## Decision

1. **Engines.** Silero VAD v6 and openWakeWord ("hey jarvis", stock model, OQ-01) through ONNX Runtime; whisper.cpp `tiny.en` (greedy, English) for speech to text; Kokoro-82M **fp32** ONNX with espeak-ng phonemes and the voice `af_heart` (OQ-04 default) for speech. Every model is pinned by revision and SHA-256 in `models/manifest.toml` and fetched only by `just fetch-models`.
2. **Everything behind interfaces** (`engines.hpp`: source, sink, VAD, wake word, STT, TTS), so the pipeline runs in tests on fixture audio with fakes and in the service with the real engines unchanged. Pure parts written by agy (ring buffer, sentence splitter, VAD gate, turn state machine, spoken lines, WAV reader, word error rate) were probed by Claude with independent sets.
3. **Threads.** The audio thread (the PipeWire source's worker, never the real-time callback) runs voice activity and the wake word; one worker per model-heavy job (speech to text, speech synthesis and playback writes); one mutex guards the conversation state and no model, device or socket call is made while it is held. Output is tagged with an *epoch*: a flush or a new command bumps it and stale queued speech is dropped.
4. **Capture strictness (FR-VOI-03).** The configured device is looked up in the PipeWire registry and its exact `object.serial` is requested; an unresolved device creates no stream at all (a target is only a preference in PipeWire). A removed device is detected from the registry and reported (a stream's sample flow does not show it).
5. **Turn taking.** The wake word runs only while speech is detected (FR-VOI-02), fed the audio just before the gate fired. A wake word resets the detectors and starts the utterance 600 ms *before* the report (the detector is late); the wake phrase is stripped from the transcript (`StripWakePhrase`, loose match). If the transcript is only the phrase (the user paused), listening continues for the command without another wake word. After any spoken output there is an 8 s window with no wake word (FR-VOI-08). Barge-in: speech detected while speaking, with a **higher** VAD threshold (0.85) than for listening, flushes output (measured 0.036 ms), cancels the request to C5 and starts the next utterance with the audio from the interruption. There is no acoustic echo cancellation: a speaker loud enough to push its own voice past that bar interrupts itself; headphones or the system's echo cancel are the remedy.
6. **Protocol.** C2 sends `VoiceCommand` on `knowledge.sock`; C5 answers on that request's own stream: a `PreambleRequest` first (FR-KNO-02), then `SpeakChunk` sentences cleaned for speech (FR-KNO-08: no markdown, URLs, code), then the closing frame. Cancelling the stream is the barge-in (C5 cancels C4); `BargeIn` is a separate unary message that only records it. C5 → C2 on `voice.sock`: `VoiceStatusRequest` (turn, speaking, follow-up, microphone used by another application, capture running), `SpeakNudge` (template id and numbers), `PreambleRequest`, `SpeakStatus`. C2 owns the spoken wording and the rounding of numbers for speech (`spoken_lines`); C5 never sends text for a nudge (R0-6).
7. **Spoken nudges.** C5 asks C2 for its status each tick; speech is chosen only when C2 is capturing, no turn or speech is under way and no other application holds the microphone, otherwise the existing policy holds or notifies. If C2 cannot take the line, the notification is shown. The reply window opens as for a notification, so "yes, show me" works by voice with no wake word. `neuroosctl nudges test --voice` is the owner's test of the voice.
8. **Two ggml copies.** whisper.cpp and llama.cpp each bundle ggml; C2 is therefore built in its own tree (`voice` and `voice-release` presets, `NEUROOS_WITH_VOICE_ENGINES`), never with `NEUROOS_WITH_LLAMA`. Their sonames differ (`.so.0` and `.so.1`) so both sets of libraries sit in `~/.local/opt/neuroos/lib`. The other presets build C2 with a stub runtime (DEGRADED "no speech engines").
9. **Sandbox and unit.** Landlock read access to the voice models, the install folder, `/usr/lib/x86_64-linux-gnu` (PipeWire plugins, espeak-ng data) and `/usr/share/pipewire`; write access only to the socket folders and the one PipeWire socket file (a rule on a single file carries file rights only). The unit binds `%t/pipewire-0` (ProtectHome empties `/run/user`) and the optional `voice.conf`, and is exempt from `RestrictRealtime` (audio scheduling, Architecture §8.1); `PrivateDevices`, `PrivateNetwork` and the rest stay.
10. **Memory (PRD §6.2: 620 MiB).** Kokoro's session uses no memory arena and no pre-packed weight copies (steady state 702 → 560 MiB); the heap is trimmed after each spoken turn; the speech-to-text audio window is sized to the utterance (3 s: 337 → 67 ms with 4 threads) after trailing silence is trimmed, with a token cap and repeat collapsing against the looping the lessons describe.
11. **Settings.** `~/.config/neuroos/voice.conf` (`key = value`): microphone and speaker node names, voice, speed, wake threshold, end-of-speech silence, follow-up seconds, speech-to-text threads. Compiled defaults, unknown key or bad value is fatal.
12. **Licences.** The openWakeWord pre-trained models are CC-BY-NC-SA-4.0 (non-commercial, PRD R-06): fine for the owner's own use, a limit on any distribution. espeak-ng (GPL-3.0, R-07) is linked dynamically from the system package, never bundled or modified.

## Consequences

- Measured on the real stack (installed units, virtual devices, the real C5/C4): a spoken question is heard, answered and spoken; the first sound comes 0.6–0.8 s after the question ends; the preamble starts well under 0.25 ms after the request (cached clip); the spoken nudge journey (UJ-2) works, including the reply without the wake word, and the audit log shows the offered skill requested only after the spoken yes.
- Idle CPU is about 2.3 % of one core with a real microphone; resident memory is 560–600 MiB, close to its 620 MiB budget (little headroom).
- No audio reaches disk: the live process holds no writable file descriptor and no audio file appears during an exchange (`scripts/voice-sc-checks.sh`).
- Found on the way: a repeated question could get an empty answer (BUG-076, C4's prefix cache).
- Not built or not measured: acoustic echo cancellation; KPI-3/4/5 on the owner's recordings (tool: `neuroos-voice-eval`); the 4 h soak (script: `scripts/voice-soak.sh`); the voice choice (script: `scripts/voice-listening-test.sh`); a 30-minute rule for memory settling beyond a short run.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Build C2 in the same tree as C4 | Two bundled ggml copies collide at configure time. |
| Kokoro int8 or fp16 | Slower than real time, or larger on CPU (lessons-from-v1 §3). |
| Wake word on every frame | Costs CPU all day; the lessons and FR-VOI-02 say only while speech is detected. |
| C5 → C2 push of text over `voice.sock` for answers | The request's own stream is simpler and carries cancellation for free. |
| Keep the wake phrase in the transcript and let C5 ignore it | C5's router and prompt would have to know it; stripping at the source keeps C5 text-only. |
| Poll the audio device for removal | A removed microphone does not look removed by sample flow; the registry says so directly. |
