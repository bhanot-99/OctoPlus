//! Everything the window shows, in one GTK-free value (ADR-0017). The window applies each
//! [`Update`] here and then redraws only what changed.

use crate::chat::{ChatMode, Conversation};
use crate::companion::CompanionView;
use crate::controller::Update;
use crate::gateway::{AuditRow, ChainStatus, EgressRow};
use crate::gpu::GpuState;
use crate::health::{Headline, PartState, Snapshot};
use crate::series::Series;
use crate::voice::VoiceView;
use octoplus_common::paths::Part;

/// How many samples a sparkline keeps (about five minutes at the default rate).
pub const SERIES_LEN: usize = 150;

/// What an update changed, so only those parts of the window redraw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    /// Part health.
    pub health: bool,
    /// Voice state.
    pub voice: bool,
    /// The companion.
    pub companion: bool,
    /// GPU.
    pub gpu: bool,
    /// Chat.
    pub chat: bool,
    /// Privacy ledgers.
    pub ledgers: bool,
    /// A notice appeared.
    pub notice: bool,
}

/// The privacy ledgers as last read.
#[derive(Debug, Clone, Default)]
pub struct Ledgers {
    /// Newest first. `None` until the first read.
    pub egress: Option<Result<Vec<EgressRow>, String>>,
    /// Newest first.
    pub audit: Option<Result<Vec<AuditRow>, String>>,
    /// Hash-chain check.
    pub chain: Option<Result<ChainStatus, String>>,
}

impl Ledgers {
    /// Bytes sent to the network in the records read, or `None` when the ledger is unknown.
    #[must_use]
    pub fn egress_bytes_out(&self) -> Option<u64> {
        match &self.egress {
            Some(Ok(rows)) => Some(rows.iter().map(|r| r.bytes_out).sum()),
            _ => None,
        }
    }

    /// How many requests have left this machine (records read), or `None` when unknown.
    #[must_use]
    pub fn egress_requests(&self) -> Option<usize> {
        match &self.egress {
            Some(Ok(rows)) => Some(rows.len()),
            _ => None,
        }
    }
}

