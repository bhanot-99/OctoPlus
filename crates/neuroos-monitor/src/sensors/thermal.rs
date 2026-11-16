//! CPU package temperature (FR-MON-07, P3-S03; ADR-0007: `/sys/class/hwmon`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use neuroos_common::config::MonitorConfig;
use neuroos_proto::v1::ThermalSample;
use neuroos_proto::v1::monitor_event::Event;
use tokio::time::MissedTickBehavior;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// Readings outside this range are sensor glitches (e.g. −128 °C after resume), not data.
const PLAUSIBLE_C: std::ops::RangeInclusive<f64> = -40.0..=150.0;

/// One `hwmon*` folder: its driver name and its `temp*_input` files with optional labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwmon {
    /// Driver name from the `name` file.
    pub name: String,
    /// `(path of temp*_input, label)`.
    pub temps: Vec<(PathBuf, Option<String>)>,
}

/// Which driver, in order of preference, and which labels count as the package sensor.
/// An empty label list means "the first input".
const PREFERENCE: &[(&str, &[&str])] = &[
    ("k10temp", &["Tctl", "Tdie"]),
    ("coretemp", &["Package id 0"]),
    ("zenpower", &[]),
    ("cpu_thermal", &[]),
    ("acpitz", &[]),
];

/// The CPU package sensor among `hwmons`: `(driver, input file)`.
#[must_use]
pub fn pick_sensor(hwmons: &[Hwmon]) -> Option<(String, PathBuf)> {
    for (driver, labels) in PREFERENCE {
        for h in hwmons.iter().filter(|h| h.name == *driver) {
            let hit = if labels.is_empty() {
                h.temps.first()
            } else {
                labels
                    .iter()
                    .find_map(|l| h.temps.iter().find(|(_, lab)| lab.as_deref() == Some(*l)))
            };
            if let Some((path, _)) = hit {
                return Some((h.name.clone(), path.clone()));
            }
        }
    }
    None
}

/// Millidegrees (the sysfs unit) to °C, or `None` for junk or an implausible value.
#[must_use]
pub fn parse_millidegrees(text: &str) -> Option<f64> {
    let milli: i64 = text.trim().parse().ok()?;
    #[allow(clippy::cast_precision_loss)]
    let c = milli as f64 / 1000.0;
    PLAUSIBLE_C.contains(&c).then_some(c)
}

/// Lists every `hwmon*` under `hwmon_dir`.
#[must_use]
pub fn discover(hwmon_dir: &Path) -> Vec<Hwmon> {
    let Ok(dir) = std::fs::read_dir(hwmon_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = dir.filter_map(Result::ok).map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|d| {
            let name = std::fs::read_to_string(d.join("name"))
                .ok()?
                .trim()
                .to_owned();
            let mut temps: Vec<(PathBuf, Option<String>)> = std::fs::read_dir(&d)
                .ok()?
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let f = e.file_name().to_string_lossy().into_owned();
                    let n = f.strip_prefix("temp")?.strip_suffix("_input")?.to_owned();
                    let label = std::fs::read_to_string(d.join(format!("temp{n}_label")))
                        .ok()
                        .map(|l| l.trim().to_owned());
                    Some((e.path(), label))
                })
                .collect();
            temps.sort();
            Some(Hwmon { name, temps })
        })
        .collect()
}

/// Samples the package temperature every `thermal_interval_s`.
#[derive(Debug)]
pub struct ThermalSensor {
    hwmon_dir: PathBuf,
    interval: Duration,
}

impl ThermalSensor {
    /// The real `/sys/class/hwmon`.
    #[must_use]
    pub fn new(cfg: &MonitorConfig) -> Self {
        Self::with_root(cfg, Path::new("/"))
    }

    /// A tree rooted at `root` (`<root>/sys/class/hwmon`), for tests.
    #[must_use]
    pub fn with_root(cfg: &MonitorConfig, root: &Path) -> Self {
        Self {
            hwmon_dir: root.join("sys/class/hwmon"),
            interval: Duration::from_secs(cfg.thermal_interval_s),
        }
    }
}

impl Sensor for ThermalSensor {
    fn name(&self) -> &'static str {
        "thermal"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let mut tick = tokio::time::interval(self.interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // hwmon numbers can change after suspend, so the sensor is found again whenever a read fails.
        let mut found: Option<(String, PathBuf)> = None;
        loop {
            tick.tick().await;
            out.set_ok(); // no CPU sensor is not an error: nothing to report
            if found.is_none() {
                found = pick_sensor(&discover(&self.hwmon_dir));
            }
            let Some((driver, path)) = found.clone() else {
                continue;
            };
            let reading = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| parse_millidegrees(&t));
            match reading {
                Some(celsius) => out.emit(
                    out.now_ns(),
                    Event::Thermal(ThermalSample {
                        sensor: driver,
                        celsius,
                    }),
                ),
                None => found = None,
            }
        }
    }
}
