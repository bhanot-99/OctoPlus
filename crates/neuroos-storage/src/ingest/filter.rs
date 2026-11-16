//! The four-stage ingest filter (FR-STO-01), as a pure state machine over C1 events:
//!
//! 1. **Collapse** a process tree to its root (children never become entities; build tools mark a
//!    `build_job` instead).
//! 2. **Drop NeuroOS's own windows.**
//! 3. **Promote** an item only after the configured occurrences or active dwell.
//! 4. **Demote passive media**: media nobody interacted with is counted, never stored.
//!
//! Time comes from the events (`observed_at_ns`), never the clock, so a recorded dump replays to
//! the same result as the live run (AB-13). Security-critical (NFR-QA-01): every decision outcome
//! has a table-driven or property test.

use std::collections::{HashMap, HashSet};

use neuroos_common::config::StorageConfig;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{Domain, FileActivityKind, MonitorEvent, PlaybackState};
use neuroos_taint::TaintFlags;

use super::adapters::{self, FileClass};
use super::{CounterRow, Demotion, FocusSegment, Item, Output};

/// Most promotion counters held; unpromoted ones are dropped oldest-first beyond this
/// (rules §9.7, bounded memory).
const COUNTER_CAP: usize = 8192;
/// An already-promoted file or repository is re-announced at most this often.
const REANNOUNCE_NS: u64 = 60_000_000_000;

/// Which stages are active. Always all on in service; the noise gate turns one off to prove it
/// would notice (rules §8.9 negative control).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stages {
    /// Stage 1.
    pub collapse: bool,
    /// Stage 2.
    pub own_windows: bool,
    /// Stage 3.
    pub promote: bool,
    /// Stage 4.
    pub media: bool,
}

impl Default for Stages {
    fn default() -> Self {
        Self {
            collapse: true,
            own_windows: true,
            promote: true,
            media: true,
        }
    }
}

/// What an item is about, for the gate.
#[derive(Debug, Clone, Copy)]
struct Subject<'a> {
    domain: Domain,
    kind: &'a str,
    label: &'a str,
    app_id: &'a str,
}

/// When something was seen and how long the user was active in it.
#[derive(Debug, Clone, Copy)]
struct Stamp {
    at: u64,
    dwell: u64,
}

#[derive(Debug, Clone)]
struct Counter {
    domain: Domain,
    app_id: String,
    occurrences: u64,
    dwell_ns: u64,
    first_ns: u64,
    last_ns: u64,
    promoted: bool,
    last_announced_ns: u64,
}

#[derive(Debug)]
struct Window {
    app_id: String,
    title: String,
    pid: u32,
    start_ns: u64,
    active_ns: u64,
    mark_ns: u64,
}

#[derive(Debug, Default)]
struct MediaState {
    playing_since: Option<u64>,
    interacted_at_start: bool,
}

/// The filter. One per ingest stream.
#[derive(Debug)]
pub struct IngestFilter {
    occurrences_needed: u64,
    dwell_needed_ns: u64,
    interaction_ns: u64,
    own_prefixes: Vec<String>,
    build_names: HashSet<String>,
    stages: Stages,
    counters: HashMap<String, Counter>,
    window: Option<Window>,
    own_focused: bool,
    idle: bool,
    idle_since: Option<u64>,
    last_focused: HashMap<String, u64>,
    media: HashMap<String, MediaState>,
}

