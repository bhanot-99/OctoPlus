#include "octoplus/voice/turn_machine.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <random>
#include <vector>

namespace octoplus::voice {
namespace {

TEST(TurnMachineTest, HappyPathWithFollowUpUtterance) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.turn_in_progress());

  // 1. Wake word heard -> Capturing
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  EXPECT_TRUE(tm.turn_in_progress());
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kBeginUtterance,
                                 .with_backlog = false,
                                 .in_follow_up = false}));

  // 2. Speech starts -> stays capturing, no actions
  t += 100'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  EXPECT_TRUE(acts.empty());

  // 3. Speech ends -> Transcribing
  t += 2'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kTranscribing);
  EXPECT_TRUE(tm.turn_in_progress());
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kTranscribe}));

  // 4. Transcription complete -> AwaitingAnswer
  t += 300'000'000;
  acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t, .text_empty = false});
  EXPECT_EQ(tm.state(), TurnState::kAwaitingAnswer);
  EXPECT_TRUE(tm.turn_in_progress());
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kSendCommand,
                                 .with_backlog = false,
                                 .in_follow_up = false}));

  // 5. Reply begins -> Speaking
  t += 500'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(tm.speaking());
  EXPECT_TRUE(acts.empty());

  // 6. C5 stream finishes -> stays speaking
  t += 1'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(acts.empty());

  // 7. Speaker idle -> FollowUp (deadline t + 8s)
  t += 2'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(tm.follow_up_open());
  EXPECT_FALSE(tm.turn_in_progress());
  EXPECT_TRUE(acts.empty());

  // 8. User speaks during FollowUp -> Capturing with backlog and in_follow_up = true
  t += 500'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kBeginUtterance,
                                 .with_backlog = true,
                                 .in_follow_up = true}));

  // 9. Speech ends -> Transcribing
  t += 1'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kTranscribing);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kTranscribe}));

  // 10. Transcribed -> in_follow_up must be TRUE!
  t += 200'000'000;
  acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t, .text_empty = false});
  EXPECT_EQ(tm.state(), TurnState::kAwaitingAnswer);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kSendCommand,
                                 .with_backlog = false,
                                 .in_follow_up = true}));

  // Finish answer and speaking for the follow-up turn.
  // With max_consecutive_follow_ups = 1 (the default), this was the one allowed follow-up:
  // playback idle returns the machine to kIdle without opening a second follow-up window (BUG-088).
  t += 200'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
  t += 500'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});
  t += 1'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.follow_up_open());
  EXPECT_EQ(tm.consecutive_follow_ups(), 0U);
  EXPECT_TRUE(acts.empty());
}

TEST(TurnMachineTest, ConsecutiveFollowUpsAreCapped) {
  // max_consecutive_follow_ups = 2 allows two follow-ups before returning to Idle
  TurnConfig cfg;
  cfg.max_consecutive_follow_ups = 2;
  TurnMachine tm(cfg);
  std::uint64_t t = 1'000'000'000;

  // Turn 0: Wake word
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 100});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 1800});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 2500});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_EQ(tm.consecutive_follow_ups(), 0U);

  // Turn 1: First follow-up
  t += 3'000'000'000;
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_TRUE(acts[0].in_follow_up);
  EXPECT_EQ(tm.consecutive_follow_ups(), 1U);
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  acts = tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_TRUE(acts[0].in_follow_up);
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 1800});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 2500});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);  // Still allowed since 1 < 2

  // Turn 2: Second follow-up
  t += 4'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_TRUE(acts[0].in_follow_up);
  EXPECT_EQ(tm.consecutive_follow_ups(), 2U);
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 1800});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 2500});
  // Now consecutive_follow_ups >= 2: machine returns to Idle!
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.follow_up_open());
  EXPECT_EQ(tm.consecutive_follow_ups(), 0U);

  // Background speech in Idle does NOT start capture without wake word
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 3000});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_TRUE(acts.empty());
}

