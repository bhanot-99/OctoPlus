#include "octoplus/voice/turn_machine.hpp"

#include <cstdint>
#include <vector>

namespace octoplus::voice {

TurnMachine::TurnMachine(TurnConfig cfg) : cfg_(cfg) {}

void TurnMachine::ResetSession() {
  session_active_ = false;
  session_started_ns_ = 0;
  still_there_asked_ = false;
  keep_going_asked_ = false;
  end_after_reply_ = false;
  quiet_first_ = false;
  quiet_listen_ns_ = 0;
}

std::uint64_t TurnMachine::SessionDeadline(std::uint64_t now_ns) const {
  return now_ns + (quiet_first_ ? quiet_listen_ns_ : cfg_.session_silence_ns);
}

void TurnMachine::OnSpeechOrWakeHeard(const TurnEvent& e) {
  if (session_active_) {
    quiet_first_ = false;
    if (keep_going_asked_) {
      session_started_ns_ = e.now_ns;
    }
    still_there_asked_ = false;
    keep_going_asked_ = false;
  }
}

std::vector<TurnAction> TurnMachine::HandleSessionBegin(const TurnEvent& e) {
  if (!cfg_.sessions_enabled || session_active_) {
    return {};
  }
  session_active_ = true;
  session_started_ns_ = e.now_ns;
  still_there_asked_ = false;
  keep_going_asked_ = false;
  end_after_reply_ = false;
  if (state_ == TurnState::kIdle) {
    state_ = TurnState::kFollowUp;
    state_entered_ns_ = e.now_ns;
    follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
  }
  return {TurnAction{.kind = TurnActionKind::kSessionStarted}};
}

std::vector<TurnAction> TurnMachine::HandleCompanionLine(const TurnEvent& e) {
  if (!cfg_.sessions_enabled || session_active_ || turn_in_progress() ||
      state_ != TurnState::kIdle) {
    return {};
  }
  session_active_ = true;
  session_started_ns_ = e.now_ns;
  still_there_asked_ = false;
  keep_going_asked_ = false;
  end_after_reply_ = false;
  quiet_first_ = true;
  quiet_listen_ns_ = e.listen_ns;
  return {};  // no start sound: the line itself is the cue
}

std::vector<TurnAction> TurnMachine::HandleSessionEnd(const TurnEvent& e) {
  if (!session_active_) {
    return {};
  }
  std::vector<TurnAction> acts;
  acts.reserve(3);  // also keeps GCC 13's -O3 stringop-overflow false alarm quiet
  if (speaking()) {
    acts.push_back(TurnAction{.kind = TurnActionKind::kFlushOutput});
    acts.push_back(TurnAction{.kind = TurnActionKind::kCancelReply});
  }
  acts.push_back(TurnAction{.kind = TurnActionKind::kSessionEnded});
  state_ = TurnState::kIdle;
  state_entered_ns_ = e.now_ns;
  consecutive_follow_ups_ = 0;
  ResetSession();
  return acts;
}

std::vector<TurnAction> TurnMachine::HandleSessionEndAfterReply(const TurnEvent& e) {
  if (!session_active_) {
    return {};
  }
  if (!turn_in_progress()) {
    return HandleSessionEnd(e);
  }
  end_after_reply_ = true;
  return {};
}

std::vector<TurnAction> TurnMachine::HandleFollowUpTick(const TurnEvent& e) {
  if (session_active_) {
    if (e.now_ns >= session_started_ns_ &&
        (e.now_ns - session_started_ns_) >= cfg_.session_max_ns && !keep_going_asked_) {
      keep_going_asked_ = true;
      follow_up_deadline_ns_ = e.now_ns + cfg_.session_grace_ns;
      return {TurnAction{.kind = TurnActionKind::kSayKeepGoing}};
    }
    if (e.now_ns >= follow_up_deadline_ns_) {
      if (quiet_first_) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        ResetSession();
        return {TurnAction{.kind = TurnActionKind::kSessionEndedQuiet}};
      }
      if (!still_there_asked_ && !keep_going_asked_) {
        still_there_asked_ = true;
        follow_up_deadline_ns_ = e.now_ns + cfg_.session_grace_ns;
        return {TurnAction{.kind = TurnActionKind::kSayStillThere}};
      }
      state_ = TurnState::kIdle;
      state_entered_ns_ = e.now_ns;
      consecutive_follow_ups_ = 0;
      ResetSession();
      return {TurnAction{.kind = TurnActionKind::kSessionEnded}};
    }
    return {};
  }
  if (e.now_ns >= follow_up_deadline_ns_) {
    state_ = TurnState::kIdle;
    state_entered_ns_ = e.now_ns;
    consecutive_follow_ups_ = 0;
  }
  return {};
}

