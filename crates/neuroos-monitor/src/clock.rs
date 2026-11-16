//! Time source. Everything time-dependent takes a [`Clock`] so tests are exact (AB-13).

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// UTC wall time for event stamps (AB-8) and a monotonic reading for timers.
pub trait Clock: Send + Sync + 'static {
    /// UTC epoch nanoseconds, for boundary timestamps only.
    fn now_ns(&self) -> u64;
    /// Monotonic instant, for durations and pause timers (wall-clock jumps must not matter).
    fn mono(&self) -> Instant;
}

/// The real clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ns(&self) -> u64 {
        // A clock before 1970 has no u64 form; 0 is visibly wrong rather than a panic.
        neuroos_common::time::now_ns().unwrap_or(0)
    }

    fn mono(&self) -> Instant {
        Instant::now()
    }
}

/// A clock tests move by hand.
#[derive(Debug)]
pub struct FakeClock {
    base: Instant,
    state: Mutex<(u64, Duration)>,
}

impl FakeClock {
    /// Starts at `start_ns` UTC.
    #[must_use]
    pub fn new(start_ns: u64) -> Self {
        Self {
            base: Instant::now(),
            state: Mutex::new((start_ns, Duration::ZERO)),
        }
    }

    /// Moves both readings forward.
    pub fn advance(&self, by: Duration) {
        let mut g = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.0 =
            g.0.saturating_add(u64::try_from(by.as_nanos()).unwrap_or(u64::MAX));
        g.1 += by;
    }

    /// Moves only the UTC reading (a wall-clock jump); the monotonic one stays put.
    pub fn jump_wall(&self, to_ns: u64) {
        let mut g = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.0 = to_ns;
    }
}

impl Clock for FakeClock {
    fn now_ns(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
    }

    fn mono(&self) -> Instant {
        self.base
            + self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .1
    }
}
