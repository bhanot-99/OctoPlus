// The seams of C2 (Phase 7 plan §0): every piece that touches hardware or a model sits behind
// one of these interfaces, so the whole pipeline runs in tests on fixture audio with fakes and
// the real engines plug in unchanged. Nothing here writes audio to disk (FR-VOI-01).
#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "neuroos/result.hpp"

namespace neuroos::voice {

// Capture and recognition run on 16 kHz mono 16-bit samples (FR-VOI-01).
inline constexpr int kCaptureRate = 16000;
// One analysis frame: 512 samples = 32 ms, the frame size of the voice-activity model.
inline constexpr std::size_t kFrameSamples = 512;

// A microphone. Frames arrive on the source's own thread and the callback must not block.
class AudioSource {
 public:
  using FrameCallback = std::function<void(std::span<const std::int16_t>)>;
  virtual ~AudioSource() = default;
  // Starts delivering audio. Fails (and delivers nothing) when the configured device cannot be
  // resolved to an exact node (FR-VOI-03).
  virtual Status Start(FrameCallback on_audio) = 0;
  virtual void Stop() = 0;
  [[nodiscard]] virtual bool running() const = 0;
  // Called once, from a worker thread, when the device goes away while capturing. A removed
  // microphone does not look removed by sample flow, so the loss is reported explicitly.
  virtual void SetOnLost(std::function<void()> /*cb*/) {}
};

// A speaker. Audio is queued; `Flush` drops everything queued and playing at once (NFR-PERF-08).
class AudioSink {
 public:
  virtual ~AudioSink() = default;
  virtual Status Start(int sample_rate) = 0;
  // Queues samples (mono, 16-bit). Never blocks the caller for long.
  virtual void Write(std::span<const std::int16_t> samples) = 0;
  // Drops queued and playing audio; must take effect in well under a millisecond of the call.
  virtual void Flush() = 0;
  // True when nothing is queued or playing.
  [[nodiscard]] virtual bool Idle() const = 0;
};

// Voice activity: a probability of speech for one frame of `kFrameSamples` samples.
class Vad {
 public:
  virtual ~Vad() = default;
  virtual float Process(std::span<const std::int16_t> frame) = 0;
  virtual void Reset() = 0;
};

// The wake word. Fed frames only while speech is detected (FR-VOI-02).
class WakeWord {
 public:
  virtual ~WakeWord() = default;
  // True on the frame at which the wake phrase is recognised.
  virtual bool Process(std::span<const std::int16_t> frame) = 0;
  virtual void Reset() = 0;
};

// Speech to text for one finished utterance.
class Stt {
 public:
  virtual ~Stt() = default;
  virtual Result<std::string> Transcribe(std::span<const std::int16_t> pcm) = 0;
};

// Text to speech: one sentence in, mono 16-bit samples at `sample_rate()` out. The one voice
// (FR-VOI-09); an implementation never switches voice.
class Tts {
 public:
  virtual ~Tts() = default;
  [[nodiscard]] virtual int sample_rate() const = 0;
  virtual Result<std::vector<std::int16_t>> Synthesize(std::string_view text) = 0;
};

}  // namespace neuroos::voice
