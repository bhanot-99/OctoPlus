#include "octoplus/voice/spoken_lines.hpp"

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <set>
#include <string>
#include <vector>

namespace octoplus::voice {
namespace {

TEST(SpokenLinesTest, SpokenPercentBoundaries) {
  EXPECT_EQ(SpokenPercent(92.4), "92");
  EXPECT_EQ(SpokenPercent(92.5), "93");
  EXPECT_EQ(SpokenPercent(0.0), "0");
  EXPECT_EQ(SpokenPercent(-5.0), "0");
  EXPECT_EQ(SpokenPercent(105.0), "100");
  EXPECT_EQ(SpokenPercent(100.0), "100");
}

TEST(SpokenLinesTest, SpokenSizeBoundaries) {
  EXPECT_EQ(SpokenSize(0.94), "less than a gigabyte");
  EXPECT_EQ(SpokenSize(0.95), "1 gigabyte");
  EXPECT_EQ(SpokenSize(1.0), "1 gigabyte");
  EXPECT_EQ(SpokenSize(1.5), "1 gigabyte");
  EXPECT_EQ(SpokenSize(1.51), "1.5 gigabytes");
  EXPECT_EQ(SpokenSize(4.0), "4 gigabytes");
  EXPECT_EQ(SpokenSize(4.5), "4.5 gigabytes");
  EXPECT_EQ(SpokenSize(9.95), "10 gigabytes");
  EXPECT_EQ(SpokenSize(10.0), "10 gigabytes");
  EXPECT_EQ(SpokenSize(12.3), "12 gigabytes");
  EXPECT_EQ(SpokenSize(-1.0), "less than a gigabyte");
}

TEST(SpokenLinesTest, SpokenHoursBoundaries) {
  EXPECT_EQ(SpokenHours(0.99), "less than an hour");
  EXPECT_EQ(SpokenHours(1.0), "an hour");
  EXPECT_EQ(SpokenHours(1.5), "an hour");
  EXPECT_EQ(SpokenHours(1.51), "2 hours");
  EXPECT_EQ(SpokenHours(2.0), "2 hours");
  EXPECT_EQ(SpokenHours(7.4), "7 hours");
}

TEST(SpokenLinesTest, SpokenMinutesBoundaries) {
  EXPECT_EQ(SpokenMinutes(0.5), "a minute");
  EXPECT_EQ(SpokenMinutes(1.0), "a few minutes");
  EXPECT_EQ(SpokenMinutes(2.9), "a few minutes");
  EXPECT_EQ(SpokenMinutes(3.0), "5 minutes");
  EXPECT_EQ(SpokenMinutes(23.0), "25 minutes");
  EXPECT_EQ(SpokenMinutes(59.0), "60 minutes");
  EXPECT_EQ(SpokenMinutes(60.0), "an hour");
  EXPECT_EQ(SpokenMinutes(89.0), "an hour");
  EXPECT_EQ(SpokenMinutes(90.0), "about 2 hours");
  EXPECT_EQ(SpokenMinutes(150.0), "about 3 hours");
}

TEST(SpokenLinesTest, AllTemplatesWithTypicalValues) {
  // 1. disk.nearly_full
  {
    std::vector<NudgeArg> args = {{"percent", 88.0}};
    auto res = RenderNudgeLine("disk.nearly_full", args);
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Heads up, your disk is 88 percent full.");
    EXPECT_EQ(res->offer, "Want me to show what's using the space?");
  }

  // 2. disk.critical
  {
    std::vector<NudgeArg> args = {{"free_gib", 0.5}};
    auto res = RenderNudgeLine("disk.critical", args);
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Your disk is almost full. You have less than a gigabyte left.");
    EXPECT_EQ(res->offer, "Want me to show what's using the space?");
  }

  // 3. disk.filling_fast (drop "about" for "less than an hour" and "an hour")
  {
    std::vector<NudgeArg> args1 = {{"hours", 0.8}};
    auto res1 = RenderNudgeLine("disk.filling_fast", args1);
    ASSERT_TRUE(res1.has_value());
    EXPECT_EQ(res1->line,
              "Your disk is filling quickly. At this rate it's full in less than an hour.");
    EXPECT_EQ(res1->offer, "Want me to show what's using the space?");

    std::vector<NudgeArg> args2 = {{"hours", 1.2}};
    auto res2 = RenderNudgeLine("disk.filling_fast", args2);
    ASSERT_TRUE(res2.has_value());
    EXPECT_EQ(res2->line, "Your disk is filling quickly. At this rate it's full in an hour.");

    std::vector<NudgeArg> args3 = {{"hours", 3.0}};
    auto res3 = RenderNudgeLine("disk.filling_fast", args3);
    ASSERT_TRUE(res3.has_value());
    EXPECT_EQ(res3->line, "Your disk is filling quickly. At this rate it's full in about 3 hours.");
  }

  // 4. battery.low
  {
    std::vector<NudgeArg> args = {{"percent", 12.0}};
    auto res = RenderNudgeLine("battery.low", args);
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Battery's at 12 percent and you're not plugged in.");
    EXPECT_EQ(res->offer, "");
  }

  // 5. thermal.hot
  {
    auto res = RenderNudgeLine("thermal.hot", {});
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Your laptop's been running hot for a while.");
    EXPECT_EQ(res->offer, "Want to know what's working hardest?");
  }

  // 6. memory.pressure
  {
    auto res = RenderNudgeLine("memory.pressure", {});
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Memory is nearly full, so things may feel slow.");
    EXPECT_EQ(res->offer, "Want me to say what's using the most?");
  }

  // 7. session.long
  {
    std::vector<NudgeArg> args = {{"minutes", 45.0}};
    auto res = RenderNudgeLine("session.long", args);
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "You've been at it for 45 minutes. Might be time for a short break.");
    EXPECT_EQ(res->offer, "");
  }

