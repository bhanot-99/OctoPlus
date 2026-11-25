//! Humour and manners (Phase 12, story C-S04, plan section 4.5).
//!
//! The owner steers humour with two plain phrases, "that was funny" and "not funny". Humour is
//! also switched off by itself at sensitive moments: late at night, when the owner sounds sad or
//! tired, and for a few turns after anything that looked like a crisis. Pure: no clock reads.

use serde::{Deserialize, Serialize};

/// How much humour the next reply should have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HumourLevel {
    /// No jokes and no teasing.
    Off,
    /// Light and rare.
    Low,
    /// A little, when it fits.
    Normal,
    /// Playful; a joke is welcome.
    High,
}

impl HumourLevel {
    /// The line that goes into the model's prompt.
    #[must_use]
    pub const fn prompt_line(self) -> &'static str {
        match self {
            Self::Off => "Humour: none right now. Do not joke or tease.",
            Self::Low => "Humour: light and rare. At most a small smile in a word.",
            Self::Normal => "Humour: a little, only when it fits. No teasing about feelings.",
            Self::High => "Humour: playful. A short joke is welcome. Never at the owner's cost.",
        }
    }
}

/// What the owner said about a joke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    /// "That was funny."
    Funny,
    /// "Not funny."
    NotFunny,
}

/// The fixed line she says back.
#[must_use]
pub const fn ack_line(f: Feedback) -> &'static str {
    match f {
        Feedback::Funny => "Glad that one landed.",
        Feedback::NotFunny => "Okay, noted. I'll tone it down.",
    }
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Reads "that was funny" and "not funny" from a short utterance (6 words at most).
#[must_use]
pub fn parse_feedback(text: &str) -> Option<Feedback> {
    let w = words(text);
    if w.is_empty() || w.len() > 6 {
        return None;
    }
    let j = w.join(" ");
    const NOT_FUNNY: [&str; 9] = [
        "not funny",
        "that was not funny",
        "that wasnt funny",
        "that is not funny",
        "bad joke",
        "that was a bad joke",
        "stop joking",
        "no more jokes",
        "that joke was bad",
    ];
    const FUNNY: [&str; 10] = [
        "that was funny",
        "that is funny",
        "thats funny",
        "good one",
        "good joke",
        "haha",
        "ha ha",
        "you are funny",
        "youre funny",
        "that made me laugh",
    ];
    if NOT_FUNNY.contains(&j.as_str()) {
        return Some(Feedback::NotFunny);
    }
    if FUNNY.contains(&j.as_str()) {
        return Some(Feedback::Funny);
    }
    None
}

/// True from one hour before the quiet hours start until they end: no jokes then.
#[must_use]
pub const fn late_hour(minute: u32, quiet_start: u32, quiet_end: u32) -> bool {
    let from = if quiet_start >= 60 {
        quiet_start - 60
    } else {
        quiet_start + 1440 - 60
    };
    if from == quiet_end {
        false
    } else if from < quiet_end {
        minute >= from && minute < quiet_end
    } else {
        minute >= from || minute < quiet_end
    }
}

/// The owner's taste in jokes, kept as small numbers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HumourState {
    /// How many "funny" the owner said.
    pub funny: u32,
    /// How many "not funny" the owner said.
    pub not_funny: u32,
    /// 0.0 (none) to 1.5 (playful); starts at 1.0.
    pub scale: f64,
}

impl Default for HumourState {
    fn default() -> Self {
        Self {
            funny: 0,
            not_funny: 0,
            scale: 1.0,
        }
    }
}

impl HumourState {
    /// Records feedback: a laugh raises the scale a little, "not funny" lowers it a lot.
    pub fn record(&mut self, f: Feedback) {
        match f {
            Feedback::Funny => {
                self.funny += 1;
                self.scale = (self.scale + 0.15).min(1.5);
            }
            Feedback::NotFunny => {
                self.not_funny += 1;
                self.scale = (self.scale - 0.3).max(0.0);
            }
        }
    }

    /// The level the owner's feedback asks for (before the sensitive-moment rules).
    #[must_use]
    pub fn level(&self) -> HumourLevel {
        if self.scale >= 1.2 {
            HumourLevel::High
        } else if self.scale >= 0.6 {
            HumourLevel::Normal
        } else if self.scale >= 0.25 {
            HumourLevel::Low
        } else {
            HumourLevel::Off
        }
    }

    /// The weight of the joke topic in the scheduler (0 means never).
    #[must_use]
    pub fn joke_weight(&self) -> f64 {
        match self.level() {
            HumourLevel::Off => 0.0,
            HumourLevel::Low => 0.4,
            HumourLevel::Normal => 1.0,
            HumourLevel::High => 1.6,
        }
    }
}

/// The level for one reply: the owner's taste, lowered to Off at a sensitive moment.
#[must_use]
pub fn effective_level(state: &HumourState, sensitive: bool) -> HumourLevel {
    if sensitive {
        HumourLevel::Off
    } else {
        state.level()
    }
}
