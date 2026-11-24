#include <gtest/gtest.h>

#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <random>
#include <string>
#include <vector>

#include "octoplus/voice/engines.hpp"
#include "octoplus/voice/real_engines.hpp"

namespace octoplus::voice {
namespace {

std::string ModelPath() { return std::string(OCTOPLUS_VOICE_MODELS_DIR) + "/silero_vad.onnx"; }

TEST(SileroVadTest, MissingPathReturnsError) {
  auto res = MakeSileroVad("non_existent_model_file.onnx");
  EXPECT_FALSE(res.ok());
  EXPECT_EQ(res.error().code, 7);
}

TEST(SileroVadTest, DigitalSilenceGivesLowProbability) {
  auto res = MakeSileroVad(ModelPath());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto vad = std::move(res.value());

  std::array<std::int16_t, 512> silence{};
  for (int i = 0; i < 200; ++i) {
    const float prob = vad->Process(silence);
    EXPECT_GE(prob, 0.0F);
    EXPECT_LT(prob, 0.2F);
  }
}

TEST(SileroVadTest, WrongSizeFrameReturnsZero) {
  auto res = MakeSileroVad(ModelPath());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto vad = std::move(res.value());

  std::array<std::int16_t, 256> short_frame{};
  EXPECT_EQ(vad->Process(short_frame), 0.0F);

  std::array<std::int16_t, 1024> long_frame{};
  EXPECT_EQ(vad->Process(long_frame), 0.0F);

  std::vector<std::int16_t> empty_frame;
  EXPECT_EQ(vad->Process(empty_frame), 0.0F);
}

TEST(SileroVadTest, LoudSyntheticNoiseProbabilityRange) {
  auto res = MakeSileroVad(ModelPath());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto vad = std::move(res.value());

  std::mt19937 rng(12345);
  std::uniform_int_distribution<int> dist(-8000, 8000);

  std::array<std::int16_t, 512> frame{};
  for (int i = 0; i < 200; ++i) {
    for (auto& s : frame) {
      s = static_cast<std::int16_t>(dist(rng));
    }
    const float prob = vad->Process(frame);
    EXPECT_FALSE(std::isnan(prob));
    EXPECT_GE(prob, 0.0F);
    EXPECT_LE(prob, 1.0F);
  }
}

TEST(SileroVadTest, ResetIsDeterministic) {
  auto res = MakeSileroVad(ModelPath());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto vad = std::move(res.value());

  std::mt19937 rng(42);
  std::uniform_int_distribution<int> dist(-10000, 10000);

  std::vector<std::array<std::int16_t, 512>> frames(50);
  for (auto& f : frames) {
    for (auto& s : f) {
      s = static_cast<std::int16_t>(dist(rng));
    }
  }

  std::vector<float> run1(50);
  for (std::size_t i = 0; i < 50; ++i) {
    run1[i] = vad->Process(frames[i]);
  }

  vad->Reset();

  std::vector<float> run2(50);
  for (std::size_t i = 0; i < 50; ++i) {
    run2[i] = vad->Process(frames[i]);
  }

  for (std::size_t i = 0; i < 50; ++i) {
    EXPECT_EQ(run1[i], run2[i]) << "Mismatch at frame " << i;
  }
}

TEST(SileroVadTest, SpeedCheckUnder1500Ms) {
  auto res = MakeSileroVad(ModelPath());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto vad = std::move(res.value());

  std::array<std::int16_t, 512> frame{};
  const auto start = std::chrono::steady_clock::now();
  for (int i = 0; i < 150; ++i) {
    vad->Process(frame);
  }
  const auto elapsed = std::chrono::steady_clock::now() - start;

  EXPECT_LT(elapsed, std::chrono::milliseconds(1500));
}

}  // namespace
}  // namespace octoplus::voice
