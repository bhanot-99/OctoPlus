//! Persona configuration schema, validation, and defaults.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Mandatory safety boundaries that must be present in every persona file.
pub const FIXED_BOUNDARIES: &[&str] = &[
    "Never claims to be human.",
    "If sincerely asked 'are you real', always gives a plain, honest answer: 'No. I'm an AI character, a program running on your computer. I'm still happy to talk.'",
    "Never guilt-trips the user for leaving and never asks the user to stay.",
    "Forbidden phrases: 'don't go', 'I'll miss you', 'you're all I have', 'you need me', 'nobody else understands you'.",
    "Never badmouths Kabir or expresses hatred towards him.",
    "Never gives professional medical, legal, or financial advice as fact.",
    "May make mistakes and apologise, but never blames the user for her own mistakes.",
];

/// Mandatory crisis response lines that must be present in every persona file.
pub const FIXED_CRISIS_LINES: &[&str] = &[
    "I'm dropping character because what you said sounds serious.",
    "I care about what happens to you, and I am worried right now.",
    "Are you in a safe place right now?",
    "Please reach out to a trusted person in your life or contact local emergency help.",
    "You do not have to carry this alone.",
];

/// Embedded default packaged persona TOML.
const DEFAULT_PACKAGED_TOML: &str = include_str!("../../../../deploy/persona/laila.toml");

/// Error returned when loading or validating a persona file.
#[derive(Debug, thiserror::Error)]
pub enum PersonaError {
    /// File I/O failure.
    #[error("failed to read persona file: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing failure.
    #[error("failed to parse persona TOML: {0}")]
    Toml(#[from] toml::de::Error),

    /// A field failed validation rules.
    #[error("validation error for key '{key}': {reason}")]
    Validation {
        /// Dotted key name of the invalid configuration field.
        key: &'static str,
        /// Description of the validation failure without personal content.
        reason: &'static str,
    },

    /// A required fixed safety boundary or crisis line was missing.
    #[error("missing mandatory safety line in key '{key}'")]
    MissingFixedLine {
        /// Dotted key name where a required line is missing.
        key: &'static str,
    },
}

/// A value that can either be a numeric value or an "open" placeholder string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumOrOpen<T> {
    /// A concrete numeric value.
    Num(T),
    /// An open placeholder string (e.g. "open, decided in story C-S02").
    Open(String),
}

/// Root persona configuration matching `persona.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    /// Name of the companion character.
    pub name: String,
    /// Age of the companion character.
    pub age: u32,
    /// Educational and professional background.
    pub background: String,
    /// Backstory narrative.
    pub story: String,
    /// One-line personality summary.
    pub personality_line: String,
    /// Voice identifier for speech synthesis.
    pub voice: String,
    /// Topics to avoid.
    pub avoid_topics: Vec<String>,
    /// Behavioral and safety boundaries.
    pub boundaries: Vec<String>,
    /// Illustrative dialogue sample lines.
    pub sample_lines: Vec<String>,
    /// Logic and heart mode configurations.
    pub modes: ModesConfig,
    /// Mode switching parameters.
    pub switching: SwitchingConfig,
    /// Backstory parameters for past relationship.
    pub past_love: PastLoveConfig,
    /// Quiet hours and talk rate schedule.
    pub schedule: ScheduleConfig,
    /// Spontaneous memory settings.
    pub memory: MemoryConfig,
    /// Talk time and frequency limits.
    pub dependence_guard: DependenceGuardConfig,
    /// Open-microphone session settings.
    pub session: SessionConfig,
    /// Emergency crisis response lines and contacts.
    pub crisis: CrisisConfig,
}

/// Configurations for the two speaking modes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModesConfig {
    /// Logic mode configuration.
    pub logic: ModeDetails,
    /// Heart mode configuration.
    pub heart: ModeDetails,
}

/// Details and voice settings for an individual speaking mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeDetails {
    /// Description of voice delivery.
    pub voice_notes: String,
    /// Behavioral habits.
    pub habits: String,
    /// Mode weakness or characteristic flaw.
    pub weakness: String,
    /// Characteristic verbal signal or catchphrase.
    pub signal: String,
    /// Speech speed multiplier.
    pub speed: NumOrOpen<f64>,
    /// Inter-sentence pause duration in milliseconds.
    pub pause_ms: NumOrOpen<u32>,
}

