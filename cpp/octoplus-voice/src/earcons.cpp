#include "octoplus/voice/earcons.hpp"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <numbers>
#include <vector>

namespace octoplus::voice {
namespace {

constexpr double kPeak = 8191.75;  // 0.25 of full scale (32767.0)
constexpr double kFadeMs = 8.0;

void GenerateNote(double freq, double dur_ms, int sample_rate, std::vector<std::int16_t>& out) {
  if (sample_rate <= 0 || dur_ms <= 0.0) {
    return;
  }
  const auto num_samples =
      static_cast<std::size_t>(std::round(dur_ms * static_cast<double>(sample_rate) / 1000.0));
  const auto fade_samples =
      static_cast<std::size_t>(std::round(kFadeMs * static_cast<double>(sample_rate) / 1000.0));

  out.reserve(out.size() + num_samples);
  for (std::size_t i = 0; i < num_samples; ++i) {
    double gain = 1.0;
    if (fade_samples > 0) {
      if (i < fade_samples) {
        gain = static_cast<double>(i) / static_cast<double>(fade_samples);
      } else if (i >= num_samples - fade_samples) {
        gain = static_cast<double>(num_samples - 1 - i) / static_cast<double>(fade_samples);
      }
    }
    const double t = static_cast<double>(i) / static_cast<double>(sample_rate);
    const double s = kPeak * gain * std::sin(2.0 * std::numbers::pi * freq * t);
    out.push_back(static_cast<std::int16_t>(std::round(s)));
  }
}

}  // namespace

std::vector<std::int16_t> MakeEarcon(EarconKind kind, int sample_rate) {
  std::vector<std::int16_t> audio;
  if (kind == EarconKind::kSessionStart) {
    // Rising two-note: 660 Hz then 880 Hz, 90 ms each
    GenerateNote(660.0, 90.0, sample_rate, audio);
    GenerateNote(880.0, 90.0, sample_rate, audio);
  } else {
    // Falling two-note: 880 Hz then 660 Hz, 110 ms each
    GenerateNote(880.0, 110.0, sample_rate, audio);
    GenerateNote(660.0, 110.0, sample_rate, audio);
  }
  return audio;
}

}  // namespace octoplus::voice
