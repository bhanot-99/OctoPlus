//! The health endpoint over a real socket (P0-S04).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use neuroos_health::{Health, HealthHandler};
use neuroos_ipc::{Client, IpcError, Server, ServerConfig};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{HealthRequest, HealthStatus, Ping};

#[tokio::test(flavor = "multi_thread")]
async fn endpoint_reports_real_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("part").join("health.sock");
    let health = Health::new("neuroos-test", "0.1.0");
    let peer_up = Arc::new(AtomicBool::new(false));
    let p = Arc::clone(&peer_up);
    health.probe(
        "peer",
        Box::new(move || {
            if p.load(Ordering::Relaxed) {
                (HealthStatus::Ok, "")
            } else {
                (HealthStatus::Degraded, "peer unavailable")
            }
        }),
    );
    let server = Server::bind(
        &path,
        ServerConfig::new(64 * 1024, Duration::from_millis(250)),
    )
    .await
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(
        server.serve(Arc::new(HealthHandler(Arc::clone(&health))), async {
            let _ = rx.await;
        }),
    );
    let c = Client::connect(&path, 64 * 1024).await.unwrap();
    let ask = || async {
        match c
            .call(
                Body::HealthRequest(HealthRequest {}),
                Duration::from_millis(250),
            )
            .await
            .unwrap()
        {
            Body::HealthResponse(r) => r,
            other => panic!("{other:?}"),
        }
    };

    let r = ask().await;
    assert_eq!(r.status, i32::from(HealthStatus::Starting));
    health.set_ready();
    let r = ask().await;
    assert_eq!(r.status, i32::from(HealthStatus::Degraded), "peer is down");
    peer_up.store(true, Ordering::Relaxed);
    let r = ask().await;
    assert_eq!(r.status, i32::from(HealthStatus::Ok));
    assert!(r.rss_bytes > 1024 * 1024);

    // Anything but a HealthRequest is refused.
    let e = c
        .call(
            Body::Ping(Ping { payload: vec![] }),
            Duration::from_millis(250),
        )
        .await
        .unwrap_err();
    assert!(matches!(e, IpcError::Remote { .. }));

    let _ = tx.send(());
    task.await.unwrap();
}
