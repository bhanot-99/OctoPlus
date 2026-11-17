#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! KPI-10 over recorded events: the committed monitor dumps are replayed through the same signal
//! builder, context builder and pure core the live engine uses, with voice available and rules
//! loosened so something fires all the time. A spoken nudge at a bad moment fails the test.

use std::collections::BTreeMap;
use std::path::Path;

use neuroos_knowledge::nudge::core::Core;
use neuroos_knowledge::nudge::eval::EvalConfig;
use neuroos_knowledge::nudge::rules;
use neuroos_knowledge::nudge::signals::{ContextInputs, build_context, build_readings};
use neuroos_knowledge::nudge::types::{Action, Delivery, PolicyConfig, Reason};
use neuroos_proto::dump::DumpReader;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{HealthSignals, MonitorEvent, PlaybackState};

const S: u64 = 1_000_000_000;

/// Rules loosened so every default rule fires on ordinary values and again soon after.
const LOOSE: &str = r#"
[rule.disk_nearly_full]
fire_at = 1
rearm = 0.5
hold_for = "0s"
cooldown = "60s"
[rule.running_hot]
fire_at = 1
rearm = 0.5
hold_for = "0s"
cooldown = "60s"
[rule.memory_pressure]
fire_at = 100
rearm = 100
hold_for = "0s"
cooldown = "60s"
[rule.battery_low]
fire_at = 100
rearm = 100
hold_for = "0s"
cooldown = "60s"
[rule.long_session]
fire_at = 1
rearm = 0.5
hold_for = "0s"
cooldown = "60s"
"#;

fn load(name: &str) -> Vec<MonitorEvent> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monitor-dumps")
        .join(name);
    let mut r =
        DumpReader::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).unwrap();
    let mut v = Vec::new();
    while let Some(e) = r.next_event().unwrap() {
        v.push(e);
    }
    v.sort_by_key(|e| e.observed_at_ns);
    v
}

#[derive(Default, Debug)]
struct Tally {
    ticks: u32,
    bad_ticks: u32,
    spoken: u32,
    notified: u32,
    held: u32,
    media_notifies: u32,
}

fn replay(events: &[MonitorEvent]) -> Tally {
    let start = events.first().unwrap().observed_at_ns;
    let end = events.last().unwrap().observed_at_ns;
    let mut core = Core::new(
        rules::load(Some(LOOSE)).unwrap(),
        EvalConfig { max_gap_s: 300 },
        PolicyConfig {
            quiet_mode: false,
            deliver_info: Delivery::Notify,
            deliver_important: Delivery::Voice,
            deliver_urgent: Delivery::Voice,
            max_spoken_per_hour: 3,
            max_spoken_per_day: 8,
            max_held: 16,
        },
    );
    let (mut idle, mut fullscreen, mut active_since) = (false, false, start);
    let mut players: BTreeMap<String, bool> = BTreeMap::new();
    let mut next = 0usize;
    let mut tally = Tally::default();
    let mut t = start + 60 * S;
    while t <= end {
        // Bring the state machine up to `t`.
        while next < events.len() && events[next].observed_at_ns <= t {
            match &events[next].event {
                Some(Event::Idle(i)) => {
                    if idle && !i.idle {
                        active_since = events[next].observed_at_ns;
                    }
                    idle = i.idle;
                }
                Some(Event::Focus(f)) => fullscreen = f.fullscreen,
                Some(Event::Media(m)) => {
                    players.insert(m.player.clone(), m.state == PlaybackState::Playing as i32);
                }
                _ => {}
            }
            next += 1;
        }
        let media = players.values().any(|p| *p);
        let window: Vec<MonitorEvent> = events
            .iter()
            .filter(|e| e.observed_at_ns + 120 * S > t && e.observed_at_ns <= t)
            .cloned()
            .collect();
        let sig = HealthSignals {
            samples: window,
            idle,
            locked: false,
            fullscreen,
            media_playing: media,
            active_since_ns: active_since,
            locked_known: false,
        };
        let built = build_readings(&sig, t, 180, 0);
        let ctx = build_context(ContextInputs {
            idle,
            fullscreen,
            media_playing: media,
            locked: false,
            locked_known: false,
            monitor_stale: built.monitor_stale,
            own_pause_active: false,
            in_turn: false,
            mic_busy: false,
            quiet_hours: false,
            voice_available: true,
        });
        let bad = ctx.paused || ctx.away || ctx.media_playing || ctx.fullscreen;
        tally.ticks += 1;
        tally.bad_ticks += u32::from(bad);
        let out = core.step(t, &built.readings, &ctx);
        for d in out.decisions {
            match d.action {
                Action::Speak => {
                    tally.spoken += 1;
                    assert!(
                        !bad,
                        "SPOKEN at a bad moment (t = {} s): {ctx:?}",
                        (t - start) / S
                    );
                }
                Action::Notify { .. } => {
                    tally.notified += 1;
                    tally.media_notifies += u32::from(d.reason == Reason::Media);
                }
                Action::Hold => tally.held += 1,
                Action::Drop => {}
            }
        }
        t += 30 * S;
    }
    tally
}

#[test]
fn no_nudge_is_spoken_at_a_bad_moment_across_the_recorded_dumps() {
    for n in 1..=3 {
        let events = load(&format!("synthetic-{n}.dump"));
        let t = replay(&events);
        println!("dump {n}: {t:?}");
        // The replay is not vacuous: it ran, it hit bad moments, nudges were spoken at good ones
        // and held or downgraded at the others.
        assert!(t.ticks > 100, "dump {n}: {t:?}");
        assert!(t.bad_ticks > 0, "dump {n} never had a bad moment: {t:?}");
        assert!(t.spoken > 0, "dump {n} never spoke: {t:?}");
        assert!(t.held + t.notified > 0, "dump {n}: {t:?}");
    }
}

#[test]
fn idle_counts_as_a_bad_moment_for_the_replay_check() {
    // A gate that ignores `away` (as a broken policy might) speaks while idle; the same replay
    // loop's assertion is what fails. Shown by checking the loop's own predicate on a bad context.
    let ctx = build_context(ContextInputs {
        idle: true,
        fullscreen: false,
        media_playing: false,
        locked: false,
        locked_known: false,
        monitor_stale: false,
        own_pause_active: false,
        in_turn: false,
        mic_busy: false,
        quiet_hours: false,
        voice_available: true,
    });
    assert!(ctx.away, "idle must count as a bad moment");
}
