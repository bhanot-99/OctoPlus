#pragma once

#include <cmath>
#include <cstdint>

namespace octoplus::voice {

struct VadGateConfig {
  float on_threshold = 0.5F;       // a frame at or above this counts as speech
  float off_threshold = 0.35F;     // a frame below this counts as silence; between thresholds
                                   // state does not change
  std::uint32_t start_frames = 2;  // consecutive speech frames needed to start (Architecture §6.5)
  std::uint64_t end_silence_ns =
      600'000'000;  // continuous silence needed to end an utterance (FR-VOI-04: 600 ms)
};

struct VadEvent {
  bool speech = false;
  bool started = false;
  bool ended = false;

  bool operator==(const VadEvent& other) const = default;
};

class VadGate {
 public:
  explicit VadGate(VadGateConfig cfg = {});

  // `prob` is the speech probability of one frame; `now_ns` is the time of that frame
  // (monotonic nanoseconds, never decreasing).
  VadEvent Update(float prob, std::uint64_t now_ns);

  void Reset();  // back to silence, forgets all counters

  void SetEndSilenceNs(std::uint64_t ns);

  [[nodiscard]] bool speech() const;

 private:
  VadGateConfig cfg_;
  bool speech_ = false;
  std::uint32_t consecutive_speech_frames_ = 0;
  bool silence_timer_started_ = false;
  std::uint64_t silence_start_ns_ = 0;
};

}  // namespace octoplus::voice
