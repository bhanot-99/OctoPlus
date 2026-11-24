#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use octoplus_common::paths::Part;
use octoplus_health::{CommonArgs, ServiceBuilder};
use octoplus_ipc::{Client, Handler, HandlerError, Request, StreamSink};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{Ping, Pong};

#[derive(Debug, Default)]
struct TestHandler;

impl Handler for TestHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::Ping(p) => Ok(Body::Pong(Pong { payload: p.payload })),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

#[tokio::test]
async fn test_service_builder_run_and_shutdown() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("octoplus");
    let config_file = temp_dir.path().join("config.toml");
    std::fs::write(&config_file, b"").unwrap();

    let args = CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(octoplus_ipc::peer::current_uid()),
        soak: false,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        ServiceBuilder::new(Part::Monitor, "octoplus-monitor-test", "0.1.0")
            .skip_restrict(true)
            .args(args)
            .external_stop(stop_rx)
            .run(|ctx| {
                // Register a handler on control socket (which allows ctl / any unit from same UID)
                ctx.spawn_server("control", Arc::new(TestHandler))?;
                ctx.health.set_ready();
                Ok(())
            })
    });

    let control_sock = runtime_dir.join("monitor/control/control.sock");
    for _ in 0..50 {
        if control_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        control_sock.exists(),
        "control socket must be created by service"
    );

    let client = Client::connect(&control_sock, 1024 * 1024).await.unwrap();
    let res = client
        .call(
            Body::Ping(Ping {
                payload: b"hello".to_vec(),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::Pong(p) => assert_eq!(p.payload, b"hello"),
        other => panic!("unexpected body {other:?}"),
    }

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}
