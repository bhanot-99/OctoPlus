#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_fetcher::FetcherHandler;
use neuroos_fetcher::search::GenericSearchAdapter;
use neuroos_fetcher::token::{self, Approval, SecretKey, Verdict};
use neuroos_fetcher::transport::MockTransport;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ApprovalToken, ErrorCode, FetchRequest, SearchRequest};

#[tokio::test]
async fn test_fetcher_validates_token_and_fetches() {
    let key = Arc::new(SecretKey::from_bytes([99u8; 32]));
    let temp_dir = tempfile::tempdir().unwrap();
    let spool_dir = temp_dir.path().join("spool");
    std::fs::create_dir_all(&spool_dir).unwrap();

    let mock_transport = MockTransport::new();
    mock_transport.register(
        "https://example.com/item",
        "<html><head><title>Item Title</title></head><body><p>Item Body Text</p></body></html>",
    );

    let handler = Arc::new(FetcherHandler::new(
        key.clone(),
        spool_dir.clone(),
        Arc::new(GenericSearchAdapter::default()),
        Arc::new(mock_transport),
    ));

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

    // 1. FetchRequest without token is denied
    let err = client
        .call(
            Body::FetchRequest(FetchRequest {
                url: "https://example.com/item".into(),
                token: None,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::PermissionDenied,
        "FetchRequest without token must be PermissionDenied"
    );

    // 2. FetchRequest with valid token succeeds
    let now = neuroos_common::time::now_ns().unwrap();
    let approval = Approval {
        action_id: [1; 16],
        nonce: [2; 16],
        capability: "web.fetch".to_string(),
        args_hash: token::args_hash(r#"{"url":"https://example.com/item"}"#),
        verdict: Verdict::Allow,
        expires_at_ns: now + 60_000_000_000,
    };
    let mac = token::issue_token(&key, &approval);
    let valid_token = ApprovalToken {
        nonce: approval.nonce.to_vec(),
        expires_at_ns: approval.expires_at_ns,
        mac: mac.to_vec(),
        action_id: approval.action_id.to_vec(),
    };

    let resp = client
        .call(
            Body::FetchRequest(FetchRequest {
                url: "https://example.com/item".into(),
                token: Some(valid_token),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    if let Body::FetchResult(result) = resp {
        assert_eq!(result.egress.len(), 1);
        assert_eq!(result.egress[0].host, "example.com");
        assert_eq!(result.spool_files.len(), 1);

        let spooled = spool_dir.join(&result.spool_files[0]);
        assert!(spooled.exists());
        let spooled_text = std::fs::read_to_string(spooled).unwrap();
        assert!(spooled_text.contains("Item Title"));
        assert!(spooled_text.contains("Item Body Text"));
    } else {
        panic!("unexpected response body: {resp:?}");
    }

    // 3. SearchRequest without token is denied
    let err = client
        .call(
            Body::SearchRequest(SearchRequest {
                query: "shampoo".into(),
                max_pages: 2,
                token: None,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    assert_eq!(
        ErrorCode::try_from(err.to_wire().code).unwrap(),
        ErrorCode::PermissionDenied
    );

    let _ = stop_tx.send(true);
    let _ = handle.await;
}

fn service_args(dir: &std::path::Path) -> (neuroos_health::CommonArgs, std::path::PathBuf) {
    let runtime_dir = dir.join("fetcher");
    let config_file = dir.join("config.toml");
    std::fs::write(&config_file, b"").unwrap();
    let args = neuroos_health::CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(neuroos_ipc::peer::current_uid()),
        soak: false,
    };
    (args, runtime_dir.join("fetch/fetch.sock"))
}

/// Regression (audit A-03): with no key file the service refuses to start; there is no
/// built-in fallback key that would let a forged approval through.
#[test]
fn service_refuses_to_start_without_its_key() {
    let temp_dir = tempfile::tempdir().unwrap();
    let (args, fetch_sock) = service_args(temp_dir.path());
    let missing = temp_dir.path().join("no-such.key");
    let res = neuroos_health::ServiceBuilder::new(
        neuroos_common::paths::Part::Fetcher,
        "neuroos-fetcher",
        "0.1.0",
    )
    .skip_restrict(true)
    .args(args)
    .run(|ctx| neuroos_fetcher::init_with_key(&missing, ctx));
    assert!(res.is_err(), "must not start without fetch.key");
    // The builder binds before `init`; once `init` fails the service is gone, so nothing
    // answers on the socket.
    assert!(
        std::os::unix::net::UnixStream::connect(&fetch_sock).is_err(),
        "no socket may be served without a key"
    );
}

#[tokio::test]
async fn test_fetcher_service_init_and_run() {
    use std::os::unix::fs::PermissionsExt;
    let temp_dir = tempfile::tempdir().unwrap();
    let (args, fetch_sock) = service_args(temp_dir.path());
    let key_path = temp_dir.path().join("fetch.key");
    std::fs::write(&key_path, [9u8; 32]).unwrap();
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o640)).unwrap();

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
        .run(move |ctx| neuroos_fetcher::init_with_key(&key_path, ctx))
    });

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
