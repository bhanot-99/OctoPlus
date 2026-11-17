#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Readings and context from C3's health signals (FR-NUD-01, FR-NUD-05).

use neuroos_knowledge::nudge::signals::{
    ContextInputs, build_context, build_readings, in_quiet_hours, local_minute,
};
use neuroos_knowledge::nudge::types::{Reading, Signal};
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{
    BatterySample, ChargeState, DiskSample, HealthSignals, MonitorEvent, ResourceSample,
    ThermalSample,
};

const S: u64 = 1_000_000_000;
const GIB: u64 = 1_073_741_824;

fn ev(at: u64, e: Event) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: at * S,
        event: Some(e),
    }
}

fn disk(at: u64, mount: &str, total_gib: u64, used_gib: u64) -> MonitorEvent {
    ev(
        at,
        Event::Disk(DiskSample {
            mount_point: mount.into(),
            total_bytes: total_gib * GIB,
            used_bytes: used_gib * GIB,
            free_bytes: (total_gib - used_gib) * GIB,
        }),
    )
}

fn battery(at: u64, percent: f64, state: ChargeState, on_ac: bool) -> MonitorEvent {
    ev(
        at,
        Event::Battery(BatterySample {
            percent,
            state: state as i32,
            time_estimate_s: 0,
            time_estimate_known: false,
            on_ac,
        }),
    )
}

fn signals(samples: Vec<MonitorEvent>) -> HealthSignals {
    HealthSignals {
        samples,
        idle: false,
        locked: false,
        fullscreen: false,
        media_playing: false,
        active_since_ns: 0,
        locked_known: false,
    }
}

fn find<'a>(r: &'a [Reading], s: Signal, key: &str) -> Option<&'a Reading> {
    r.iter().find(|x| x.signal == s && x.key == key)
}

#[test]
fn disk_battery_temperature_and_memory_become_readings() {
    let s = signals(vec![
        disk(100, "/", 100, 50),
        disk(160, "/", 100, 92), // newer sample of the same mount wins
        disk(160, "/home", 200, 20),
        battery(150, 12.0, ChargeState::Discharging, false),
        ev(
            150,
            Event::Thermal(ThermalSample {
                sensor: "cpu".into(),
                celsius: 71.0,
            }),
        ),
        ev(
            155,
            Event::Thermal(ThermalSample {
                sensor: "gpu".into(),
                celsius: 88.5,
            }),
        ),
        ev(
            150,
            Event::Resources(ResourceSample {
                cpu_percent: 10.0,
                mem_total_bytes: 16 * GIB,
                mem_available_bytes: GIB,
                top_process: "x".into(),
                top_process_rss_bytes: 1,
            }),
        ),
    ]);
    let b = build_readings(&s, 170 * S, 180, 0);
    assert!(!b.monitor_stale);
    let root = find(&b.readings, Signal::DiskUsedPercent, "/").unwrap();
    assert!((root.value - 92.0).abs() < 1e-9 && (root.extra - 8.0).abs() < 1e-9);
    let home = find(&b.readings, Signal::DiskUsedPercent, "/home").unwrap();
    assert!((home.value - 10.0).abs() < 1e-9);
    assert_eq!(
        find(&b.readings, Signal::BatteryPercent, "").unwrap().value,
        12.0
    );
    assert_eq!(
        find(&b.readings, Signal::TemperatureC, "").unwrap().value,
        88.5
    );
    let mem = find(&b.readings, Signal::MemoryAvailablePercent, "").unwrap();
    assert!((mem.value - 6.25).abs() < 1e-9);
}

#[test]
fn the_battery_reading_exists_only_while_discharging_off_mains() {
    for (state, on_ac, expect) in [
        (ChargeState::Discharging, false, true),
        (ChargeState::Discharging, true, false),
        (ChargeState::Charging, false, false),
        (ChargeState::Full, true, false),
    ] {
        let s = signals(vec![battery(100, 9.0, state, on_ac)]);
        let b = build_readings(&s, 110 * S, 180, 0);
        assert_eq!(
            find(&b.readings, Signal::BatteryPercent, "").is_some(),
            expect,
            "{state:?} {on_ac}"
        );
    }
}

#[test]
fn stale_or_missing_samples_mean_a_stale_monitor_and_no_sample_readings() {
    let s = signals(vec![disk(100, "/", 100, 99)]);
    let b = build_readings(&s, 100 * S + 181 * S, 180, 0);
    assert!(b.monitor_stale);
    assert!(find(&b.readings, Signal::DiskUsedPercent, "/").is_none());
    let b = build_readings(&signals(vec![]), 500 * S, 180, 0);
    assert!(b.monitor_stale);
    // Exactly at the limit is still fresh.
    let b = build_readings(&s, 100 * S + 180 * S, 180, 0);
    assert!(!b.monitor_stale);
}

