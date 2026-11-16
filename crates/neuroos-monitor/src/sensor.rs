//! The sensor contract (P3 step 1): a sensor observes and emits; it knows nothing about
//! privacy, the bus subscribers or other sensors.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neuroos_health::Health;
use neuroos_proto::v1::{HealthStatus, MonitorEvent, monitor_event::Event};

use crate::bus::Bus;
use crate::clock::Clock;

/// Why a sensor stopped. Messages are fixed text — never window titles, paths or tags (R0-10).
#[derive(Debug, thiserror::Error)]
pub enum SensorError {
    /// The source (a file, a socket, a protocol) is not there. Retryable.
    #[error("sensor source unavailable: {0}")]
    Unavailable(&'static str),
    /// Reading the source failed. Retryable.
    #[error("sensor i/o failed: {detail}")]
    Io {
        /// What was being read.
        detail: &'static str,
        /// The OS error.
        #[source]
        source: std::io::Error,
    },
    /// The source sent something the sensor cannot interpret. Retryable.
    #[error("sensor protocol error: {0}")]
    Protocol(&'static str),
}

impl SensorError {
    /// Fixed text for the health detail.
    #[must_use]
    pub fn detail(&self) -> &'static str {
        match self {
            Self::Unavailable(d) | Self::Protocol(d) => d,
            Self::Io { detail, .. } => detail,
        }
    }
}

/// What a sensor holds while it runs: a way to emit and to say "I am working".
#[derive(Clone)]
pub struct SensorOut {
    name: &'static str,
    bus: Arc<Bus>,
    clock: Arc<dyn Clock>,
    health: Arc<Health>,
    worked: Arc<AtomicBool>,
}

impl std::fmt::Debug for SensorOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SensorOut")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl SensorOut {
    /// Used by the supervisor; tests build one with [`SensorOut::for_test`].
    #[must_use]
    pub fn new(
        name: &'static str,
        bus: Arc<Bus>,
        clock: Arc<dyn Clock>,
        health: Arc<Health>,
    ) -> Self {
        Self {
            name,
            bus,
            clock,
            health,
            worked: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The clock sensors read for their own timing.
    #[must_use]
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Current UTC ns, to stamp an observation at the moment it is made (AB-8).
    #[must_use]
    pub fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    /// Says the sensor works even though it has nothing to emit (e.g. a desktop with no battery).
    pub fn set_ok(&self) {
        self.worked.store(true, Ordering::Release);
        self.health.set(self.name, HealthStatus::Ok, "ok");
    }

    /// Emits one raw observation made at `observed_at_ns`.
    pub fn emit(&self, observed_at_ns: u64, event: Event) {
        self.set_ok();
        self.health
            .counter(&format!("events.{}", self.name))
            .fetch_add(1, Ordering::Relaxed);
        self.bus.publish(MonitorEvent {
            observed_at_ns,
            event: Some(event),
        });
    }

    /// Sets a gauge in the part's health (e.g. how many players are watched).
    pub fn gauge(&self, name: &str, value: f64) {
        self.health.gauge(name, value);
    }

    /// Whether the sensor reported working since the last call (supervisor use).
    #[must_use]
    pub fn take_worked(&self) -> bool {
        self.worked.swap(false, Ordering::AcqRel)
    }
}

/// One source of observations. Runs until it fails or the monitor stops; the supervisor
/// restarts it after a failure with jittered backoff (AB-10).
pub trait Sensor: Send + 'static {
    /// Stable lowercase name: the health subsystem and counter suffix.
    fn name(&self) -> &'static str;

    /// Observe and emit through `out`. Return `Err` when the source is lost.
    /// Blocking work belongs on its own thread, never on this task (AB-11).
    fn run(&mut self, out: &SensorOut) -> impl Future<Output = Result<(), SensorError>> + Send;
}
