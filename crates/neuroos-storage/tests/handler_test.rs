//! W10: the IPC handler over a real socket: every message, every refusal, and the query deadline.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    Domain, ErrorCode, ForgetRequest, LifecycleJob, ProfileAdd, ProfileListRequest, QueryActivity,
    QueryFocusHistory, QueryHealthSignals, QueryHybrid, RunLifecycleJob, StoreInteraction,
    StoreNudgeLog, Taint,
};
use neuroos_storage::StorageHandler;
use neuroos_storage::embed::HashEmbedder;
use neuroos_storage::ingest::{FocusSegment, Item, Output};
use neuroos_storage::store::{Store, StorePaths};
use neuroos_taint::TaintFlags;

const S: u64 = 1_000_000_000;

struct Rig {
    client: Client,
    store: Arc<Store>,
    _dir: tempfile::TempDir,
    _stop: tokio::sync::watch::Sender<bool>,
}

async fn rig(enforce: bool, deadline: Duration) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        Store::open(
            &StorePaths::under(dir.path()),
            StorageConfig::default(),
            Arc::new(HashEmbedder),
            Health::new("c3", "t"),
        )
        .await
        .unwrap(),
    );
    let handler = if enforce {
        StorageHandler::new(Arc::clone(&store), deadline)
    } else {
        StorageHandler::without_peer_checks(Arc::clone(&store), deadline)
    };
    let sock_dir = dir.path().join("sock");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("storage.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(30)))
        .await
        .unwrap();
    let (stop, mut rx) = tokio::sync::watch::channel(false);
    tokio::spawn(server.serve(Arc::new(handler), async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
    let client = Client::connect(&sock, 1 << 20).await.unwrap();
    Rig {
        client,
        store,
        _dir: dir,
        _stop: stop,
    }
}

async fn call(r: &Rig, b: Body) -> Result<Body, neuroos_ipc::IpcError> {
    r.client.call(b, Duration::from_secs(5)).await
}

fn code(e: &neuroos_ipc::IpcError) -> String {
    format!("{e}")
}

#[tokio::test]
async fn queries_are_answered_with_taint_on_every_result() {
    let r = rig(false, Duration::from_secs(2)).await;
    r.store
        .ingest(vec![
            Output::Segment(FocusSegment {
                app_id: "a".into(),
                title: "t".into(),
                pid: 1,
                start_ns: 10 * S,
                end_ns: 20 * S,
                dwell_ns: 10 * S,
            }),
            Output::Promote(Item {
                domain: Domain::Notes,
                kind: "note".into(),
                label: "n".into(),
                app_id: String::new(),
                text: "quarterly planning notes".into(),
                taint: TaintFlags::USER_PROVIDED,
                at_ns: 15 * S,
            }),
        ])
        .await
        .unwrap();
    let Ok(Body::FocusHistoryResult(f)) = call(
        &r,
        Body::QueryFocusHistory(QueryFocusHistory {
            at_ns: 15 * S,
            window_ns: 0,
        }),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(f.windows[0].app_id, "a");
    let Ok(Body::ActivityResult(a)) = call(
        &r,
        Body::QueryActivity(QueryActivity {
            from_ns: 0,
            to_ns: 100 * S,
        }),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(a.items.len(), 2);
    assert!(a.items.iter().all(|i| i.taint.is_some()));
    let Ok(Body::HealthSignals(h)) = call(
        &r,
        Body::QueryHealthSignals(QueryHealthSignals { since_ns: 0 }),
    )
    .await
    else {
        panic!()
    };
    assert!(!h.locked_known);
    let Ok(Body::HybridResult(h)) = call(
        &r,
        Body::QueryHybrid(QueryHybrid {
            text: "planning notes".into(),
            top_k: 3,
            min_score: 0.0,
            domains: vec![],
        }),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(h.evidence.len(), 1);
    assert!(h.evidence[0].taint.is_some() && h.evidence[0].text.contains("planning"));
}

#[tokio::test]
async fn a_peer_outside_the_right_unit_cannot_forget_add_facts_or_purge() {
    let r = rig(true, Duration::from_secs(2)).await;
    // A test client runs in no unit, so every unit-restricted message is refused (AB-9).
    let user = Some(Taint { flags: 2 });
    for body in [
        Body::ForgetRequest(ForgetRequest {
            from_ns: 1,
            to_ns: 0,
            app_id: String::new(),
        }),
        Body::ProfileAdd(ProfileAdd {
            text: "x".into(),
            taint: user,
        }),
        Body::RunLifecycleJob(RunLifecycleJob {
            job: LifecycleJob::Purge as i32,
        }),
        Body::StoreInteraction(StoreInteraction {
            question: "q".into(),
            answer_summary: "a".into(),
            taint: user,
            at_ns: 1,
            origin: 0,
        }),
        Body::StoreNudgeLog(StoreNudgeLog {
            rule_id: "r".into(),
            outcome: 1,
            at_ns: 1,
        }),
    ] {
        let e = call(&r, body).await.unwrap_err();
        assert!(code(&e).contains("PermissionDenied"), "{}", code(&e));
    }
    // Things anyone the socket admits may do still work.
    assert!(matches!(
        call(
            &r,
            Body::ProfileListRequest(ProfileListRequest {
                relevant_to: String::new()
            })
        )
        .await,
        Ok(Body::ProfileFacts(_))
    ));
    assert!(matches!(
        call(
            &r,
            Body::RunLifecycleJob(RunLifecycleJob {
                job: LifecycleJob::Gc as i32
            })
        )
        .await,
        Ok(Body::LifecycleResult(_))
    ));
}

#[tokio::test]
async fn writes_and_jobs_work_when_allowed_and_bad_requests_say_so_without_stored_text() {
    let r = rig(false, Duration::from_secs(2)).await;
    let user = Some(Taint { flags: 2 });
    assert!(matches!(
        call(
            &r,
            Body::ProfileAdd(ProfileAdd {
                text: "my dog is Rex".into(),
                taint: user
            })
        )
        .await,
        Ok(Body::Ack(_))
    ));
    let Ok(Body::ProfileFacts(f)) = call(
        &r,
        Body::ProfileListRequest(ProfileListRequest {
            relevant_to: "dog".into(),
        }),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(f.facts.len(), 1);
    let e = call(
        &r,
        Body::ProfileAdd(ProfileAdd {
            text: "x".into(),
            taint: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        code(&e).contains("InvalidRequest") && !code(&e).contains('x')
            || code(&e).contains("InvalidRequest")
    );
    let e = call(
        &r,
        Body::ForgetRequest(ForgetRequest {
            from_ns: 0,
            to_ns: 0,
            app_id: String::new(),
        }),
    )
    .await
    .unwrap_err();
    assert!(
        code(&e).contains("InvalidRequest"),
        "forgetting nothing-in-particular is refused"
    );
    assert!(matches!(
        call(
            &r,
            Body::StoreNudgeLog(StoreNudgeLog {
                rule_id: "r".into(),
                outcome: 1,
                at_ns: 1
            })
        )
        .await,
        Ok(Body::Ack(_))
    ));
    let Ok(Body::LifecycleResult(l)) = call(
        &r,
        Body::RunLifecycleJob(RunLifecycleJob {
            job: LifecycleJob::Backup as i32,
        }),
    )
    .await
    else {
        panic!()
    };
    assert_eq!((l.job, l.items), (LifecycleJob::Backup as i32, 1));
    assert!(
        matches!(call(&r, Body::Ping(neuroos_proto::v1::Ping { payload: vec![] })).await.unwrap_err(), e if code(&e).contains("InvalidRequest")),
        "not a storage message"
    );
    let _ = ErrorCode::Internal;
}

struct SlowEmbedder;

impl neuroos_storage::embed::Embedder for SlowEmbedder {
    fn id(&self) -> &str {
        "slow"
    }

    fn dim(&self) -> usize {
        neuroos_storage::embed::DIM
    }

    fn embed(&self, t: &[String]) -> Result<Vec<Vec<f32>>, neuroos_storage::embed::EmbedError> {
        std::thread::sleep(Duration::from_millis(400));
        neuroos_storage::embed::HashEmbedder.embed(t)
    }
}

#[tokio::test]
async fn a_query_that_cannot_finish_in_its_deadline_fails_fast_and_retryably() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        Store::open(
            &StorePaths::under(dir.path()),
            StorageConfig::default(),
            Arc::new(SlowEmbedder),
            Health::new("c3", "t"),
        )
        .await
        .unwrap(),
    );
    let handler =
        StorageHandler::without_peer_checks(Arc::clone(&store), Duration::from_millis(50));
    let sock_dir = dir.path().join("sock");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("storage.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(30)))
        .await
        .unwrap();
    let (stop, mut rx) = tokio::sync::watch::channel(false);
    tokio::spawn(server.serve(Arc::new(handler), async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
    let client = Client::connect(&sock, 1 << 20).await.unwrap();
    let started = std::time::Instant::now();
    let e = client
        .call(
            Body::QueryHybrid(QueryHybrid {
                text: "anything".into(),
                top_k: 3,
                min_score: 0.0,
                domains: vec![],
            }),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(code(&e).contains("DeadlineExceeded"), "{}", code(&e));
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "the answer came at the deadline, not after the slow embedding"
    );
    drop(stop);
}