std::vector<TurnAction> TurnMachine::EndSessionOnDeviceLost(std::vector<TurnAction> acts) {
  if (session_active_) {
    acts.reserve(acts.size() + 1);
    acts.push_back(TurnAction{.kind = TurnActionKind::kSessionEnded});
    ResetSession();
  }
  return acts;
}

// Branchy by nature (cognitive complexity): the conversation state machine: state by event.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
std::vector<TurnAction> TurnMachine::OnEvent(const TurnEvent& event) {
  // A cancel by the user ends the turn the same way a lost device does: discard, flush, cancel.
  TurnEvent e = event;
  if (e.kind == TurnEventKind::kUserCancel) {
    e.kind = TurnEventKind::kDeviceLost;
  }

  if (e.kind == TurnEventKind::kSessionBegin) {
    return HandleSessionBegin(e);
  }
  if (e.kind == TurnEventKind::kCompanionLine) {
    return HandleCompanionLine(e);
  }
  if (e.kind == TurnEventKind::kSessionEnd) {
    return HandleSessionEnd(e);
  }
  if (e.kind == TurnEventKind::kSessionEndAfterReply) {
    return HandleSessionEndAfterReply(e);
  }

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
          if (session_active_) {
            state_ = TurnState::kFollowUp;
            state_entered_ns_ = e.now_ns;
            follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
            return {TurnAction{.kind = TurnActionKind::kDiscardCapture}};
          }
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
        OnSpeechOrWakeHeard(e);
        return {};
      }
      if (e.kind == TurnEventKind::kDeviceLost) {
        state_ = TurnState::kIdle;
        state_entered_ns_ = e.now_ns;
        consecutive_follow_ups_ = 0;
        return EndSessionOnDeviceLost({TurnAction{.kind = TurnActionKind::kDiscardCapture}});
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
        if (session_active_) {
          state_ = TurnState::kFollowUp;
          state_entered_ns_ = e.now_ns;
          follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
          return {};
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
        return EndSessionOnDeviceLost({});
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
        if (session_active_) {
          if (end_after_reply_) {
            state_ = TurnState::kIdle;
            state_entered_ns_ = e.now_ns;
            consecutive_follow_ups_ = 0;
            ResetSession();
            return {TurnAction{.kind = TurnActionKind::kSessionEnded}};
          }
          state_ = TurnState::kFollowUp;
          state_entered_ns_ = e.now_ns;
          follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
          return {};
        }
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
          if (session_active_) {
            state_ = TurnState::kFollowUp;
            state_entered_ns_ = e.now_ns;
            follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
            return {TurnAction{.kind = TurnActionKind::kCancelReply}};
          }
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
        OnSpeechOrWakeHeard(e);
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
        return EndSessionOnDeviceLost({TurnAction{.kind = TurnActionKind::kCancelReply}});
      }
      return {};
    }

    case TurnState::kSpeaking: {
      if (e.kind == TurnEventKind::kSpeechStarted || e.kind == TurnEventKind::kWakeWord) {
        OnSpeechOrWakeHeard(e);
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
          if (session_active_) {
            if (end_after_reply_) {
              state_ = TurnState::kIdle;
              state_entered_ns_ = e.now_ns;
              consecutive_follow_ups_ = 0;
              ResetSession();
              return {TurnAction{.kind = TurnActionKind::kSessionEnded}};
            }
            state_ = TurnState::kFollowUp;
            state_entered_ns_ = e.now_ns;
            follow_up_deadline_ns_ = SessionDeadline(e.now_ns);
            return {};
          }
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
        return EndSessionOnDeviceLost({
            TurnAction{.kind = TurnActionKind::kFlushOutput},
            TurnAction{.kind = TurnActionKind::kCancelReply},
        });
      }
      return {};
    }

    case TurnState::kFollowUp: {
      if (e.kind == TurnEventKind::kSpeechStarted) {
        OnSpeechOrWakeHeard(e);
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
        OnSpeechOrWakeHeard(e);
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
        return HandleFollowUpTick(e);
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
        return EndSessionOnDeviceLost({});
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

bool TurnMachine::session_active() const { return session_active_; }

std::uint64_t TurnMachine::session_started_ns() const { return session_started_ns_; }

std::uint64_t TurnMachine::session_ns_left(std::uint64_t now_ns) const {
  if (!session_active_ || now_ns < session_started_ns_) {
    return 0;
  }
  const std::uint64_t elapsed = now_ns - session_started_ns_;
  return (elapsed >= cfg_.session_max_ns) ? 0 : (cfg_.session_max_ns - elapsed);
}

}  // namespace octoplus::voice
