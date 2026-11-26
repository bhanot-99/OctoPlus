//! Pulse: WHEN Laila starts a chat on her own (Phase 12, story C-S03, plan section 4.4).
//!
//! Pure: no clock reads, no I/O. Time and the state of the moment come in as [`PulseInput`];
//! randomness comes from a seeded [`Rng`], so a simulation is repeatable. The scheduler never
//! decides WHAT is said, only whether this is a good moment and which kind of talk it is.
//!
//! Hard gates come first (quiet hours, calls, fullscreen, guests, caps, snoozes). Only a moment
//! that passes every gate can talk, with a small chance per 30 s tick that is shaped by what the
//! owner did with earlier talks at that hour of the day. A floor makes sure she is never silent
//! for hours while the owner is around and free.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::companion::persona::{NumOrOpen, Persona};
use crate::companion::rng::Rng;
use crate::nudge::signals::in_quiet_hours;
use crate::nudge::types::UserContext;

const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;

/// Seconds between two evaluations (the runner calls [`Pulse::tick`] this often).
pub const TICK_S: u64 = 30;
/// Shortest time between two talks.
pub const MIN_GAP_MIN: u64 = 10;
/// A talk is forced at the next allowed moment after this long without one (owner active).
pub const FLOOR_GAP_MIN: u64 = 90;
/// Quiet time after a talk the owner did not answer.
pub const COOLDOWN_MIN: u64 = 20;
/// Quiet time after the owner said "not now".
pub const SNOOZE_MIN: u64 = 60;
/// Idle time after which the owner counts as away.
pub const AWAY_IDLE_MS: u64 = 600_000;
/// How long the owner has to answer a talk before it counts as ignored.
pub const REPLY_WINDOW_S: u64 = 30;
/// Daily cap used when the persona leaves it open.
pub const DEFAULT_DAY_CAP: u32 = 40;
/// Persisted state format version.
const SNAPSHOT_VERSION: u32 = 1;

/// Why no talk happened at this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaitReason {
    /// The companion is paused (global switch).
    Paused,
    /// Guest mode is on.
    Guest,
    /// Inside the quiet hours.
    QuietHours,
    /// An open-microphone session is already running.
    InSession,
    /// A turn is in progress or the owner is speaking.
    UserBusy,
    /// Another application uses the microphone (call, recording).
    MicBusy,
    /// A fullscreen window is focused.
    Fullscreen,
    /// Media is playing.
    MediaLoud,
    /// The owner is away or the screen is locked.
    Away,
    /// The machine is in battery saving mode.
    BatterySaver,
    /// Speech output is not available.
    NoVoice,
    /// The owner asked for quiet ("not now", "quiet for an hour").
    Snoozed,
    /// A recent talk was ignored.
    Cooldown,
    /// The last talk was too recent.
    MinGap,
    /// The hourly cap is reached.
    HourCap,
    /// The daily cap is reached.
    DayCap,
    /// Every gate is open, the chance said not now.
    NotNow,
}

/// What kind of talk to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Topic {
    /// "How is it going?"
    CheckIn,
    /// A short joke.
    Joke,
    /// Something the owner mentioned earlier.
    FollowUp,
    /// A small observation about the day.
    Observation,
    /// A fun fact.
    FunFact,
    /// A gentle suggestion to take a break.
    BreakReminder,
    /// First talk of the day, before 11:00.
    Morning,
    /// Around lunch time.
    Lunch,
    /// In the evening.
    Evening,
}

/// The scheduler's answer for one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseDecision {
    /// Say nothing now.
    Wait(WaitReason),
    /// Start a talk of this kind.
    Talk(Topic),
}

/// What the owner did with a talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// The owner answered within the reply window.
    Accepted,
    /// Nobody answered.
    Ignored,
    /// The owner said "not now".
    NotNow,
}