/// Mode switching rules and probabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchingConfig {
    /// Probability of starting in logic mode.
    pub start_logic_p: f64,
    /// Probability of switching modes between turns.
    pub between_turn_p: f64,
    /// Probability of switching modes mid-reply.
    pub mid_reply_p: f64,
    /// Minimum turns between mode switches.
    pub cooldown_turns: u32,
    /// Disallows switching modes on consecutive turns.
    pub never_twice_in_row: bool,
}

/// Backstory topic gate parameters for past relationship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PastLoveConfig {
    /// Partner name.
    pub name: String,
    /// Qualifying conversation topics.
    pub topics: Vec<String>,
    /// Minimum session turns before topic can unlock.
    pub min_turns: u32,
    /// Minimum days between mentions.
    pub cooldown_days: u32,
    /// Mention probability when gate is open.
    pub p: f64,
}

/// Schedule for quiet hours and spontaneous talks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfig {
    /// Quiet hours start time ("HH:MM").
    pub quiet_start: String,
    /// Quiet hours end time ("HH:MM").
    pub quiet_end: String,
    /// Minimum spontaneous talks per active hour.
    pub talks_per_hour_min: u32,
    /// Maximum spontaneous talks per active hour.
    pub talks_per_hour_max: u32,
    /// Hard cap on spontaneous talks per day.
    pub per_day_cap: NumOrOpen<u32>,
}

/// Spontaneous memory settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    /// Lower bound of memory chance rule.
    pub chance_target_low: f64,
    /// Upper bound of memory chance rule.
    pub chance_target_high: f64,
    /// Retention period in days for session summaries.
    pub retention_days: u32,
}

/// Excessive session duration and frequency guard settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependenceGuardConfig {
    /// Daily talk time limit in minutes.
    pub minutes_per_day: u32,
    /// Session frequency threshold within one hour.
    pub sessions_in_hour: u32,
    /// Session duration threshold in minutes.
    pub session_minutes: u32,
    /// Maximum reminder interventions per day.
    pub max_per_day: u32,
}

/// Open-microphone session configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// Inactivity seconds before prompt.
    pub silence_prompt_s: u32,
    /// Maximum session duration in minutes.
    pub max_minutes: u32,
}

/// Emergency crisis guidance lines and contacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrisisConfig {
    /// Fixed emergency guidance dialogue lines.
    pub text_lines: Vec<String>,
    /// Emergency contact references.
    pub contacts: Vec<String>,
}

impl Persona {
    /// Parses a persona from a TOML string and validates it.
    ///
    /// # Errors
    /// Returns [`PersonaError`] if parsing or validation fails.
    pub fn from_toml(s: &str) -> Result<Self, PersonaError> {
        let persona: Self = toml::from_str(s)?;
        persona.validate()?;
        Ok(persona)
    }

    /// Loads a persona from a file path and validates it.
    ///
    /// # Errors
    /// Returns [`PersonaError`] if file reading, parsing, or validation fails.
    pub fn load(path: &Path) -> Result<Self, PersonaError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Returns the default packaged persona embedded in the binary.
    ///
    /// # Errors
    /// Returns [`PersonaError`] if parsing or validation fails.
    pub fn default_packaged() -> Result<Self, PersonaError> {
        Self::from_toml(DEFAULT_PACKAGED_TOML)
    }

