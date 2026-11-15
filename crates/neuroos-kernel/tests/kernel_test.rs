#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_kernel::KernelHandler;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, EvaluateRequest, Ping, Taint, Tier};

#[tokio::test]
async fn test_kernel_evaluation_and_taint_enforcement() {
    let handler = Arc::new(KernelHandler::new());
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("kernel");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("kernel.sock");

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

    // 1. Valid EvaluateRequest with context_taint
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "answer".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Safe as i32);
            assert!(res.allowed_without_dialog);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 2. EvaluateRequest with missing context_taint MUST be rejected (rules.md §6.4)
    let err = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "answer".into(),
                context_taint: None,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::InvalidRequest,
        "missing context_taint must be rejected with InvalidRequest"
    );

    // 3. Invalid body (Ping)
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
async fn test_kernel_service_init_and_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("neuroos");
    let config_file = temp_dir.path().join("config.toml");
    std::fs::write(&config_file, b"").unwrap();

    let args = neuroos_health::CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(neuroos_ipc::peer::current_uid()),
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        neuroos_health::ServiceBuilder::new(
            neuroos_common::paths::Part::Kernel,
            "neuroos-kernel",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(neuroos_kernel::init)
    });

    let kernel_sock = runtime_dir.join("kernel/kernel/kernel.sock");
    for _ in 0..50 {
        if kernel_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(kernel_sock.exists());
    let client = Client::connect(&kernel_sock, 1024 * 1024).await.unwrap();
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "answer".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    assert!(matches!(res, Body::EvaluateResult(_)));

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}
