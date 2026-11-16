#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `privacy.pause` as a kernel skill against a fake C1 `monitor/control`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, Server, StreamSink};
use neuroos_kernel::ControlSkills;
use neuroos_kernel::dispatch::KernelSkills;
use neuroos_proto::v1::PrivacyState;
use neuroos_proto::v1::envelope::Body;
use serde_json::json;

#[derive(Debug, Default)]
struct FakeMonitor {
    last_ns: AtomicU64,
}

impl Handler for FakeMonitor {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::PauseRequest(p) => {
                self.last_ns.store(p.duration_ns, Ordering::SeqCst);
                Ok(Body::PrivacyState(PrivacyState {
                    paused: true,
                    paused_until_ns: 0,
                }))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

#[tokio::test]
async fn privacy_pause_sends_one_validated_message_to_c1() {
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("control");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("control.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(1)))
        .await
        .unwrap();
    let monitor = Arc::new(FakeMonitor::default());
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(server.serve(Arc::clone(&monitor), async move {
        let _ = stop_rx.wait_for(|s| *s).await;
    }));

    let skills = ControlSkills::new(
        Arc::new(ReconnectingClient::new(&sock, 1 << 20)),
        Duration::from_secs(1),
    );
    let v = skills
        .run("privacy.pause", &json!({"minutes": 30}))
        .await
        .unwrap();
    assert_eq!(v, json!({"paused": true, "minutes": 30}));
    assert_eq!(
        monitor.last_ns.load(Ordering::SeqCst),
        30 * 60 * 1_000_000_000
    );

    // Re-checked even though the schema already did (fail closed on its own).
    for bad in [
        json!({"minutes": 0}),
        json!({"minutes": 481}),
        json!({}),
        json!({"minutes": "5"}),
    ] {
        assert_eq!(
            skills.run("privacy.pause", &bad).await,
            Err("bad_args".into()),
            "{bad}"
        );
    }
    assert_eq!(
        skills.run("system.cleanup", &json!({})).await,
        Err("unknown_kernel_skill".into())
    );

    let _ = stop_tx.send(true);
    let _ = handle.await;
    // C1 gone: unavailable, never a silent success.
    let down = ControlSkills::new(
        Arc::new(ReconnectingClient::new(
            &dir.path().join("none.sock"),
            1 << 20,
        )),
        Duration::from_millis(200),
    );
    assert_eq!(
        down.run("privacy.pause", &json!({"minutes": 5})).await,
        Err("monitor_unavailable".into())
    );
}
