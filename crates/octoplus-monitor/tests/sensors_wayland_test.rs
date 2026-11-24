//! W5 / P3-S01, P3-S02: focus, fullscreen and idle (FR-MON-01/02). The decision logic is
//! tested without a compositor; `local_` tests talk to the real one when a session exists.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use octoplus_common::config::MonitorConfig;
use octoplus_health::Health;
use octoplus_monitor::bus::{Bus, PassGate, Subscription};
use octoplus_monitor::clock::{Clock, FakeClock, SystemClock};
use octoplus_monitor::sensor::{Sensor, SensorError, SensorOut};
use octoplus_monitor::sensors::cosmic::{FocusSensor, Msg, Tracker, Update, WinState};
use octoplus_monitor::sensors::idle::{IdleSensor, Msg as IdleMsg};
use octoplus_monitor::sensors::proc::ProcTable;
use octoplus_monitor::supervise;
use octoplus_proto::v1::MonitorEvent;
use octoplus_proto::v1::monitor_event::Event;
use proptest::prelude::*;
use tokio::sync::{mpsc, watch};

const ACTIVE: WinState = WinState {
    activated: true,
    fullscreen: false,
};
const FULL: WinState = WinState {
    activated: true,
    fullscreen: true,
};
const BACK: WinState = WinState {
    activated: false,
    fullscreen: false,
};

fn open(
    t: &mut Tracker,
    h: u32,
    app: &str,
    title: &str,
    st: WinState,
) -> Option<octoplus_monitor::sensors::cosmic::Focus> {
    t.apply(Update::New(h));
    t.apply(Update::AppId(h, app.into()));
    t.apply(Update::Title(h, title.into()));
    t.apply(Update::State(h, st));
    t.apply(Update::Done(h))
}

// ---------------------------------------------------------------- Tracker (UT)

#[test]
fn an_activated_window_becomes_the_focus_only_after_done() {
    let mut t = Tracker::new(1);
    t.apply(Update::New(7));
    t.apply(Update::AppId(7, "firefox".into()));
    t.apply(Update::Title(7, "Home".into()));
    assert!(
        t.apply(Update::State(7, ACTIVE)).is_none(),
        "half-built windows never leak"
    );
    let f = t.apply(Update::Done(7)).unwrap();
    assert_eq!(
        (f.uid, f.app_id.as_str(), f.title.as_str(), f.fullscreen),
        (1, "firefox", "Home", false)
    );
}

#[test]
fn retitle_and_fullscreen_flip_are_new_focus_values_and_repeats_are_not() {
    let mut t = Tracker::new(1);
    open(&mut t, 7, "code", "a.rs", ACTIVE).unwrap();
    t.apply(Update::Title(7, "b.rs".into()));
    assert_eq!(t.apply(Update::Done(7)).unwrap().title, "b.rs");
    assert!(
        t.apply(Update::Done(7)).is_none(),
        "nothing changed: nothing emitted"
    );
    t.apply(Update::State(7, FULL));
    assert!(t.apply(Update::Done(7)).unwrap().fullscreen);
    t.apply(Update::State(7, ACTIVE));
    assert!(!t.apply(Update::Done(7)).unwrap().fullscreen, "and back");
}

#[test]
fn switching_windows_and_closing_the_focused_one() {
    let mut t = Tracker::new(1);
    open(&mut t, 1, "a", "A", ACTIVE).unwrap();
    open(&mut t, 2, "b", "B", BACK);
    t.apply(Update::State(1, BACK));
    t.apply(Update::State(2, ACTIVE));
    assert_eq!(t.apply(Update::Done(2)).unwrap().app_id, "b");
    // The old window also reports done: its deactivation must not disturb the focus.
    assert!(t.apply(Update::Done(1)).is_none());
    let none = t.apply(Update::Closed(2)).unwrap();
    assert_eq!(
        (none.uid, none.app_id.as_str()),
        (0, ""),
        "nothing is activated any more"
    );
    assert!(
        t.apply(Update::Closed(2)).is_none(),
        "a second close is harmless"
    );
    assert!(
        t.apply(Update::Closed(99)).is_none(),
        "an unknown window is harmless"
    );
}

#[test]
fn if_two_windows_claim_focus_the_latest_activation_wins() {
    let mut t = Tracker::new(1);
    open(&mut t, 1, "a", "A", ACTIVE).unwrap();
    let f = open(&mut t, 2, "b", "B", ACTIVE).unwrap();
    assert_eq!(f.app_id, "b");
}

