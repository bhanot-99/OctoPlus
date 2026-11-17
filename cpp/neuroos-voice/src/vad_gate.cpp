#include "neuroos/voice/vad_gate.hpp"

#include <cmath>
#include <cstdint>

namespace neuroos::voice {

VadGate::VadGate(VadGateConfig cfg) : cfg_(cfg) {}

VadEvent VadGate::Update(float prob, std::uint64_t now_ns) {
  VadEvent event;
  const bool is_nan = std::isnan(prob);

  if (!speech_) {
    // In silence: count consecutive frames with prob >= on_threshold.
    // Any frame below on_threshold (or NaN or between-thresholds) resets the count.
    if (!is_nan && prob >= cfg_.on_threshold) {
      ++consecutive_speech_frames_;
      if (consecutive_speech_frames_ >= cfg_.start_frames) {
        speech_ = true;
        event.started = true;
        consecutive_speech_frames_ = 0;
        silence_timer_started_ = false;
        silence_start_ns_ = 0;
      }
    } else {
      consecutive_speech_frames_ = 0;
    }
  } else {
    // In speech:
    // A frame >= off_threshold keeps speech and clears the silence timer.
    // The first frame < off_threshold (or NaN) starts a silence timer.
    if (!is_nan && prob >= cfg_.off_threshold) {
      silence_timer_started_ = false;
      silence_start_ns_ = 0;
    } else {
      if (!silence_timer_started_) {
        silence_timer_started_ = true;
        silence_start_ns_ = now_ns;
      }
      if (now_ns >= silence_start_ns_ && (now_ns - silence_start_ns_) >= cfg_.end_silence_ns) {
        speech_ = false;
        event.ended = true;
        silence_timer_started_ = false;
        silence_start_ns_ = 0;
        consecutive_speech_frames_ = 0;
      }
    }
  }

  event.speech = speech_;
  return event;
}

void VadGate::Reset() {
  speech_ = false;
  consecutive_speech_frames_ = 0;
  silence_timer_started_ = false;
  silence_start_ns_ = 0;
}

bool VadGate::speech() const { return speech_; }

}  // namespace neuroos::voice