TEST(TurnMachineTest, MaxConsecutiveFollowUpsZeroDisablesFollowUp) {
  TurnConfig cfg;
  cfg.max_consecutive_follow_ups = 0;
  TurnMachine tm(cfg);
  const std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 100});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 1800});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 2500});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.follow_up_open());
}

TEST(TurnMachineTest, BargeInOrderInSpeaking) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 300});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);

  // User speaks over answer (barge-in)
  t += 1'000'000'000;
  const auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);

  // Exactly in this order: kFlushOutput, kCancelReply, kSendBargeIn,
  // kBeginUtterance(with_backlog=true)
  ASSERT_EQ(acts.size(), 4U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kFlushOutput}));
  EXPECT_EQ(acts[1], (TurnAction{.kind = TurnActionKind::kCancelReply}));
  EXPECT_EQ(acts[2], (TurnAction{.kind = TurnActionKind::kSendBargeIn}));
  EXPECT_EQ(acts[3], (TurnAction{.kind = TurnActionKind::kBeginUtterance, .with_backlog = true}));
}

TEST(TurnMachineTest, WakeWordDuringSpeaking) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 300});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);

  // Wake word heard during speaking
  t += 500'000'000;
  const auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  ASSERT_EQ(acts.size(), 4U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kFlushOutput}));
  EXPECT_EQ(acts[1], (TurnAction{.kind = TurnActionKind::kCancelReply}));
  EXPECT_EQ(acts[2], (TurnAction{.kind = TurnActionKind::kSendBargeIn}));
  EXPECT_EQ(acts[3], (TurnAction{.kind = TurnActionKind::kBeginUtterance, .with_backlog = true}));
}

TEST(TurnMachineTest, BargeInInAwaitingAnswer) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  ASSERT_EQ(tm.state(), TurnState::kAwaitingAnswer);

  // Speech starts before any reply audio
  t += 500'000'000;
  const auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  // The output is flushed too: a preamble or first sentence may already be queued (review F2).
  ASSERT_EQ(acts.size(), 4U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kFlushOutput}));
  EXPECT_EQ(acts[1], (TurnAction{.kind = TurnActionKind::kCancelReply}));
  EXPECT_EQ(acts[2], (TurnAction{.kind = TurnActionKind::kSendBargeIn}));
  EXPECT_EQ(acts[3], (TurnAction{.kind = TurnActionKind::kBeginUtterance, .with_backlog = true}));
}

TEST(TurnMachineTest, WakeWordDuringFollowUpIsNotFollowUp) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  // Jump to follow up via nudge
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t, .nudge = true});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 500});
  ASSERT_EQ(tm.state(), TurnState::kFollowUp);

  // Wake word in follow up -> with_backlog = false, not a follow up
  t += 1'000'000'000;
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kBeginUtterance, .with_backlog = false}));

  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  acts = tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_FALSE(acts[0].in_follow_up);
}

TEST(TurnMachineTest, NudgeSpokenFromIdleThenFollowUpReply) {
  TurnMachine tm;
  std::uint64_t t = 1'000'000'000;

  // Unprompted nudge from idle
  auto acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t, .nudge = true});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(acts.empty());

  // Nudge playback idle -> kFollowUp (without needing kReplyFinished)
  t += 1'500'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(acts.empty());

  // Another nudge during follow up
  t += 500'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t, .nudge = true});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(acts.empty());
}

TEST(TurnMachineTest, EmptyTranscriptDropsToIdle) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  ASSERT_EQ(tm.state(), TurnState::kTranscribing);

  const auto acts = tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = true});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_TRUE(acts.empty());
}

TEST(TurnMachineTest, NoSpeechTimeoutInCapturing) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  ASSERT_EQ(tm.state(), TurnState::kCapturing);

  // 4.9s later: still capturing
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t + 4'900'000'000});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  EXPECT_TRUE(acts.empty());

  // 5.0s later: timeout -> Idle, discard capture
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t + 5'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kDiscardCapture}));
}

