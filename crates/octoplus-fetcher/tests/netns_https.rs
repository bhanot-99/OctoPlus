//! The real HTTPS transport against a local fixture server in a private network namespace
//! (rules.md §8.4; FR-FET-03, FR-FET-04, FR-FET-06, FR-FET-09).
//!
//! Ignored by default: run by `tests/scripts/fetcher-netns-test.sh`, which creates the
//! namespace (no route out), the throw-away CA and the server, and sets
//! `OCTOPLUS_NETNS_CA` and `OCTOPLUS_NETNS_LOG`. Names resolve through a fixed table:
//! `test.example` / `search.example` → 93.184.216.34 (public-looking, on the namespace's
//! loopback), `private.example` → 10.9.9.9, `mixed.example` → both.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use octoplus_fetcher::FetcherHandler;
use octoplus_fetcher::search::GenericSearchAdapter;
use octoplus_fetcher::ssrf::SafeUrl;
use octoplus_fetcher::token::{self, Approval, SecretKey, Verdict};
use octoplus_fetcher::transport::{
    HttpsTransport, StaticResolver, Transport, TransportConfig, TransportError, USER_AGENT,
};
use octoplus_ipc::Client;
use octoplus_ipc::server::{Server, ServerConfig, ensure_private_dir};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{ApprovalToken, SearchRequest};

const PUBLIC: &str = "93.184.216.34";
const PRIVATE: &str = "10.9.9.9";
const DEADLINE: Duration = Duration::from_secs(3);

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn resolver() -> StaticResolver {
    StaticResolver::new([
        ("test.example".to_string(), vec![ip(PUBLIC)]),
        ("search.example".to_string(), vec![ip(PUBLIC)]),
        ("private.example".to_string(), vec![ip(PRIVATE)]),
        ("mixed.example".to_string(), vec![ip(PUBLIC), ip(PRIVATE)]),
    ])
}

fn transport() -> HttpsTransport {
    let ca = std::env::var("OCTOPLUS_NETNS_CA")
        .expect("run by tests/scripts/fetcher-netns-test.sh (OCTOPLUS_NETNS_CA unset)");
    HttpsTransport::new(TransportConfig {
        resolver: Arc::new(resolver()),
        deadline: DEADLINE,
        test_root_pem: Some(std::fs::read(ca).unwrap()),
    })
    .unwrap()
}

/// Requests the fixture server logged for `path`, as (path, headers) JSON objects.
fn logged(path: &str) -> Vec<serde_json::Value> {
    let log = std::env::var("OCTOPLUS_NETNS_LOG").unwrap();
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["path"] == path)
        .collect()
}

