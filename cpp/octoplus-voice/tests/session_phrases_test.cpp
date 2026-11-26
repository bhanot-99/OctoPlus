#include "octoplus/voice/session_phrases.hpp"

#include <gtest/gtest.h>

namespace octoplus::voice {
namespace {

TEST(SessionPhrasesTest, StopPhrasesPositives) {
  EXPECT_TRUE(IsStopPhrase("bye"));
  // Companion rhythm controls close the chat after their fixed reply.
  EXPECT_TRUE(IsStopPhrase("not now"));
  EXPECT_TRUE(IsStopPhrase("Not right now"));
  EXPECT_TRUE(IsStopPhrase("be quiet for an hour"));
  EXPECT_TRUE(IsStopPhrase("stop talking"));
  EXPECT_FALSE(IsStopPhrase("I think now is not the time to be quiet for sure"));
  EXPECT_TRUE(IsStopPhrase("Bye!"));
  EXPECT_TRUE(IsStopPhrase("goodbye"));
  EXPECT_TRUE(IsStopPhrase("good bye"));
  EXPECT_TRUE(IsStopPhrase("Good night."));
  EXPECT_TRUE(IsStopPhrase("stop listening"));
  EXPECT_TRUE(IsStopPhrase("that's all"));
  EXPECT_TRUE(IsStopPhrase("thats all"));
  EXPECT_TRUE(IsStopPhrase("see you"));
  EXPECT_TRUE(IsStopPhrase("talk later"));
  EXPECT_TRUE(IsStopPhrase("gotta go"));

  // With polite prefix or suffix within 5 words
  EXPECT_TRUE(IsStopPhrase("okay, bye!"));
  EXPECT_TRUE(IsStopPhrase("see you tomorrow"));
  EXPECT_TRUE(IsStopPhrase("that's all thank you"));
  EXPECT_TRUE(IsStopPhrase("alright, gotta go now"));
  EXPECT_TRUE(IsStopPhrase("well, good night then"));
}

TEST(SessionPhrasesTest, StopPhrasesNegatives) {
  // Long sentences containing stop words (> 5 words)
  EXPECT_FALSE(IsStopPhrase("i said bye to my friend yesterday after the long meeting"));
  EXPECT_FALSE(IsStopPhrase("that is all the files in the folder from last week"));
  EXPECT_FALSE(IsStopPhrase("we need to talk later about the deployment issues"));

  // Substring or unrelated phrases
  EXPECT_FALSE(IsStopPhrase("stop the music"));
  EXPECT_FALSE(IsStopPhrase("stop playback"));
  EXPECT_FALSE(IsStopPhrase("all files"));
  EXPECT_FALSE(IsStopPhrase("see the document"));
  EXPECT_FALSE(IsStopPhrase("standby mode"));
  EXPECT_FALSE(IsStopPhrase(""));
  EXPECT_FALSE(IsStopPhrase("   "));
}

TEST(SessionPhrasesTest, GuestPhrasesPositives) {
  EXPECT_TRUE(IsGuestPhrase("someone's here"));
  EXPECT_TRUE(IsGuestPhrase("someone is here"));
  EXPECT_TRUE(IsGuestPhrase("somebody's here"));
  EXPECT_TRUE(IsGuestPhrase("guest mode"));
  EXPECT_TRUE(IsGuestPhrase("i have a guest"));
  EXPECT_TRUE(IsGuestPhrase("i have guests"));
  EXPECT_TRUE(IsGuestPhrase("i have company"));
  EXPECT_TRUE(IsGuestPhrase("i've got company"));
  EXPECT_TRUE(IsGuestPhrase("people are here"));

  // Within 6 words
  EXPECT_TRUE(IsGuestPhrase("hey, someone's here now"));
  EXPECT_TRUE(IsGuestPhrase("hold on, i have company"));
  EXPECT_TRUE(IsGuestPhrase("turn on guest mode please"));
}

TEST(SessionPhrasesTest, GuestPhrasesNegatives) {
  EXPECT_FALSE(IsGuestPhrase("someone is coming over tomorrow afternoon for lunch"));
  EXPECT_FALSE(IsGuestPhrase("are there people here today in the office?"));
  EXPECT_FALSE(IsGuestPhrase("what company do you work for?"));
  EXPECT_FALSE(IsGuestPhrase(""));
}

TEST(SessionPhrasesTest, GuestEndPhrasesPositives) {
  EXPECT_TRUE(IsGuestEndPhrase("guest mode off"));
  EXPECT_TRUE(IsGuestEndPhrase("they left"));
  EXPECT_TRUE(IsGuestEndPhrase("they're gone"));
  EXPECT_TRUE(IsGuestEndPhrase("you can talk again"));
  EXPECT_TRUE(IsGuestEndPhrase("i'm back"));

  // Within 6 words
  EXPECT_TRUE(IsGuestEndPhrase("okay, guest mode off"));
  EXPECT_TRUE(IsGuestEndPhrase("they left already"));
  EXPECT_TRUE(IsGuestEndPhrase("they're gone now"));
  EXPECT_TRUE(IsGuestEndPhrase("hey, i'm back now"));
}

TEST(SessionPhrasesTest, GuestEndPhrasesNegatives) {
  EXPECT_FALSE(IsGuestEndPhrase("they left all their belongings in the other meeting room"));
  EXPECT_FALSE(IsGuestEndPhrase("when i'm back from vacation next week we will talk"));
  EXPECT_FALSE(IsGuestEndPhrase("turn left at the corner"));
  EXPECT_FALSE(IsGuestEndPhrase(""));
}

}  // namespace
}  // namespace octoplus::voice
