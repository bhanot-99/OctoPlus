#include "octoplus/voice/vad_gate.hpp"

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <limits>
#include <random>

namespace octoplus::voice {
namespace {

TEST(VadGateTest, OneFrameBlipDoesNotStart) {
  VadGate gate;
  const auto ev1 = gate.Update(0.6F, 1000);
  EXPECT_FALSE(ev1.started);
  EXPECT_FALSE(ev1.speech);
  EXPECT_FALSE(gate.speech());

  const auto ev2 = gate.Update(0.2F, 2000);
  EXPECT_FALSE(ev2.started);
  EXPECT_FALSE(ev2.speech);
  EXPECT_FALSE(gate.speech());
}

TEST(VadGateTest, TwoFramesStartExactlyOnSecond) {
  VadGate gate;
  const auto ev1 = gate.Update(0.5F, 1000);
  EXPECT_FALSE(ev1.started);
  EXPECT_FALSE(ev1.speech);

  const auto ev2 = gate.Update(0.55F, 2000);
  EXPECT_TRUE(ev2.started);
  EXPECT_TRUE(ev2.speech);
  EXPECT_TRUE(gate.speech());

  const auto ev3 = gate.Update(0.6F, 3000);
  EXPECT_FALSE(ev3.started);
  EXPECT_TRUE(ev3.speech);
}

TEST(VadGateTest, HysteresisBandHoldsSpeech) {
  VadGate gate;
  gate.Update(0.6F, 1000);
  gate.Update(0.6F, 2000);
  ASSERT_TRUE(gate.speech());

  // In [0.35, 0.50), state stays speech and silence timer is not started
  const auto ev = gate.Update(0.4F, 3000);
  EXPECT_FALSE(ev.started);
  EXPECT_FALSE(ev.ended);
  EXPECT_TRUE(ev.speech);
  EXPECT_TRUE(gate.speech());
}

TEST(VadGateTest, Silence599MsDoesNotEnd600MsDoes) {
  VadGate gate;
  gate.Update(0.6F, 0);
  gate.Update(0.6F, 32'000'000);
  ASSERT_TRUE(gate.speech());

  // Silence starts at t = 100'000'000
  const auto ev1 = gate.Update(0.1F, 100'000'000);
  EXPECT_FALSE(ev1.ended);
  EXPECT_TRUE(ev1.speech);

  // At 599 ms (100 ms + 599 ms = 699 ms): does not end
  const auto ev2 = gate.Update(0.1F, 699'000'000);
  EXPECT_FALSE(ev2.ended);
  EXPECT_TRUE(ev2.speech);

  // At 600 ms (100 ms + 600 ms = 700 ms): ends
  const auto ev3 = gate.Update(0.1F, 700'000'000);
  EXPECT_TRUE(ev3.ended);
  EXPECT_FALSE(ev3.speech);
  EXPECT_FALSE(gate.speech());
}

TEST(VadGateTest, SpeechResumingAt500MsCancelsEnd) {
  VadGate gate;
  gate.Update(0.6F, 0);
  gate.Update(0.6F, 32'000'000);
  ASSERT_TRUE(gate.speech());

  // Silence begins at 100 ms
  gate.Update(0.1F, 100'000'000);
  // At 600 ms (500 ms elapsed), speech resumes at 0.4 (>= off_threshold)
  const auto ev_res = gate.Update(0.4F, 600'000'000);
  EXPECT_FALSE(ev_res.ended);
  EXPECT_TRUE(ev_res.speech);

  // New silence starts at 700 ms
  gate.Update(0.1F, 700'000'000);
  // At 1299 ms (599 ms elapsed): still speech
  const auto ev_stay = gate.Update(0.1F, 1'299'000'000);
  EXPECT_FALSE(ev_stay.ended);
  EXPECT_TRUE(ev_stay.speech);

  // At 1300 ms (600 ms elapsed): ends
  const auto ev_end = gate.Update(0.1F, 1'300'000'000);
  EXPECT_TRUE(ev_end.ended);
  EXPECT_FALSE(ev_end.speech);
}

TEST(VadGateTest, BetweenThresholdsFramesInSilenceResetStartCount) {
  VadGate gate;
  gate.Update(0.6F, 1000);  // 1 frame
  // Frame between 0.35 and 0.50 while in silence resets start count
  gate.Update(0.4F, 2000);

  const auto ev1 = gate.Update(0.6F, 3000);  // 1 frame again
  EXPECT_FALSE(ev1.started);
  EXPECT_FALSE(ev1.speech);

  const auto ev2 = gate.Update(0.6F, 4000);  // 2nd frame
  EXPECT_TRUE(ev2.started);
  EXPECT_TRUE(ev2.speech);
}

TEST(VadGateTest, Reset) {
  VadGate gate;
  gate.Update(0.6F, 1000);
  gate.Update(0.6F, 2000);
  ASSERT_TRUE(gate.speech());

  gate.Reset();
  EXPECT_FALSE(gate.speech());

  const auto ev = gate.Update(0.6F, 3000);
  EXPECT_FALSE(ev.started);
  EXPECT_FALSE(ev.speech);
}

TEST(VadGateTest, NanCountsAsSilence) {
  VadGate gate;
  const float nan = std::numeric_limits<float>::quiet_NaN();

  // In silence: NaN resets speech count
  gate.Update(0.6F, 1000);
  gate.Update(nan, 2000);
  const auto ev1 = gate.Update(0.6F, 3000);
  EXPECT_FALSE(ev1.started);
  EXPECT_FALSE(ev1.speech);

  // Start speech
  gate.Update(0.6F, 4000);
  ASSERT_TRUE(gate.speech());

  // In speech: NaN starts silence timer
  gate.Update(nan, 100'000'000);
  const auto ev2 = gate.Update(nan, 700'000'000);
  EXPECT_TRUE(ev2.ended);
  EXPECT_FALSE(ev2.speech);
}

TEST(VadGateTest, PropertyStrictlyAlternatingStartedEnded) {
  VadGate gate;
  std::mt19937 rng(42);
  std::uniform_real_distribution<float> prob_dist(-0.1F, 1.1F);
  std::bernoulli_distribution nan_dist(0.05);

  bool in_speech = false;
  std::uint64_t now_ns = 0;

  for (int i = 0; i < 20000; ++i) {
    now_ns += 32'000'000;  // 32 ms frame
    const float prob = nan_dist(rng) ? std::numeric_limits<float>::quiet_NaN() : prob_dist(rng);
    const auto ev = gate.Update(prob, now_ns);

    EXPECT_FALSE(ev.started && ev.ended);
    EXPECT_EQ(ev.speech, gate.speech());

    if (ev.started) {
      EXPECT_FALSE(in_speech) << "Started when already in speech at step " << i;
      EXPECT_TRUE(ev.speech);
      in_speech = true;
    }
    if (ev.ended) {
      EXPECT_TRUE(in_speech) << "Ended when not in speech at step " << i;
      EXPECT_FALSE(ev.speech);
      in_speech = false;
    }
    EXPECT_EQ(in_speech, ev.speech);
  }
}

}  // namespace
}  // namespace octoplus::voice