/// Settings, read from the persona file plus fixed defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PulseConfig {
    /// Quiet hours start, minutes after midnight.
    pub quiet_start_min: u32,
    /// Quiet hours end, minutes after midnight.
    pub quiet_end_min: u32,
    /// Fewest talks an hour wanted.
    pub per_hour_min: u32,
    /// Most talks an hour allowed.
    pub per_hour_max: u32,
    /// Most talks a day allowed.
    pub per_day_cap: u32,
}

fn hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl PulseConfig {
    /// Reads the schedule of a validated persona.
    #[must_use]
    pub fn from_persona(p: &Persona) -> Self {
        let cap = match &p.schedule.per_day_cap {
            NumOrOpen::Num(n) => (*n).clamp(1, 100),
            NumOrOpen::Open(_) => DEFAULT_DAY_CAP,
        };
        Self {
            quiet_start_min: hhmm(&p.schedule.quiet_start).unwrap_or(23 * 60),
            quiet_end_min: hhmm(&p.schedule.quiet_end).unwrap_or(8 * 60),
            per_hour_min: p.schedule.talks_per_hour_min.max(1),
            per_hour_max: p
                .schedule
                .talks_per_hour_max
                .max(p.schedule.talks_per_hour_min)
                .max(1),
            per_day_cap: cap,
        }
    }
}

/// The state of the moment, supplied by the runner at every tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PulseInput {
    /// Now, in nanoseconds since the epoch.
    pub now_ns: u64,
    /// Minutes after local midnight.
    pub minute_of_day: u16,
    /// Local day number (any counter that changes at local midnight).
    pub day_index: u64,
    /// Pause, turn, microphone, media, fullscreen, away, voice (the nudge engine's view).
    /// Its `quiet_hours` field is ignored: Pulse computes quiet hours itself.
    pub ctx: UserContext,
    /// An open-microphone session is running.
    pub in_session: bool,
    /// Guest mode is on.
    pub guest: bool,
    /// Battery saving mode.
    pub battery_saver: bool,
    /// Milliseconds since the owner last touched the machine.
    pub idle_ms: u64,
    /// The owner just came back from being idle or away.
    pub returned_from_idle: bool,
    /// A long task just finished.
    pub task_finished: bool,
    /// A natural pause in the owner's work.
    pub natural_pause: bool,
}

/// Counts only (no text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PulseCounters {
    /// Talks started.
    pub talks: u64,
    /// Waits per reason, indexed by [`WaitReason`] order of declaration.
    pub waits: [u64; 17],
    /// Accepted talks.
    pub accepted: u64,
    /// Ignored talks.
    pub ignored: u64,
    /// "Not now" answers.
    pub not_now: u64,
}

fn wait_index(r: WaitReason) -> usize {
    match r {
        WaitReason::Paused => 0,
        WaitReason::Guest => 1,
        WaitReason::QuietHours => 2,
        WaitReason::InSession => 3,
        WaitReason::UserBusy => 4,
        WaitReason::MicBusy => 5,
        WaitReason::Fullscreen => 6,
        WaitReason::MediaLoud => 7,
        WaitReason::Away => 8,
        WaitReason::BatterySaver => 9,
        WaitReason::NoVoice => 10,
        WaitReason::Snoozed => 11,
        WaitReason::Cooldown => 12,
        WaitReason::MinGap => 13,
        WaitReason::HourCap => 14,
        WaitReason::DayCap => 15,
        WaitReason::NotNow => 16,
    }
}

/// What the owner did with talks at one hour of the day (counts only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourBucket {
    /// Answered talks.
    pub accepted: u32,
    /// Ignored talks.
    pub ignored: u32,
    /// "Not now" answers.
    pub not_now: u32,
}

/// What is kept between runs: small numbers only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PulseSnapshot {
    /// Format version.
    pub version: u32,
    /// "Chat more / less" scale.
    pub rate_scale: f64,
    /// 24 hour buckets.
    pub buckets: Vec<HourBucket>,
}