  // 8. part.degraded
  {
    auto res = RenderNudgeLine("part.degraded", {});
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "Part of me isn't working right now, so I may miss things.");
    EXPECT_EQ(res->offer, "");
  }

  // 9. nudge.while_away
  {
    std::vector<NudgeArg> args1 = {{"count", 1.0}};
    auto res1 = RenderNudgeLine("nudge.while_away", args1);
    ASSERT_TRUE(res1.has_value());
    EXPECT_EQ(res1->line, "One thing needed your attention while you were away.");
    EXPECT_EQ(res1->offer, "");

    std::vector<NudgeArg> args0 = {{"count", 0.0}};  // n < 1 treated as 1
    auto res0 = RenderNudgeLine("nudge.while_away", args0);
    ASSERT_TRUE(res0.has_value());
    EXPECT_EQ(res0->line, "One thing needed your attention while you were away.");

    std::vector<NudgeArg> args3 = {{"count", 3.0}};
    auto res3 = RenderNudgeLine("nudge.while_away", args3);
    ASSERT_TRUE(res3.has_value());
    EXPECT_EQ(res3->line, "3 things needed your attention while you were away.");
  }

  // 10. nudge.test
  {
    auto res = RenderNudgeLine("nudge.test", {});
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line, "This is a test of the nudge system. Nothing needs attention.");
    EXPECT_EQ(res->offer, "");
  }
}