TEST(TurnMachineTest, MaxUtteranceTimeoutInCapturing) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  // Speech started, so no-speech timeout won't fire
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 1'000'000'000});

  // At 30s: max utterance reached -> Transcribing
  const auto acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t + 30'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kTranscribing);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kTranscribe}));
}

TEST(TurnMachineTest, AnswerTimeoutInAwaitingAnswer) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  ASSERT_EQ(tm.state(), TurnState::kAwaitingAnswer);

  // 60s timeout in AwaitingAnswer -> cancel reply, back to idle
  const auto acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t + 200 + 60'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kCancelReply}));
}

TEST(TurnMachineTest, ReplyFinishedWithNothingSpoken) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  ASSERT_EQ(tm.state(), TurnState::kAwaitingAnswer);

  // Reply finished without reply started (nothing spoken) -> FollowUp
  const auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 500});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(acts.empty());
}

TEST(TurnMachineTest, PlaybackIdleBeforeReplyFinished) {
  TurnMachine tm;
  std::uint64_t const t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 100});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 300});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);

  // Playback idle arrives while more text may come
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 400});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(acts.empty());

  // Reply finished arrives
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 500});
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);

  // Playback idle arrives again -> now moves to follow up
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 600});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
}

TEST(TurnMachineTest, DeviceLostInEveryState) {
  // 1. In kIdle
  {
    TurnMachine tm;
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 100});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_TRUE(acts.empty());
  }
  // 2. In kCapturing
  {
    TurnMachine tm;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 100});
    ASSERT_EQ(tm.state(), TurnState::kCapturing);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 200});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kDiscardCapture}));
  }
  // 3. In kTranscribing
  {
    TurnMachine tm;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 100});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = 200});
    ASSERT_EQ(tm.state(), TurnState::kTranscribing);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 300});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_TRUE(acts.empty());
  }
  // 4. In kAwaitingAnswer
  {
    TurnMachine tm;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 100});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = 200});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = 300, .text_empty = false});
    ASSERT_EQ(tm.state(), TurnState::kAwaitingAnswer);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 400});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kCancelReply}));
  }
  // 5. In kSpeaking
  {
    TurnMachine tm;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = 100, .nudge = true});
    ASSERT_EQ(tm.state(), TurnState::kSpeaking);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 200});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    ASSERT_EQ(acts.size(), 2U);
    EXPECT_EQ(acts[0], (TurnAction{.kind = TurnActionKind::kFlushOutput}));
    EXPECT_EQ(acts[1], (TurnAction{.kind = TurnActionKind::kCancelReply}));
  }
  // 6. In kFollowUp
  {
    TurnMachine tm;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = 100, .nudge = true});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = 200});
    ASSERT_EQ(tm.state(), TurnState::kFollowUp);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 300});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_TRUE(acts.empty());
  }
}

TEST(TurnMachineTest, PropertyRandomEventsNeverCrashes) {
  TurnMachine tm;
  std::mt19937 rng(1337);
  std::uniform_int_distribution<int> kind_dist(0, 8);
  std::bernoulli_distribution bool_dist(0.5);
  std::uniform_int_distribution<std::uint64_t> step_ns_dist(0, 100'000'000);

  std::uint64_t now_ns = 1'000'000'000;

  for (int i = 0; i < 20000; ++i) {
    now_ns += step_ns_dist(rng);
    TurnEvent ev;
    ev.kind = static_cast<TurnEventKind>(kind_dist(rng));
    ev.now_ns = now_ns;
    ev.text_empty = bool_dist(rng);
    ev.nudge = bool_dist(rng);

    tm.OnEvent(ev);

    const TurnState s = tm.state();
    EXPECT_TRUE(s == TurnState::kIdle || s == TurnState::kCapturing ||
                s == TurnState::kTranscribing || s == TurnState::kAwaitingAnswer ||
                s == TurnState::kSpeaking || s == TurnState::kFollowUp);

    const bool in_progress = (s == TurnState::kCapturing || s == TurnState::kTranscribing ||
                              s == TurnState::kAwaitingAnswer || s == TurnState::kSpeaking);
    EXPECT_EQ(tm.turn_in_progress(), in_progress);
    EXPECT_EQ(tm.follow_up_open(), s == TurnState::kFollowUp);
    EXPECT_EQ(tm.speaking(), s == TurnState::kSpeaking);
  }
}

}  // namespace
}  // namespace octoplus::voice