/// The scheduler.
#[derive(Debug, Clone)]
pub struct Pulse {
    cfg: PulseConfig,
    rng: Rng,
    paused: bool,
    rate_scale: f64,
    buckets: [HourBucket; 24],
    talks: VecDeque<u64>,
    day_index: u64,
    talks_today: u32,
    last_talk_ns: Option<u64>,
    snooze_until_ns: u64,
    cooldown_until_ns: u64,
    recent: VecDeque<Topic>,
    followup_available: bool,
    /// Multiplier of the joke topic's weight (0 turns jokes off).
    joke_weight: f64,
    /// Talks in a row that were ignored or answered with "not now".
    consecutive_unwelcome: u32,
    morning_day: Option<u64>,
    lunch_day: Option<u64>,
    evening_day: Option<u64>,
    counters: PulseCounters,
}

impl Pulse {
    /// A new scheduler with no history.
    #[must_use]
    pub fn new(cfg: PulseConfig, seed: u64) -> Self {
        Self {
            cfg,
            rng: Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15),
            paused: false,
            rate_scale: 1.0,
            buckets: [HourBucket::default(); 24],
            talks: VecDeque::new(),
            day_index: u64::MAX,
            talks_today: 0,
            last_talk_ns: None,
            snooze_until_ns: 0,
            cooldown_until_ns: 0,
            recent: VecDeque::new(),
            followup_available: false,
            joke_weight: 1.0,
            consecutive_unwelcome: 0,
            morning_day: None,
            lunch_day: None,
            evening_day: None,
            counters: PulseCounters::default(),
        }
    }

    /// A scheduler that continues from saved numbers (bad or old data falls back to defaults).
    #[must_use]
    pub fn restore(cfg: PulseConfig, seed: u64, snap: &PulseSnapshot) -> Self {
        let mut p = Self::new(cfg, seed);
        if snap.version == SNAPSHOT_VERSION && snap.buckets.len() == 24 {
            if snap.rate_scale.is_finite() {
                p.rate_scale = snap.rate_scale.clamp(0.5, 1.5);
            }
            for (i, b) in snap.buckets.iter().enumerate() {
                p.buckets[i] = *b;
            }
        }
        p
    }

    /// The numbers worth keeping.
    #[must_use]
    pub fn snapshot(&self) -> PulseSnapshot {
        PulseSnapshot {
            version: SNAPSHOT_VERSION,
            rate_scale: self.rate_scale,
            buckets: self.buckets.to_vec(),
        }
    }

    /// Counts for the health report.
    #[must_use]
    pub fn counters(&self) -> &PulseCounters {
        &self.counters
    }

    /// Talks started in the last hour (as of the last tick).
    #[must_use]
    pub fn talks_in_last_hour(&self) -> usize {
        self.talks.len()
    }

    /// Talks started today.
    #[must_use]
    pub const fn talks_today(&self) -> u32 {
        self.talks_today
    }

    /// The weight learned for an hour of the day (0.1 to 2.0; 1.0 with no history).
    #[must_use]
    pub fn hour_weight(&self, hour: usize) -> f64 {
        let b = self.buckets[hour.min(23)];
        let a = f64::from(b.accepted);
        let total = f64::from(b.accepted + b.ignored + b.not_now);
        (2.0 * (a + 1.0) / (total + 2.0)).clamp(0.1, 2.0)
    }

    /// The current "chat more / less" scale.
    #[must_use]
    pub const fn rate_scale(&self) -> f64 {
        self.rate_scale
    }

    /// Pause or resume all spontaneous talks.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// The schedule this scheduler runs on.
    #[must_use]
    pub const fn config(&self) -> PulseConfig {
        self.cfg
    }

    /// Whole minutes the owner's "not now" or "quiet for ..." still holds her back (0 if none).
    #[must_use]
    pub fn snooze_minutes_left(&self, now_ns: u64) -> u32 {
        let left = self.snooze_until_ns.saturating_sub(now_ns);
        u32::try_from(left.div_ceil(NS_PER_MIN)).unwrap_or(u32::MAX)
    }

    /// Whether the companion is paused.
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// "Quiet for N minutes".
    pub fn snooze_for(&mut self, now_ns: u64, minutes: u32) {
        let until = now_ns.saturating_add(u64::from(minutes) * NS_PER_MIN);
        self.snooze_until_ns = self.snooze_until_ns.max(until);
    }

    /// "Stop talking to me for today": quiet until the quiet hours end.
    pub fn quiet_until_morning(&mut self, now_ns: u64, minute_of_day: u16) {
        let end = self.cfg.quiet_end_min;
        let m = u32::from(minute_of_day);
        let mins = if m < end { end - m } else { 1440 - m + end };
        self.snooze_for(now_ns, mins);
    }

    /// "Chat more".
    pub fn chat_more(&mut self) {
        self.rate_scale = (self.rate_scale * 1.25).min(1.5);
    }

    /// "Chat less".
    pub fn chat_less(&mut self) {
        self.rate_scale = (self.rate_scale * 0.75).max(0.5);
    }

    /// How much the joke topic is wanted (0 means never), from the owner's feedback (C-S04).
    pub fn set_joke_weight(&mut self, weight: f64) {
        self.joke_weight = weight.clamp(0.0, 2.0);
    }

    /// A follow-up topic exists (set by the memory part, story C-S05).
    pub fn set_followup_available(&mut self, available: bool) {
        self.followup_available = available;
    }

    /// Records what the owner did with the last talk.
    pub fn record_response(&mut self, now_ns: u64, minute_of_day: u16, r: Response) {
        let hour = usize::from(minute_of_day / 60).min(23);
        match r {
            Response::Accepted => {
                self.buckets[hour].accepted += 1;
                self.counters.accepted += 1;
                self.consecutive_unwelcome = 0;
            }
            Response::Ignored => {
                self.buckets[hour].ignored += 1;
                self.counters.ignored += 1;
                self.consecutive_unwelcome = self.consecutive_unwelcome.saturating_add(1);
                self.cooldown_until_ns = now_ns.saturating_add(COOLDOWN_MIN * NS_PER_MIN);
            }
            Response::NotNow => {
                self.buckets[hour].not_now += 1;
                self.counters.not_now += 1;
                self.consecutive_unwelcome = self.consecutive_unwelcome.saturating_add(1);
                self.snooze_until_ns = self
                    .snooze_until_ns
                    .max(now_ns.saturating_add(SNOOZE_MIN * NS_PER_MIN));
            }
        }
    }

    fn roll_day(&mut self, day_index: u64) {
        if day_index != self.day_index {
            self.day_index = day_index;
            self.talks_today = 0;
        }
    }

    fn prune(&mut self, now_ns: u64) {
        let hour_ago = now_ns.saturating_sub(60 * NS_PER_MIN);
        while self.talks.front().is_some_and(|t| *t <= hour_ago) {
            self.talks.pop_front();
        }
    }

    fn gate(&self, i: &PulseInput) -> Option<WaitReason> {
        let now = i.now_ns;
        if self.paused || i.ctx.paused {
            return Some(WaitReason::Paused);
        }
        if i.guest {
            return Some(WaitReason::Guest);
        }
        if in_quiet_hours(
            u32::from(i.minute_of_day),
            self.cfg.quiet_start_min,
            self.cfg.quiet_end_min,
        ) {
            return Some(WaitReason::QuietHours);
        }
        if i.in_session {
            return Some(WaitReason::InSession);
        }
        if i.ctx.in_turn {
            return Some(WaitReason::UserBusy);
        }
        if i.ctx.mic_busy {
            return Some(WaitReason::MicBusy);
        }
        if i.ctx.fullscreen {
            return Some(WaitReason::Fullscreen);
        }
        if i.ctx.media_playing {
            return Some(WaitReason::MediaLoud);
        }
        if i.ctx.away || i.idle_ms >= AWAY_IDLE_MS {
            return Some(WaitReason::Away);
        }
        if i.battery_saver {
            return Some(WaitReason::BatterySaver);
        }
        if !i.ctx.voice_available {
            return Some(WaitReason::NoVoice);
        }
        if now < self.snooze_until_ns {
            return Some(WaitReason::Snoozed);
        }
        if now < self.cooldown_until_ns {
            return Some(WaitReason::Cooldown);
        }
        if self
            .last_talk_ns
            .is_some_and(|l| now.saturating_sub(l) < MIN_GAP_MIN * NS_PER_MIN)
        {
            return Some(WaitReason::MinGap);
        }
        if self.talks.len() >= self.cfg.per_hour_max as usize {
            return Some(WaitReason::HourCap);
        }
        if self.talks_today >= self.cfg.per_day_cap {
            return Some(WaitReason::DayCap);
        }
        None
    }

    fn chance(&self, i: &PulseInput) -> f64 {
        let rate = f64::from(self.cfg.per_hour_min + self.cfg.per_hour_max) / 2.0;
        let ticks_per_hour = 3600.0 / TICK_S as f64;
        let hour = usize::from(i.minute_of_day / 60);
        let mut p = rate / ticks_per_hour * self.hour_weight(hour) * self.rate_scale;
        if i.returned_from_idle {
            p *= 3.0;
        }
        if i.task_finished {
            p *= 2.0;
        }
        if i.natural_pause {
            p *= 1.5;
        }
        // The floor stretches (90, 180, 270 minutes) while talks keep being unwelcome.
        let floor_ns =
            FLOOR_GAP_MIN * NS_PER_MIN * (1 + u64::from(self.consecutive_unwelcome.min(2)));
        let floor_due = match self.last_talk_ns {
            Some(l) => i.now_ns.saturating_sub(l) >= floor_ns,
            None => true,
        } || (self.talks_today == 0 && self.consecutive_unwelcome == 0);
        if floor_due && i.idle_ms < AWAY_IDLE_MS {
            p = 1.0;
        }
        p.clamp(0.0, 1.0)
    }

    fn pick_topic(&mut self, i: &PulseInput) -> Topic {
        let m = u32::from(i.minute_of_day);
        let day = i.day_index;
        let since_last = self
            .last_talk_ns
            .map_or(u64::MAX, |l| i.now_ns.saturating_sub(l));
        let mut options: Vec<(Topic, f64)> = Vec::with_capacity(9);
        if self.talks_today == 0 && m < 11 * 60 && self.morning_day != Some(day) {
            // The first talk of the day before 11:00 is always the morning one.
            self.morning_day = Some(day);
            self.recent.push_back(Topic::Morning);
            while self.recent.len() > 3 {
                self.recent.pop_front();
            }
            return Topic::Morning;
        }
        if (12 * 60..14 * 60).contains(&m) && self.lunch_day != Some(day) {
            options.push((Topic::Lunch, 6.0));
        }
        if (18 * 60..21 * 60).contains(&m) && self.evening_day != Some(day) {
            options.push((Topic::Evening, 6.0));
        }
        if self.followup_available {
            options.push((Topic::FollowUp, 3.0));
        }
        if since_last >= FLOOR_GAP_MIN * NS_PER_MIN && i.idle_ms < 60_000 {
            options.push((Topic::BreakReminder, 2.0));
        }
        options.push((Topic::CheckIn, 3.0));
        if self.joke_weight > 0.0 {
            options.push((Topic::Joke, 0.8 * self.joke_weight));
        }
        options.push((Topic::Observation, 1.0));
        options.push((Topic::FunFact, 1.0));

        let fresh: Vec<(Topic, f64)> = options
            .iter()
            .copied()
            .filter(|(t, _)| !self.recent.contains(t))
            .collect();
        let pool = if fresh.is_empty() { options } else { fresh };
        let total: f64 = pool.iter().map(|(_, w)| *w).sum();
        let mut x = self.rng.next_f64() * total;
        let mut chosen = pool[0].0;
        for (t, w) in &pool {
            chosen = *t;
            if x < *w {
                break;
            }
            x -= *w;
        }
        // Never the same topic twice in a row, even when the pool is tiny.
        if self.recent.back() == Some(&chosen)
            && let Some((t, _)) = pool.iter().find(|(t, _)| *t != chosen)
        {
            chosen = *t;
        }
        match chosen {
            Topic::Morning => self.morning_day = Some(day),
            Topic::Lunch => self.lunch_day = Some(day),
            Topic::Evening => self.evening_day = Some(day),
            _ => {}
        }
        self.recent.push_back(chosen);
        while self.recent.len() > 3 {
            self.recent.pop_front();
        }
        chosen
    }

    /// One evaluation. Call it every [`TICK_S`] seconds.
    pub fn tick(&mut self, input: &PulseInput) -> PulseDecision {
        self.roll_day(input.day_index);
        self.prune(input.now_ns);
        if let Some(reason) = self.gate(input) {
            self.counters.waits[wait_index(reason)] += 1;
            return PulseDecision::Wait(reason);
        }
        let p = self.chance(input);
        if !self.rng.chance(p) {
            self.counters.waits[wait_index(WaitReason::NotNow)] += 1;
            return PulseDecision::Wait(WaitReason::NotNow);
        }
        let topic = self.pick_topic(input);
        self.talks.push_back(input.now_ns);
        self.talks_today += 1;
        self.last_talk_ns = Some(input.now_ns);
        self.counters.talks += 1;
        PulseDecision::Talk(topic)
    }
}

