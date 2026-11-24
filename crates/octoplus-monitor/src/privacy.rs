//! Pause and exclusion: the only thing that may stop or change an event (AB-5, FR-MON-10,
//! FR-PRV-01/02). It is the bus's [`Gate`]; no sensor knows it exists.
//!
//! Rules (ADR-0011): while paused nothing is published, including samples; an excluded
//! window becomes a redacted focus event so its predecessor's span ends without leaking
//! anything; a process tree is dropped while an excluded window is focused; a process name
//! on the exclusion list is blanked in samples. Resuming asks the sensors for a snapshot.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use octoplus_health::Health;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{FocusChanged, MonitorEvent, PrivacyState};
use tokio::sync::watch;

use crate::bus::Gate;
use crate::clock::Clock;
use crate::exclusion::Exclusion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pause {
    Running,
    /// `until` is a monotonic deadline; `None` means until resumed.
    Paused {
        until: Option<Instant>,
    },
}

/// Pause state plus the exclusion list.
pub struct Privacy {
    clock: Arc<dyn Clock>,
    exclusion: Exclusion,
    health: Arc<Health>,
    pause: Mutex<Pause>,
    /// Bumped on every pause and resume (see [`Gate::state_epoch`]).
    epoch: AtomicU64,
    /// True while the last focus event was an excluded window.
    excluded_focus: AtomicBool,
    /// Bumped on every resume; sensors that hold state re-emit it when it changes.
    snapshot_tx: watch::Sender<u64>,
}

impl std::fmt::Debug for Privacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Privacy").finish_non_exhaustive()
    }
}

/// The event that replaces an excluded window's focus: it ends the previous span and
/// carries nothing about the window.
#[must_use]
pub fn redacted_focus() -> FocusChanged {
    FocusChanged {
        app_id: String::new(),
        title: String::new(),
        pid: 0,
        fullscreen: false,
        fullscreen_known: false,
    }
}

impl Privacy {
    /// New state, running.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, exclusion: Exclusion, health: Arc<Health>) -> Arc<Self> {
        health.gauge("paused", 0.0);
        let (snapshot_tx, _rx) = watch::channel(0);
        Arc::new(Self {
            clock,
            exclusion,
            health,
            pause: Mutex::new(Pause::Running),
            epoch: AtomicU64::new(0),
            excluded_focus: AtomicBool::new(false),
            snapshot_tx,
        })
    }

    fn lock_pause(&self) -> std::sync::MutexGuard<'_, Pause> {
        self.pause.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sensors watch this; a changed value means "re-emit your current state" (resume).
    #[must_use]
    pub fn snapshot_requests(&self) -> watch::Receiver<u64> {
        self.snapshot_tx.subscribe()
    }

    /// Pauses for `duration_ns` (0 = until resumed). A later request replaces an earlier one.
    pub fn pause(&self, duration_ns: u64) -> PrivacyState {
        let until = if duration_ns == 0 {
            None
        } else {
            // An unrepresentable deadline pauses until resumed: the private direction.
            self.clock
                .mono()
                .checked_add(Duration::from_nanos(duration_ns))
        };
        *self.lock_pause() = Pause::Paused { until };
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.health.gauge("paused", 1.0);
        self.state()
    }

    /// Resumes and asks the sensors for a snapshot.
    pub fn resume(&self) -> PrivacyState {
        let was_paused = {
            let mut p = self.lock_pause();
            let was = *p != Pause::Running;
            *p = Pause::Running;
            was
        };
        if was_paused {
            self.epoch.fetch_add(1, Ordering::AcqRel);
            self.health.gauge("paused", 0.0);
            self.snapshot_tx.send_modify(|g| *g = g.wrapping_add(1));
        }
        self.state()
    }

    /// Ends a timed pause whose deadline has passed. Safe to call at any time and by a
    /// stale timer: it does nothing unless the deadline is really over.
    pub fn expire_if_due(&self) {
        let due = matches!(*self.lock_pause(), Pause::Paused { until: Some(u) } if u <= self.clock.mono());
        if due {
            self.resume();
        }
    }

    /// Current state; the deadline is derived from the monotonic timer, so a wall-clock
    /// jump cannot move it.
    #[must_use]
    pub fn state(&self) -> PrivacyState {
        let now = self.clock.mono();
        match *self.lock_pause() {
            Pause::Running => PrivacyState {
                paused: false,
                paused_until_ns: 0,
            },
            Pause::Paused { until: None } => PrivacyState {
                paused: true,
                paused_until_ns: 0,
            },
            // A deadline that has passed is not a pause, even before the expiry check runs.
            Pause::Paused { until: Some(u) } if u <= now => PrivacyState {
                paused: false,
                paused_until_ns: 0,
            },
            Pause::Paused { until: Some(u) } => {
                let left = u.saturating_duration_since(now);
                let left_ns = u64::try_from(left.as_nanos()).unwrap_or(u64::MAX);
                PrivacyState {
                    paused: true,
                    paused_until_ns: self.clock.now_ns().saturating_add(left_ns),
                }
            }
        }
    }

    fn is_paused(&self) -> bool {
        self.expire_if_due();
        *self.lock_pause() != Pause::Running
    }
}

impl Gate for Privacy {
    fn snapshot_allowed(&self) -> bool {
        !self.is_paused()
    }

    fn state_epoch(&self) -> u64 {
        // A timed pause that has run out ends here too, so the epoch moves before it is read.
        self.expire_if_due();
        self.epoch.load(Ordering::Acquire)
    }

    fn admit(&self, mut event: MonitorEvent) -> Option<MonitorEvent> {
        // Exclusion state follows focus even while paused: a window focused during a pause
        // must still hide the process tree that arrives right after resume.
        if let Some(Event::Focus(f)) = event.event.as_mut() {
            let excluded = self.exclusion.is_excluded(&f.app_id, &f.title);
            self.excluded_focus.store(excluded, Ordering::Release);
            if excluded {
                *f = redacted_focus();
            }
        }
        if self.is_paused() {
            return None;
        }
        match event.event.as_mut()? {
            Event::ProcessTree(_) if self.excluded_focus.load(Ordering::Acquire) => return None,
            Event::Resources(r) if self.exclusion.matches_app(&r.top_process) => {
                r.top_process.clear();
                r.top_process_rss_bytes = 0;
            }
            _ => {}
        }
        Some(event)
    }
}