async fn fetch(url: &str) -> octoplus_fetcher::transport::Attempt {
    transport().fetch(&SafeUrl::parse(url).unwrap()).await
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_fetches_a_page_over_real_tls() {
    let a = fetch("https://test.example/page").await;
    let page = a.outcome.expect("page");
    assert_eq!(page.status, 200);
    assert!(page.body.contains("NETNS-OK"));
    assert_eq!(a.hops.len(), 1);
    assert_eq!(a.hops[0].host, "test.example");
    assert!(a.hops[0].bytes_out > 0 && a.hops[0].bytes_in > page.body.len() as u64);
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_sends_only_the_fixed_hygienic_headers() {
    // Two requests: the first page sets a cookie; the second must not send it back.
    let t = transport();
    let url = SafeUrl::parse("https://test.example/page2").unwrap();
    t.fetch(&url).await.outcome.unwrap();
    t.fetch(&url).await.outcome.unwrap();
    let reqs = logged("/page2");
    assert!(reqs.len() >= 2, "{reqs:?}");
    for r in reqs {
        let h = r["headers"].as_object().unwrap();
        let names: Vec<String> = h.keys().map(|k| k.to_ascii_lowercase()).collect();
        for banned in [
            "cookie",
            "referer",
            "authorization",
            "accept-language",
            "x-forwarded-for",
        ] {
            assert!(!names.contains(&banned.to_string()), "sent {banned}: {h:?}");
        }
        let ua = h
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str().unwrap());
        assert_eq!(ua, Some(USER_AGENT));
    }
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_follows_a_validated_redirect_and_records_each_hop() {
    let a = fetch("https://test.example/redirect").await;
    assert!(a.outcome.expect("page").body.contains("NETNS-OK"));
    let urls: Vec<&str> = a.hops.iter().map(|h| h.url.as_str()).collect();
    assert_eq!(
        urls,
        ["https://test.example/redirect", "https://test.example/page"]
    );
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_refuses_redirects_to_private_addresses_and_other_schemes() {
    // A name that resolves to a private address: refused at resolution, never connected.
    let a = fetch("https://test.example/redirect-private").await;
    assert_eq!(
        a.outcome,
        Err(TransportError::Connect("blocked address".to_string()))
    );
    assert_eq!(a.hops.len(), 2);
    assert_eq!(
        a.hops[1].bytes_out, 0,
        "nothing may be sent to the private host"
    );
    assert!(
        logged("/")
            .iter()
            .all(|r| r["headers"]["Host"] != "private.example")
    );

    // An IP-literal redirect and a downgrade to http are refused before connecting.
    for path in ["/redirect-ip", "/redirect-http"] {
        let a = fetch(&format!("https://test.example{path}")).await;
        assert!(
            matches!(a.outcome, Err(TransportError::Redirect(_))),
            "{path}: {:?}",
            a.outcome
        );
        assert_eq!(a.hops.len(), 1, "{path}");
    }
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_refuses_a_name_with_any_private_address() {
    for url in ["https://private.example/page", "https://mixed.example/page"] {
        let a = fetch(url).await;
        assert_eq!(
            a.outcome,
            Err(TransportError::Connect("blocked address".to_string())),
            "{url}"
        );
        assert_eq!(a.hops[0].bytes_out, 0, "{url}");
    }
    assert!(SafeUrl::parse("https://10.9.9.9/page").is_err());
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_stops_after_three_redirects() {
    let a = fetch("https://test.example/redirect-loop").await;
    assert!(matches!(a.outcome, Err(TransportError::Redirect(_))));
    assert_eq!(a.hops.len(), 4, "the original request and three redirects");
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_enforces_type_size_status_and_time_limits() {
    assert_eq!(
        fetch("https://test.example/json").await.outcome,
        Err(TransportError::NotText)
    );
    assert_eq!(
        fetch("https://test.example/big").await.outcome,
        Err(TransportError::TooLarge)
    );
    let chunked = fetch("https://test.example/big-chunked").await;
    assert_eq!(chunked.outcome, Err(TransportError::TooLarge));
    assert!(
        chunked.hops[0].bytes_in > 5 * 1024 * 1024,
        "bytes read are still counted"
    );
    assert_eq!(
        fetch("https://test.example/missing").await.outcome,
        Err(TransportError::Status(404))
    );

    let started = Instant::now();
    let slow = fetch("https://test.example/slow").await;
    assert_eq!(slow.outcome, Err(TransportError::Timeout));
    assert!(started.elapsed() < DEADLINE + Duration::from_secs(1));
    assert_eq!(
        slow.hops.len(),
        1,
        "the timed-out request is still recorded"
    );
}

#[tokio::test]
#[ignore = "needs the private network namespace: tests/scripts/fetcher-netns-test.sh"]
async fn netns_search_reads_result_pages_and_records_every_request() {
    let key = SecretKey::from_bytes([3; 32]);
    let spool = tempfile::tempdir().unwrap();
    let handler = Arc::new(FetcherHandler::new(
        Arc::new(SecretKey::from_bytes([3; 32])),
        spool.path().to_path_buf(),
        Arc::new(GenericSearchAdapter::new("https://search.example/html/")),
        Arc::new(transport()),
    ));
    let query = "dry scalp shampoo";
    let nonce = [5u8; 16];
    let action_id = [6u8; 16];
    let expires_at_ns = octoplus_common::time::now_ns().unwrap() + 60_000_000_000;
    let approval = Approval {
        action_id,
        nonce,
        capability: "web.search".to_string(),
        args_hash: token::args_hash(&format!(r#"{{"query":"{query}"}}"#)),
        verdict: Verdict::Allow,
        expires_at_ns,
    };
    let mac = token::issue_token(&key, &approval);
    let req = SearchRequest {
        query: query.to_string(),
        max_pages: 3,
        token: Some(ApprovalToken {
            nonce: nonce.to_vec(),
            expires_at_ns,
            mac: mac.to_vec(),
            action_id: action_id.to_vec(),
        }),
    };
    let sock_dir = spool.path().join("sock");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("fetch.sock");
    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(20)),
    )
    .await
    .unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(server.serve(handler, async move {
        let _ = stop_rx.await;
    }));
    let client = Client::connect(&sock, 1024 * 1024).await.unwrap();
    let body = client
        .call(Body::SearchRequest(req), Duration::from_secs(20))
        .await
        .unwrap();
    let _ = stop_tx.send(());
    let _ = serving.await;
    let Body::FetchResult(result) = body else {
        panic!("unexpected {body:?}")
    };

    // The search request, then the two public result pages; the private one is refused
    // at resolution but still recorded with nothing sent.
    let hosts: Vec<&str> = result.egress.iter().map(|e| e.host.as_str()).collect();
    assert_eq!(
        hosts,
        [
            "search.example",
            "test.example",
            "test.example",
            "private.example"
        ]
    );
    assert_eq!(
        result.egress[0].sent_text, query,
        "the ledger shows the approved query"
    );
    assert_eq!(result.egress[3].bytes_out, 0);
    assert_eq!(result.spool_files.len(), 2);
    let q = logged("/html/?q=dry+scalp+shampoo");
    assert_eq!(q.len(), 1, "exactly the approved text was sent");

    let doc = std::fs::read_to_string(spool.path().join(&result.spool_files[0])).unwrap();
    let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(v["url"], "https://test.example/page");
    assert!(v["text"].as_str().unwrap().contains("NETNS-OK"));
    assert!(
        !v["text"].as_str().unwrap().contains("secret"),
        "scripts are not text"
    );
}
