//! Kabir backstory topic gating and cooldown controls.

use crate::companion::persona::PastLoveConfig;
use crate::companion::rng::Rng;

/// Nanoseconds in one 24-hour day.
const NS_PER_DAY: u64 = 86_400_000_000_000;

/// Topic cues related to love, loneliness, and regret.
const LOVE_TOPIC_CUES: &[&str] = &[
    "break up",
    "breakup",
    "breakups",
    "heart break",
    "heartbreak",
    "heartbreaks",
    "loneliness",
    "lonely",
    "love",
    "miss someone",
    "missing someone",
    "regret",
    "regrets",
    "relationship",
    "relationships",
];

/// Normalizes text for topic cue matching.
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

/// Returns true if the text relates to love, loneliness, heartbreak, or regret.
pub fn love_topic(text: &str) -> bool {
    let norm = normalize_for_matching(text);
    LOVE_TOPIC_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Context passed into the Kabir gate evaluation for a dialogue turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KabirContext {
    /// True if the current conversation topic relates to love, loneliness, or regret.
    pub topic_is_love_loneliness_regret: bool,
    /// Number of turns completed in the current session.
    pub session_turns: u32,
    /// Current timestamp in nanoseconds since UNIX epoch.
    pub now_ns: u64,
    /// True if the session is currently in crisis mode.
    pub in_crisis: bool,
}

/// Cumulative counters for Kabir gate evaluations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KabirCounters {
    /// Number of times the gate conditions opened.
    pub opened: u64,
    /// Number of times the gate allowed a Kabir mention after the chance roll.
    pub allowed: u64,
    /// Number of times a Kabir mention was recorded as spoken.
    pub mentions: u64,
}

/// Gate controlling spontaneous or topic-triggered mentions of Kabir.
#[derive(Debug)]
pub struct KabirGate {
    past_love: PastLoveConfig,
    rng: Rng,
    last_mention_ns: u64,
    counters: KabirCounters,
}

impl KabirGate {
    /// Creates a new Kabir gate from configuration and a deterministic PRNG seed.
    pub fn new(past_love: &PastLoveConfig, seed: u64) -> Self {
        Self {
            past_love: past_love.clone(),
            rng: Rng::new(seed),
            last_mention_ns: 0,
            counters: KabirCounters::default(),
        }
    }

    /// Evaluates whether Kabir may be mentioned in the current turn.
    ///
    /// The gate opens only when:
    /// 1. Not in crisis mode (`!ctx.in_crisis`).
    /// 2. Cooldown period in days has elapsed since the last mention.
    /// 3. Either the conversation topic is love/loneliness/regret or session turns meet the minimum.
    ///
    /// If opened, a probabilistic check (`past_love.p`) determines whether mention is allowed.
    pub fn allowed(&mut self, ctx: KabirContext) -> bool {
        if ctx.in_crisis {
            return false;
        }

        let cooldown_ns = self.past_love.cooldown_days as u64 * NS_PER_DAY;
        let cooldown_met = self.last_mention_ns == 0
            || ctx.now_ns.saturating_sub(self.last_mention_ns) >= cooldown_ns;

        let trigger_met =
            ctx.topic_is_love_loneliness_regret || ctx.session_turns >= self.past_love.min_turns;

        if cooldown_met && trigger_met {
            self.counters.opened += 1;
            if self.rng.chance(self.past_love.p) {
                self.counters.allowed += 1;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Records that Kabir was mentioned at the specified timestamp.
    pub fn mentioned(&mut self, now_ns: u64) {
        self.last_mention_ns = now_ns;
        self.counters.mentions += 1;
    }

    /// Returns the current evaluation counters.
    pub fn counters(&self) -> KabirCounters {
        self.counters
    }
}
