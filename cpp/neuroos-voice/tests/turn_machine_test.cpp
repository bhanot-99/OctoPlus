#include "neuroos/voice/turn_machine.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <random>
#include <vector>

namespace neuroos::voice {
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
                                 .in_follow_up = false}));

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

  // Finish answer and speaking
  t += 200'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyStarted, .now_ns = t});
  t += 500'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kReplyFinished, .now_ns = t});
  t += 1'000'000'000;
  tm.OnEvent(TurnEvent{.kind = TurnEventKind::kPlaybackIdle, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kFollowUp);

  // 11. FollowUp expires on Tick -> Idle
  t += 8'000'000'001;
  acts = tm.OnEvent(TurnEvent{.kind = TurnEventKind::kTick, .now_ns = t});
  EXPECT_EQ(tm.state(), TurnState::kIdle);
  EXPECT_FALSE(tm.follow_up_open());
  EXPECT_TRUE(acts.empty());
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
}  // namespace neuroos::voice

namespace neuroos::voice {
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

}  // namespace
}  // namespace neuroos::voice
