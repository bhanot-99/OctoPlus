#include "neuroos/voice/voice_config.hpp"

#include <gtest/gtest.h>

namespace neuroos::voice {
namespace {

TEST(VoiceConfig, EmptyTextGivesTheDefaults) {
  const auto c = ParseVoiceConfig("");
  ASSERT_TRUE(c.ok());
  EXPECT_EQ(c.value().voice, "af_heart");
  EXPECT_EQ(c.value().end_silence_ms, 600U);
  EXPECT_EQ(c.value().follow_up_s, 8U);
  EXPECT_EQ(c.value().max_consecutive_follow_ups, 1U);
  EXPECT_TRUE(c.value().capture_node.empty());
  EXPECT_FALSE(c.value().barge_in_by_speech) << "only the wake phrase interrupts by default";
}

TEST(VoiceConfig, BargeInCanBeSwitchedToAnySpeech) {
  const auto speech = ParseVoiceConfig("barge_in = speech\n");
  ASSERT_TRUE(speech.ok());
  EXPECT_TRUE(speech.value().barge_in_by_speech);
  const auto wake = ParseVoiceConfig("barge_in = wake\n");
  ASSERT_TRUE(wake.ok());
  EXPECT_FALSE(wake.value().barge_in_by_speech);
  EXPECT_FALSE(ParseVoiceConfig("barge_in = sometimes\n").ok());
}

TEST(VoiceConfig, KeysOverrideTheDefaults) {
  const auto c = ParseVoiceConfig(
      "# my setup\ncapture_node = \"alsa_input.usb-mic\"\nvoice = af_bella  # nicer\n"
      "wake_threshold = 0.6\nfollow_up_s = 12\nmax_consecutive_follow_ups = "
      "2\nspeed_milli=1100\nstt_threads = 2\n");
  ASSERT_TRUE(c.ok());
  EXPECT_EQ(c.value().capture_node, "alsa_input.usb-mic");
  EXPECT_EQ(c.value().voice, "af_bella");
  EXPECT_FLOAT_EQ(c.value().wake_threshold, 0.6F);
  EXPECT_EQ(c.value().follow_up_s, 12U);
  EXPECT_EQ(c.value().max_consecutive_follow_ups, 2U);
  EXPECT_EQ(c.value().speed_milli, 1100U);
  EXPECT_EQ(c.value().stt_threads, 2U);
}

TEST(VoiceConfig, AnUnknownKeyABadValueOrAPathLikeVoiceIsRefused) {
  for (const char* bad :
       {"colour = red\n", "voice = ../../etc/passwd\n", "voice = AF_HEART\n",
        "follow_up_s = soon\n", "wake_threshold = 7\n", "end_silence_ms = 5\n", "just a line\n",
        "stt_threads = 0\n", "speed_milli = 99999\n", "max_consecutive_follow_ups = 15\n"}) {
    EXPECT_FALSE(ParseVoiceConfig(bad).ok()) << bad;
  }
}

}  // namespace
}  // namespace neuroos::voice