fn key(domain: Domain, kind: &str, label: &str) -> String {
    format!("{}:{kind}:{label}", domain as i32)
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Whether a media player and a window's app id are the same program: `brave` and
/// `brave-browser`, `firefox` and `org.mozilla.firefox`. Names under three letters never match.
#[must_use]
pub fn same_program(player: &str, app_id: &str) -> bool {
    let p = norm(player);
    let a = norm(app_id);
    let last = norm(app_id.rsplit('.').next().unwrap_or(app_id));
    p.len() >= 3 && a.len() >= 3 && (a.contains(&p) || (last.len() >= 3 && p.contains(&last)))
}

impl IngestFilter {
    /// A filter with every stage on, thresholds from `cfg` (FR-STO-01 numbers live in PRD §5.3).
    #[must_use]
    pub fn new(cfg: &StorageConfig) -> Self {
        Self::with_stages(cfg, Stages::default())
    }

    /// A filter with chosen stages (the noise gate's negative control).
    #[must_use]
    pub fn with_stages(cfg: &StorageConfig, stages: Stages) -> Self {
        Self {
            occurrences_needed: u64::from(cfg.promote_min_occurrences),
            dwell_needed_ns: cfg.promote_min_dwell_s.saturating_mul(1_000_000_000),
            interaction_ns: cfg.media_interaction_s.saturating_mul(1_000_000_000),
            own_prefixes: cfg
                .own_app_prefixes
                .iter()
                .map(|p| p.to_ascii_lowercase())
                .collect(),
            build_names: cfg
                .build_process_names
                .iter()
                .map(|n| n.to_ascii_lowercase())
                .collect(),
            stages,
            counters: HashMap::new(),
            window: None,
            own_focused: false,
            idle: false,
            idle_since: None,
            last_focused: HashMap::new(),
            media: HashMap::new(),
        }
    }

    /// Restores promotion counters saved in `event_counters`, so a restart keeps its progress.
    pub fn restore(&mut self, rows: impl IntoIterator<Item = CounterRow>) {
        for r in rows {
            let domain = Domain::try_from(r.domain).unwrap_or(Domain::Unspecified);
            self.counters.insert(
                r.key,
                Counter {
                    domain,
                    app_id: r.app_id,
                    occurrences: r.occurrences,
                    dwell_ns: r.dwell_ns,
                    first_ns: r.first_ns,
                    last_ns: r.last_ns,
                    promoted: r.promoted,
                    last_announced_ns: 0,
                },
            );
        }
    }

    fn is_own(&self, app_id: &str) -> bool {
        let a = app_id.to_ascii_lowercase();
        self.own_prefixes.iter().any(|p| a.starts_with(p.as_str()))
    }

    /// Feeds one event; returns what to store, count or drop.
    pub fn push(&mut self, ev: &MonitorEvent) -> Vec<Output> {
        let at = ev.observed_at_ns;
        let mut out = Vec::new();
        match &ev.event {
            Some(Event::Focus(f)) => self.on_focus(at, f, &mut out),
            Some(Event::Idle(i)) => self.on_idle(at, i.idle, &mut out),
            Some(Event::Media(m)) => self.on_media(at, m, &mut out),
            Some(Event::ProcessTree(t)) => self.on_tree(at, t, &mut out),
            Some(Event::File(f)) => self.on_file(at, f, &mut out),
            Some(Event::Resources(_) | Event::Disk(_) | Event::Battery(_) | Event::Thermal(_)) => {
                out.extend(adapters::health_points(ev).into_iter().map(Output::Health));
            }
            Some(Event::Lock(_)) | None => {}
        }
        self.bound_counters();
        out
    }

    /// Ends the current focus segment (shutdown or the end of a replay).
    pub fn finish(&mut self, now_ns: u64) -> Vec<Output> {
        let mut out = Vec::new();
        self.close_window(now_ns, &mut out);
        out
    }

    // ---- stage 3: the promotion gate ------------------------------------------------------

    /// Counts one occurrence (and dwell); returns the row and whether it was promoted *now*.
    fn bump(&mut self, k: &str, who: (Domain, &str), st: Stamp) -> (CounterRow, bool) {
        let (domain, app_id) = who;
        let (at, dwell) = (st.at, st.dwell);
        let c = self
            .counters
            .entry(k.to_owned())
            .or_insert_with(|| Counter {
                domain,
                app_id: app_id.to_owned(),
                occurrences: 0,
                dwell_ns: 0,
                first_ns: at,
                last_ns: at,
                promoted: false,
                last_announced_ns: 0,
            });
        c.occurrences += 1;
        c.dwell_ns = c.dwell_ns.saturating_add(dwell);
        c.last_ns = at;
        let passes = !self.stages.promote
            || c.occurrences >= self.occurrences_needed
            || c.dwell_ns > self.dwell_needed_ns;
        let newly = passes && !c.promoted;
        if newly {
            c.promoted = true;
        }
        (row(k, c), newly)
    }

    fn add_dwell(&mut self, k: &str, at: u64, dwell: u64) -> (Option<CounterRow>, bool) {
        let Some(c) = self.counters.get_mut(k) else {
            return (None, false);
        };
        c.dwell_ns = c.dwell_ns.saturating_add(dwell);
        c.last_ns = at;
        let newly = !c.promoted
            && (c.occurrences >= self.occurrences_needed || c.dwell_ns > self.dwell_needed_ns);
        if newly {
            c.promoted = true;
        }
        (Some(row(k, c)), newly)
    }

    fn promoted(&self, k: &str) -> bool {
        self.counters.get(k).is_some_and(|c| c.promoted)
    }

    /// Re-announce an already-promoted item at most once a minute.
    fn due(&mut self, k: &str, at: u64) -> bool {
        match self.counters.get_mut(k) {
            Some(c)
                if at.saturating_sub(c.last_announced_ns) >= REANNOUNCE_NS
                    || c.last_announced_ns == 0 =>
            {
                c.last_announced_ns = at;
                true
            }
            _ => false,
        }
    }

    fn bound_counters(&mut self) {
        if self.counters.len() <= COUNTER_CAP {
            return;
        }
        let mut stale: Vec<(String, u64)> = self
            .counters
            .iter()
            .filter(|(_, c)| !c.promoted)
            .map(|(k, c)| (k.clone(), c.last_ns))
            .collect();
        stale.sort_by_key(|(_, t)| *t);
        let drop_n = self.counters.len() - COUNTER_CAP + COUNTER_CAP / 8;
        for (k, _) in stale.into_iter().take(drop_n) {
            self.counters.remove(&k);
        }
    }

    // ---- window focus and idle ---------------------------------------------------------------

    fn accrue(&mut self, at: u64) {
        let idle = self.idle;
        if let Some(w) = self.window.as_mut() {
            if !idle {
                w.active_ns = w.active_ns.saturating_add(at.saturating_sub(w.mark_ns));
            }
            w.mark_ns = at;
        }
    }

    fn close_window(&mut self, at: u64, out: &mut Vec<Output>) {
        self.accrue(at);
        let Some(w) = self.window.take() else { return };
        // Interaction is measured from the moment the user *left* the window.
        self.last_focused.insert(w.app_id.to_ascii_lowercase(), at);
        out.push(Output::Segment(FocusSegment {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            pid: w.pid,
            start_ns: w.start_ns,
            end_ns: at.max(w.start_ns),
            dwell_ns: w.active_ns,
        }));
        let k = key(Domain::WindowFocus, "app", &w.app_id);
        let (row, newly) = self.add_dwell(&k, at, w.active_ns);
        out.extend(row.map(Output::Counter));
        // A title is worth a chunk only if the user really spent time in it.
        let long = w.active_ns > self.dwell_needed_ns;
        if (newly || self.promoted(&k)) && long {
            out.push(Output::Promote(Item {
                domain: Domain::WindowFocus,
                kind: "app".into(),
                label: w.app_id.clone(),
                app_id: w.app_id.clone(),
                text: adapters::focus_text(&w.app_id, &w.title),
                taint: TaintFlags::empty(),
                at_ns: at,
            }));
        }
    }

    fn on_focus(&mut self, at: u64, f: &neuroos_proto::v1::FocusChanged, out: &mut Vec<Output>) {
        self.close_window(at, out);
        let empty = f.app_id.is_empty() && f.title.is_empty();
        self.own_focused = false;
        if empty {
            return; // nothing observable is focused (an excluded or closed window)
        }
        if self.stages.own_windows && self.is_own(&f.app_id) {
            self.own_focused = true;
            out.push(Output::Demoted(Demotion::OwnWindow));
            return;
        }
        self.last_focused.insert(f.app_id.to_ascii_lowercase(), at);
        self.window = Some(Window {
            app_id: f.app_id.clone(),
            title: f.title.clone(),
            pid: f.pid,
            start_ns: at,
            active_ns: 0,
            mark_ns: at,
        });
        let k = key(Domain::WindowFocus, "app", &f.app_id);
        let (row, newly) = self.bump(&k, (Domain::WindowFocus, &f.app_id), Stamp { at, dwell: 0 });
        out.push(Output::Counter(row));
        if newly {
            out.push(Output::Promote(Item {
                domain: Domain::WindowFocus,
                kind: "app".into(),
                label: f.app_id.clone(),
                app_id: f.app_id.clone(),
                text: adapters::focus_text(&f.app_id, ""),
                taint: TaintFlags::empty(),
                at_ns: at,
            }));
        } else if !self.promoted(&k) {
            out.push(Output::Demoted(Demotion::BelowThreshold));
        }
    }

    fn on_idle(&mut self, at: u64, idle: bool, out: &mut Vec<Output>) {
        self.accrue(at);
        if idle == self.idle {
            return;
        }
        self.idle = idle;
        if idle {
            self.idle_since = Some(at);
            return;
        }
        let Some(since) = self.idle_since.take() else {
            return;
        };
        let away = at.saturating_sub(since);
        let k = key(Domain::IdlePresence, "presence", "presence");
        let (row, newly) = self.bump(&k, (Domain::IdlePresence, ""), Stamp { at, dwell: 0 });
        out.push(Output::Counter(row));
        if newly || self.promoted(&k) {
            out.push(Output::Promote(Item {
                domain: Domain::IdlePresence,
                kind: "presence".into(),
                label: "presence".into(),
                app_id: String::new(),
                text: adapters::presence_text(away),
                taint: TaintFlags::empty(),
                at_ns: at,
            }));
        }
    }

    // ---- stage 4: media -----------------------------------------------------------------------

    fn interacted(&self, player: &str, at: u64) -> bool {
        if let Some(w) = &self.window
            && same_program(player, &w.app_id)
        {
            return true;
        }
        self.last_focused.iter().any(|(app, t)| {
            same_program(player, app) && at.saturating_sub(*t) <= self.interaction_ns
        })
    }

    fn on_media(&mut self, at: u64, m: &neuroos_proto::v1::MediaChanged, out: &mut Vec<Output>) {
        self.accrue(at);
        let player = m.player.to_ascii_lowercase();
        let interacted = self.interacted(&player, at);
        let state = PlaybackState::try_from(m.state).unwrap_or(PlaybackState::Unspecified);
        let st = self.media.entry(player.clone()).or_default();
        let mut dwell = 0;
        if state == PlaybackState::Playing {
            if st.playing_since.is_none() {
                st.playing_since = Some(at);
                st.interacted_at_start = interacted;
            }
        } else if let Some(since) = st.playing_since.take()
            && st.interacted_at_start
        {
            dwell = at.saturating_sub(since);
        }
        if self.stages.media && !interacted {
            out.push(Output::Demoted(Demotion::PassiveMedia));
            return;
        }
        let k = key(Domain::MediaPlayback, "media", &player);
        let (row, newly) = self.bump(&k, (Domain::MediaPlayback, ""), Stamp { at, dwell });
        out.push(Output::Counter(row));
        if newly || self.promoted(&k) {
            let state_word = match state {
                PlaybackState::Playing => "playing",
                PlaybackState::Paused => "paused",
                PlaybackState::Stopped => "stopped",
                PlaybackState::Unspecified => "changed",
            };
            out.push(Output::Promote(Item {
                domain: Domain::MediaPlayback,
                kind: "media".into(),
                label: player.clone(),
                app_id: String::new(),
                text: adapters::media_text(&m.player, state_word, &m.title, &m.artist),
                taint: TaintFlags::empty(),
                at_ns: at,
            }));
        } else {
            out.push(Output::Demoted(Demotion::BelowThreshold));
        }
    }

    // ---- stage 1: process trees -----------------------------------------------------------------

    fn on_tree(&mut self, at: u64, t: &neuroos_proto::v1::ProcessTree, out: &mut Vec<Output>) {
        if self.own_focused && self.stages.own_windows {
            out.push(Output::Demoted(Demotion::OwnWindow));
            return;
        }
        let Some(root) = t
            .nodes
            .iter()
            .find(|n| n.pid == t.root_pid)
            .or_else(|| t.nodes.first())
        else {
            return;
        };
        let app_id = self
            .window
            .as_ref()
            .map(|w| w.app_id.clone())
            .unwrap_or_default();
        if !self.stages.collapse {
            // Negative control only: every process becomes its own item, as the old prototype did.
            for n in &t.nodes {
                let who = Subject {
                    domain: Domain::ProcessActivity,
                    kind: "process",
                    label: &n.name,
                    app_id: &app_id,
                };
                self.gated_item(who, adapters::process_text(&n.name), at, out);
            }
            return;
        }
        let tools: std::collections::BTreeSet<String> = t
            .nodes
            .iter()
            .filter(|n| n.pid != root.pid)
            .map(|n| n.name.to_ascii_lowercase())
            .filter(|n| self.build_names.contains(n))
            .collect();
        if t.nodes.len() > 1 {
            out.push(Output::Demoted(Demotion::Subprocess));
        }
        if tools.is_empty() {
            let who = Subject {
                domain: Domain::ProcessActivity,
                kind: "process",
                label: &root.name,
                app_id: &app_id,
            };
            self.gated_item(who, adapters::process_text(&root.name), at, out);
        } else {
            let tools: Vec<String> = tools.into_iter().collect();
            let text = adapters::build_text(&root.name, &tools);
            let who = Subject {
                domain: Domain::BuildJob,
                kind: "build",
                label: &root.name,
                app_id: &app_id,
            };
            self.gated_item(who, text, at, out);
        }
    }

    fn gated_item(&mut self, who: Subject<'_>, text: String, at: u64, out: &mut Vec<Output>) {
        let k = key(who.domain, who.kind, who.label);
        let (row, newly) = self.bump(&k, (who.domain, who.app_id), Stamp { at, dwell: 0 });
        out.push(Output::Counter(row));
        if newly || (self.promoted(&k) && self.due(&k, at)) {
            if newly && let Some(c) = self.counters.get_mut(&k) {
                c.last_announced_ns = at;
            }
            out.push(Output::Promote(Item {
                domain: who.domain,
                kind: who.kind.to_owned(),
                label: who.label.to_owned(),
                app_id: who.app_id.to_owned(),
                text,
                taint: TaintFlags::empty(),
                at_ns: at,
            }));
        } else if !self.promoted(&k) {
            out.push(Output::Demoted(Demotion::BelowThreshold));
        }
    }

    // ---- folders --------------------------------------------------------------------------------

    fn on_file(&mut self, at: u64, f: &neuroos_proto::v1::FileActivity, out: &mut Vec<Output>) {
        let Some(class) = adapters::classify_file(&f.path) else {
            return;
        };
        let kind = FileActivityKind::try_from(f.kind).unwrap_or(FileActivityKind::Unspecified);
        if kind == FileActivityKind::Removed && class.domain != Domain::GitActivity {
            let k = key(class.domain, class.kind, &class.label);
            self.counters.remove(&k);
            out.push(Output::SourceGone {
                domain: class.domain,
                kind: class.kind.to_owned(),
                label: class.label,
            });
            return;
        }
        let verb = if kind == FileActivityKind::Created {
            "created"
        } else {
            "changed"
        };
        let text = adapters::file_text(&class, verb);
        let FileClass {
            domain,
            kind: ekind,
            label,
        } = class;
        let who = Subject {
            domain,
            kind: ekind,
            label: &label,
            app_id: "",
        };
        self.gated_item(who, text, at, out);
    }
}

fn row(k: &str, c: &Counter) -> CounterRow {
    CounterRow {
        key: k.to_owned(),
        domain: c.domain as i32,
        app_id: c.app_id.clone(),
        occurrences: c.occurrences,
        dwell_ns: c.dwell_ns,
        first_ns: c.first_ns,
        last_ns: c.last_ns,
        promoted: c.promoted,
    }
}
