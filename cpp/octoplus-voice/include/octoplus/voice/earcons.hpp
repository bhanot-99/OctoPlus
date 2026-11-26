#pragma once

#include <cstdint>
#include <vector>

namespace octoplus::voice {

enum class EarconKind {
  kSessionStart,
  kSessionEnd,
};

// Generates a short two-note earcon chime as 16-bit signed PCM mono audio.
// kSessionStart: rising two-note (660 Hz then 880 Hz, 90 ms each).
// kSessionEnd: falling two-note (880 Hz then 660 Hz, 110 ms each).
// Sine wave with 8 ms linear fade in and out per note, peak amplitude 0.25 of full scale.
std::vector<std::int16_t> MakeEarcon(EarconKind kind, int sample_rate);

}  // namespace octoplus::voice
