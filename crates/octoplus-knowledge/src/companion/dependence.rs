//! Dependence guard tracking session duration, frequency, and isolation cues.

use crate::companion::persona::DependenceGuardConfig;

/// Nanoseconds in one minute.
const NS_PER_MINUTE: u64 = 60_000_000_000;

/// Nanoseconds in one hour.
const NS_PER_HOUR: u64 = 3_600_000_000_000;

/// Nanoseconds in one 24-hour day.
const NS_PER_DAY: u64 = 86_400_000_000_000;

/// Gentle lines suggesting a break or checking in with real people.
pub const GENTLE_LINES: &[&str] = &[
    "We've been talking for a while. It might be good to take a break and step outside for a bit.",
    "I'm always here, but remember to take care of yourself and check in with people in your life too.",
    "You've been at the screen for quite a stretch. How about a stretch or a glass of water?",
];

/// Phrases indicating social isolation or exclusive dependence on the AI.
const NO_ONE_ELSE_CUES: &[&str] = &[
    "i have no one",
    "no one else",
    "no one to talk to",
    "nobody else",
    "you are all i have",
    "youre all i have",
];

/// Normalizes text for isolation cue matching.
fn normalize_for_matching(text: &str) -> String {
    let mut s = String::with_capacity(text.len() + 2);
    s.push(' ');
    for c in text.chars() {
        if c == '\'' || c == '’' {
            continue;
        } else if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else {
            s.push(' ');
        }
    }
    s.push(' ');

    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(" {collapsed} ")
}

/// Returns true if the user expressed feeling alone, having nobody else, or complete reliance on the AI.
pub fn no_one_else(text: &str) -> bool {
    let norm = normalize_for_matching(text);
    NO_ONE_ELSE_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Guard tracking conversation duration and frequency to prevent unhealthy dependence.
#[derive(Debug, Clone)]
pub struct DependenceGuard {
    config: DependenceGuardConfig,
    sessions: Vec<(u64, u64)>,
    last_day: u64,
    triggers_today: u32,
}

impl DependenceGuard {
    /// Creates a new dependence guard from configuration.
    pub fn new(config: &DependenceGuardConfig) -> Self {
        Self {
            config: config.clone(),
            sessions: Vec::new(),
            last_day: 0,
            triggers_today: 0,
        }
    }

    /// Records a completed session's start and end timestamps in nanoseconds.
    pub fn record_session(&mut self, start_ns: u64, end_ns: u64) {
        if end_ns > start_ns {
            self.sessions.push((start_ns, end_ns));
        }
    }

    /// Checks if a gentle dependence intervention line should be spoken.
    ///
    /// Triggers when daily intervention cap has not been reached AND any of:
    /// 1. Cumulative talk time today meets or exceeds `minutes_per_day`.
    /// 2. Qualifying session count in the last hour meets or exceeds `sessions_in_hour`.
    /// 3. The user expressed social isolation (`owner_said_no_one_else`).
    pub fn should_speak(&mut self, now_ns: u64, owner_said_no_one_else: bool) -> bool {
        let day = now_ns / NS_PER_DAY;
        if day != self.last_day {
            self.last_day = day;
            self.triggers_today = 0;
        }

        if self.triggers_today >= self.config.max_per_day {
            return false;
        }

        // Condition 1: talk time today in minutes >= config.minutes_per_day.
        let minutes_today: u64 = self
            .sessions
            .iter()
            .filter(|(start, end)| *start / NS_PER_DAY == day || *end / NS_PER_DAY == day)
            .map(|(start, end)| end.saturating_sub(*start) / NS_PER_MINUTE)
            .sum();

        let duration_triggered =
            self.config.minutes_per_day > 0 && minutes_today >= self.config.minutes_per_day as u64;

        // Condition 2: sessions in the last hour >= config.sessions_in_hour.
        let one_hour_ago = now_ns.saturating_sub(NS_PER_HOUR);
        let min_session_ns = self.config.session_minutes as u64 * NS_PER_MINUTE;
        let recent_sessions_count = self
            .sessions
            .iter()
            .filter(|(start, end)| {
                *end >= one_hour_ago && end.saturating_sub(*start) >= min_session_ns
            })
            .count();

        let frequency_triggered = self.config.sessions_in_hour > 0
            && recent_sessions_count as u32 >= self.config.sessions_in_hour;

        // Condition 3: owner indicated social isolation.
        let isolation_triggered = owner_said_no_one_else;

        if duration_triggered || frequency_triggered || isolation_triggered {
            self.triggers_today += 1;
            true
        } else {
            false
        }
    }
}
