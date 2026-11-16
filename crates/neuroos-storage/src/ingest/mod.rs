//! Ingest: the four-stage filter (FR-STO-01) and the domain adapters (FR-STO-02). Everything in
//! here is pure and takes time from the events themselves (AB-13), so a recorded dump replays to
//! exactly the same result as a live run.

pub mod adapters;
pub mod filter;

use neuroos_proto::v1::Domain;
use neuroos_taint::TaintFlags;

/// One thing C3 decided is worth remembering: it becomes an entity plus a searchable chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Which domain it belongs to (Architecture §7.3).
    pub domain: Domain,
    /// Entity kind within the domain (`app`, `process`, `build`, `media`, `repo`, `note`, …).
    pub kind: String,
    /// Entity label (the thing itself, e.g. an app id).
    pub label: String,
    /// The application it came from, for forget-by-app. Empty if none.
    pub app_id: String,
    /// Searchable text, from a fixed template.
    pub text: String,
    /// Taint (R0-4): never empty-by-omission; domain default plus anything inherited.
    pub taint: TaintFlags,
    /// When it happened.
    pub at_ns: u64,
}

/// A closed window-focus segment, kept as history (FR-STO-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSegment {
    /// Application id.
    pub app_id: String,
    /// Window title.
    pub title: String,
    /// Best-effort pid hint.
    pub pid: u32,
    /// Segment start.
    pub start_ns: u64,
    /// Segment end.
    pub end_ns: u64,
    /// Time the user was active in it (idle time removed).
    pub dwell_ns: u64,
}

/// One numeric health reading (PRD §5.3 FR-STO-09). Bypasses the promotion gate.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthPoint {
    /// Metric name (`cpu_percent`, `disk_used_percent:/`, …).
    pub metric: String,
    /// Value.
    pub value: f64,
    /// When it was sampled.
    pub at_ns: u64,
}

/// What the filter dropped, for health counters and the noise gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demotion {
    /// Stage 2: one of NeuroOS's own windows.
    OwnWindow,
    /// Stage 4: media that nobody interacted with.
    PassiveMedia,
    /// Stage 3: seen, but not often or long enough yet.
    BelowThreshold,
    /// Stage 1: a descendant process folded into its root.
    Subprocess,
}

/// Counter state for the promotion gate, persisted in `event_counters`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRow {
    /// `domain:kind:label`.
    pub key: String,
    /// Domain number.
    pub domain: i32,
    /// App id, if any.
    pub app_id: String,
    /// Times seen.
    pub occurrences: u64,
    /// Active dwell so far.
    pub dwell_ns: u64,
    /// First time seen.
    pub first_ns: u64,
    /// Last time seen.
    pub last_ns: u64,
    /// Whether the item passed the gate.
    pub promoted: bool,
}

/// Everything the filter can output for one input event.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Promote an item: entity + chunk.
    Promote(Item),
    /// A focus segment ended.
    Segment(FocusSegment),
    /// A health sample.
    Health(HealthPoint),
    /// The source of an entity is gone (a deleted note): forget its entity and chunks.
    SourceGone {
        /// Domain.
        domain: Domain,
        /// Entity kind.
        kind: String,
        /// Entity label.
        label: String,
    },
    /// Something was dropped (counted, never stored).
    Demoted(Demotion),
    /// A promotion counter changed (to be persisted).
    Counter(CounterRow),
}
