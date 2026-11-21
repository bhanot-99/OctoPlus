#include <gtest/gtest.h>

#include <array>
#include <chrono>
#include <cstdint>
#include <random>
#include <string>
#include <vector>

#include "neuroos/voice/engines.hpp"
#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

WakeWordPaths DefaultPaths(float threshold = 0.5F) {
  const std::string dir = NEUROOS_VOICE_MODELS_DIR;
  return WakeWordPaths{
      .melspectrogram = dir + "/melspectrogram.onnx",
      .embedding = dir + "/embedding_model.onnx",
      .classifier = dir + "/hey_jarvis_v0.1.onnx",
      .threshold = threshold,
  };
}

TEST(OpenWakeWordTest, MissingPathReturnsError) {
  WakeWordPaths const bad_paths{
      .melspectrogram = "non_existent_mel.onnx",
      .embedding = "non_existent_emb.onnx",
      .classifier = "non_existent_cls.onnx",
      .threshold = 0.5F,
  };
  auto res = MakeOpenWakeWord(bad_paths);
  EXPECT_FALSE(res.ok());
  EXPECT_EQ(res.error().code, 7);
}

TEST(OpenWakeWordTest, ThirtySecondsOfSilenceNeverDetects) {
  auto res = MakeOpenWakeWord(DefaultPaths());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto ww = std::move(res.value());

  // 30 seconds at 16000 Hz = 480000 samples. 480000 / 512 = 937.5 frames -> 938 frames
  std::array<std::int16_t, 512> silence{};
  for (int i = 0; i < 938; ++i) {
    EXPECT_FALSE(ww->Process(silence)) << "Detected on frame " << i;
  }
}

TEST(OpenWakeWordTest, ThirtySecondsOfNoiseNeverDetects) {
  auto res = MakeOpenWakeWord(DefaultPaths());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto ww = std::move(res.value());

  std::mt19937 rng(42);
  std::uniform_int_distribution<int> dist(-3000, 3000);

  std::array<std::int16_t, 512> frame{};
  for (int i = 0; i < 938; ++i) {
    for (auto& s : frame) {
      s = static_cast<std::int16_t>(dist(rng));
    }
    EXPECT_FALSE(ww->Process(frame)) << "Detected on noise frame " << i;
  }
}

TEST(OpenWakeWordTest, ResetPreservesBehavior) {
  auto res = MakeOpenWakeWord(DefaultPaths());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto ww = std::move(res.value());

  std::mt19937 rng(1337);
  std::uniform_int_distribution<int> dist(-3000, 3000);

  std::vector<std::array<std::int16_t, 512>> frames(100);
  for (auto& f : frames) {
    for (auto& s : f) {
      s = static_cast<std::int16_t>(dist(rng));
    }
  }

  for (const auto& f : frames) {
    EXPECT_FALSE(ww->Process(f));
  }

  ww->Reset();

  for (const auto& f : frames) {
    EXPECT_FALSE(ww->Process(f));
  }
}

TEST(OpenWakeWordTest, BoundedDetectionRateAndWarmup) {
  // Construct with threshold 0.0F so any score >= 0 could trigger after warm-up
  auto res = MakeOpenWakeWord(DefaultPaths(0.0F));
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto ww = std::move(res.value());

  std::array<std::int16_t, 512> frame{};
  std::mt19937 rng(7);
  std::uniform_int_distribution<int> dist(-2000, 2000);
  for (auto& s : frame) {
    s = static_cast<std::int16_t>(dist(rng));
  }

  // 5 chunks = 5 * 1280 = 6400 samples.
  // In 512-sample frames, 12 frames = 6144 samples (< 6400).
  // Frame 13 brings total to 6656 samples (completing chunk 5 at sample 6400).
  // During these first 6400 samples, warm-up MUST prevent detection.
  std::size_t samples_fed = 0;
  int true_count = 0;
  int total_calls = 0;

  for (int i = 0; i < 12; ++i) {
    EXPECT_FALSE(ww->Process(frame)) << "Detected during warm-up at frame " << i;
    samples_fed += 512;
  }
  // The 13th frame completes chunk 5 (the last warm-up chunk, sample 6400).
  // Still chunk 5 warm-up!
  EXPECT_FALSE(ww->Process(frame)) << "Detected during warm-up chunk 5";
  samples_fed += 512;
  EXPECT_GE(samples_fed, 5 * 1280U);

  // Now feed another 50 frames and verify detection rate is bounded:
  // At most 1 detection per chunk (each chunk requires 2 or 3 frames).
  // Out of N frames, detections cannot exceed (N / 2) + 1.
  for (int i = 0; i < 50; ++i) {
    if (ww->Process(frame)) {
      ++true_count;
    }
    ++total_calls;
  }

  // Each chunk is 1280 samples, which takes 2.5 frames (512 each).
  // In 50 frames, at most 20 chunks can finish.
  EXPECT_LE(true_count, 21);
}

TEST(OpenWakeWordTest, SpeedCheckUnderThreeSeconds) {
  auto res = MakeOpenWakeWord(DefaultPaths());
  ASSERT_TRUE(res.ok()) << res.error().message;
  auto ww = std::move(res.value());

  // 10 seconds of 16 kHz audio = 160,000 samples = ~313 frames of 512 samples
  std::array<std::int16_t, 512> frame{};
  const auto start = std::chrono::steady_clock::now();
  for (int i = 0; i < 313; ++i) {
    ww->Process(frame);
  }
  const auto elapsed = std::chrono::steady_clock::now() - start;

  EXPECT_LT(elapsed, std::chrono::seconds(3));
}

}  // namespace
}  // namespace neuroos::voice
