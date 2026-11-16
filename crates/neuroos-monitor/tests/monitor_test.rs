#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::server::ServerConfig;
use neuroos_ipc::{Client, Server};
use neuroos_monitor::{ControlHandler, StreamHandler};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, MonitorSubscribe, PauseRequest, Ping, ResumeRequest};

#[tokio::test]
async fn test_control_handler_pause_and_resume() {
    let handler = Arc::new(ControlHandler::new());
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("control");
    neuroos_ipc::server::ensure_private_dir(&sock_dir).unwrap();
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
            assert_eq!(state.paused_until_ns, 60_000_000_000);
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
    neuroos_ipc::server::ensure_private_dir(&sock_dir).unwrap();
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
    let runtime_dir = temp_dir.path().join("neuroos");
    let config_file = temp_dir.path().join("config.toml");
    std::fs::write(&config_file, b"").unwrap();

    let args = neuroos_health::CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(neuroos_ipc::peer::current_uid()),
        soak: false,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        neuroos_health::ServiceBuilder::new(
            neuroos_common::paths::Part::Monitor,
            "neuroos-monitor",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(neuroos_monitor::init)
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

fn test_bus() -> Arc<neuroos_monitor::bus::Bus> {
    neuroos_monitor::bus::Bus::new(
        16,
        Arc::new(neuroos_monitor::bus::PassGate),
        Arc::new(neuroos_monitor::clock::SystemClock),
        neuroos_health::Health::new("neuroos-monitor", "0.1.0"),
    )
}
