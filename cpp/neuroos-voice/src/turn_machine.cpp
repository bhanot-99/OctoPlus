#include "neuroos/voice/turn_machine.hpp"

#include <cstdint>
#include <vector>

namespace neuroos::voice {

TurnMachine::TurnMachine(TurnConfig cfg) : cfg_(cfg) {}

// Branchy by nature (cognitive complexity): the conversation state machine: state by event.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
std::vector<TurnAction> TurnMachine::OnEvent(const TurnEvent& e) {
  switch (state_) {
    case TurnState::kIdle: {
      if (e.kind == TurnEventKind::kWakeWord) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = false;
        utterance_is_follow_up_ = false;
        consecutive_follow_ups_ = 0;
        TurnAction act;
        act.kind = TurnActionKind::kBeginUtterance;
        act.with_backlog = false;
        act.in_follow_up = false;
        return {act};
      }
      if (e.kind == TurnEventKind::kReplyStarted && e.nudge) {
        state_ = TurnState::kSpeaking;
        state_entered_ns_ = e.now_ns;
        reply_is_nudge_ = true;
        reply_finished_ = false;
        consecutive_follow_ups_ = 0;
        return {};
      }
      return {};
    }

    case TurnState::kCapturing: {
      if (e.kind == TurnEventKind::kSpeechEnded) {
        state_ = TurnState::kTranscribing;
        state_entered_ns_ = e.now_ns;
        TurnAction act;
        act.kind = TurnActionKind::kTranscribe;
        act.with_backlog = false;
        act.in_follow_up = false;
        return {act};
      }
      if (e.kind == TurnEventKind::kTick) {
        if (!speech_started_in_capture_ && e.now_ns >= state_entered_ns_ &&
            (e.now_ns - state_entered_ns_) >= cfg_.no_speech_timeout_ns) {
          state_ = TurnState::kIdle;
          state_entered_ns_ = e.now_ns;
          consecutive_follow_ups_ = 0;
          TurnAction act;
          act.kind = TurnActionKind::kDiscardCapture;
          act.with_backlog = false;
          act.in_follow_up = false;
          return {act};
        }
        if (e.now_ns >= state_entered_ns_ &&
            (e.now_ns - state_entered_ns_) >= cfg_.max_utterance_ns) {
          state_ = TurnState::kTranscribing;
          state_entered_ns_ = e.now_ns;
          TurnAction act;
          act.kind = TurnActionKind::kTranscribe;
          act.with_backlog = false;
          act.in_follow_up = false;
          return {act};
        }
        return {};
      }
      if (e.kind == TurnEventKind::kSpeechStarted) {
        speech_started_in_capture_ = true;
        return {};
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        TurnAction act;
        act.kind = TurnActionKind::kDiscardCapture;
        act.with_backlog = false;
        act.in_follow_up = false;
        return {act};
      }
      return {};
    }

    case TurnState::kTranscribing: {
      // "Hey Jarvis ..." and then a pause before the command: listen again, no new wake word.
      if (e.kind == TurnEventKind::kWakeOnly) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = false;
        utterance_is_follow_up_ = false;
        consecutive_follow_ups_ = 0;
        TurnAction act;
        act.kind = TurnActionKind::kBeginUtterance;
        act.with_backlog = false;
        act.in_follow_up = false;
        return {act};
      }
      if (e.kind == TurnEventKind::kTranscribed) {
        if (!e.text_empty) {
          state_ = TurnState::kAwaitingAnswer;
          state_entered_ns_ = e.now_ns;
          reply_is_nudge_ = false;
          reply_finished_ = false;
          TurnAction act;
          act.kind = TurnActionKind::kSendCommand;
          act.with_backlog = false;
          act.in_follow_up = utterance_is_follow_up_;
          return {act};
        }
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return {};
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return {};
      }
      return {};
    }

