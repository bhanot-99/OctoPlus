//! The mic button's logic (ADR-0017, `docs/design/octopit-ux.md` §3): what C2's status means for
//! the screen, and what a press on the button does. Pure; no audio ever passes through OctoPit.

use octoplus_proto::v1::{TurnPhase, VoiceStatus};

/// What the voice dock shows. Every state has a text label (`VoiceView::label`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// The voice part cannot be reached at all.
    Unavailable,
    /// The voice part answers but is not capturing the microphone.
    Off,
    /// Waiting for the wake phrase or a press.
    Idle,
    /// Recording a command.
    Listening,
    /// Turning speech into text, or waiting for the answer.
    Thinking,
    /// Speaking an answer or a nudge.
    Speaking,
    /// The follow-up window is open: no wake phrase needed for a moment.
    FollowUp,
    /// Another application holds the microphone.
    Busy,
}

/// What a press on the mic button asks C2 to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAction {
    /// Start listening now.
    Start,
    /// Stop listening and answer what was said.
    Stop,
    /// Drop the turn: stop speaking or thinking.
    Cancel,
    /// The button does nothing in this state.
    None,
}

/// A voice status ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceView {
    /// The state.
    pub state: VoiceState,
    /// Microphone level, `0.0..=1.0`.
    pub level: f32,
    /// Wake phrases heard since the voice part started.
    pub wake_words: u64,
    /// An open-microphone chat is running (Phase 12).
    pub session_active: bool,
    /// Seconds until that chat reaches its longest length (0 when none).
    pub session_seconds_left: u32,
    /// Seconds of guest mode left (0 when it is off).
    pub guest_seconds_left: u32,
}