namespace octoplus::voice {
namespace {

TEST(TurnMachineWakeOnly, AWakePhraseAloneKeepsListeningForTheCommand) {
  TurnMachine m;
  TurnEvent wake;
  wake.kind = TurnEventKind::kWakeWord;
  wake.now_ns = 0;
  m.OnEvent(wake);
  TurnEvent end;
  end.kind = TurnEventKind::kSpeechEnded;
  end.now_ns = 1'000'000'000;
  m.OnEvent(end);
  ASSERT_EQ(m.state(), TurnState::kTranscribing);
  TurnEvent only;
  only.kind = TurnEventKind::kWakeOnly;
  only.now_ns = 1'100'000'000;
  const auto actions = m.OnEvent(only);
  EXPECT_EQ(m.state(), TurnState::kCapturing);
  ASSERT_EQ(actions.size(), 1U);
  EXPECT_EQ(actions[0].kind, TurnActionKind::kBeginUtterance);
  EXPECT_FALSE(actions[0].with_backlog);
  // and the no-speech timeout still returns it to idle if nobody speaks
  TurnEvent tick;
  tick.kind = TurnEventKind::kTick;
  tick.now_ns = 1'100'000'000 + 5'000'000'000;
  m.OnEvent(tick);
  EXPECT_EQ(m.state(), TurnState::kIdle);
}

// The mic button's cancel ends the turn from every busy state, like a lost device.
TEST(TurnMachineTest, UserCancelEndsTheTurnFromEveryBusyState) {
  const auto cancel = [](TurnMachine& m, std::uint64_t t) {
    return m.OnEvent(TurnEvent{.kind = TurnEventKind::kUserCancel, .now_ns = t});
  };
  {
    TurnMachine m;
    m.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 1});
    ASSERT_EQ(m.state(), TurnState::kCapturing);
    cancel(m, 2);
    EXPECT_EQ(m.state(), TurnState::kIdle);
  }
  {
    TurnMachine m;
    m.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 1});
    m.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = 2});
    ASSERT_EQ(m.state(), TurnState::kTranscribing);
    cancel(m, 3);
    EXPECT_EQ(m.state(), TurnState::kIdle);
  }
  {
    TurnMachine m;
    m.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = 1, .nudge = true});
    ASSERT_EQ(m.state(), TurnState::kSpeaking);
    const auto acts = cancel(m, 2);
    EXPECT_EQ(m.state(), TurnState::kIdle);
    ASSERT_EQ(acts.size(), 2U);
    EXPECT_EQ(acts[0].kind, TurnActionKind::kFlushOutput);
    EXPECT_EQ(acts[1].kind, TurnActionKind::kCancelReply);
  }
  {
    TurnMachine m;
    EXPECT_TRUE(cancel(m, 1).empty()) << "nothing to cancel when idle";
    EXPECT_EQ(m.state(), TurnState::kIdle);
  }
}

