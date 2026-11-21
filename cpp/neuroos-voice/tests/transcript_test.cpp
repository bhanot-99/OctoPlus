#include "neuroos/voice/transcript.hpp"

#include <gtest/gtest.h>

namespace neuroos::voice {
namespace {

TEST(StripWakePhrase, RemovesTheWakePhraseHoweverItIsSpelled) {
  EXPECT_EQ(StripWakePhrase("Hey Jarvis, what time is it?"), "what time is it?");
  EXPECT_EQ(StripWakePhrase("hey jarvis what time is it"), "what time is it");
  EXPECT_EQ(StripWakePhrase("Jarvis, pause for an hour."), "pause for an hour.");
  EXPECT_EQ(StripWakePhrase("Hey, Jervis. Show me my notes"), "Show me my notes");
  EXPECT_EQ(StripWakePhrase("hay jarves what is two plus two"), "what is two plus two");
  EXPECT_EQ(StripWakePhrase("Okay Jarvis open the calendar"), "open the calendar");
  EXPECT_EQ(StripWakePhrase("And Jarvis, show me what is open"), "show me what is open");
}

TEST(StripWakePhrase, LeavesOrdinaryTextAlone) {
  EXPECT_EQ(StripWakePhrase("what time is it"), "what time is it");
  EXPECT_EQ(StripWakePhrase("Service the car on Tuesday"), "Service the car on Tuesday");
  EXPECT_EQ(StripWakePhrase("Harvest the apples"), "Harvest the apples");
  EXPECT_EQ(StripWakePhrase("a book about Jarvis"), "a book about Jarvis");
  EXPECT_EQ(StripWakePhrase(""), "");
}

TEST(StripWakePhrase, AnUtteranceThatWasOnlyTheWakePhraseBecomesEmpty) {
  EXPECT_EQ(StripWakePhrase("Hey Jarvis."), "");
  EXPECT_EQ(StripWakePhrase("Jarvis"), "");
}

TEST(CollapseRepeats, KeepsALoopedSentenceOnce) {
  EXPECT_EQ(CollapseRepeats("Remember that my sister's birthday is in March. Remember that my "
                            "sister's birthday is in March. Remember that my sister's birthday "
                            "is in March."),
            "Remember that my sister's birthday is in March.");
  EXPECT_EQ(CollapseRepeats("no no no no"), "no");
}

TEST(CollapseRepeats, KeepsALoopThatWasCutOffMidSentenceOnce) {
  EXPECT_EQ(CollapseRepeats("How much disc space is left? How much disc space is left? How much "
                            "disc space is left? How much disc space"),
            "How much disc space is left?");
}

TEST(CollapseRepeats, LeavesRealTextAlone) {
  EXPECT_EQ(CollapseRepeats("what is two plus two"), "what is two plus two");
  EXPECT_EQ(CollapseRepeats("very very good"), "very very good")
      << "two repeats can be real speech";
  EXPECT_EQ(CollapseRepeats("go go"), "go go");
  EXPECT_EQ(CollapseRepeats(""), "");
}

}  // namespace
}  // namespace neuroos::voice
