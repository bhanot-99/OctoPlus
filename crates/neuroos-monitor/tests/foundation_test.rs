//! W1: bus, supervisor and stream handler (P3-S01 plumbing, AB-10/11/12).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use neuroos_health::Health;
use neuroos_ipc::server::ServerConfig;
use neuroos_ipc::{Client, Server};
use neuroos_monitor::StreamHandler;
use neuroos_monitor::bus::{Bus, PassGate};
use neuroos_monitor::clock::{Clock, FakeClock, SystemClock};
use neuroos_monitor::sensor::{Sensor, SensorError, SensorOut};
use neuroos_monitor::supervisor::supervise;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{HealthStatus, IdleChanged, MonitorEvent, MonitorSubscribe};

fn bus(cap: usize, clock: Arc<dyn Clock>) -> (Arc<Bus>, Arc<Health>) {
    let health = Health::new("neuroos-monitor", "test");
    (
        Bus::new(cap, Arc::new(PassGate), clock, Arc::clone(&health)),
        health,
    )
}

fn idle(ns: u64, idle: bool) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: ns,
        event: Some(Event::Idle(IdleChanged { idle })),
    }
}

#[test]
fn slow_subscriber_loses_oldest_and_counts_it() {
    let (bus, health) = bus(3, Arc::new(FakeClock::new(1_000)));
    let sub = bus.subscribe();
    for i in 0..5 {
        bus.publish(idle(i, true));
    }
    assert_eq!(sub.dropped(), 2);
    let got: Vec<u64> = std::iter::from_fn(|| sub.try_next())
        .map(|e| e.observed_at_ns)
        .collect();
    assert_eq!(got, vec![2, 3, 4], "the newest three survive, in order");
    assert_eq!(health.counter("events_dropped").load(Ordering::Relaxed), 2);
    assert_eq!(health.counter("events_emitted").load(Ordering::Relaxed), 5);
}

#[test]
fn each_subscriber_has_its_own_queue_and_drop_unsubscribes() {
    let (bus, health) = bus(2, Arc::new(FakeClock::new(0)));
    let a = bus.subscribe();
    let b = bus.subscribe();
    bus.publish(idle(1, true));
    assert!(a.try_next().is_some());
    assert!(b.try_next().is_some());
    drop(a);
    let snap = health.snapshot();
    let subs = snap
        .gauges
        .iter()
        .find(|g| g.name == "subscribers")
        .unwrap();
    assert!((subs.value - 1.0).abs() < f64::EPSILON);
    drop(b);
    bus.publish(idle(2, false)); // nobody listening: must not panic or block
}

struct Failing {
    runs: Arc<AtomicU32>,
    fail_first: u32,
}

impl Sensor for Failing {
    fn name(&self) -> &'static str {
        "fake"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            return Err(SensorError::Unavailable("fake source gone"));
        }
        out.emit(out.now_ns(), Event::Idle(IdleChanged { idle: true }));
        std::future::pending::<()>().await;
        Ok(())
    }
}

fn status_of(health: &Health, name: &str) -> HealthStatus {
    health
        .snapshot()
        .subsystems
        .iter()
        .find(|s| s.name == name)
        .unwrap()
        .status()
}

#[tokio::test(start_paused = true)]
async fn supervisor_restarts_a_failing_sensor_and_recovers_health() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let (bus, health) = bus(8, Arc::clone(&clock));
    let sub = bus.subscribe();
    let runs = Arc::new(AtomicU32::new(0));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(supervise(
        Failing {
            runs: Arc::clone(&runs),
            fail_first: 2,
        },
        bus,
        clock,
        Arc::clone(&health),
        stop_rx,
    ));
    // Two failures, two backoffs (each at most the doubling cap), then the third run emits.
    let ev = tokio::time::timeout(Duration::from_secs(5), sub.next())
        .await
        .unwrap();
    assert!(matches!(ev.event, Some(Event::Idle(_))));
    assert_eq!(runs.load(Ordering::SeqCst), 3);
    assert_eq!(status_of(&health, "fake"), HealthStatus::Ok);
    assert_eq!(health.counter("restarts.fake").load(Ordering::Relaxed), 2);
    stop_tx.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn failing_sensor_is_degraded_with_fixed_detail() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let (bus, health) = bus(8, Arc::clone(&clock));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(supervise(
        Failing {
            runs: Arc::new(AtomicU32::new(0)),
            fail_first: u32::MAX,
        },
        bus,
        clock,
        Arc::clone(&health),
        stop_rx,
    ));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(status_of(&health, "fake"), HealthStatus::Degraded);
    let snap = health.snapshot();
    let detail = &snap
        .subsystems
        .iter()
        .find(|s| s.name == "fake")
        .unwrap()
        .detail;
    assert_eq!(detail, "fake source gone");
    stop_tx.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn stream_delivers_events_in_order_and_ends_with_a_final_frame() {
    let (bus, _health) = bus(16, Arc::new(SystemClock));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handler = Arc::new(StreamHandler::new(Arc::clone(&bus), stop_rx.clone()));
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("stream");
    neuroos_ipc::server::ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("stream.sock");
    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let mut srv_stop = stop_rx;
    let serve = tokio::spawn(server.serve(handler, async move {
        let _ = srv_stop.wait_for(|s| *s).await;
    }));
    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();
    let mut stream = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    // The subscription exists once the handler runs; wait for it, no sleeping.
    for _ in 0..200 {
        if bus.subscriber_count() > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    for i in 1..=3 {
        bus.publish(idle(i, i % 2 == 1));
    }
    for i in 1..=3 {
        match stream.next().await.unwrap().unwrap() {
            Body::MonitorEvent(e) => assert_eq!(e.observed_at_ns, i),
            other => panic!("unexpected {other:?}"),
        }
    }
    stop_tx.send(true).unwrap();
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        Body::Ack(_)
    ));
    let _ = serve.await;
}

