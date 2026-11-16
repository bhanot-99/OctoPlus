//! Battery state (FR-MON-06, P3-S03; ADR-0007 decision 1: `/sys` only, no UPower). The
//! time estimate is reported only when the hardware gives a real rate; it is never guessed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use neuroos_common::config::MonitorConfig;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{BatterySample, ChargeState};
use tokio::time::MissedTickBehavior;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// How often the sensor looks for a change between the forced re-emits (FR-MON-06 says
/// "emit on change and at least every `battery_interval_s`").
pub const CHANGE_POLL: Duration = Duration::from_secs(5);

/// What one `BAT*` folder says. Energy is in µWh, charge in µAh, rate in µW / µA.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    /// `capacity`, percent.
    pub capacity: Option<f64>,
    /// `status` text.
    pub status: Option<String>,
    /// `energy_now`.
    pub energy_now: Option<u64>,
    /// `energy_full`.
    pub energy_full: Option<u64>,
    /// `charge_now`.
    pub charge_now: Option<u64>,
    /// `charge_full`.
    pub charge_full: Option<u64>,
    /// `power_now`.
    pub power_now: Option<u64>,
    /// `current_now`.
    pub current_now: Option<u64>,
}

/// Parses an unsigned sysfs number; anything else is "missing".
#[must_use]
pub fn parse_u64(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// Maps the kernel's `status` word. Anything unknown stays unspecified.
#[must_use]
pub fn parse_status(text: &str) -> ChargeState {
    match text.trim() {
        "Charging" => ChargeState::Charging,
        "Discharging" => ChargeState::Discharging,
        "Full" => ChargeState::Full,
        "Not charging" => ChargeState::NotCharging,
        _ => ChargeState::Unspecified,
    }
}

fn sum(mut values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values.try_fold(0u64, |a, v| v.map(|v| a.saturating_add(v)))
}

#[allow(clippy::cast_precision_loss)] // µWh/µAh counts are far below 2^52
fn ratio_percent(now: u64, full: u64) -> Option<f64> {
    (full > 0).then(|| (now as f64 / full as f64 * 100.0).clamp(0.0, 100.0))
}

/// Combines the batteries and the mains state into one sample, or `None` with no battery.
/// `mains_online` is `None` when no mains supply exists.
#[must_use]
pub fn combine(batteries: &[Reading], mains_online: Option<bool>) -> Option<BatterySample> {
    if batteries.is_empty() {
        return None;
    }
    let energy = sum(batteries.iter().map(|b| b.energy_now))
        .zip(sum(batteries.iter().map(|b| b.energy_full)));
    let charge = sum(batteries.iter().map(|b| b.charge_now))
        .zip(sum(batteries.iter().map(|b| b.charge_full)));
    let (now_full, rate) = match (energy, charge) {
        (Some(e), _) if e.1 > 0 => (Some(e), sum(batteries.iter().map(|b| b.power_now))),
        (_, Some(c)) if c.1 > 0 => (Some(c), sum(batteries.iter().map(|b| b.current_now))),
        _ => (None, None),
    };
    let caps: Vec<f64> = batteries.iter().filter_map(|b| b.capacity).collect();
    let percent = now_full
        .and_then(|(n, f)| ratio_percent(n, f))
        .or_else(|| (!caps.is_empty()).then(|| caps.iter().sum::<f64>() / caps.len() as f64))?;

    let states: Vec<ChargeState> = batteries
        .iter()
        .map(|b| {
            b.status
                .as_deref()
                .map_or(ChargeState::Unspecified, parse_status)
        })
        .collect();
    let any = |s: ChargeState| states.contains(&s);
    let state = if any(ChargeState::Charging) {
        ChargeState::Charging
    } else if any(ChargeState::Discharging) {
        ChargeState::Discharging
    } else if states.iter().all(|s| *s == ChargeState::Full) {
        ChargeState::Full
    } else if any(ChargeState::NotCharging) {
        ChargeState::NotCharging
    } else {
        ChargeState::Unspecified
    };

    let estimate = match (now_full, rate, state) {
        (Some((now, _)), Some(r), ChargeState::Discharging) if r > 0 => Some(secs(now, r)),
        (Some((now, full)), Some(r), ChargeState::Charging) if r > 0 => {
            Some(secs(full.saturating_sub(now), r))
        }
        _ => None,
    };
    Some(BatterySample {
        percent,
        state: state as i32,
        time_estimate_s: estimate.unwrap_or(0),
        time_estimate_known: estimate.is_some(),
        on_ac: mains_online.unwrap_or(state != ChargeState::Discharging),
    })
}

/// Seconds to use up `amount` at `rate` (both in the same micro-unit per hour).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn secs(amount: u64, rate: u64) -> u64 {
    (amount as f64 / rate as f64 * 3600.0).min(u64::MAX as f64) as u64
}