TEST(TurnMachineTest, SessionsDisabledIgnoresSessionEvents) {
  TurnConfig cfg;
  cfg.sessions_enabled = false;
  TurnMachine tm(cfg);

  EXPECT_FALSE(tm.session_active());
  EXPECT_EQ(tm.session_started_ns(), 0U);
  EXPECT_EQ(tm.session_ns_left(1'000'000'000), 0U);

  // kSessionBegin ignored
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = 1'000'000'000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());

  // kSessionEnd ignored
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEnd, .now_ns = 2'000'000'000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kIdle);

  // kSessionEndAfterReply ignored
  acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEndAfterReply, .now_ns = 3'000'000'000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kIdle);

  // Old behavior works untouched
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kWakeWord, .now_ns = 4'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kBeginUtterance);
}

TEST(TurnMachineTest, SessionSixConsecutiveFollowUpsWithoutWakeWord) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  std::uint64_t t = 1'000'000'000;

  // Start session from idle
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(tm.session_active());
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionStarted);

  // 6 consecutive follow-up turns without wake word
  for (std::uint32_t i = 1; i <= 6; ++i) {
    t += 2'000'000'000;
    acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
    EXPECT_EQ(tm.state(), TurnState::kCapturing);
    EXPECT_EQ(tm.consecutive_follow_ups(), i);
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_TRUE(acts[0].in_follow_up);

    t += 1'000'000'000;
    acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t});
    EXPECT_EQ(tm.state(), TurnState::kTranscribing);
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_EQ(acts[0].kind, TurnActionKind::kTranscribe);

    t += 300'000'000;
    acts = tm.OnEvent(
        TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t, .text_empty = false});
    EXPECT_EQ(tm.state(), TurnState::kAwaitingAnswer);
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_TRUE(acts[0].in_follow_up);

    t += 400'000'000;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
    EXPECT_EQ(tm.state(), TurnState::kSpeaking);

    t += 1'000'000'000;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});

    t += 500'000'000;
    acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
    EXPECT_EQ(tm.state(), TurnState::kFollowUp);
    EXPECT_TRUE(tm.session_active());
    EXPECT_TRUE(acts.empty());
  }

  EXPECT_EQ(tm.consecutive_follow_ups(), 6U);
  EXPECT_TRUE(tm.session_active());
}

TEST(TurnMachineTest, SessionSilenceTriggersStillThereThenEnds) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  std::uint64_t const t0 = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t0});
  ASSERT_EQ(tm.state(), TurnState::kFollowUp);

  // Before 90s: nothing happens
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t0 + 89'999'999'999});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // At 90s silence: exactly one kSayStillThere
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t0 + 90'000'000'000});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSayStillThere);
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // Repeated ticks during grace period emit nothing more
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t0 + 95'000'000'000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // 20s later (at 110s total): grace period expired -> kSessionEnded and Idle
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t0 + 110'000'000'000});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
}

TEST(TurnMachineTest, SessionSpeechAfterStillThereResetsSilence) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  ASSERT_EQ(tm.state(), TurnState::kFollowUp);

  // 90s silence triggers still there
  t += 90'000'000'000;
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSayStillThere);

  // User speaks 5s into grace period -> resets still_there flag and continues session
  t += 5'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);

  t += 1'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t});
  t += 200'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t, .text_empty = false});
  t += 300'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
  t += 1'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});
  t += 500'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(tm.session_active());

  // A later 90s silence asks kSayStillThere again
  t += 90'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSayStillThere);

  // Grace expires -> session ends
  t += 20'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
}

TEST(TurnMachineTest, SessionMaxDurationTriggersKeepGoingAndCanContinue) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  ASSERT_EQ(tm.state(), TurnState::kFollowUp);

  // Turn at 500s
  t += 500'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1'000'000'000});
  tm.OnEvent(TurnEvent{
      .kind = TurnEventKind::kTranscribed, .now_ns = t + 1'200'000'000, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1'400'000'000});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 2'000'000'000});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 3'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // At 15 minutes (900s from session begin): max session length reached
  t = 1'000'000'000 + 900'000'000'000;
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSayKeepGoing);

  // Subsequent ticks before grace expires emit nothing (asked once)
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t + 5'000'000'000});
  EXPECT_TRUE(acts.empty());

  // User speaks to continue -> new 15 minute window starts
  t += 10'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  EXPECT_EQ(tm.session_started_ns(), t);

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1'000'000'000});
  tm.OnEvent(TurnEvent{
      .kind = TurnEventKind::kTranscribed, .now_ns = t + 1'200'000'000, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1'400'000'000});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 2'000'000'000});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 3'000'000'000});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // 15 minutes after new start: kSayKeepGoing asked again
  t += 900'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSayKeepGoing);

  // 20s grace expires without speech -> session ends
  t += 20'000'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
}