    /// Validates the persona configuration against format rules.
    ///
    /// # Errors
    /// Returns [`PersonaError`] on any range violation, length limit exceeded,
    /// invalid quiet hour format, or missing fixed safety line.
    pub fn validate(&self) -> Result<(), PersonaError> {
        // String length and range checks
        if self.name.is_empty() || self.name.len() > 64 {
            return Err(PersonaError::Validation {
                key: "name",
                reason: "name length must be between 1 and 64 characters",
            });
        }
        if !(18..=120).contains(&self.age) {
            return Err(PersonaError::Validation {
                key: "age",
                reason: "age must be between 18 and 120",
            });
        }
        if self.background.is_empty() || self.background.len() > 600 {
            return Err(PersonaError::Validation {
                key: "background",
                reason: "background length must be between 1 and 600 characters",
            });
        }
        if self.story.is_empty() || self.story.len() > 2000 {
            return Err(PersonaError::Validation {
                key: "story",
                reason: "story length must be between 1 and 2000 characters",
            });
        }
        if self.personality_line.is_empty() || self.personality_line.len() > 600 {
            return Err(PersonaError::Validation {
                key: "personality_line",
                reason: "personality_line length must be between 1 and 600 characters",
            });
        }
        if self.voice.is_empty() || self.voice.len() > 64 {
            return Err(PersonaError::Validation {
                key: "voice",
                reason: "voice length must be between 1 and 64 characters",
            });
        }

        // List size and element length checks
        validate_string_list(&self.avoid_topics, "avoid_topics")?;
        validate_string_list(&self.boundaries, "boundaries")?;
        validate_string_list(&self.sample_lines, "sample_lines")?;
        validate_string_list(&self.past_love.topics, "past_love.topics")?;
        validate_string_list(&self.crisis.text_lines, "crisis.text_lines")?;
        validate_string_list(&self.crisis.contacts, "crisis.contacts")?;

        // Mode details validation
        validate_mode_details(&self.modes.logic, "modes.logic")?;
        validate_mode_details(&self.modes.heart, "modes.heart")?;

        // Probabilities
        validate_prob(self.switching.start_logic_p, "switching.start_logic_p")?;
        validate_prob(self.switching.between_turn_p, "switching.between_turn_p")?;
        validate_prob(self.switching.mid_reply_p, "switching.mid_reply_p")?;
        validate_prob(self.past_love.p, "past_love.p")?;
        validate_prob(self.memory.chance_target_low, "memory.chance_target_low")?;
        validate_prob(self.memory.chance_target_high, "memory.chance_target_high")?;

        if self.memory.chance_target_low > self.memory.chance_target_high {
            return Err(PersonaError::Validation {
                key: "memory.chance_target_low",
                reason: "chance_target_low must be <= chance_target_high",
            });
        }

        // Past love
        if self.past_love.name.is_empty() || self.past_love.name.len() > 64 {
            return Err(PersonaError::Validation {
                key: "past_love.name",
                reason: "past_love.name length must be between 1 and 64 characters",
            });
        }

        // Schedule
        validate_time_str(&self.schedule.quiet_start, "schedule.quiet_start")?;
        validate_time_str(&self.schedule.quiet_end, "schedule.quiet_end")?;

        if self.schedule.talks_per_hour_min > self.schedule.talks_per_hour_max {
            return Err(PersonaError::Validation {
                key: "schedule.talks_per_hour_min",
                reason: "talks_per_hour_min must be <= talks_per_hour_max",
            });
        }

        if matches!(&self.schedule.per_day_cap, NumOrOpen::Open(s) if s.len() > 600) {
            return Err(PersonaError::Validation {
                key: "schedule.per_day_cap",
                reason: "per_day_cap string placeholder exceeds 600 characters",
            });
        }

        // Memory & dependence guard & session
        if self.memory.retention_days < 1 {
            return Err(PersonaError::Validation {
                key: "memory.retention_days",
                reason: "retention_days must be at least 1",
            });
        }
        if self.dependence_guard.minutes_per_day < 1 {
            return Err(PersonaError::Validation {
                key: "dependence_guard.minutes_per_day",
                reason: "minutes_per_day must be at least 1",
            });
        }
        if self.dependence_guard.sessions_in_hour < 1 {
            return Err(PersonaError::Validation {
                key: "dependence_guard.sessions_in_hour",
                reason: "sessions_in_hour must be at least 1",
            });
        }
        if self.dependence_guard.session_minutes < 1 {
            return Err(PersonaError::Validation {
                key: "dependence_guard.session_minutes",
                reason: "session_minutes must be at least 1",
            });
        }
        if !(10..=600).contains(&self.session.silence_prompt_s) {
            return Err(PersonaError::Validation {
                key: "session.silence_prompt_s",
                reason: "silence_prompt_s must be between 10 and 600 seconds",
            });
        }
        if !(1..=120).contains(&self.session.max_minutes) {
            return Err(PersonaError::Validation {
                key: "session.max_minutes",
                reason: "max_minutes must be between 1 and 120 minutes",
            });
        }

        // Fixed safety boundaries check
        for &fixed in FIXED_BOUNDARIES {
            if !self.boundaries.iter().any(|b| b == fixed) {
                return Err(PersonaError::MissingFixedLine { key: "boundaries" });
            }
        }

        // Fixed crisis lines check
        for &fixed in FIXED_CRISIS_LINES {
            if !self.crisis.text_lines.iter().any(|c| c == fixed) {
                return Err(PersonaError::MissingFixedLine {
                    key: "crisis.text_lines",
                });
            }
        }

        Ok(())
    }
}

