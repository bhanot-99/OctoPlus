# Phase 7 build plan — Voice (C2) and spoken nudges

**Branch:** `phase-7` · **Stories:** P7-S01…S07 (`phases.md` §10) · **Report:** `reports/phase-07-voice.md`
**Started:** 2026-11-17 on the owner's go-ahead ("start executing phase 7 as planned"), recorded as D-35 in `memory.md`. Claude's design calls below are open to overturn at sign-off.
**Roles:** agy builds the pure C++ pieces (buffers, chunker, state machine, VAD logic, clip/line templates) against interfaces Claude fixes first. Claude builds the pipeline wiring, the engine adapters, C5's Rust side, the units and the measurements, and checks every agy piece with its own probes. The owner approves downloads, supplies recordings, runs the listening test and the long soak, and signs off.

Requirements: `PRD.md` FR-VOI-01…12, FR-KNO-02/08/10, FR-NUD-05, FR-PRV-04, NFR-PERF-01/08/09, KPI-3/4/5, OQ-01, OQ-04; mechanisms `Architecture.md` §6.1, §6.2, §6.3 (barge-in), §9.1; `design.md` §9; lessons `docs/lessons-from-v1.md` §3.

## 0. Design in one page

```
mic (PipeWire, exact node id) ─► ring buffer ─► VAD (every frame) ─► wake word (only while speech) ─► utterance capture
                                                                                      │ silence ≥ 600 ms
                                       whisper.cpp tiny.en ◄──────────────────────────┘
                                              │ VoiceCommand ──► C5 (knowledge.sock)
C5 ─► PreambleRequest ─► cached clip, instantly          C5 ─► SpeakChunk (sentence by sentence) ─► Kokoro ─► speaker
C5 ─► SpeakNudge(template id + numbers) ─► cached opener + spoken numbers      C5 ◄─ BargeIn (flush < 1 ms, then notify)
C5 ─► VoiceStatusRequest ─► {turn, speaking, follow-up, mic used by another app, capture running}
```

Rules that shape everything:
- **Capture only from the named device, resolved to an exact node id first; if it cannot be resolved, no capture at all (FR-VOI-03).** A target node is only a preference in PipeWire (lesson), so the node is looked up in the registry and a missing one means no stream is created.
- **No audio is ever written anywhere (FR-VOI-01, FR-PRV-04).** Audio lives in bounded memory buffers; the Landlock policy grants no write access except the socket folders; a test lists files created during a session.
- **One voice, one speed (FR-VOI-09).** Kokoro fp32, one voice (`af_heart` until the owner's listening test, OQ-04). If the engine fails, cached status clips play while it restarts; the voice never switches.
- **Barge-in is local (Architecture §6.3).** The listener keeps running while speaking (separate threads, one consumer of the capture buffer); two consecutive speech frames flush output, then C5 is told.
- **Block on audio arrival; never poll** (lesson: idle CPU went to a polling loop). Trim the heap after each turn (lesson: glibc kept freed blocks).
- **The speech text comes from C5 only** (Architecture §10): C2 never talks to C4. Spoken nudges are a template id plus numbers (R0-6, FR-NUD-12); C2 owns the spoken wording and the one place numbers are rounded for speech (design.md §8).
- **Every part tolerates its peers absent, in either start order** (lesson: start order hid a bug).
- **Every pure piece is a class behind an interface** (`AudioSource`, `AudioSink`, `Vad`, `WakeWord`, `Stt`, `Tts`), so the whole pipeline runs in tests on fixture audio with fake or real engines.

## 1. Work packages

| WP | What | Builder | Checker | Needs owner |
| :-- | :-- | :-- | :-- | :-- |
| W0 | Plan, D-35, ADR-0015 (Proposed), engine interfaces fixed in a header | Claude | agy review | |
| W1 | Pure pieces: ring buffer, sentence chunker, turn state machine (listening, follow-up, speaking, barge-in), VAD hysteresis, speech number rounding + nudge line templates | agy (3 bounded tasks) | Claude probes | |
| W2 | Pipeline orchestrator over the interfaces with fakes; `VoiceCommand` out; barge-in; follow-up window; clip cache; status; unit and fixture-audio tests | Claude | agy | |
| W3 | C5 side (Rust): hear `VoiceCommand`, stream `SpeakChunk` per sentence, `PreambleRequest`, `BargeIn` cancels the generation, `VoiceStatus` feeds the nudge context, `SpeakNudge` replaces the notification when voice is free | Claude | agy review | |
| W4 | Real engines: PipeWire capture and playback with node resolution and device-change watch; Silero VAD and openWakeWord through ONNX Runtime; whisper.cpp; Kokoro with espeak-ng phonemes | Claude (agy for any single adapter) | Claude | **Downloads and the whisper.cpp submodule (§3)** |
| W5 | C2 service: sockets, health parts, Landlock and unit file (PipeWire socket only), config, `neuroosctl voice status`, regenerated units, `install-dev` | Claude | Claude | |
| W6 | Integration tests: virtual microphone and sink, both start orders, real C5/C4 streaming; fault injection (device unplugged, C5 absent, engine killed); SC tests (no file written, no node means no capture, no spoken nudge while another app holds the microphone) | Claude | agy review | |
| W7 | Measurements: preamble, barge-in flush, transcription, idle CPU on the real microphone, idle/steady/peak RSS | Claude | Claude | microphone in a normal room |
| W8 | agy read-only review of Claude's code; fix verified findings; BUGS.md, security.md rows, ADR final, report, `just ci` | both | Claude | |

**Owner-only at the end:** the recorded command set (KPI-4, KPI-5), 8 h or more of real ambient audio (KPI-3), the voice listening test (OQ-04), the wake phrase decision (OQ-01), the 4 h soak, UJ-1 and UJ-2 by voice, sign-off and merge.

## 2. Decisions Claude takes (ADR-0015 "Proposed")

1. Build order: pure logic and interfaces first, fakes-based pipeline second, real engines last, so almost everything is verified before any model is needed.
2. Wake phrase: "hey jarvis" with the stock openWakeWord model (OQ-01 default). Its pre-trained models are non-commercial (R-06): fine for the owner's own use, recorded as a limit in the ADR.
3. espeak-ng (GPL-3.0, R-07) is linked dynamically from the system package, never bundled or modified; recorded in the ADR.
4. Kokoro fp32 (lesson: int8 is slower than real time, fp16 uses more memory).
5. Follow-up window 8 s, end-of-speech silence 600 ms, both config (PRD).
6. Nudge by voice: C5 asks `VoiceStatus` before each spoken nudge and speaks only if no turn is in progress and no other app holds the microphone; otherwise the existing notification path is used.

## 3. What needs the owner's approval before W4 starts

Downloads (pinned by revision and SHA-256 in `models/manifest.toml`, nothing fetched at run time): whisper.cpp as a pinned submodule and the `tiny.en` model, Silero VAD, the openWakeWord feature models and the "hey jarvis" model, Kokoro-82M fp32 with its voices. System packages are already present (libpipewire 1.6.8, espeak-ng 1.51). Sizes are recorded in the manifest before anything is fetched.