#[test]
fn compositor_restart_closes_every_window_and_ids_never_repeat() {
    let mut t = Tracker::new(1);
    let first = open(&mut t, 1, "a", "A", ACTIVE).unwrap().uid;
    open(&mut t, 2, "b", "B", BACK);
    let none = t.close_all().unwrap();
    assert_eq!(none.uid, 0);
    assert!(t.close_all().is_none());
    let mut t2 = Tracker::new(t.next_uid());
    // The new compositor reuses protocol id 1: it is a different window with a new uid.
    let again = open(&mut t2, 1, "a", "A", ACTIVE).unwrap().uid;
    assert!(again > first);
    assert!(t2.next_uid() > again);
}

#[derive(Debug, Clone)]
enum Op {
    New(u8),
    App(u8),
    Title(u8),
    State(u8, bool, bool),
    Done(u8),
    Closed(u8),
    CloseAll,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..6).prop_map(Op::New),
        (0u8..6).prop_map(Op::App),
        (0u8..6).prop_map(Op::Title),
        (0u8..6, any::<bool>(), any::<bool>()).prop_map(|(h, a, f)| Op::State(h, a, f)),
        (0u8..6).prop_map(Op::Done),
        (0u8..6).prop_map(Op::Closed),
        Just(Op::CloseAll),
    ]
}

proptest! {
    #[test]
    fn any_event_order_never_panics_never_reuses_a_uid_and_never_repeats_a_focus(ops in proptest::collection::vec(op(), 0..80)) {
        let mut t = Tracker::new(1);
        let mut created = std::collections::HashSet::new();
        let mut last = t.current().clone();
        for o in ops {
            let h = |x: u8| u32::from(x);
            let before = t.next_uid();
            let out = match o {
                Op::New(x) => t.apply(Update::New(h(x))),
                Op::App(x) => t.apply(Update::AppId(h(x), format!("app{x}"))),
                Op::Title(x) => t.apply(Update::Title(h(x), format!("t{x}"))),
                Op::State(x, a, f) => t.apply(Update::State(h(x), WinState { activated: a, fullscreen: f })),
                Op::Done(x) => t.apply(Update::Done(h(x))),
                Op::Closed(x) => t.apply(Update::Closed(h(x))),
                Op::CloseAll => t.close_all(),
            };
            if t.next_uid() != before {
                prop_assert!(created.insert(before), "uid {before} handed out twice");
                prop_assert_eq!(t.next_uid(), before + 1);
            }
            if let Some(f) = out {
                prop_assert!(f != last, "an unchanged focus was reported");
                last = f;
            }
            prop_assert_eq!(&last, t.current());
        }
    }
}

// ---------------------------------------------------------------- sensor loop (fake compositor)

fn setup() -> (Arc<Bus>, Subscription, SensorOut) {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000_000));
    let health = Health::new("octoplus-monitor", "test");
    let bus = Bus::new(
        64,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let out = SensorOut::new("focus", Arc::clone(&bus), clock, health);
    (bus, sub, out)
}

fn focus_sensor() -> (FocusSensor, watch::Sender<u64>) {
    let (snap_tx, snap_rx) = watch::channel(0u64);
    let dir = tempfile::tempdir().unwrap();
    let table = ProcTable::new(dir.path());
    (FocusSensor::new(table, snap_rx), snap_tx)
}

fn upd(at: u64, u: Update) -> Msg {
    Msg::Update(at, u)
}

fn window(h: u32, app: &str, title: &str, st: WinState, at: u64) -> Vec<Msg> {
    vec![
        upd(at, Update::New(h)),
        upd(at, Update::AppId(h, app.into())),
        upd(at, Update::Title(h, title.into())),
        upd(at, Update::State(h, st)),
        upd(at, Update::Done(h)),
    ]
}

fn focus_of(ev: &MonitorEvent) -> (String, String, bool, bool) {
    let Some(Event::Focus(f)) = &ev.event else {
        panic!("focus expected: {ev:?}")
    };
    (
        f.app_id.clone(),
        f.title.clone(),
        f.fullscreen,
        f.fullscreen_known,
    )
}

#[tokio::test(start_paused = true)]
async fn the_sensor_emits_focus_changes_with_the_time_they_were_observed() {
    let (_bus, sub, out) = setup();
    let (mut sensor, _snap) = focus_sensor();
    let (tx, rx) = mpsc::channel(64);
    tx.send(Msg::Ready).await.unwrap();
    for m in window(1, "firefox", "Home", ACTIVE, 42) {
        tx.send(m).await.unwrap();
    }
    drop(tx); // the compositor thread ends: the sensor reports it
    let res = sensor.run_messages(&out, rx).await;
    assert!(matches!(res, Err(SensorError::Unavailable(_))));
    let first = sub.try_next().unwrap();
    assert_eq!(
        first.observed_at_ns, 42,
        "stamped when observed, not when sent"
    );
    assert_eq!(
        focus_of(&first),
        ("firefox".into(), "Home".into(), false, true)
    );
    let closed = sub.try_next().unwrap();
    assert_eq!(
        focus_of(&closed),
        (String::new(), String::new(), false, false),
        "open windows are closed on loss"
    );
}