TEST(TurnMachineTest, SessionEndWhileSpeakingFlushesAndEnds) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  const std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 100});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);
  ASSERT_TRUE(tm.speaking());

  // kSessionEnd while speaking flushes output, cancels reply, and emits session ended
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEnd, .now_ns = t + 2000});
  ASSERT_EQ(acts.size(), 3U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kFlushOutput);
  EXPECT_EQ(acts[1].kind, TurnActionKind::kCancelReply);
  EXPECT_EQ(acts[2].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
}

TEST(TurnMachineTest, SessionEndAfterReplyWaitsForPlaybackIdle) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  const std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 100});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);

  // kSessionEndAfterReply while speaking: no immediate action, remains in session
  auto acts =
      tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEndAfterReply, .now_ns = t + 2000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);
  EXPECT_TRUE(tm.session_active());

  // Reply finishes, still playing
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t + 3000});
  EXPECT_TRUE(acts.empty());
  EXPECT_EQ(tm.state(), TurnState::kSpeaking);

  // Playback becomes idle -> session ends instead of follow-up
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t + 4000});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());

  // kSessionEndAfterReply when no turn in progress ends at once
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t + 5000});
  ASSERT_EQ(tm.state(), TurnState::kFollowUp);
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEndAfterReply, .now_ns = t + 6000});
  ASSERT_EQ(acts.size(), 1U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
}

TEST(TurnMachineTest, SessionDeviceLostEndsSession) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;

  // In kFollowUp
  {
    TurnMachine tm(cfg);
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = 100});
    ASSERT_EQ(tm.state(), TurnState::kFollowUp);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 200});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_FALSE(tm.session_active());
    ASSERT_EQ(acts.size(), 1U);
    EXPECT_EQ(acts[0].kind, TurnActionKind::kSessionEnded);
  }

  // In kSpeaking
  {
    TurnMachine tm(cfg);
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = 100});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = 200});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = 300});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = 400, .text_empty = false});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = 500});
    ASSERT_EQ(tm.state(), TurnState::kSpeaking);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kDeviceLost, .now_ns = 600});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_FALSE(tm.session_active());
    ASSERT_EQ(acts.size(), 3U);
    EXPECT_EQ(acts[0].kind, TurnActionKind::kFlushOutput);
    EXPECT_EQ(acts[1].kind, TurnActionKind::kCancelReply);
    EXPECT_EQ(acts[2].kind, TurnActionKind::kSessionEnded);
  }

  // In kCapturing with kUserCancel
  {
    TurnMachine tm(cfg);
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = 100});
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = 200});
    ASSERT_EQ(tm.state(), TurnState::kCapturing);
    auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kUserCancel, .now_ns = 300});
    EXPECT_EQ(tm.state(), TurnState::kIdle);
    EXPECT_FALSE(tm.session_active());
    ASSERT_EQ(acts.size(), 2U);
    EXPECT_EQ(acts[0].kind, TurnActionKind::kDiscardCapture);
    EXPECT_EQ(acts[1].kind, TurnActionKind::kSessionEnded);
  }
}

TEST(TurnMachineTest, SessionBargeInDuringSpeakingKeepsSession) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);
  std::uint64_t t = 1'000'000'000;

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t + 100});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t + 1000});
  tm.OnEvent(
      TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t + 1200, .text_empty = false});
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t + 1400});
  ASSERT_EQ(tm.state(), TurnState::kSpeaking);

  // Barge-in: user speaks while assistant is speaking
  t += 2'000'000'000;
  auto acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kCapturing);
  EXPECT_TRUE(tm.session_active());
  ASSERT_EQ(acts.size(), 4U);
  EXPECT_EQ(acts[0].kind, TurnActionKind::kFlushOutput);
  EXPECT_EQ(acts[1].kind, TurnActionKind::kCancelReply);
  EXPECT_EQ(acts[2].kind, TurnActionKind::kSendBargeIn);
  EXPECT_EQ(acts[3].kind, TurnActionKind::kBeginUtterance);
  EXPECT_TRUE(acts[3].with_backlog);

  // Complete the barge-in turn
  t += 1'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = t});
  t += 200'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = t, .text_empty = false});
  t += 300'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
  t += 1'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});
  t += 500'000'000;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);
  EXPECT_TRUE(tm.session_active());
}

