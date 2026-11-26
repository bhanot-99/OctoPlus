#include "octoplus/voice/earcons.hpp"

#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace octoplus::voice {
namespace {

double EstimateFrequency(std::span<const std::int16_t> samples, int sample_rate) {
  if (samples.size() < 2 || sample_rate <= 0) {
    return 0.0;
  }
  std::size_t crossings = 0;
  for (std::size_t i = 0; i + 1 < samples.size(); ++i) {
    if ((samples[i] >= 0 && samples[i + 1] < 0) || (samples[i] < 0 && samples[i + 1] >= 0)) {
      ++crossings;
    }
  }
  const double dur_s = static_cast<double>(samples.size()) / static_cast<double>(sample_rate);
  return static_cast<double>(crossings) / (2.0 * dur_s);
}

TEST(EarconsTest, LengthAndLimitsAt16kHz) {
  constexpr int kRate = 16000;

  // Session start: 90 ms + 90 ms = 180 ms -> 2880 samples
  const auto start = MakeEarcon(EarconKind::kSessionStart, kRate);
  EXPECT_EQ(start.size(), 2880U);

  // Session end: 110 ms + 110 ms = 220 ms -> 3520 samples
  const auto end = MakeEarcon(EarconKind::kSessionEnd, kRate);
  EXPECT_EQ(end.size(), 3520U);

  // Start and end are different
  EXPECT_NE(start, end);

  // Peak limit: 0.25 of full scale (8192)
  for (const auto s : start) {
    EXPECT_LE(std::abs(s), 8192);
  }
  for (const auto s : end) {
    EXPECT_LE(std::abs(s), 8192);
  }

  // Starts and ends near zero (fade in/out)
  EXPECT_LE(std::abs(start.front()), 5);
  EXPECT_LE(std::abs(start.back()), 5);
  EXPECT_LE(std::abs(end.front()), 5);
  EXPECT_LE(std::abs(end.back()), 5);
}

TEST(EarconsTest, LengthAndLimitsAt24kHz) {
  constexpr int kRate = 24000;

  // Session start: 180 ms -> 4320 samples
  const auto start = MakeEarcon(EarconKind::kSessionStart, kRate);
  EXPECT_EQ(start.size(), 4320U);

  // Session end: 220 ms -> 5280 samples
  const auto end = MakeEarcon(EarconKind::kSessionEnd, kRate);
  EXPECT_EQ(end.size(), 5280U);

  // Peak limit
  for (const auto s : start) {
    EXPECT_LE(std::abs(s), 8192);
  }
  for (const auto s : end) {
    EXPECT_LE(std::abs(s), 8192);
  }
}

TEST(EarconsTest, DominantFrequenciesWithinFivePercent) {
  for (const int sample_rate : {16000, 24000}) {
    // Session start: 660 Hz then 880 Hz
    const auto start = MakeEarcon(EarconKind::kSessionStart, sample_rate);
    const std::size_t half_start = start.size() / 2;
    const std::span<const std::int16_t> start_note1(start.data(), half_start);
    const std::span<const std::int16_t> start_note2(start.data() + half_start, half_start);

    const double f_start1 = EstimateFrequency(start_note1, sample_rate);
    const double f_start2 = EstimateFrequency(start_note2, sample_rate);
    EXPECT_NEAR(f_start1, 660.0, 660.0 * 0.05);
    EXPECT_NEAR(f_start2, 880.0, 880.0 * 0.05);

    // Session end: 880 Hz then 660 Hz
    const auto end = MakeEarcon(EarconKind::kSessionEnd, sample_rate);
    const std::size_t half_end = end.size() / 2;
    const std::span<const std::int16_t> end_note1(end.data(), half_end);
    const std::span<const std::int16_t> end_note2(end.data() + half_end, half_end);

    const double f_end1 = EstimateFrequency(end_note1, sample_rate);
    const double f_end2 = EstimateFrequency(end_note2, sample_rate);
    EXPECT_NEAR(f_end1, 880.0, 880.0 * 0.05);
    EXPECT_NEAR(f_end2, 660.0, 660.0 * 0.05);
  }
}

}  // namespace
}  // namespace octoplus::voice
