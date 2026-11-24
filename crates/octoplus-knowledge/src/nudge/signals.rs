//! From C3's health signals to evaluator readings and to the user's context (FR-NUD-01, 05).
//!
//! Pure: the clock and the time zone come in as arguments. Nothing here sees a window title,
//! a file name or any evidence text: `HealthSignals` carries numbers and flags only, and this
//! module reads only numbers and flags from it (R0-6, FR-NUD-04, S-3 of the plan).

use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{ChargeState, HealthSignals, MonitorEvent};

use super::types::{Reading, Signal, UserContext};

const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;
const GIB: f64 = 1_073_741_824.0;

/// What the monitor's recent samples say, and whether they are recent at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// One reading per measurable signal and key.
    pub readings: Vec<Reading>,
    /// No sample is newer than `stale_after_s`: the monitor is paused or gone, so nothing
    /// sample-based is reported and the user context treats it as "paused" (plan decision 5).
    pub monitor_stale: bool,
}

/// Turns the latest samples into readings. `peers_degraded` is how many of C5's own peers
/// (C4, C6) are unreachable; C3 being unreachable makes the engine idle instead.
#[must_use]
pub fn build_readings(
    signals: &HealthSignals,
    now_ns: u64,
    stale_after_s: u64,
    peers_degraded: u32,
) -> Built {
    let newest = signals
        .samples
        .iter()
        .map(|e| e.observed_at_ns)
        .max()
        .unwrap_or(0);
    let monitor_stale = newest == 0 || now_ns.saturating_sub(newest) > stale_after_s * NS_PER_S;

    let mut readings = Vec::new();
    if !monitor_stale {
        let latest = |pick: &dyn Fn(&MonitorEvent) -> bool| -> Vec<&MonitorEvent> {
            let mut v: Vec<&MonitorEvent> = signals.samples.iter().filter(|e| pick(e)).collect();
            v.sort_by_key(|e| e.observed_at_ns);
            v
        };

        // Disk: the newest sample per mount point.
        let mut disks: Vec<(&str, f64, f64)> = Vec::new();
        for e in latest(&|e| matches!(e.event, Some(Event::Disk(_)))) {
            if let Some(Event::Disk(d)) = &e.event
                && d.total_bytes > 0
            {
                let used = 100.0 * d.used_bytes as f64 / d.total_bytes as f64;
                let free_gib = d.free_bytes as f64 / GIB;
                disks.retain(|(m, _, _)| *m != d.mount_point);
                disks.push((d.mount_point.as_str(), used, free_gib));
            }
        }
        for (mount, used, free_gib) in disks {
            readings.push(Reading {
                signal: Signal::DiskUsedPercent,
                key: mount.to_string(),
                value: used,
                extra: free_gib,
            });
        }

        // Battery: only while discharging and off mains power.
        if let Some(Event::Battery(b)) = latest(&|e| matches!(e.event, Some(Event::Battery(_))))
            .last()
            .and_then(|e| e.event.as_ref())
            && !b.on_ac
            && b.state == ChargeState::Discharging as i32
        {
            readings.push(Reading {
                signal: Signal::BatteryPercent,
                key: String::new(),
                value: b.percent,
                extra: 0.0,
            });
        }

        // Temperature: the hottest of the newest sample per sensor.
        let mut sensors: Vec<(&str, f64)> = Vec::new();
        for e in latest(&|e| matches!(e.event, Some(Event::Thermal(_)))) {
            if let Some(Event::Thermal(t)) = &e.event {
                sensors.retain(|(s, _)| *s != t.sensor);
                sensors.push((t.sensor.as_str(), t.celsius));
            }
        }
        if let Some(hot) = sensors
            .iter()
            .map(|(_, c)| *c)
            .filter(|c| c.is_finite())
            .reduce(f64::max)
        {
            readings.push(Reading {
                signal: Signal::TemperatureC,
                key: String::new(),
                value: hot,
                extra: 0.0,
            });
        }

        // Memory: available as a percent of the total.
        if let Some(Event::Resources(r)) = latest(&|e| matches!(e.event, Some(Event::Resources(_))))
            .last()
            .and_then(|e| e.event.as_ref())
            && r.mem_total_bytes > 0
        {
            readings.push(Reading {
                signal: Signal::MemoryAvailablePercent,
                key: String::new(),
                value: 100.0 * r.mem_available_bytes as f64 / r.mem_total_bytes as f64,
                extra: 0.0,
            });
        }
    }

    // Minutes of continuous activity; 0 while idle (which re-arms the break reminder).
    let active_minutes = if signals.idle || signals.active_since_ns == 0 {
        0.0
    } else {
        (now_ns.saturating_sub(signals.active_since_ns) / NS_PER_MIN) as f64
    };
    readings.push(Reading {
        signal: Signal::ActiveMinutes,
        key: String::new(),
        value: active_minutes,
        extra: 0.0,
    });
    readings.push(Reading {
        signal: Signal::PartsDegraded,
        key: String::new(),
        value: f64::from(peers_degraded),
        extra: 0.0,
    });
    Built {
        readings,
        monitor_stale,
    }
}

/// True when `minute` (minutes after local midnight) is inside `[start, end)`, which may wrap
/// midnight (22:00 to 08:00). `start == end` means no quiet hours.
#[must_use]
pub const fn in_quiet_hours(minute: u32, start: u32, end: u32) -> bool {
    if start == end {
        false
    } else if start < end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    }
}

/// Minutes after local midnight for an instant in the given time zone.
#[must_use]
pub fn local_minute(now_ns: u64, tz: &jiff::tz::TimeZone) -> u32 {
    let secs = i64::try_from(now_ns / NS_PER_S).unwrap_or(0);
    let offset = jiff::Timestamp::from_second(secs)
        .map(|t| tz.to_offset(t).seconds())
        .unwrap_or(0);
    let local = secs.saturating_add(i64::from(offset));
    u32::try_from(local.rem_euclid(86_400) / 60).unwrap_or(0)
}

/// Everything the policy gate needs to know about the moment.
#[derive(Debug, Clone, Copy)]
pub struct ContextInputs {
    /// What C3 says about idle, fullscreen and media.
    pub idle: bool,
    /// A fullscreen window is focused.
    pub fullscreen: bool,
    /// Media is playing.
    pub media_playing: bool,
    /// The screen is locked (only meaningful when `locked_known`).
    pub locked: bool,
    /// Whether C3 can tell (false in v1.0, ADR-0011).
    pub locked_known: bool,
    /// The monitor's data is stale (see [`Built::monitor_stale`]).
    pub monitor_stale: bool,
    /// C5 itself dispatched a pause that has not ended.
    pub own_pause_active: bool,
    /// A turn is in progress.
    pub in_turn: bool,
    /// Another app is using the microphone (Phase 7; false until then).
    pub mic_busy: bool,
    /// Inside quiet hours.
    pub quiet_hours: bool,
    /// Speech output exists (Phase 7).
    pub voice_available: bool,
}

/// The policy gate's view of the moment (design.md §8.3).
#[must_use]
pub const fn build_context(i: ContextInputs) -> UserContext {
    UserContext {
        paused: i.own_pause_active || i.monitor_stale,
        quiet_hours: i.quiet_hours,
        in_turn: i.in_turn,
        mic_busy: i.mic_busy,
        media_playing: i.media_playing,
        fullscreen: i.fullscreen,
        away: i.idle || (i.locked_known && i.locked),
        voice_available: i.voice_available,
    }
}
