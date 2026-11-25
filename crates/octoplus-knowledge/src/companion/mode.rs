//! Two-mode conversation state machine for Laila.
//!
//! Manages random and context-influenced switching between Logic and Heart modes,
//! mid-reply transitions, cooldown periods, and prompt instructions.

use serde::{Deserialize, Serialize};

use super::persona::SwitchingConfig;
use super::rng::Rng;

/// Cues suggesting sadness, fatigue, stress, or emotional vulnerability.
pub const SAD_OR_TIRED_CUES: &[&str] = &[
    "sad",
    "tired",
    "lonely",
    "upset",
    "stressed",
    "miss",
    "hurt",
    "anxious",
    "exhausted",
    "depressed",
    "unhappy",
    "overwhelmed",
    "crying",
    "pain",
];

/// Cues suggesting a request for analysis, planning, debugging, or facts.
pub const PLAN_OR_FACTS_CUES: &[&str] = &[
    "plan",
    "steps",
    "how do i",
    "fix",
    "deadline",
    "explain",
    "facts",
    "analyze",
    "solve",
    "debug",
    "calculate",
    "logic",
    "schedule",
    "requirements",
];

/// The active conversational persona mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Analytical, structured, factual, and direct.
    Logic,
    /// Warm, empathetic, reflective, and supportive.
    Heart,
}

impl Mode {
    /// Returns the opposite mode.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Logic => Self::Heart,
            Self::Heart => Self::Logic,
        }
    }
}

/// Contextual conversational signals extracted from user text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    /// Whether user text expresses sadness, fatigue, loneliness, or emotional stress.
    pub sad_or_tired: bool,
    /// Whether user text asks for planning, execution steps, or analytical facts.
    pub wants_plan_or_facts: bool,
}

/// Detects emotional and analytical signals from user input text.
#[must_use]
pub fn signals_from_text(text: &str) -> Signals {
    let lower = text.to_lowercase();
    let sad_or_tired = SAD_OR_TIRED_CUES.iter().any(|&cue| lower.contains(cue));
    let wants_plan_or_facts = PLAN_OR_FACTS_CUES.iter().any(|&cue| lower.contains(cue));
    Signals {
        sad_or_tired,
        wants_plan_or_facts,
    }
}

/// Outcome of a mode machine step for an incoming turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeDecision {
    /// Mode in which the turn begins.
    pub mode: Mode,
    /// Whether a between-turn mode switch occurred prior to this turn.
    pub switched: bool,
    /// Whether a mid-reply switch should take place partway through, and to which mode.
    pub mid_reply_switch: Option<Mode>,
}

/// Counters tracking mode machine turns and transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    /// Total turns processed.
    pub turns: u64,
    /// Total between-turn switches.
    pub switches: u64,
    /// Total mid-reply switches.
    pub mid_reply_switches: u64,
    /// Total turns delivered in Heart mode.
    pub heart_turns: u64,
    /// Total turns delivered in Logic mode.
    pub logic_turns: u64,
}

/// State machine driving Laila's mode switching and transitions.
#[derive(Debug, Clone)]
pub struct ModeMachine {
    mode: Mode,
    turns_since_switch: u32,
    last_was_switch: bool,
    switching: SwitchingConfig,
    rng: Rng,
    counters: Counters,
}

impl ModeMachine {
    /// Creates a new `ModeMachine` with parameters from `SwitchingConfig` and a seed.
    ///
    /// Determines the initial mode via `start_logic_p`.
    #[must_use]
    pub fn new(switching: &SwitchingConfig, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let start_logic = rng.chance(switching.start_logic_p);
        let mode = if start_logic {
            Mode::Logic
        } else {
            Mode::Heart
        };
        Self {
            mode,
            turns_since_switch: switching.cooldown_turns,
            last_was_switch: false,
            switching: switching.clone(),
            rng,
            counters: Counters::default(),
        }
    }

    /// Advances the mode machine by one turn.
    ///
    /// Evaluates between-turn switches and mid-reply switches based on configuration,
    /// cooldown constraints, and input context signals.
    pub fn step(&mut self, sig: Signals) -> ModeDecision {
        self.counters.turns = self.counters.turns.saturating_add(1);

        let can_between_switch = (!self.switching.never_twice_in_row || !self.last_was_switch)
            && (self.turns_since_switch >= self.switching.cooldown_turns);

        let mut p = self.switching.between_turn_p;
        if (sig.sad_or_tired && self.mode == Mode::Logic)
            || (sig.wants_plan_or_facts && self.mode == Mode::Heart)
        {
            p = (2.0 * self.switching.between_turn_p).min(1.0);
        }

        let mut switched = false;
        if can_between_switch && self.rng.chance(p) {
            switched = true;
            self.mode = self.mode.other();
            self.counters.switches = self.counters.switches.saturating_add(1);
            self.last_was_switch = true;
            self.turns_since_switch = 0;
        }

        let mid_reply_switch = if !switched {
            self.last_was_switch = false;
            self.turns_since_switch = self.turns_since_switch.saturating_add(1);
            if self.rng.chance(self.switching.mid_reply_p) {
                let target = self.mode.other();
                self.mode = target;
                self.counters.mid_reply_switches =
                    self.counters.mid_reply_switches.saturating_add(1);
                Some(target)
            } else {
                None
            }
        } else {
            None
        };

        let start_mode = if let Some(target) = mid_reply_switch {
            target.other()
        } else {
            self.mode
        };

        match start_mode {
            Mode::Heart => self.counters.heart_turns = self.counters.heart_turns.saturating_add(1),
            Mode::Logic => self.counters.logic_turns = self.counters.logic_turns.saturating_add(1),
        }

        ModeDecision {
            mode: start_mode,
            switched,
            mid_reply_switch,
        }
    }

    /// Returns current cumulative counters.
    #[must_use]
    pub const fn counters(&self) -> Counters {
        self.counters
    }

    /// Returns the current active mode.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }
}

/// Generates a prompt instruction line for the model based on a mode decision.
///
/// Returns `None` when no mode transition occurred for this turn.
#[must_use]
pub fn switch_instruction(decision: &ModeDecision) -> Option<&'static str> {
    // These are written for a small model: concrete example lines, and never the words that
    // name the two sides (it reads those aloud).
    if let Some(target) = decision.mid_reply_switch {
        match target {
            Mode::Heart => Some(
                "Start your answer brisk and precise, then soften partway through with a short natural aside such as 'Sorry, my brain just went analytical on you.' Never use the words mode, heart or logic.",
            ),
            Mode::Logic => Some(
                "Start your answer soft and open, then turn precise partway through with a short natural aside such as 'Okay, let me think about this properly.' Never use the words mode, heart or logic.",
            ),
        }
    } else if decision.switched {
        match decision.mode {
            Mode::Heart => Some(
                "Your tone just softened. If it fits, begin with a short natural line such as 'Hey... wait. How are you actually doing?' and then answer. Never use the words mode, heart or logic.",
            ),
            Mode::Logic => Some(
                "Your tone just sharpened. If it fits, begin with a short natural line such as 'Sorry, my brain just went analytical on you.' and then answer. Never use the words mode, heart or logic.",
            ),
        }
    } else {
        None
    }
}