#[test]
fn active_minutes_and_degraded_parts() {
    let mut s = signals(vec![disk(100, "/", 100, 10)]);
    s.active_since_ns = 10 * S;
    let b = build_readings(&s, (10 + 95 * 60) * S, 100_000, 2);
    assert_eq!(
        find(&b.readings, Signal::ActiveMinutes, "").unwrap().value,
        95.0
    );
    assert_eq!(
        find(&b.readings, Signal::PartsDegraded, "").unwrap().value,
        2.0
    );
    // Idle counts as zero activity, which re-arms the break reminder.
    s.idle = true;
    let b = build_readings(&s, (10 + 95 * 60) * S, 100_000, 0);
    assert_eq!(
        find(&b.readings, Signal::ActiveMinutes, "").unwrap().value,
        0.0
    );
    // A zero-total disk never divides by zero.
    let z = signals(vec![disk(100, "/z", 0, 0)]);
    assert!(
        find(
            &build_readings(&z, 101 * S, 180, 0).readings,
            Signal::DiskUsedPercent,
            "/z"
        )
        .is_none()
    );
}

#[test]
fn quiet_hours_wrap_midnight() {
    let (start, end) = (22 * 60, 8 * 60);
    for (minute, expect) in [
        (21 * 60 + 59, false),
        (22 * 60, true),
        (23 * 60 + 59, true),
        (0, true),
        (7 * 60 + 59, true),
        (8 * 60, false),
        (12 * 60, false),
    ] {
        assert_eq!(in_quiet_hours(minute, start, end), expect, "{minute}");
    }
    // Same-day window and the empty window.
    assert!(in_quiet_hours(13 * 60, 12 * 60, 14 * 60));
    assert!(!in_quiet_hours(14 * 60, 12 * 60, 14 * 60));
    assert!(!in_quiet_hours(5, 600, 600));
}

#[test]
fn the_local_minute_follows_the_time_zone_and_survives_extremes() {
    let utc = jiff::tz::TimeZone::UTC;
    let ist = jiff::tz::TimeZone::fixed(jiff::tz::Offset::from_seconds(19_800).unwrap());
    // 2027-01-01 00:00:00 UTC.
    let t = 1_798_761_600_u64 * S;
    assert_eq!(local_minute(t, &utc), 0);
    assert_eq!(local_minute(t + 90 * 60 * S, &utc), 90);
    // India: UTC+5:30, so midnight UTC is 05:30 there and 22:00 UTC is 03:30 the next day.
    assert_eq!(local_minute(t, &ist), 330);
    assert_eq!(local_minute(t - 2 * 3600 * S, &ist), 3 * 60 + 30);
    assert!(local_minute(u64::MAX, &utc) < 1440);
    assert!(local_minute(0, &utc) < 1440);
}

#[test]
fn context_maps_every_bad_moment() {
    let base = ContextInputs {
        idle: false,
        fullscreen: false,
        media_playing: false,
        locked: false,
        locked_known: false,
        monitor_stale: false,
        own_pause_active: false,
        in_turn: false,
        mic_busy: false,
        quiet_hours: false,
        voice_available: false,
    };
    let c = build_context(base);
    assert!(!c.paused && !c.away && !c.in_turn && !c.fullscreen && !c.media_playing);
    assert!(
        build_context(ContextInputs {
            own_pause_active: true,
            ..base
        })
        .paused
    );
    assert!(
        build_context(ContextInputs {
            monitor_stale: true,
            ..base
        })
        .paused
    );
    assert!(build_context(ContextInputs { idle: true, ..base }).away);
    // Lock state only counts when it is actually known (v1.0: unknown, ADR-0011).
    assert!(
        !build_context(ContextInputs {
            locked: true,
            locked_known: false,
            ..base
        })
        .away
    );
    assert!(
        build_context(ContextInputs {
            locked: true,
            locked_known: true,
            ..base
        })
        .away
    );
    assert!(
        build_context(ContextInputs {
            in_turn: true,
            ..base
        })
        .in_turn
    );
    assert!(
        build_context(ContextInputs {
            mic_busy: true,
            ..base
        })
        .mic_busy
    );
    assert!(
        build_context(ContextInputs {
            quiet_hours: true,
            ..base
        })
        .quiet_hours
    );
    assert!(
        build_context(ContextInputs {
            fullscreen: true,
            media_playing: true,
            voice_available: true,
            ..base
        })
        .voice_available
    );
}