    case TurnState::kAwaitingAnswer: {
      if (e.kind == TurnEventKind::kReplyStarted) {
        state_ = TurnState::kSpeaking;
        state_entered_ns_ = e.now_ns;
        reply_is_nudge_ = e.nudge;
        reply_finished_ = false;
        return {};
      }
      if (e.kind == TurnEventKind::kReplyFinished) {
        if (consecutive_follow_ups_ < cfg_.max_consecutive_follow_ups) {
          state_ = TurnState::kFollowUp;
          state_entered_ns_ = e.now_ns;
          follow_up_deadline_ns_ = e.now_ns + cfg_.follow_up_ns;
          return {};
        }
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return {};
      }
      if (e.kind == TurnEventKind::kTick) {
        if (e.now_ns >= state_entered_ns_ &&
            (e.now_ns - state_entered_ns_) >= cfg_.answer_timeout_ns) {
          state_ = TurnState::kIdle;
          state_entered_ns_ = e.now_ns;
          consecutive_follow_ups_ = 0;
          TurnAction act;
          act.kind = TurnActionKind::kCancelReply;
          act.with_backlog = false;
          act.in_follow_up = false;
          return {act};
        }
        return {};
      }
      if (e.kind == TurnEventKind::kSpeechStarted) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = true;
        utterance_is_follow_up_ = false;
        consecutive_follow_ups_ = 0;
        // A preamble or a first sentence may already be queued with the current epoch: flush
        // so that nothing of the old reply is spoken over the user (the review's F2).
        return {
            TurnAction{
                .kind = TurnActionKind::kFlushOutput, .with_backlog = false, .in_follow_up = false},
            TurnAction{
                .kind = TurnActionKind::kCancelReply, .with_backlog = false, .in_follow_up = false},
            TurnAction{
                .kind = TurnActionKind::kSendBargeIn, .with_backlog = false, .in_follow_up = false},
            TurnAction{.kind = TurnActionKind::kBeginUtterance,
                       .with_backlog = true,
                       .in_follow_up = false},
        };
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        TurnAction act;
        act.kind = TurnActionKind::kCancelReply;
        act.with_backlog = false;
        act.in_follow_up = false;
        return {act};
      }
      return {};
    }

    case TurnState::kSpeaking: {
      if (e.kind == TurnEventKind::kSpeechStarted || e.kind == TurnEventKind::kWakeWord) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = (e.kind == TurnEventKind::kSpeechStarted);
        utterance_is_follow_up_ = false;
        consecutive_follow_ups_ = 0;
        return {
            TurnAction{
                .kind = TurnActionKind::kFlushOutput, .with_backlog = false, .in_follow_up = false},
            TurnAction{
                .kind = TurnActionKind::kCancelReply, .with_backlog = false, .in_follow_up = false},
            TurnAction{
                .kind = TurnActionKind::kSendBargeIn, .with_backlog = false, .in_follow_up = false},
            TurnAction{.kind = TurnActionKind::kBeginUtterance,
                       .with_backlog = true,
                       .in_follow_up = false},
        };
      }
      if (e.kind == TurnEventKind::kReplyFinished) {
        reply_finished_ = true;
        return {};
      }
      if (e.kind == TurnEventKind::kPlaybackIdle) {
        if (reply_finished_ || reply_is_nudge_) {
          if (consecutive_follow_ups_ < cfg_.max_consecutive_follow_ups) {
            state_ = TurnState::kFollowUp;
            state_entered_ns_ = e.now_ns;
            follow_up_deadline_ns_ = e.now_ns + cfg_.follow_up_ns;
            return {};
          }
          state_ = TurnState::kIdle;
          state_entered_ns_ = e.now_ns;
          consecutive_follow_ups_ = 0;
          return {};
        }
        return {};
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return {
            TurnAction{
                .kind = TurnActionKind::kFlushOutput, .with_backlog = false, .in_follow_up = false},
            TurnAction{
                .kind = TurnActionKind::kCancelReply, .with_backlog = false, .in_follow_up = false},
        };
      }
      return {};
    }

    case TurnState::kFollowUp: {
      if (e.kind == TurnEventKind::kSpeechStarted) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = true;
        utterance_is_follow_up_ = true;
        ++consecutive_follow_ups_;
        return {
            TurnAction{.kind = TurnActionKind::kBeginUtterance,
                       .with_backlog = true,
                       .in_follow_up = true},
        };
      }
      if (e.kind == TurnEventKind::kWakeWord) {
        state_ = TurnState::kCapturing;
        state_entered_ns_ = e.now_ns;
        speech_started_in_capture_ = false;
        utterance_is_follow_up_ = false;
        consecutive_follow_ups_ = 0;
        return {
            TurnAction{.kind = TurnActionKind::kBeginUtterance,
                       .with_backlog = false,
                       .in_follow_up = false},
        };
      }
      if (e.kind == TurnEventKind::kTick) {
        if (e.now_ns >= follow_up_deadline_ns_) {
          state_ = TurnState::kIdle;
          state_entered_ns_ = e.now_ns;
          consecutive_follow_ups_ = 0;
        }
        return {};
      }
      if (e.kind == TurnEventKind::kReplyStarted && e.nudge) {
        state_ = TurnState::kSpeaking;
        state_entered_ns_ = e.now_ns;
        reply_is_nudge_ = true;
        reply_finished_ = false;
        consecutive_follow_ups_ = 0;
        return {};
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return {};
      }
      return {};
    }
  }

  return {};
}

TurnState TurnMachine::state() const { return state_; }

bool TurnMachine::turn_in_progress() const {
  return state_ == TurnState::kCapturing || state_ == TurnState::kTranscribing ||
         state_ == TurnState::kAwaitingAnswer || state_ == TurnState::kSpeaking;
}

bool TurnMachine::follow_up_open() const { return state_ == TurnState::kFollowUp; }

bool TurnMachine::speaking() const { return state_ == TurnState::kSpeaking; }

std::uint32_t TurnMachine::consecutive_follow_ups() const { return consecutive_follow_ups_; }

}  // namespace neuroos::voice