/// The whole window's data.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Part health.
    pub snapshot: Snapshot,
    /// Whether any snapshot arrived yet (the first screen shows loading until it does).
    pub have_snapshot: bool,
    /// The voice dock.
    pub voice: VoiceView,
    /// The companion's rhythm, talks and memories.
    pub companion: CompanionView,
    /// The GPU, `None` until the first read.
    pub gpu: Option<GpuState>,
    /// This window's conversation.
    pub conversation: Conversation,
    /// Selected Chat page target (Laila or Assistant).
    pub chat_mode: ChatMode,
    /// Privacy ledgers.
    pub ledgers: Ledgers,
    /// The newest notice, shown once.
    pub notice: Option<String>,
    /// GPU busy percent.
    pub gpu_util: Series,
    /// GPU memory percent.
    pub gpu_mem: Series,
    /// GPU temperature.
    pub gpu_temp: Series,
    /// Total memory of all parts, MiB.
    pub rss_total: Series,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// The state before anything was read.
    #[must_use]
    pub fn new() -> Self {
        Self {
            snapshot: Snapshot::offline(),
            have_snapshot: false,
            voice: VoiceView::unavailable(),
            companion: CompanionView::unavailable(),
            gpu: None,
            conversation: Conversation::new(),
            chat_mode: ChatMode::Laila,
            ledgers: Ledgers::default(),
            notice: None,
            gpu_util: Series::new(SERIES_LEN),
            gpu_mem: Series::new(SERIES_LEN),
            gpu_temp: Series::new(SERIES_LEN),
            rss_total: Series::new(SERIES_LEN),
        }
    }

    /// Applies one update and says what changed.
    pub fn apply(&mut self, update: Update) -> Changed {
        let mut changed = Changed::default();
        match update {
            Update::Snapshot(s) => {
                #[allow(clippy::cast_precision_loss)]
                self.rss_total.push(s.total_rss_mib as f64);
                self.snapshot = s;
                self.have_snapshot = true;
                changed.health = true;
            }
            Update::Voice(v) => {
                if v != self.voice {
                    self.voice = v;
                    changed.voice = true;
                }
            }
            Update::Companion(c) => {
                if c != self.companion {
                    self.companion = c;
                    changed.companion = true;
                }
            }
            Update::Gpu(g) => {
                if let GpuState::Present(r) = &g {
                    if let Some(u) = r.utilization_pct {
                        self.gpu_util.push(u);
                    }
                    if let Some(f) = r.memory_fraction() {
                        self.gpu_mem.push(f * 100.0);
                    }
                    if let Some(t) = r.temperature_c {
                        self.gpu_temp.push(t);
                    }
                }
                self.gpu = Some(g);
                changed.gpu = true;
            }
            Update::Chat(e) => {
                self.conversation.apply(e);
                changed.chat = true;
            }
            Update::Ledgers {
                egress,
                audit,
                chain,
            } => {
                self.ledgers = Ledgers {
                    egress: Some(egress),
                    audit: Some(audit),
                    chain: Some(chain),
                };
                changed.ledgers = true;
            }
            Update::Notice(n) => {
                self.notice = Some(n);
                changed.notice = true;
            }
        }
        changed
    }

    /// Updates the selected chat mode and returns whether it changed.
    pub fn set_chat_mode(&mut self, mode: ChatMode) -> bool {
        if self.chat_mode == mode {
            false
        } else {
            self.chat_mode = mode;
            true
        }
    }

    /// The sentence at the top of the header: one thing, the most important.
    #[must_use]
    pub fn headline_text(&self) -> String {
        if !self.have_snapshot {
            return "Connecting\u{2026}".to_owned();
        }
        match self.snapshot.headline() {
            Headline::Healthy => "All systems healthy".to_owned(),
            Headline::Unreachable => "OctoPlus is not running".to_owned(),
            Headline::Attention { part, state } => {
                let title = self
                    .snapshot
                    .parts
                    .iter()
                    .find(|p| p.part == part)
                    .map_or("A part", |p| p.title);
                let what = match state {
                    PartState::Starting => "is starting",
                    PartState::Degraded => "needs attention",
                    PartState::Down => "is not running",
                    PartState::Ok => "is fine",
                };
                format!("{title} {what}")
            }
        }
    }

    /// The part card for `part`.
    #[must_use]
    pub fn part(&self, part: Part) -> Option<&crate::health::PartCard> {
        self.snapshot.parts.iter().find(|p| p.part == part)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatEvent;
    use crate::gpu::GpuReading;
    use crate::voice::VoiceState;

    fn reading(util: f64) -> GpuState {
        GpuState::Present(GpuReading {
            name: "G".into(),
            driver: "1".into(),
            utilization_pct: Some(util),
            memory_used_mib: Some(1024.0),
            memory_total_mib: Some(4096.0),
            temperature_c: Some(50.0),
            power_w: None,
            power_limit_w: None,
        })
    }

    #[test]
    fn starts_connecting_and_never_claims_health_before_a_snapshot() {
        let s = AppState::new();
        assert_eq!(s.headline_text(), "Connecting\u{2026}");
        assert!(!s.have_snapshot);
    }

    #[test]
    fn an_offline_snapshot_says_octoplus_is_not_running_or_names_a_part() {
        let mut s = AppState::new();
        let c = s.apply(Update::Snapshot(Snapshot::offline()));
        assert!(c.health && s.have_snapshot);
        assert!(!s.headline_text().contains("healthy"));
    }

    #[test]
    fn gpu_updates_feed_the_series() {
        let mut s = AppState::new();
        let c = s.apply(Update::Gpu(reading(40.0)));
        assert!(c.gpu);
        s.apply(Update::Gpu(reading(60.0)));
        assert_eq!(s.gpu_util.len(), 2);
        assert_eq!(s.gpu_mem.last(), Some(25.0));
        assert_eq!(s.gpu_temp.last(), Some(50.0));
        s.apply(Update::Gpu(GpuState::NotAvailable));
        assert_eq!(s.gpu_util.len(), 2, "no reading adds nothing");
    }

    #[test]
    fn an_unchanged_voice_view_does_not_redraw() {
        let mut s = AppState::new();
        let v = VoiceView::unavailable();
        assert!(!s.apply(Update::Voice(v)).voice);
        let mut w = v;
        w.state = VoiceState::Idle;
        assert!(s.apply(Update::Voice(w)).voice);
    }

    #[test]
    fn chat_events_flow_into_the_conversation() {
        let mut s = AppState::new();
        assert!(s.conversation.begin("hello").is_some());
        let c = s.apply(Update::Chat(ChatEvent::Chunk("Hi".into())));
        assert!(c.chat);
        assert_eq!(s.conversation.turns()[0].answer(), "Hi");
    }

    #[test]
    fn chat_mode_defaults_to_laila_and_tracks_changes() {
        let mut s = AppState::new();
        assert_eq!(s.chat_mode, ChatMode::Laila);
        assert!(!s.set_chat_mode(ChatMode::Laila));
        assert!(s.set_chat_mode(ChatMode::Assistant));
        assert_eq!(s.chat_mode, ChatMode::Assistant);
    }

    #[test]
    fn ledgers_report_zero_only_when_the_ledger_was_read() {
        let mut l = Ledgers::default();
        assert_eq!(l.egress_bytes_out(), None, "unknown is not zero");
        l.egress = Some(Ok(vec![]));
        assert_eq!(l.egress_bytes_out(), Some(0));
        assert_eq!(l.egress_requests(), Some(0));
        l.egress = Some(Err("down".into()));
        assert_eq!(l.egress_bytes_out(), None);
    }
}
