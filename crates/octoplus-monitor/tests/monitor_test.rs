#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use octoplus_ipc::server::ServerConfig;
use octoplus_ipc::{Client, Server};
use octoplus_monitor::{ControlHandler, StreamHandler};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{ErrorCode, MonitorSubscribe, PauseRequest, Ping, ResumeRequest};

#[tokio::test]
async fn test_control_handler_pause_and_resume() {
    let handler = Arc::new(ControlHandler::new(test_privacy()));
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("control");
    octoplus_ipc::server::ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("control.sock");

    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };
    let handle = tokio::spawn(server.serve(handler, wait(stop_rx)));

    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();

    // Pause request
    let res = client
        .call(
            Body::PauseRequest(PauseRequest {
                duration_ns: 60_000_000_000,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();
    match res {
        Body::PrivacyState(state) => {
            assert!(state.paused);
            // A timed pause reports a UTC deadline in the future, not the duration echoed back.
            assert!(state.paused_until_ns > octoplus_common::time::now_ns().unwrap());
        }
        other => panic!("unexpected body {other:?}"),
    }

    // Resume request
    let res = client
        .call(
            Body::ResumeRequest(ResumeRequest {}),
            Duration::from_millis(500),
        )
        .await
        .unwrap();
    match res {
        Body::PrivacyState(state) => {
            assert!(!state.paused);
            assert_eq!(state.paused_until_ns, 0);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // Invalid body (Ping)
    let err = client
        .call(
            Body::Ping(Ping {
                payload: b"ping".to_vec(),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::InvalidRequest
    );

    let _ = stop_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn test_stream_handler_subscribes_and_keeps_open() {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handler = Arc::new(StreamHandler::new(test_bus(), stop_rx));

    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("stream");
    octoplus_ipc::server::ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("stream.sock");

    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();

    let (srv_stop_tx, srv_stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };
    let handle = tokio::spawn(server.serve(handler, wait(srv_stop_rx)));

    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();

    // Call MonitorSubscribe with stream
    let mut stream = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    // Stream should remain open without sending frames yet
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger stop
    let _ = stop_tx.send(true);
    let item = stream.next().await.unwrap().unwrap();
    assert!(matches!(item, Body::Ack(_)));

    let _ = srv_stop_tx.send(true);
    let _ = handle.await;
}

#[tokio::test]
async fn test_monitor_service_init_and_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("octoplus");
    let config_file = temp_dir.path().join("config.toml");
    std::fs::write(&config_file, b"").unwrap();

    let args = octoplus_health::CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(octoplus_ipc::peer::current_uid()),
        soak: false,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        octoplus_health::ServiceBuilder::new(
            octoplus_common::paths::Part::Monitor,
            "octoplus-monitor",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(octoplus_monitor::init)
    });

    let control_sock = runtime_dir.join("monitor/control/control.sock");
    for _ in 0..50 {
        if control_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(control_sock.exists());
    let client = Client::connect(&control_sock, 1024 * 1024).await.unwrap();
    let res = client
        .call(
            Body::PauseRequest(PauseRequest { duration_ns: 10 }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    assert!(matches!(res, Body::PrivacyState(_)));

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}

fn test_bus() -> Arc<octoplus_monitor::bus::Bus> {
    octoplus_monitor::bus::Bus::new(
        16,
        Arc::new(octoplus_monitor::bus::PassGate),
        Arc::new(octoplus_monitor::clock::SystemClock),
        octoplus_health::Health::new("octoplus-monitor", "0.1.0"),
    )
}

fn test_privacy() -> Arc<octoplus_monitor::Privacy> {
    octoplus_monitor::Privacy::new(
        Arc::new(octoplus_monitor::clock::SystemClock),
        octoplus_monitor::exclusion::Exclusion::default(),
        octoplus_health::Health::new("octoplus-monitor", "0.1.0"),
    )
}

/// IT (local): every real sensor started the way the service starts them, a real subscriber
/// on a socket sees a disk sample (the disk sensor samples at once), and health has a row for
/// every sensor. (The peer-unit policy of the real `stream.sock` rejects a test process on
/// purpose, so the real-unit run is checked by `scripts/p3-live-check.sh`.)
#[tokio::test]
async fn local_all_sensors_stream_real_events_and_each_has_a_health_row() {
    use octoplus_monitor::bus::Bus;
    use octoplus_monitor::exclusion::Exclusion;
    let health = octoplus_health::Health::new("octoplus-monitor", "test");
    let clock: Arc<dyn octoplus_monitor::clock::Clock> =
        Arc::new(octoplus_monitor::clock::SystemClock);
    let privacy = octoplus_monitor::Privacy::new(
        Arc::clone(&clock),
        Exclusion::default(),
        Arc::clone(&health),
    );
    let bus = Bus::new(
        256,
        Arc::clone(&privacy) as Arc<dyn octoplus_monitor::bus::Gate>,
        clock,
        Arc::clone(&health),
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let cfg = octoplus_common::config::MonitorConfig::default();
    let handles = octoplus_monitor::spawn_sensors(&cfg, &bus, &privacy, &health, &stop_rx);

    let sub = bus.subscribe();
    let mut seen_disk = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline && !seen_disk {
        if let Ok(e) = tokio::time::timeout(Duration::from_millis(500), sub.next()).await {
            seen_disk |= matches!(
                e.event,
                Some(octoplus_proto::v1::monitor_event::Event::Disk(_))
            );
        }
    }
    assert!(seen_disk, "a real disk sample must arrive");
    let snap = health.snapshot();
    for name in [
        "resources",
        "disk",
        "battery",
        "thermal",
        "focus",
        "proc",
        "idle",
        "media",
        "folders",
    ] {
        assert!(
            snap.subsystems.iter().any(|s| s.name == name),
            "no health row for {name}"
        );
    }
    // A late subscriber starts with the latest state, so a reconnecting C3 is not blind.
    let late = bus.subscribe();
    assert!(late.try_next().is_some());
    stop_tx.send(true).unwrap();
    for h in handles {
        h.await.unwrap();
    }
}