fn read(dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(file)).ok()
}

/// Reads every `BAT*` and the mains supply under `power_supply`.
#[must_use]
pub fn read_supplies(power_supply: &Path) -> (Vec<Reading>, Option<bool>) {
    let mut batteries = Vec::new();
    let mut mains: Option<bool> = None;
    let Ok(dir) = std::fs::read_dir(power_supply) else {
        return (batteries, mains);
    };
    let mut entries: Vec<_> = dir.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if name.starts_with("BAT") {
            let num = |f: &str| read(&p, f).and_then(|t| parse_u64(&t));
            batteries.push(Reading {
                capacity: num("capacity").map(|c| c as f64),
                status: read(&p, "status").map(|s| s.trim().to_owned()),
                energy_now: num("energy_now"),
                energy_full: num("energy_full"),
                charge_now: num("charge_now"),
                charge_full: num("charge_full"),
                power_now: num("power_now"),
                current_now: num("current_now"),
            });
        } else if ["AC", "ADP", "ACAD", "Mains"]
            .iter()
            .any(|n| name.starts_with(n))
        {
            let online = read(&p, "online")
                .and_then(|t| parse_u64(&t))
                .map(|v| v == 1);
            mains = Some(mains.unwrap_or(false) || online.unwrap_or(false));
        }
    }
    (batteries, mains)
}

fn key(s: &BatterySample) -> (i64, i32, bool) {
    #[allow(clippy::cast_possible_truncation)]
    (s.percent.round() as i64, s.state, s.on_ac)
}

/// Emits on change and at least every `battery_interval_s`.
#[derive(Debug)]
pub struct BatterySensor {
    power_supply: PathBuf,
    interval: Duration,
}

impl BatterySensor {
    /// The real `/sys/class/power_supply`.
    #[must_use]
    pub fn new(cfg: &MonitorConfig) -> Self {
        Self::with_root(cfg, Path::new("/"))
    }

    /// A tree rooted at `root` (`<root>/sys/class/power_supply`), for tests.
    #[must_use]
    pub fn with_root(cfg: &MonitorConfig, root: &Path) -> Self {
        Self {
            power_supply: root.join("sys/class/power_supply"),
            interval: Duration::from_secs(cfg.battery_interval_s),
        }
    }
}

impl Sensor for BatterySensor {
    fn name(&self) -> &'static str {
        "battery"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let mut tick = tokio::time::interval(CHANGE_POLL.min(self.interval));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // tokio's clock, like the interval above, so the forced re-emit follows the same time.
        let mut last: Option<((i64, i32, bool), tokio::time::Instant)> = None;
        loop {
            tick.tick().await;
            let (batteries, mains) = read_supplies(&self.power_supply);
            out.set_ok(); // a desktop with no battery works; it just has nothing to say
            let Some(sample) = combine(&batteries, mains) else {
                continue;
            };
            let now = tokio::time::Instant::now();
            let due = last
                .is_none_or(|(k, at)| k != key(&sample) || now.duration_since(at) >= self.interval);
            if due {
                last = Some((key(&sample), now));
                out.emit(out.now_ns(), Event::Battery(sample));
            }
        }
    }
}