TEST(SpokenLinesTest, ArgumentValidationFailures) {
  // Wrong arg name
  std::vector<NudgeArg> wrong_arg = {{"wrong", 10.0}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", wrong_arg), std::nullopt);

  // Missing arg
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", {}), std::nullopt);

  // Extra arg
  std::vector<NudgeArg> extra_arg = {{"percent", 10.0}, {"extra", 5.0}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", extra_arg), std::nullopt);

  // Extra arg on 0-arg template
  std::vector<NudgeArg> extra_on_zero = {{"foo", 1.0}};
  EXPECT_EQ(RenderNudgeLine("thermal.hot", extra_on_zero), std::nullopt);

  // Duplicate arg name
  std::vector<NudgeArg> dup_arg = {{"percent", 10.0}, {"percent", 20.0}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", dup_arg), std::nullopt);

  // Unknown template
  EXPECT_EQ(RenderNudgeLine("unknown.template", {}), std::nullopt);
}

TEST(SpokenLinesTest, NonFiniteValuesReturnNullopt) {
  const double nan = std::numeric_limits<double>::quiet_NaN();
  const double inf = std::numeric_limits<double>::infinity();

  std::vector<NudgeArg> nan_arg = {{"percent", nan}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", nan_arg), std::nullopt);

  std::vector<NudgeArg> inf_arg = {{"percent", inf}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", inf_arg), std::nullopt);

  std::vector<NudgeArg> neg_inf_arg = {{"percent", -inf}};
  EXPECT_EQ(RenderNudgeLine("disk.nearly_full", neg_inf_arg), std::nullopt);
}

TEST(SpokenLinesTest, PreamblesAndStatusPhrases) {
  // Preambles
  const auto& thinking = PreamblePhrases(1);
  ASSERT_EQ(thinking.size(), 3U);
  EXPECT_EQ(thinking[0], "One moment.");
  EXPECT_EQ(thinking[1], "Let me think.");
  EXPECT_EQ(thinking[2], "Hmm, let me see.");

  const auto& looking = PreamblePhrases(2);
  ASSERT_EQ(looking.size(), 3U);
  EXPECT_EQ(looking[0], "Let me look.");
  EXPECT_EQ(looking[1], "Checking.");
  EXPECT_EQ(looking[2], "Looking at that now.");

  const auto& ack = PreamblePhrases(3);
  ASSERT_EQ(ack.size(), 3U);
  EXPECT_EQ(ack[0], "Okay.");
  EXPECT_EQ(ack[1], "Got it.");
  EXPECT_EQ(ack[2], "Sure.");

  // Fallback -> thinking
  EXPECT_EQ(PreamblePhrases(99), thinking);
  EXPECT_EQ(PreamblePhrases(0), thinking);

  // Status phrases
  EXPECT_EQ(StatusPhrase(1), "I can't think right now. Give me a moment.");
  EXPECT_EQ(StatusPhrase(2), "Sorry, I didn't catch that.");
  EXPECT_EQ(StatusPhrase(3), "My voice is restarting, one moment.");
  // Fallback -> error line (3)
  EXPECT_EQ(StatusPhrase(99), "My voice is restarting, one moment.");
  EXPECT_EQ(StatusPhrase(0), "My voice is restarting, one moment.");

  // AllCachedPhrases has 12 distinct entries
  const auto cached = AllCachedPhrases();
  ASSERT_EQ(cached.size(), 12U);
  std::set<std::string> const unique(cached.begin(), cached.end());
  EXPECT_EQ(unique.size(), 12U);
}

TEST(SpokenLinesTest, NoGrammarBugsLikeOneHoursOrOneThings) {
  // Verify no standalone "1 minutes", "1 hours", "1 things" (like "about 1 hours" or "1 hours")
  for (double m = 0.0; m <= 150.0; m += 0.5) {
    const std::string s = SpokenMinutes(m);
    EXPECT_NE(s, "1 minutes");
    EXPECT_NE(s, "1 hours");
    EXPECT_EQ(s.find("about 1 hours"), std::string::npos);
  }
  for (double h = 0.0; h <= 24.0; h += 0.25) {
    const std::string s = SpokenHours(h);
    EXPECT_NE(s, "1 hours");
  }
  for (double c = 0.0; c <= 10.0; c += 0.5) {
    std::vector<NudgeArg> args = {{"count", c}};
    auto res = RenderNudgeLine("nudge.while_away", args);
    ASSERT_TRUE(res.has_value());
    EXPECT_EQ(res->line.find("1 things"), std::string::npos);
  }
}

}  // namespace
}  // namespace octoplus::voice
