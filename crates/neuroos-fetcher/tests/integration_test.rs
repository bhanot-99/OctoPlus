#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use neuroos_common::time::now_ns;
use neuroos_fetcher::FetcherHandler;
use neuroos_fetcher::search::GenericSearchAdapter;
use neuroos_fetcher::ssrf::AddressPolicy;
use neuroos_fetcher::token::{self, Approval, SecretKey, Verdict};
use neuroos_fetcher::transport::MockTransport;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ApprovalToken, ErrorCode, SearchRequest};
use tempfile::tempdir;

#[tokio::test]
async fn test_phase_8_end_to_end_search_and_fetch_pipeline() {
    let temp_dir = tempdir().unwrap();
    let spool_dir = temp_dir.path().join("spool");
    std::fs::create_dir_all(&spool_dir).unwrap();

    let key = Arc::new(SecretKey::from_bytes([0x77u8; 32]));

    let mock_transport = MockTransport::new();
    // Register search endpoint response (SearXNG HTML search results)
    let search_html = r#"
        <div class="result default_result">
            <h3 class="result_title"><a href="https://example.org/rust2026">Rust 2026 Edition</a></h3>
            <p class="result-snippet">Rust 2026 brings new security primitives.</p>
        </div>
    "#;
    mock_transport.register("https://search.example/search?q=rust+2026", search_html);
    mock_transport.register(
        "https://example.org/rust2026",
        "<html><head><title>Rust 2026 Edition</title></head><body><p>Rust 2026 is officially released with memory-safe kernel architecture.</p></body></html>",
    );

    let adapter = GenericSearchAdapter::new("https://search.example/search");
    let handler = Arc::new(FetcherHandler::new(
        key.clone(),
        spool_dir.clone(),
        Arc::new(adapter),
        Arc::new(mock_transport),
    ));

    let sock_dir = temp_dir.path().join("fetch_ipc");
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
    let _handle = tokio::spawn(server.serve(handler, wait(stop_rx)));

    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();

    // 1. Send SearchRequest with valid approval token
    let now = now_ns().unwrap();
    let search_approval = Approval {
        action_id: [10; 16],
        nonce: [20; 16],
        capability: "web.search".to_string(),
        args_hash: token::args_hash(r#"{"query":"rust 2026"}"#),
        verdict: Verdict::Allow,
        expires_at_ns: now + 60_000_000_000,
    };
    let mac = token::issue_token(&key, &search_approval);
    let search_token = ApprovalToken {
        nonce: search_approval.nonce.to_vec(),
        expires_at_ns: search_approval.expires_at_ns,
        mac: mac.to_vec(),
        action_id: search_approval.action_id.to_vec(),
    };

    let resp = client
        .call(
            Body::SearchRequest(SearchRequest {
                query: "rust 2026".into(),
                max_pages: 1,
                token: Some(search_token),
            }),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    let fetch_result = match resp {
        Body::FetchResult(r) => r,
        other => panic!("expected FetchResult, got {other:?}"),
    };

    assert_eq!(fetch_result.spool_files.len(), 1);
    assert_eq!(fetch_result.egress.len(), 2); // 1 search + 1 top-N result fetch

    // Verify spool document content
    let spool_file_path = spool_dir.join(&fetch_result.spool_files[0]);
    let spool_content = std::fs::read_to_string(spool_file_path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&spool_content).unwrap();
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["url"], "https://example.org/rust2026");
    assert_eq!(doc["title"], "Rust 2026 Edition");
    assert!(
        doc["text"]
            .as_str()
            .unwrap()
            .contains("Rust 2026 is officially released")
    );

    // 2. Anti-replay check: reusing the same search token fails
    let reuse_approval = Approval {
        action_id: [10; 16],
        nonce: [20; 16],
        capability: "web.search".to_string(),
        args_hash: token::args_hash(r#"{"query":"rust 2026"}"#),
        verdict: Verdict::Allow,
        expires_at_ns: now + 60_000_000_000,
    };
    let reuse_mac = token::issue_token(&key, &reuse_approval);
    let reuse_token = ApprovalToken {
        nonce: reuse_approval.nonce.to_vec(),
        expires_at_ns: reuse_approval.expires_at_ns,
        mac: reuse_mac.to_vec(),
        action_id: reuse_approval.action_id.to_vec(),
    };
    let err = client
        .call(
            Body::SearchRequest(SearchRequest {
                query: "rust 2026".into(),
                max_pages: 1,
                token: Some(reuse_token),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::PermissionDenied
    );

    let _ = stop_tx.send(true);
}

#[test]
fn test_anti_ssrf_comprehensive_corpus() {
    let policy = AddressPolicy;

    // Loopback IPv4 and IPv6
    assert!(
        policy
            .validate_ip("127.0.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("127.0.0.2".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("::1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("::ffff:127.0.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );

    // RFC 1918 Private IPv4
    assert!(
        policy
            .validate_ip("10.0.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("10.255.255.255".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("172.16.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("172.31.255.255".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("192.168.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("192.168.255.255".parse::<IpAddr>().unwrap())
            .is_err()
    );

    // Link-local & cloud metadata
    assert!(
        policy
            .validate_ip("169.254.169.254".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("fe80::1".parse::<IpAddr>().unwrap())
            .is_err()
    );

    // Multicast & broadcast
    assert!(
        policy
            .validate_ip("224.0.0.1".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("255.255.255.255".parse::<IpAddr>().unwrap())
            .is_err()
    );
    assert!(
        policy
            .validate_ip("ff02::1".parse::<IpAddr>().unwrap())
            .is_err()
    );

    // Public IPs must succeed
    assert!(
        policy
            .validate_ip("93.184.216.34".parse::<IpAddr>().unwrap())
            .is_ok()
    );
    assert!(
        policy
            .validate_ip(
                "2606:2800:220:1:248:1893:25c8:1946"
                    .parse::<IpAddr>()
                    .unwrap()
            )
            .is_ok()
    );
}
