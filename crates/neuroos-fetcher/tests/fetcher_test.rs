#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_fetcher::FetcherHandler;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, FetchRequest, Ping};

#[tokio::test]
async fn test_fetcher_stubs_return_unsupported() {
    let handler = Arc::new(FetcherHandler::new());
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("fetch");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("fetch.sock");

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

    // 1. FetchRequest returns UnsupportedVersion in Phase 1
    let err = client
        .call(
            Body::FetchRequest(FetchRequest {
                url: "https://example.com".into(),
                token: None,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();

    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::UnsupportedVersion,
        "FetchRequest must return UnsupportedVersion in Phase 1 (rules.md §6.2)"
    );

    // 2. Invalid body (Ping) returns InvalidRequest
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
async fn test_fetcher_service_init_and_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("fetcher");
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
            neuroos_common::paths::Part::Fetcher,
            "neuroos-fetcher",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(neuroos_fetcher::init)
    });

    let fetch_sock = runtime_dir.join("fetch/fetch.sock");
    for _ in 0..50 {
        if fetch_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        fetch_sock.exists(),
        "fetch socket must be created by fetcher"
    );

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}
