#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `web.fetch` / `web.search` as kernel skills against a fake C7: every request C7 reports
//! reaches the egress ledger, a lost answer still leaves an `unconfirmed` line, and there
//! is no web skill without a key (R0-2, FR-FET-09, audit A-03 / A-05).

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, Server, StreamSink};
use neuroos_kernel::dispatch::KernelSkills;
use neuroos_kernel::keys::SecretKey;
use neuroos_kernel::{AuditActor, ControlSkills};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{EgressEntry, ErrorCode, FetchResult};
use serde_json::json;

const ACTION: &str = "0192f0c4-7d6e-7b3a-9c1d-2e3f4a5b6c7d";

#[derive(Debug, Clone, Copy)]
enum Behaviour {
    /// A search with a redirect hop: three requests.
    Reports,
    /// Refuses the token: nothing was sent.
    Refuses,
    /// Never answers within the kernel's deadline.
    Hangs,
}

struct FakeFetcher(Behaviour);

impl Handler for FakeFetcher {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        assert!(matches!(
            req.body,
            Body::SearchRequest(_) | Body::FetchRequest(_)
        ));
        match self.0 {
            Behaviour::Reports => {
                let e = |host: &str, text: &str| EgressEntry {
                    host: host.into(),
                    sent_text: text.into(),
                    bytes_out: 120,
                    bytes_in: 900,
                    at_ns: 1,
                };
                Ok(Body::FetchResult(FetchResult {
                    egress: vec![
                        e("html.duckduckgo.com", "dry scalp"),
                        e("a.example", "https://a.example/x"),
                        e("b.example", "https://b.example/moved"),
                    ],
                    spool_files: vec!["search_1_0.json".into()],
                }))
            }
            Behaviour::Refuses => Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "approval token verification failed",
                false,
            )),
            Behaviour::Hangs => {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Err(HandlerError::unsupported())
            }
        }
    }
}

struct Rig {
    skills: ControlSkills,
    ledger: std::path::PathBuf,
    _dir: tempfile::TempDir,
    _stop: tokio::sync::watch::Sender<bool>,
}

async fn rig(behaviour: Behaviour, key: Option<SecretKey>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("fetch");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("fetch.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(10)))
        .await
        .unwrap();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(server.serve(Arc::new(FakeFetcher(behaviour)), async move {
        let _ = stop_rx.wait_for(|s| *s).await;
    }));

    let audit_dir = dir.path().join("audit");
    ensure_private_dir(&audit_dir).unwrap();
    let audit = AuditActor::start(&audit_dir).unwrap();
    let control = Arc::new(ReconnectingClient::new(
        &dir.path().join("none.sock"),
        1 << 20,
    ));
    let skills = ControlSkills::new(control, Duration::from_secs(1)).with_fetcher(
        Arc::new(ReconnectingClient::new(&sock, 1 << 20)),
        key.map(Arc::new),
        Arc::new(audit),
        Duration::from_millis(300),
    );
    Rig {
        skills,
        ledger: audit_dir.join("egress.jsonl"),
        _dir: dir,
        _stop: stop_tx,
    }
}

fn ledger_hosts(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["host"].as_str().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn every_reported_request_reaches_the_ledger() {
    let r = rig(Behaviour::Reports, Some(SecretKey::from_bytes([1; 32]))).await;
    let v = r
        .skills
        .run_action("web.search", &json!({"query": "dry scalp"}), ACTION)
        .await
        .unwrap();
    assert_eq!(v["requests"], 3);
    assert_eq!(
        ledger_hosts(&r.ledger),
        ["html.duckduckgo.com", "a.example", "b.example"]
    );
}

#[tokio::test]
async fn a_refusal_before_sending_writes_nothing() {
    let r = rig(Behaviour::Refuses, Some(SecretKey::from_bytes([1; 32]))).await;
    let res = r
        .skills
        .run_action("web.fetch", &json!({"url": "https://a.example/"}), ACTION)
        .await;
    assert!(res.unwrap_err().starts_with("fetch_failed"));
    assert!(ledger_hosts(&r.ledger).is_empty());
}

/// Regression (audit A-05): when C7's answer is lost the request may still have left, so
/// the ledger gets an `unconfirmed` line with the approved text instead of nothing.
#[tokio::test]
async fn a_lost_answer_leaves_an_unconfirmed_line() {
    let r = rig(Behaviour::Hangs, Some(SecretKey::from_bytes([1; 32]))).await;
    let res = r
        .skills
        .run_action("web.fetch", &json!({"url": "https://a.example/"}), ACTION)
        .await;
    assert!(res.is_err());
    assert_eq!(ledger_hosts(&r.ledger), ["unconfirmed"]);
    let line = std::fs::read_to_string(&r.ledger).unwrap();
    assert!(line.contains("https://a.example/"));
}

/// Regression (audit A-03): no key, no web skill (there is no built-in fallback key).
#[tokio::test]
async fn no_key_means_no_web_skill() {
    let r = rig(Behaviour::Reports, None).await;
    let res = r
        .skills
        .run_action("web.search", &json!({"query": "x"}), ACTION)
        .await;
    assert_eq!(res, Err("fetch_key_unavailable".to_string()));
    assert!(ledger_hosts(&r.ledger).is_empty());
}
