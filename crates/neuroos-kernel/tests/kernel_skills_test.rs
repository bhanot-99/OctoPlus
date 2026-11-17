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

/// A fake C3 that records the three writes the kernel-kind skills make.
#[derive(Debug, Default)]
struct FakeStorage {
    seen: std::sync::Mutex<Vec<String>>,
}

impl Handler for FakeStorage {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        let line = match &req.body {
            Body::ProfileAdd(a) => format!(
                "add:{}:{}",
                a.text,
                a.taint.as_ref().map_or(u32::MAX, |t| t.flags)
            ),
            Body::ProfileRemove(r) => format!("remove:{:?}", r.ids),
            Body::ForgetRequest(f) => format!("forget:{}:{}:{}", f.from_ns, f.to_ns, f.app_id),
            _ => return Err(HandlerError::unsupported()),
        };
        self.seen.lock().unwrap().push(line);
        match req.body {
            Body::ForgetRequest(_) => Ok(Body::ForgetResult(neuroos_proto::v1::ForgetResult {
                rows_deleted: 3,
                vectors_deleted: 2,
            })),
            _ => Ok(Body::Ack(neuroos_proto::v1::Ack {})),
        }
    }
}

#[tokio::test]
async fn profile_and_forget_skills_call_c3_with_validated_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("storage");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("storage.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(1)))
        .await
        .unwrap();
    let storage = Arc::new(FakeStorage::default());
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(server.serve(Arc::clone(&storage), async move {
        let _ = stop_rx.wait_for(|s| *s).await;
    }));
    let skills = ControlSkills::new(
        Arc::new(ReconnectingClient::new(&dir.path().join("m.sock"), 1 << 20)),
        Duration::from_millis(200),
    )
    .with_storage(
        Arc::new(ReconnectingClient::new(&sock, 1 << 20)),
        Duration::from_secs(1),
    );

    // The fact is stored as the user's own words (USER_PROVIDED), trimmed.
    let user_provided = neuroos_taint::TaintFlags::USER_PROVIDED.bits();
    assert_eq!(
        skills
            .run("profile.remember", &json!({"fact": "  my dog is Rex "}))
            .await,
        Ok(json!({"saved": true}))
    );
    assert_eq!(
        skills.run("profile.forget", &json!({"fact_id": 7})).await,
        Ok(json!({"removed": 7}))
    );
    assert_eq!(
        skills
            .run("memory.forget", &json!({"since_ns": 5, "app": "org.x"}))
            .await,
        Ok(json!({"rows_deleted": 3, "vectors_deleted": 2}))
    );
    assert_eq!(
        *storage.seen.lock().unwrap(),
        vec![
            format!("add:my dog is Rex:{user_provided}"),
            "remove:[7]".to_string(),
            "forget:5:0:org.x".to_string(),
        ]
    );

    // Bad arguments never reach C3 (fail closed on their own).
    let before = storage.seen.lock().unwrap().len();
    for (cap, bad) in [
        ("profile.remember", json!({"fact": "   "})),
        ("profile.remember", json!({"fact": "a\u{0}b"})),
        ("profile.remember", json!({"fact": "x".repeat(513)})),
        ("profile.remember", json!({})),
        ("profile.forget", json!({"fact_id": 0})),
        ("profile.forget", json!({"fact_id": "1"})),
        ("memory.forget", json!({})),
        ("memory.forget", json!({"since_ns": 9, "until_ns": 3})),
        // Open-ended ranges would mean "everything before/after": only purge may do that.
        ("memory.forget", json!({"until_ns": 5})),
        ("memory.forget", json!({"since_ns": 5})),
        ("memory.forget", json!({"since_ns": "1"})),
        ("memory.forget", json!({"app": 5})),
    ] {
        assert_eq!(
            skills.run(cap, &bad).await,
            Err("bad_args".into()),
            "{cap} {bad}"
        );
    }
    assert_eq!(storage.seen.lock().unwrap().len(), before);

    let _ = stop_tx.send(true);
    let _ = handle.await;
    // C3 gone: unavailable, never a silent success; and without a client at all.
    let down = skills
        .run("profile.remember", &json!({"fact": "still here"}))
        .await;
    assert_eq!(down, Err("storage_unavailable".into()));
    let none = ControlSkills::new(
        Arc::new(ReconnectingClient::new(&dir.path().join("m.sock"), 1 << 20)),
        Duration::from_millis(200),
    );
    assert_eq!(
        none.run("profile.forget", &json!({"fact_id": 1})).await,
        Err("storage_unavailable".into())
    );
}
