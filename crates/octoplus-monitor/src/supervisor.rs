//! Runs a sensor and keeps it running: a failure marks only that sensor DEGRADED and the
//! sensor restarts after jittered backoff (AB-10). One dead sensor never blinds the rest.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use octoplus_health::Health;
use octoplus_ipc::backoff::{Backoff, jitter_seed};
use octoplus_proto::v1::HealthStatus;
use tokio::sync::watch;

use crate::bus::Bus;
use crate::clock::Clock;
use crate::sensor::{Sensor, SensorOut};

/// Resolves when the stop signal is set (or its sender is gone). Returns `()` so the
/// non-`Send` watch guard never lives across an await in a caller's `select!`.
pub async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// Runs `sensor` until `stop` turns true.
pub async fn supervise<S: Sensor>(
    mut sensor: S,
    bus: Arc<Bus>,
    clock: Arc<dyn Clock>,
    health: Arc<Health>,
    mut stop: watch::Receiver<bool>,
) {
    let name = sensor.name();
    let out = SensorOut::new(name, bus, clock, Arc::clone(&health));
    let restarts = health.counter(&format!("restarts.{name}"));
    health.set(name, HealthStatus::Starting, "starting");
    let mut backoff = Backoff::default();
    loop {
        let result = tokio::select! {
            r = sensor.run(&out) => r,
            () = stopped(&mut stop) => return,
        };
        let detail = match &result {
            Err(e) => e.detail(),
            Ok(()) => "sensor ended unexpectedly",
        };
        // A sensor that worked before it failed has earned a fresh, short backoff.
        if out.take_worked() {
            backoff.reset();
        }
        health.set(name, HealthStatus::Degraded, detail);
        restarts.fetch_add(1, Ordering::Relaxed);
        let delay = backoff.next_delay(jitter_seed());
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = stopped(&mut stop) => return,
        }
    }
}