#[test]
fn a_late_subscriber_starts_with_the_latest_state_of_each_kind_in_time_order() {
    use neuroos_proto::v1::{DiskSample, FileActivity, FocusChanged, ProcessTree};
    let (bus, _h) = bus(16, Arc::new(FakeClock::new(1_000)));
    let ev = |t: u64, e: Event| MonitorEvent {
        observed_at_ns: t,
        event: Some(e),
    };
    let focus = |title: &str| {
        Event::Focus(FocusChanged {
            title: title.into(),
            ..Default::default()
        })
    };
    let disk = |m: &str| {
        Event::Disk(DiskSample {
            mount_point: m.into(),
            ..Default::default()
        })
    };
    bus.publish(ev(1, focus("old")));
    bus.publish(ev(2, disk("/")));
    bus.publish(ev(3, disk("/boot")));
    bus.publish(ev(4, focus("new")));
    bus.publish(ev(5, Event::ProcessTree(ProcessTree::default())));
    bus.publish(ev(6, Event::File(FileActivity::default())));
    bus.publish(ev(7, Event::Idle(IdleChanged { idle: true })));
    let late = bus.subscribe();
    let got: Vec<u64> = std::iter::from_fn(|| late.try_next())
        .map(|e| e.observed_at_ns)
        .collect();
    assert_eq!(
        got,
        vec![2, 3, 4, 7],
        "one per state key, newest of each, oldest first; trees and files are not state"
    );
}

#[test]
fn the_snapshot_is_not_handed_out_while_paused_and_is_bounded_by_the_queue() {
    use neuroos_monitor::exclusion::Exclusion;
    use neuroos_monitor::privacy::Privacy;
    let clock = Arc::new(FakeClock::new(1));
    let health = Health::new("neuroos-monitor", "test");
    let privacy = Privacy::new(
        Arc::clone(&clock) as Arc<dyn Clock>,
        Exclusion::default(),
        Arc::clone(&health),
    );
    let bus = Bus::new(
        2,
        Arc::clone(&privacy) as Arc<dyn neuroos_monitor::bus::Gate>,
        clock as Arc<dyn Clock>,
        health,
    );
    for i in 1..=4u64 {
        bus.publish(MonitorEvent {
            observed_at_ns: i,
            event: Some(Event::Disk(neuroos_proto::v1::DiskSample {
                mount_point: format!("/m{i}"),
                ..Default::default()
            })),
        });
    }
    let sub = bus.subscribe();
    let got: Vec<u64> = std::iter::from_fn(|| sub.try_next())
        .map(|e| e.observed_at_ns)
        .collect();
    assert_eq!(got, vec![3, 4], "a small queue gets the newest states");
    privacy.pause(0);
    let during = bus.subscribe();
    assert!(
        during.try_next().is_none(),
        "nothing is handed out during a pause"
    );
}

/// FI: a subscriber that never reads must not slow the sensors, must not grow memory without
/// bound, and must still get the newest events in order when it starts reading again.
#[tokio::test]
async fn a_stalled_socket_subscriber_never_blocks_publishing_and_loses_only_old_events() {
    let (bus, health) = bus(32, Arc::new(SystemClock));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handler = Arc::new(StreamHandler::new(Arc::clone(&bus), stop_rx.clone()));
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("stream");
    neuroos_ipc::server::ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("stream.sock");
    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(30)),
    )
    .await
    .unwrap();
    let mut srv_stop = stop_rx;
    let serve = tokio::spawn(server.serve(handler, async move {
        let _ = srv_stop.wait_for(|s| *s).await;
    }));
    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();
    let mut stream = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    while bus.subscriber_count() == 0 {
        tokio::task::yield_now().await;
    }
    // The client is not reading. Publish far more than any queue can hold.
    let started = std::time::Instant::now();
    for i in 1..=20_000u64 {
        bus.publish(idle(i, i % 2 == 0));
        if i % 1000 == 0 {
            tokio::task::yield_now().await; // let the handler move events along
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "publishing never waits for a reader"
    );
    assert!(
        health.counter("events_dropped").load(Ordering::Relaxed) > 0,
        "the loss is counted, not silent"
    );
    // The IPC layer cuts a reader that falls too far behind with a retryable error (ADR-0003);
    // what arrived before that is in order.
    let mut last = 0;
    let mut ended_with = String::from("silence");
    loop {
        match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            Ok(Some(Ok(Body::MonitorEvent(e)))) => {
                assert!(e.observed_at_ns > last, "order is kept");
                last = e.observed_at_ns;
            }
            Ok(other) => {
                ended_with = format!("{other:?}");
                break;
            }
            Err(_) => break,
        }
    }
    assert!(
        ended_with.contains("ResourceExhausted") && ended_with.contains("retryable: true"),
        "a stalled reader is cut loose with a retryable error, got {ended_with}"
    );
    // Reconnecting works at once and starts from the latest state, so nothing important is lost.
    let mut again = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    let Some(Ok(Body::MonitorEvent(first))) =
        tokio::time::timeout(Duration::from_secs(2), again.next())
            .await
            .unwrap()
    else {
        panic!("a snapshot event was expected on reconnect");
    };
    assert_eq!(
        first.observed_at_ns, 20_000,
        "the reconnect snapshot is the newest state"
    );
    stop_tx.send(true).unwrap();
    let _ = serve.await;
}