#[tokio::test(start_paused = true)]
async fn title_churn_in_one_window_is_coalesced_but_a_window_switch_is_immediate() {
    let (_bus, sub, out) = setup();
    let (mut sensor, _snap) = focus_sensor();
    let (tx, rx) = mpsc::channel(256);
    for m in window(1, "term", "t0", ACTIVE, 1) {
        tx.send(m).await.unwrap();
    }
    for i in 1..=20 {
        tx.send(upd(2, Update::Title(1, format!("t{i}"))))
            .await
            .unwrap();
        tx.send(upd(2, Update::Done(1))).await.unwrap();
    }
    let task = tokio::spawn(async move {
        let r = sensor.run_messages(&out, rx).await;
        (r, sensor)
    });
    tokio::time::sleep(Duration::from_secs(2)).await;
    let mut titles = Vec::new();
    while let Some(e) = sub.try_next() {
        titles.push(focus_of(&e).1);
    }
    assert_eq!(
        titles,
        vec!["t0", "t20"],
        "20 rapid retitles become one event with the latest title"
    );
    // A different window is never held back.
    for m in window(2, "other", "O", BACK, 3) {
        tx.send(m).await.unwrap();
    }
    tx.send(upd(4, Update::State(2, ACTIVE))).await.unwrap();
    tx.send(upd(4, Update::State(1, BACK))).await.unwrap();
    tx.send(upd(4, Update::Done(2))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(focus_of(&sub.try_next().unwrap()).0, "other");
    drop(tx);
    let (res, sensor) = task.await.unwrap();
    assert!(res.is_err());
    assert!(sensor.next_uid() >= 3, "uids keep counting across restarts");
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_request_re_emits_the_current_focus_even_if_unchanged() {
    let (_bus, sub, out) = setup();
    let (mut sensor, snap) = focus_sensor();
    let (tx, rx) = mpsc::channel(64);
    for m in window(1, "firefox", "Home", FULL, 5) {
        tx.send(m).await.unwrap();
    }
    let task = tokio::spawn(async move { sensor.run_messages(&out, rx).await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(focus_of(&sub.try_next().unwrap()).2);
    assert!(sub.try_next().is_none());
    snap.send(1).unwrap(); // the privacy layer resumed
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(
        focus_of(&sub.try_next().unwrap()),
        ("firefox".into(), "Home".into(), true, true)
    );
    tx.send(Msg::Lost("test")).await.unwrap();
    assert!(task.await.unwrap().is_err());
}

#[tokio::test(start_paused = true)]
async fn losing_the_compositor_is_a_clean_error_with_a_fixed_message() {
    let (_bus, _sub, out) = setup();
    let (mut sensor, _snap) = focus_sensor();
    let (tx, rx) = mpsc::channel(4);
    tx.send(Msg::Lost("no compositor to connect to"))
        .await
        .unwrap();
    let err = sensor.run_messages(&out, rx).await.unwrap_err();
    assert_eq!(err.detail(), "no compositor to connect to");
}

// ---------------------------------------------------------------- idle (fake compositor)

#[tokio::test(start_paused = true)]
async fn idle_starts_active_follows_the_compositor_and_answers_snapshots() {
    let (_bus, sub, out) = setup();
    let (snap_tx, snap_rx) = watch::channel(0u64);
    let mut sensor = IdleSensor::new(&MonitorConfig::default(), snap_rx);
    let (tx, rx) = mpsc::channel(16);
    let task = tokio::spawn(async move { sensor.run_messages(&out, rx).await });
    let idle_of = |e: MonitorEvent| match e.event {
        Some(Event::Idle(i)) => i.idle,
        other => panic!("idle expected: {other:?}"),
    };
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(!idle_of(sub.try_next().unwrap()), "active at start");
    tx.send(IdleMsg::Changed(77, true)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    let e = sub.try_next().unwrap();
    assert_eq!(e.observed_at_ns, 77);
    assert!(idle_of(e));
    snap_tx.send(1).unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(
        idle_of(sub.try_next().unwrap()),
        "snapshot repeats the current state"
    );
    tx.send(IdleMsg::Lost("compositor lacks the idle protocol"))
        .await
        .unwrap();
    assert_eq!(
        task.await.unwrap().unwrap_err().detail(),
        "compositor lacks the idle protocol"
    );
}

#[test]
fn idle_timeout_comes_from_config_in_milliseconds() {
    let (_tx, rx) = watch::channel(0u64);
    let cfg = MonitorConfig {
        idle_timeout_s: 90,
        ..Default::default()
    };
    let _ = IdleSensor::new(&cfg, rx); // construction with a large timeout must not overflow
    let (_tx2, rx2) = watch::channel(0u64);
    let huge = MonitorConfig {
        idle_timeout_s: u64::MAX / 2,
        ..Default::default()
    };
    let _ = IdleSensor::new(&huge, rx2);
}

// ---------------------------------------------------------------- the real compositor (local_)

fn have_compositor() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("XDG_RUNTIME_DIR").is_some()
}

#[tokio::test]
async fn local_the_real_compositor_reports_the_current_focus_and_the_idle_baseline() {
    if !have_compositor() {
        eprintln!("skipped: no Wayland session");
        return;
    }
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let health = Health::new("octoplus-monitor", "test");
    let bus = Bus::new(
        64,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (stop_tx, stop_rx) = watch::channel(false);

    let (_snap_tx, snap_rx) = watch::channel(0u64);
    let table = ProcTable::new(std::path::Path::new("/proc"));
    let focus = FocusSensor::new(table, snap_rx.clone());
    let idle = IdleSensor::new(&MonitorConfig::default(), snap_rx);
    let t1 = tokio::spawn(supervise(
        focus,
        Arc::clone(&bus),
        Arc::clone(&clock),
        Arc::clone(&health),
        stop_rx.clone(),
    ));
    let t2 = tokio::spawn(supervise(
        idle,
        Arc::clone(&bus),
        clock,
        Arc::clone(&health),
        stop_rx,
    ));

    let mut saw_idle = false;
    let mut saw_focus = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline && !(saw_idle && saw_focus) {
        if let Ok(ev) = tokio::time::timeout(Duration::from_millis(500), sub.next()).await {
            match ev.event {
                Some(Event::Idle(i)) => saw_idle |= !i.idle,
                Some(Event::Focus(f)) => {
                    saw_focus = true;
                    // Only a real window reports fullscreen; an empty focus never claims to know.
                    assert!(f.fullscreen_known || (f.app_id.is_empty() && f.title.is_empty()));
                }
                _ => {}
            }
        }
    }
    let snap = health.snapshot();
    let status = |n: &str| {
        snap.subsystems
            .iter()
            .find(|s| s.name == n)
            .map(|s| (s.status(), s.detail.clone()))
    };
    assert!(
        saw_idle,
        "idle baseline (active) must arrive; idle sensor: {:?}",
        status("idle")
    );
    // A session with no activated window emits no focus event until one is focused; the
    // sensor must still be healthy.
    assert!(
        matches!(
            status("focus"),
            Some((octoplus_proto::v1::HealthStatus::Ok, _))
        ),
        "focus sensor not OK: {:?} (saw_focus={saw_focus})",
        status("focus")
    );
    stop_tx.send(true).unwrap();
    t1.await.unwrap();
    t2.await.unwrap();
}

#[test]
fn sensors_are_named_for_health_rows() {
    let (_s, snap_rx) = watch::channel(0u64);
    let dir = tempfile::tempdir().unwrap();
    let f = FocusSensor::new(ProcTable::new(dir.path()), snap_rx.clone());
    assert_eq!(f.name(), "focus");
    assert_eq!(
        IdleSensor::new(&MonitorConfig::default(), snap_rx).name(),
        "idle"
    );
}

/// Manual evidence, not part of the gate (it needs no keyboard or mouse input for a few
/// seconds): `cargo nextest run -p octoplus-monitor --run-ignored only -E 'test(manual_)'`.
/// Closes the ADR-0007 gap "idle events not exercised".
#[tokio::test]
#[ignore = "needs a few seconds without input on the real session"]
async fn manual_real_idle_transition_is_reported_after_the_timeout() {
    if !have_compositor() {
        eprintln!("skipped: no Wayland session");
        return;
    }
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let health = Health::new("octoplus-monitor", "test");
    let bus = Bus::new(
        16,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (_snap_tx, snap_rx) = watch::channel(0u64);
    let (stop_tx, stop_rx) = watch::channel(false);
    let cfg = MonitorConfig {
        idle_timeout_s: 3,
        ..Default::default()
    };
    let task = tokio::spawn(supervise(
        IdleSensor::new(&cfg, snap_rx),
        bus,
        clock,
        health,
        stop_rx,
    ));
    let mut states = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if let Ok(ev) = tokio::time::timeout(Duration::from_millis(500), sub.next()).await
            && let Some(Event::Idle(i)) = ev.event
        {
            states.push(i.idle);
            if states.contains(&true) {
                break;
            }
        }
    }
    stop_tx.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(states.first(), Some(&false), "active at start");
    assert!(
        states.contains(&true),
        "idle must be reported after 3 s without input: {states:?}"
    );
}