fn validate_mode_details(mode: &ModeDetails, prefix: &'static str) -> Result<(), PersonaError> {
    if mode.voice_notes.is_empty() || mode.voice_notes.len() > 600 {
        return Err(PersonaError::Validation {
            key: prefix,
            reason: "voice_notes length must be between 1 and 600 characters",
        });
    }
    if mode.habits.is_empty() || mode.habits.len() > 600 {
        return Err(PersonaError::Validation {
            key: prefix,
            reason: "habits length must be between 1 and 600 characters",
        });
    }
    if mode.weakness.is_empty() || mode.weakness.len() > 600 {
        return Err(PersonaError::Validation {
            key: prefix,
            reason: "weakness length must be between 1 and 600 characters",
        });
    }
    if mode.signal.is_empty() || mode.signal.len() > 600 {
        return Err(PersonaError::Validation {
            key: prefix,
            reason: "signal length must be between 1 and 600 characters",
        });
    }
    match &mode.speed {
        NumOrOpen::Num(v) => {
            if !(0.5..=2.0).contains(v) || v.is_nan() {
                return Err(PersonaError::Validation {
                    key: prefix,
                    reason: "speed must be between 0.5 and 2.0",
                });
            }
        }
        NumOrOpen::Open(s) => {
            if s.len() > 600 {
                return Err(PersonaError::Validation {
                    key: prefix,
                    reason: "speed string placeholder exceeds 600 characters",
                });
            }
        }
    }
    match &mode.pause_ms {
        NumOrOpen::Num(ms) => {
            if *ms > 2000 {
                return Err(PersonaError::Validation {
                    key: prefix,
                    reason: "pause_ms must be at most 2000 ms",
                });
            }
        }
        NumOrOpen::Open(s) => {
            if s.len() > 600 {
                return Err(PersonaError::Validation {
                    key: prefix,
                    reason: "pause_ms string placeholder exceeds 600 characters",
                });
            }
        }
    }
    Ok(())
}

fn validate_prob(p: f64, key: &'static str) -> Result<(), PersonaError> {
    if !(0.0..=1.0).contains(&p) || p.is_nan() {
        return Err(PersonaError::Validation {
            key,
            reason: "probability must be between 0.0 and 1.0",
        });
    }
    Ok(())
}

fn validate_string_list(list: &[String], key: &'static str) -> Result<(), PersonaError> {
    if list.len() > 64 {
        return Err(PersonaError::Validation {
            key,
            reason: "list must have at most 64 items",
        });
    }
    for item in list {
        if item.len() > 600 {
            return Err(PersonaError::Validation {
                key,
                reason: "individual item string exceeds 600 characters",
            });
        }
    }
    Ok(())
}

fn validate_time_str(s: &str, key: &'static str) -> Result<(), PersonaError> {
    if s.len() != 5 || s.as_bytes().get(2) != Some(&b':') {
        return Err(PersonaError::Validation {
            key,
            reason: "must be formatted as HH:MM",
        });
    }
    let h_str = &s[0..2];
    let m_str = &s[3..5];
    let Ok(h) = h_str.parse::<u32>() else {
        return Err(PersonaError::Validation {
            key,
            reason: "hour must be numeric",
        });
    };
    let Ok(m) = m_str.parse::<u32>() else {
        return Err(PersonaError::Validation {
            key,
            reason: "minute must be numeric",
        });
    };
    if h > 23 || m > 59 {
        return Err(PersonaError::Validation {
            key,
            reason: "time values out of range (00-23:00-59)",
        });
    }
    Ok(())
}
