#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, Ping, QueryHybrid};
use neuroos_storage::StorageHandler;

#[tokio::test]
async fn test_storage_hybrid_query_returns_evidence_with_taint() {
    let handler = Arc::new(StorageHandler::new());
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("storage");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("storage.sock");

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

    // QueryHybrid
    let res = client
        .call(
            Body::QueryHybrid(QueryHybrid {
                text: "test search".into(),
                top_k: 5,
                min_score: 0.0,
                domains: Vec::new(),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::HybridResult(res) => {
            assert!(!res.evidence.is_empty(), "evidence should not be empty");
            let ev = &res.evidence[0];
            assert!(ev.taint.is_some(), "evidence must have non-empty taint");
            let t = ev.taint.as_ref().unwrap();
            assert_ne!(t.flags, 0, "taint flags must be non-zero");
        }
        other => panic!("unexpected body {other:?}"),
    }

    // Invalid body
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
async fn test_storage_service_init_and_run() {
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
            neuroos_common::paths::Part::Storage,
            "neuroos-storage",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(neuroos_storage::init)
    });

    let storage_sock = runtime_dir.join("storage/storage/storage.sock");
    for _ in 0..50 {
        if storage_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(storage_sock.exists());
    let client = Client::connect(&storage_sock, 1024 * 1024).await.unwrap();
    let res = client
        .call(
            Body::QueryHybrid(QueryHybrid {
                text: "test".into(),
                top_k: 1,
                min_score: 0.0,
                domains: Vec::new(),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    assert!(matches!(res, Body::HybridResult(_)));

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}