impl VoiceView {
    /// The view for a voice part that cannot be reached.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            state: VoiceState::Unavailable,
            level: 0.0,
            wake_words: 0,
            session_active: false,
            session_seconds_left: 0,
            guest_seconds_left: 0,
        }
    }

    /// Converts C2's answer to a status question.
    #[must_use]
    pub fn from_status(s: &VoiceStatus) -> Self {
        let phase = TurnPhase::try_from(s.phase).unwrap_or(TurnPhase::Idle);
        let state = match phase {
            TurnPhase::Capturing => VoiceState::Listening,
            TurnPhase::Transcribing | TurnPhase::AwaitingAnswer => VoiceState::Thinking,
            TurnPhase::Speaking => VoiceState::Speaking,
            TurnPhase::FollowUp => VoiceState::FollowUp,
            TurnPhase::Idle => {
                if !s.capture_running {
                    VoiceState::Off
                } else if s.mic_in_use_by_other {
                    VoiceState::Busy
                } else {
                    VoiceState::Idle
                }
            }
        };
        #[allow(clippy::cast_precision_loss)]
        let level = (s.input_level_permille.min(1000) as f32) / 1000.0;
        Self {
            state,
            level,
            wake_words: s.wake_words,
            session_active: s.session_active,
            session_seconds_left: s.session_seconds_left,
            guest_seconds_left: s.guest_seconds_left,
        }
    }

    /// What a press does now.
    #[must_use]
    pub const fn press(&self) -> MicAction {
        match self.state {
            VoiceState::Idle | VoiceState::FollowUp => MicAction::Start,
            VoiceState::Listening => MicAction::Stop,
            VoiceState::Thinking | VoiceState::Speaking => MicAction::Cancel,
            VoiceState::Unavailable | VoiceState::Off | VoiceState::Busy => MicAction::None,
        }
    }

    /// The short text that is always shown with the state (never colour alone, UX-4).
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self.state {
            VoiceState::Unavailable => "Voice is unavailable",
            VoiceState::Off => "Voice is off",
            VoiceState::Idle => "Say \u{201c}hey jarvis\u{201d} or press to talk",
            VoiceState::Listening => "Listening\u{2026}",
            VoiceState::Thinking => "Thinking\u{2026}",
            VoiceState::Speaking => "Speaking\u{2026}",
            VoiceState::FollowUp => "Listening for a follow-up",
            VoiceState::Busy => "Another app is using the microphone",
        }
    }

    /// "Chatting, 4:32 left" while an open-microphone chat runs, "Guest mode, 12 min left" while
    /// she is quiet for company; `None` otherwise.
    #[must_use]
    pub fn chat_line(&self) -> Option<String> {
        if self.state == VoiceState::Unavailable || self.state == VoiceState::Off {
            return None;
        }
        if self.guest_seconds_left > 0 {
            return Some(format!(
                "Guest mode, {} min left",
                self.guest_seconds_left.div_ceil(60)
            ));
        }
        self.session_active.then(|| {
            format!(
                "Chatting, {}:{:02} left",
                self.session_seconds_left / 60,
                self.session_seconds_left % 60
            )
        })
    }

    /// The label on the button itself, for screen readers and tooltips.
    #[must_use]
    pub const fn button_label(&self) -> &'static str {
        match self.press() {
            MicAction::Start => "Start listening",
            MicAction::Stop => "Stop listening",
            MicAction::Cancel => "Stop",
            MicAction::None => "Microphone unavailable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(phase: TurnPhase) -> VoiceStatus {
        VoiceStatus {
            capture_running: true,
            phase: phase as i32,
            ..VoiceStatus::default()
        }
    }

    #[test]
    fn each_turn_phase_has_a_state_a_label_and_a_press() {
        let cases = [
            (TurnPhase::Idle, VoiceState::Idle, MicAction::Start),
            (TurnPhase::Capturing, VoiceState::Listening, MicAction::Stop),
            (
                TurnPhase::Transcribing,
                VoiceState::Thinking,
                MicAction::Cancel,
            ),
            (
                TurnPhase::AwaitingAnswer,
                VoiceState::Thinking,
                MicAction::Cancel,
            ),
            (TurnPhase::Speaking, VoiceState::Speaking, MicAction::Cancel),
            (TurnPhase::FollowUp, VoiceState::FollowUp, MicAction::Start),
        ];
        for (phase, state, action) in cases {
            let v = VoiceView::from_status(&status(phase));
            assert_eq!(v.state, state, "{phase:?}");
            assert_eq!(v.press(), action, "{phase:?}");
            assert!(!v.label().is_empty() && !v.button_label().is_empty());
        }
    }

    #[test]
    fn idle_without_capture_is_off_and_the_button_does_nothing() {
        let s = VoiceStatus {
            capture_running: false,
            ..VoiceStatus::default()
        };
        let v = VoiceView::from_status(&s);
        assert_eq!(v.state, VoiceState::Off);
        assert_eq!(v.press(), MicAction::None);
    }

    #[test]
    fn another_application_on_the_microphone_is_busy() {
        let s = VoiceStatus {
            capture_running: true,
            mic_in_use_by_other: true,
            ..VoiceStatus::default()
        };
        assert_eq!(VoiceView::from_status(&s).state, VoiceState::Busy);
    }

    #[test]
    fn level_is_clamped_and_unknown_phase_is_idle() {
        let mut s = status(TurnPhase::Idle);
        s.input_level_permille = 5000;
        s.phase = 99;
        let v = VoiceView::from_status(&s);
        assert!((v.level - 1.0).abs() < f32::EPSILON);
        assert_eq!(v.state, VoiceState::Idle);
        s.input_level_permille = 250;
        assert!((VoiceView::from_status(&s).level - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_running_chat_and_guest_mode_are_said_in_words() {
        let mut s = status(TurnPhase::FollowUp);
        assert_eq!(VoiceView::from_status(&s).chat_line(), None);
        s.session_active = true;
        s.session_seconds_left = 272;
        assert_eq!(
            VoiceView::from_status(&s).chat_line().as_deref(),
            Some("Chatting, 4:32 left")
        );
        s.session_seconds_left = 5;
        assert_eq!(
            VoiceView::from_status(&s).chat_line().as_deref(),
            Some("Chatting, 0:05 left")
        );
        s.guest_seconds_left = 601;
        assert_eq!(
            VoiceView::from_status(&s).chat_line().as_deref(),
            Some("Guest mode, 11 min left"),
            "guest mode wins: she is quiet"
        );
        assert_eq!(VoiceView::unavailable().chat_line(), None);
    }

    #[test]
    fn unavailable_view_does_nothing() {
        let v = VoiceView::unavailable();
        assert_eq!(v.press(), MicAction::None);
        assert_eq!(v.state, VoiceState::Unavailable);
    }
}
