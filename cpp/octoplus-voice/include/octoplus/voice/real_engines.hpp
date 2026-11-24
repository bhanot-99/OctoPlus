// Factories for the real engines behind the interfaces in engines.hpp (Phase 7 W4). Compiled
// only in the `voice` presets (OCTOPLUS_WITH_VOICE_ENGINES): they need ONNX Runtime, whisper.cpp,
// PipeWire and espeak-ng. Every factory returns an Error instead of throwing, and a model that
// cannot be loaded means the part reports DEGRADED rather than guessing (rules.md §6).
#pragma once

#include <memory>
#include <string>

#include "octoplus/result.hpp"
#include "octoplus/voice/engines.hpp"
#include "octoplus/voice/pipeline.hpp"

namespace octoplus::voice {

// Silero VAD v6, 16 kHz: 512-sample frames in, a speech probability out.
Result<std::unique_ptr<Vad>> MakeSileroVad(const std::string& model_path);

struct WakeWordPaths {
  std::string melspectrogram;
  std::string embedding;
  std::string classifier;  // e.g. hey_jarvis_v0.1.onnx
  float threshold = 0.5F;
};
// openWakeWord: melspectrogram -> embedding -> classifier over a sliding window.
Result<std::unique_ptr<WakeWord>> MakeOpenWakeWord(const WakeWordPaths& paths);

// whisper.cpp tiny.en, greedy decoding, English.
Result<std::unique_ptr<Stt>> MakeWhisperStt(const std::string& model_path, int threads);

struct KokoroPaths {
  std::string model;      // kokoro-v1.0-fp32.onnx
  std::string tokenizer;  // kokoro-tokenizer.json (the vocabulary)
  std::string voice;      // voices/af_heart.bin
  float speed = 1.0F;
};
// Kokoro-82M, one voice: espeak-ng phonemes -> tokens -> waveform at 24 kHz.
Result<std::unique_ptr<Tts>> MakeKokoroTts(const KokoroPaths& paths);

struct PipeWireOptions {
  // The capture device by its node name (config); resolved to an exact node id before any
  // stream is created. Empty means "the default source", resolved the same way (FR-VOI-03).
  std::string capture_node_name;
  // The playback device by node name; empty is the default sink.
  std::string playback_node_name;
};
Result<std::unique_ptr<AudioSource>> MakePipeWireSource(const PipeWireOptions& opts);
Result<std::unique_ptr<AudioSink>> MakePipeWireSink(const PipeWireOptions& opts);
// Watches the registry for other applications capturing audio (FR-VOI-10).
Result<std::unique_ptr<MicBusyProbe>> MakePipeWireMicProbe();

}  // namespace octoplus::voice