/// A spoken control for the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseCommand {
    /// "Not now": quiet for an hour.
    NotNow,
    /// "Quiet for N minutes".
    QuietFor {
        /// Minutes of quiet.
        minutes: u32,
    },
    /// "Stop talking to me for today".
    QuietUntilMorning,
    /// "Chat more".
    ChatMore,
    /// "Chat less".
    ChatLess,
    /// "Pause the companion".
    Pause,
    /// "Resume the companion".
    Resume,
}

fn norm(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| if c == '\'' || c == '’' { '\0' } else { c })
        .filter(|c| *c != '\0')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn number_word(w: &str) -> Option<u32> {
    const WORDS: [&str; 20] = [
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    if let Ok(n) = w.parse::<u32>() {
        return Some(n);
    }
    WORDS
        .iter()
        .position(|x| *x == w)
        .map(|p| u32::try_from(p + 1).unwrap_or(1))
}

fn quiet_minutes(words: &[String]) -> Option<u32> {
    let pos = words.iter().position(|w| w == "quiet")?;
    let rest = &words[pos + 1..];
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    let rest = match rest.as_slice() {
        ["for", tail @ ..] => tail,
        _ => return None,
    };
    match rest {
        ["today"] | ["the", "day"] | ["the", "rest", "of", "the", "day"] => Some(u32::MAX),
        ["half", "an", "hour"] => Some(30),
        ["an" | "a" | "one", "hour"] | ["1", "hour"] => Some(60),
        ["a" | "one", "minute"] => Some(1),
        [n, unit] => {
            let n = number_word(n)?;
            match *unit {
                "minute" | "minutes" | "min" | "mins" => Some(n.clamp(1, 600)),
                "hour" | "hours" => Some((n * 60).clamp(1, 600)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Reads a short spoken control ("not now", "quiet for an hour", "chat more"). Only whole short
/// utterances (8 words at most) count, so "I'm not now interested" does nothing.
#[must_use]
pub fn parse_pulse_command(text: &str) -> Option<PulseCommand> {
    let mut words = norm(text);
    if words.is_empty() || words.len() > 8 {
        return None;
    }
    while words
        .first()
        .is_some_and(|w| matches!(w.as_str(), "please" | "just" | "ok" | "okay" | "hey" | "so"))
    {
        words.remove(0);
    }
    let joined = words.join(" ");
    for lead in ["can you ", "could you ", "would you ", "will you "] {
        if let Some(rest) = joined.strip_prefix(lead) {
            return parse_core(&norm(rest)).or_else(|| parse_core(&words));
        }
    }
    parse_core(&words)
}

fn parse_core(words: &[String]) -> Option<PulseCommand> {
    let joined = words.join(" ");
    let j = joined
        .trim_end_matches(" please")
        .trim_start_matches("please ")
        .to_string();
    match j.as_str() {
        "not now" | "not right now" | "no not now" | "maybe later" => {
            return Some(PulseCommand::NotNow);
        }
        "chat more" | "chatmore" | "chant more" | "check more" | "talk more"
        | "talk to me more" | "be more chatty" | "chat with me more" => {
            return Some(PulseCommand::ChatMore);
        }
        "chat less" | "chatless" | "chant less" | "check less" | "talk less"
        | "talk to me less" | "be less chatty" | "chat with me less" => {
            return Some(PulseCommand::ChatLess);
        }
        "pause the companion" | "pause companion" | "pause laila" | "pause the chat" => {
            return Some(PulseCommand::Pause);
        }
        "resume the companion" | "resume companion" | "unpause the companion" | "resume laila" => {
            return Some(PulseCommand::Resume);
        }
        "stop talking to me for today" | "stop talking to me today" | "no more chat today" => {
            return Some(PulseCommand::QuietUntilMorning);
        }
        _ => {}
    }
    let w: Vec<String> = j.split(' ').map(str::to_string).collect();
    match quiet_minutes(&w) {
        Some(u32::MAX) => Some(PulseCommand::QuietUntilMorning),
        Some(minutes) => Some(PulseCommand::QuietFor { minutes }),
        None => None,
    }
}

/// The plain line she says back.
#[must_use]
pub const fn ack_line(c: &PulseCommand) -> &'static str {
    match c {
        PulseCommand::NotNow => "Okay, I'll leave you be for a while.",
        PulseCommand::QuietFor { .. } => "Okay, I'll be quiet for a while.",
        PulseCommand::QuietUntilMorning => "Okay, I'll be quiet for the rest of the day.",
        PulseCommand::ChatMore => "Okay, I'll talk a bit more.",
        PulseCommand::ChatLess => "Okay, I'll talk a bit less.",
        PulseCommand::Pause => "Okay, paused. Say resume when you want me back.",
        PulseCommand::Resume => "Okay, I'm back.",
    }
}

/// Applies a spoken control to the scheduler.
pub fn apply_command(p: &mut Pulse, c: &PulseCommand, now_ns: u64, minute_of_day: u16) {
    match c {
        PulseCommand::NotNow => p.record_response(now_ns, minute_of_day, Response::NotNow),
        PulseCommand::QuietFor { minutes } => p.snooze_for(now_ns, *minutes),
        PulseCommand::QuietUntilMorning => p.quiet_until_morning(now_ns, minute_of_day),
        PulseCommand::ChatMore => p.chat_more(),
        PulseCommand::ChatLess => p.chat_less(),
        PulseCommand::Pause => p.set_paused(true),
        PulseCommand::Resume => p.set_paused(false),
    }
}

/// The name of a topic as OctoPit shows it.
#[must_use]
pub const fn topic_label(t: Topic) -> &'static str {
    match t {
        Topic::CheckIn => "check_in",
        Topic::Joke => "joke",
        Topic::FollowUp => "follow_up",
        Topic::Observation => "observation",
        Topic::FunFact => "fun_fact",
        Topic::BreakReminder => "break_reminder",
        Topic::Morning => "morning",
        Topic::Lunch => "lunch",
        Topic::Evening => "evening",
    }
}

/// What the owner did with a talk, as OctoPit shows it ("" while waiting).
#[must_use]
pub const fn response_label(r: Option<Response>) -> &'static str {
    match r {
        None => "",
        Some(Response::Accepted) => "answered",
        Some(Response::Ignored) => "ignored",
        Some(Response::NotNow) => "not_now",
    }
}

/// Minutes after midnight as "HH:MM".
#[must_use]
pub fn clock_label(minute: u32) -> String {
    format!("{:02}:{:02}", (minute / 60) % 24, minute % 60)
}
