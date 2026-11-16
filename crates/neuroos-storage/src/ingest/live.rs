//! What C3 knows *right now* but has not yet written down: the open focus segment, whether the
//! user is idle, whether media is playing, and a bounded ring of recent health samples. It feeds
//! `QueryFocusHistory` (an open window is not in `focus_history` yet) and `QueryHealthSignals`
//! (FR-STO-07, FR-STO-09). Pure: it only looks at events.

use std::collections::{HashMap, VecDeque};

use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{MonitorEvent, PlaybackState};

/// Recent health events kept (about an hour and a half of C1's default intervals).
pub const RING_CAP: usize = 2048;

/// The window the user is in now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// Application id.
    pub app_id: String,
    /// Title.
    pub title: String,
    /// When it took focus.
    pub start_ns: u64,
}

/// The live picture.
#[derive(Debug, Default)]
pub struct LiveState {
    /// The focused window, if any is observable.
    pub window: Option<OpenWindow>,
    /// Idle now.
    pub idle: bool,
    /// Fullscreen, when the compositor says so.
    pub fullscreen: bool,
    /// Since when the user has been active (0 while idle or unknown).
    pub active_since_ns: u64,
    playing: HashMap<String, bool>,
    ring: VecDeque<MonitorEvent>,
}

impl LiveState {
    /// Updates from one C1 event.
    pub fn apply(&mut self, ev: &MonitorEvent) {
        let at = ev.observed_at_ns;
        match &ev.event {
            Some(Event::Focus(f)) => {
                if f.app_id.is_empty() && f.title.is_empty() {
                    self.window = None;
                    self.fullscreen = false;
                } else {
                    self.window = Some(OpenWindow {
                        app_id: f.app_id.clone(),
                        title: f.title.clone(),
                        start_ns: at,
                    });
                    self.fullscreen = f.fullscreen_known && f.fullscreen;
                }
                if self.active_since_ns == 0 && !self.idle {
                    self.active_since_ns = at;
                }
            }
            Some(Event::Idle(i)) => {
                self.idle = i.idle;
                self.active_since_ns = if i.idle { 0 } else { at };
            }
            Some(Event::Media(m)) => {
                let playing = PlaybackState::try_from(m.state) == Ok(PlaybackState::Playing);
                self.playing.insert(m.player.to_ascii_lowercase(), playing);
            }
            Some(Event::Resources(_) | Event::Disk(_) | Event::Battery(_) | Event::Thermal(_)) => {
                if self.ring.len() >= RING_CAP {
                    self.ring.pop_front();
                }
                self.ring.push_back(ev.clone());
            }
            _ => {}
        }
    }

    /// Whether any player is playing.
    #[must_use]
    pub fn media_playing(&self) -> bool {
        self.playing.values().any(|p| *p)
    }

    /// Health events at or after `since_ns`, oldest first.
    #[must_use]
    pub fn samples_since(&self, since_ns: u64) -> Vec<MonitorEvent> {
        self.ring
            .iter()
            .filter(|e| e.observed_at_ns >= since_ns)
            .cloned()
            .collect()
    }
}

impl LiveState {
    /// Forgets everything (purge).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
