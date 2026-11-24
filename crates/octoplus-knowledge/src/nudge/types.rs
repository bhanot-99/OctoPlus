//! Plain types shared by the nudge engine's pure parts (FR-NUD-02, design.md §8).
//!
//! No I/O, no clock, no randomness. Time is always passed in as epoch nanoseconds (AB-13).
//! Nothing here can hold evidence text or a skill call: a rule *names* the skill it offers, and
//! only the turn engine dispatches it, from the user's reply (FR-NUD-10, R0-6).

pub use octoplus_common::config::NudgeDelivery as Delivery;

/// How loudly a nudge may speak (FR-NUD-02). Ordered: `Info < Important < Urgent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Nice to know.
    Info,
    /// Worth interrupting for.
    Important,
    /// Needs attention now.
    Urgent,
}

/// What a rule watches. Each is one number per `key` (a mount point, or "" for machine-wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Signal {
    /// Percent of a filesystem used, 0..=100. `Reading::extra` is the free space in GiB.
    DiskUsedPercent,
    /// Hours until a filesystem is full at the recent rate. Derived by the evaluator from
    /// [`Signal::DiskUsedPercent`] readings (their `extra`); never supplied directly.
    /// `Reading::extra` is the free space in GiB.
    DiskFillHours,
    /// Battery percent, supplied only while discharging and not on mains power.
    BatteryPercent,
    /// Hottest sensor, in degrees Celsius.
    TemperatureC,
    /// Memory still available, as a percent of the total.
    MemoryAvailablePercent,
    /// Minutes of continuous activity (0 when idle).
    ActiveMinutes,
    /// How many OctoPlus parts are degraded or unreachable.
    PartsDegraded,
}

/// Which side of the threshold means "the condition holds".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Holds when `value >= fire_at` (re-arms when `value < rearm`, with `rearm <= fire_at`).
    AtLeast,
    /// Holds when `value <= fire_at` (re-arms when `value > rearm`, with `rearm >= fire_at`).
    AtMost,
}

/// Where a template number comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgSource {
    /// The reading's `value`.
    Value,
    /// The reading's `extra`.
    Extra,
}

/// One declarative rule (FR-NUD-02, design.md §8.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Stable id, `[a-z0-9_]{1,40}`.
    pub id: String,
    /// What it watches.
    pub signal: Signal,
    /// Which side of `fire_at` holds.
    pub direction: Direction,
    /// Threshold at which the condition holds.
    pub fire_at: f64,
    /// The value must pass this (the far side of `fire_at`) before the rule can fire again.
    pub rearm: f64,
    /// The condition must hold this long, in seconds, before the rule fires.
    pub hold_for_s: u64,
    /// Minimum time between two firings of this rule and key, in seconds.
    pub cooldown_s: u64,
    /// How loudly it may speak.
    pub priority: Priority,
    /// Delivery for this rule; `None` means "what the priority says" (config).
    pub deliver: Option<Delivery>,
    /// C6 notification template id (also the spoken-template key in Phase 7).
    pub template: String,
    /// Template arguments: `(name, source)`, names exactly as the template declares them.
    pub args: Vec<(String, ArgSource)>,
    /// Skill that may be *offered*; dispatched only from the user's reply.
    pub offer_skill: Option<String>,
    /// False for a rule that ships switched off.
    pub enabled: bool,
}

/// One measurement handed to the evaluator at a tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// What was measured.
    pub signal: Signal,
    /// Which instance (mount point, or "").
    pub key: String,
    /// The number the rule compares.
    pub value: f64,
    /// A second number a template may use (see [`Signal`]).
    pub extra: f64,
}

/// A rule that fired: what the policy gate decides about.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The rule that fired (`"merged"` for a merged "while you were away" line).
    pub rule_id: String,
    /// The instance it fired for.
    pub key: String,
    /// Its priority.
    pub priority: Priority,
    /// The rule's own delivery, if any.
    pub deliver: Option<Delivery>,
    /// C6 template id.
    pub template: String,
    /// Template numbers, `(name, value)`, already resolved from the reading.
    pub args: Vec<(String, f64)>,
    /// The skill that may be offered.
    pub offer_skill: Option<String>,
    /// When it fired.
    pub at_ns: u64,
}

/// What the evaluator remembers about a rule, for `octoplusctl nudges list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleStatus {
    /// Rule id.
    pub rule_id: String,
    /// Switched on?
    pub enabled: bool,
    /// Snoozed until this time (0 = not snoozed).
    pub snoozed_until_ns: u64,
    /// When it last fired (0 = never).
    pub last_fired_ns: u64,
}

/// Why the policy gate decided what it did (logged, never spoken).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Nothing in the way.
    Clear,
    /// The rule or priority is set to `off`.
    DeliveryOff,
    /// Paused.
    Paused,
    /// Quiet hours.
    QuietHours,
    /// A turn is in progress or the user is speaking.
    InTurn,
    /// Another app is using the microphone.
    MicBusy,
    /// Media is playing: a notification instead of speech.
    Media,
    /// A fullscreen window is focused: a notification instead of speech.
    Fullscreen,
    /// The user is away or the screen is locked.
    Away,
    /// The spoken-nudge limit for the hour or day is reached.
    RateLimited,
    /// Global quiet mode forces notifications.
    QuietMode,
    /// Voice is not available yet (Phase 7): spoken becomes a notification.
    NoVoice,
    /// A held nudge replaced by a newer one of the same rule and key.
    Replaced,
    /// A held nudge pushed out because the held list is full (oldest, lowest priority first).
    Overflow,
}

/// What to do with a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Say it (Phase 7).
    Speak,
    /// Show a notification; `silent` means no sound.
    Notify {
        /// No sound.
        silent: bool,
    },
    /// Keep it until the context clears.
    Hold,
    /// Throw it away (delivery off, or replaced).
    Drop,
}

/// The gate's answer for one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// What was decided about.
    pub candidate: Candidate,
    /// What to do.
    pub action: Action,
    /// Why.
    pub reason: Reason,
    /// For a merged "while you were away" line: the `(rule_id, key)` of every nudge it stands for.
    pub merged_from: Vec<(String, String)>,
}

/// What is going on around the user right now (inputs to the policy gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UserContext {
    /// Sensing and listening are paused.
    pub paused: bool,
    /// Inside the configured quiet hours.
    pub quiet_hours: bool,
    /// A turn is in progress or the user is speaking.
    pub in_turn: bool,
    /// Another app is using the microphone.
    pub mic_busy: bool,
    /// Media is playing.
    pub media_playing: bool,
    /// A fullscreen window is focused.
    pub fullscreen: bool,
    /// The user is away or the screen is locked.
    pub away: bool,
    /// Speech output exists (false until Phase 7).
    pub voice_available: bool,
}

/// Policy knobs (all from `[nudges]` config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyConfig {
    /// Global quiet mode: notifications only (FR-NUD-07).
    pub quiet_mode: bool,
    /// Delivery for `Priority::Info` rules with no delivery of their own.
    pub deliver_info: Delivery,
    /// Delivery for `Priority::Important`.
    pub deliver_important: Delivery,
    /// Delivery for `Priority::Urgent`.
    pub deliver_urgent: Delivery,
    /// Most spoken nudges per rolling hour (FR-NUD-06).
    pub max_spoken_per_hour: u32,
    /// Most spoken nudges per rolling day.
    pub max_spoken_per_day: u32,
    /// Most nudges held at once.
    pub max_held: usize,
}