TEST(TurnMachineTest, SessionAccessorsAndNsLeft) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);

  EXPECT_FALSE(tm.session_active());
  EXPECT_EQ(tm.session_started_ns(), 0U);
  EXPECT_EQ(tm.session_ns_left(1'000'000'000), 0U);

  std::uint64_t const t0 = 100'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = t0});
  EXPECT_TRUE(tm.session_active());
  EXPECT_EQ(tm.session_started_ns(), t0);
  EXPECT_EQ(tm.session_ns_left(t0), 900'000'000'000U);
  EXPECT_EQ(tm.session_ns_left(t0 + 100'000'000'000), 800'000'000'000U);
  EXPECT_EQ(tm.session_ns_left(t0 + 900'000'000'000), 0U);
  EXPECT_EQ(tm.session_ns_left(t0 + 1'000'000'000'000), 0U);

  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionEnd, .now_ns = t0 + 50'000'000'000});
  EXPECT_FALSE(tm.session_active());
  EXPECT_EQ(tm.session_started_ns(), 0U);
  EXPECT_EQ(tm.session_ns_left(t0 + 50'000'000'000), 0U);
}

TEST(TurnMachineTest, SessionThreeHourSimulatedLoopNeverStalls) {
  TurnConfig cfg;
  cfg.sessions_enabled = true;
  TurnMachine tm(cfg);

  std::mt19937 rng(42);
  std::uniform_int_distribution<int> interval_dist(20, 180);  // speech burst every 20-180 seconds

  std::uint64_t now_ns = 1'000'000'000ULL;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = now_ns});
  std::uint64_t last_speech_ns = now_ns;
  int next_burst_sec = interval_dist(rng);

  constexpr int kThreeHoursSec = 3 * 3600;
  for (int sec = 0; sec < kThreeHoursSec; ++sec) {
    now_ns += 1'000'000'000ULL;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = now_ns});

    --next_burst_sec;
    if (next_burst_sec <= 0) {
      if (tm.state() == TurnState::kIdle) {
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSessionBegin, .now_ns = now_ns});
      }
      if (tm.state() == TurnState::kFollowUp) {
        now_ns += 50'000'000ULL;
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechStarted, .now_ns = now_ns});
        now_ns += 1'500'000'000ULL;
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kSpeechEnded, .now_ns = now_ns});
        now_ns += 200'000'000ULL;
        tm.OnEvent(
            TurnEvent{.kind = TurnEventKind::kTranscribed, .now_ns = now_ns, .text_empty = false});
        now_ns += 300'000'000ULL;
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = now_ns});
        now_ns += 1'000'000'000ULL;
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = now_ns});
        now_ns += 500'000'000ULL;
        tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = now_ns});
        last_speech_ns = now_ns;
      }
      next_burst_sec = interval_dist(rng);
    }

    if (now_ns >= last_speech_ns && (now_ns - last_speech_ns) >= 20ULL * 60ULL * 1'000'000'000ULL) {
      EXPECT_EQ(tm.state(), TurnState::kIdle);
      EXPECT_FALSE(tm.session_active());
    }
  }

  // After 3 hours, tick for 25 minutes of silence: must return to Idle
  for (int sec = 0; sec < 25 * 60; ++sec) {
    now_ns += 1'000'000'000ULL;
    tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = now_ns});
  }
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.session_active());
  EXPECT_EQ(tm.session_ns_left(now_ns), 0U);
}

}  // namespace
}  // namespace octoplus::voice
