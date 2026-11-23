//! The gateway against mock parts on real sockets (ADR-0017): what OctoPit sends, what it does
//! when a part answers, is slow, refuses or is missing.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::server::{Server, ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_octopit_core::chat::ChatEvent;
use neuroos_octopit_core::gateway::Gateway;
use neuroos_octopit_core::health::PartState;
use neuroos_octopit_core::voice::{MicAction, VoiceState};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    Ack, AnswerChunk, AnswerDone, ErrorCode, HealthResponse, HealthStatus, PartStatus,
    StatusReport, Taint, TurnPhase, VoiceControlAction, VoiceStatus,
};
use tokio::sync::{mpsc, watch};

struct Mock {
    seen: Mutex<Vec<String>>,
    slow_answer: bool,
}

impl Handler for Mock {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::StatusRequest(_) => Ok(Body::StatusReport(StatusReport {
                overall: HealthStatus::Ok as i32,
                parts: vec![PartStatus {
                    part: "inference".into(),
                    reachable: true,
                    last: Some(HealthResponse {
                        status: HealthStatus::Ok as i32,
                        rss_bytes: 100 * 1024 * 1024,
                        ..HealthResponse::default()
                    }),
                    ..PartStatus::default()
                }],
                generated_at_ns: 5_000_000_000,
            })),
            Body::VoiceStatusRequest(_) => Ok(Body::VoiceStatus(VoiceStatus {
                capture_running: true,
                phase: TurnPhase::Capturing as i32,
                input_level_permille: 500,
                ..VoiceStatus::default()
            })),
            Body::VoiceControl(c) => {
                self.seen.lock().unwrap().push(format!("voice:{}", c.action));
                if c.action == VoiceControlAction::StartListening as i32 {
                    Ok(Body::Ack(Ack {}))
                } else {
                    Err(HandlerError::new(
                        ErrorCode::Unavailable,
                        "the microphone is not capturing",
                        true,
                    ))
                }
            }
            Body::AskRequest(a) => {
                self.seen.lock().unwrap().push(format!("ask:{}", a.text));
                sink.send(Body::AnswerChunk(AnswerChunk { text: "Four".into() }))
                    .await
                    .ok();
                if self.slow_answer {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                sink.send(Body::AnswerChunk(AnswerChunk { text: ".".into() }))
                    .await
                    .ok();
                Ok(Body::AnswerDone(AnswerDone {
                    route: 3,
                    taint: Some(Taint { flags: 1 }),
                    used_web: true,
                    ..AnswerDone::default()
                }))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

struct Rig {
    root: std::path::PathBuf,
    mock: Arc<Mock>,
    stop: watch::Sender<bool>,
    _dir: tempfile::TempDir,
}

async fn rig(parts: &[(Part, &str)], slow_answer: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let mock = Arc::new(Mock {
        seen: Mutex::new(Vec::new()),
        slow_answer,
    });
    let (stop, rx) = watch::channel(false);
    for (part, name) in parts {
        let sock = paths::socket_path(&root, *part, name);
        ensure_private_dir(sock.parent().unwrap()).unwrap();
        let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(5)))
            .await
            .unwrap();
        let mut rx = rx.clone();
        tokio::spawn(server.serve(Arc::clone(&mock), async move {
            let _ = rx.wait_for(|s| *s).await;
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    Rig {
        root,
        mock,
        stop,
        _dir: dir,
    }
}

#[tokio::test]
async fn snapshot_voice_and_the_mic_button_reach_the_parts() {
    let r = rig(
        &[(Part::Healthd, "healthd"), (Part::Voice, "control")],
        false,
    )
    .await;
    let gw = Gateway::new(&r.root);

    let snap = gw.snapshot().await;
    assert_eq!(snap.overall, PartState::Ok);
    let inf = snap.parts.iter().find(|p| p.part == Part::Inference).unwrap();
    assert_eq!(inf.state, PartState::Ok);
    assert_eq!(snap.parts.len(), 8, "every part is shown, missing ones are down");
    let storage = snap.parts.iter().find(|p| p.part == Part::Storage).unwrap();
    assert_eq!(storage.state, PartState::Down);

    let v = gw.voice().await;
    assert_eq!(v.state, VoiceState::Listening);
    assert!((v.level - 0.5).abs() < 1e-6);

    assert_eq!(gw.mic(MicAction::Start).await, Ok(()));
    let refused = gw.mic(MicAction::Cancel).await;
    assert_eq!(refused, Err("the microphone is not capturing".to_owned()));
    assert_eq!(gw.mic(MicAction::None).await, Ok(()), "a dead button sends nothing");
    let seen = r.mock.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            format!("voice:{}", VoiceControlAction::StartListening as i32),
            format!("voice:{}", VoiceControlAction::Cancel as i32)
        ]
    );
    let _ = r.stop.send(true);
}

#[tokio::test]
async fn missing_parts_become_plain_values_never_hangs_or_panics() {
    let dir = tempfile::tempdir().unwrap();
    let gw = Gateway::new(dir.path());
    let snap = tokio::time::timeout(Duration::from_secs(5), gw.snapshot())
        .await
        .unwrap();
    assert_eq!(snap.overall, PartState::Down);
    assert_eq!(gw.voice().await.state, VoiceState::Unavailable);
    assert!(gw.mic(MicAction::Start).await.is_err());
    assert!(gw.egress(10).await.is_err());
    assert!(gw.audit(10).await.is_err());
    assert!(gw.verify_audit().await.is_err());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_c, cancel) = watch::channel(false);
    gw.ask("hello".into(), tx, cancel).await;
    assert_eq!(
        rx.recv().await,
        Some(ChatEvent::Failed("The assistant is not running.".to_owned()))
    );
    assert_eq!(rx.recv().await, None, "exactly one final event");
}

#[tokio::test]
async fn an_answer_streams_chunks_then_done_with_its_flags() {
    let r = rig(&[(Part::Knowledge, "knowledge")], false).await;
    let gw = Gateway::new(&r.root);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_c, cancel) = watch::channel(false);
    gw.ask("What is 2+2?".into(), tx, cancel).await;
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    assert_eq!(events[0], ChatEvent::Chunk("Four".into()));
    assert_eq!(events[1], ChatEvent::Chunk(".".into()));
    match &events[2] {
        ChatEvent::Done(info) => {
            assert_eq!(info.route, "general chat");
            assert!(info.external && info.used_web && !info.status_only);
        }
        other => panic!("expected Done, got {other:?}"),
    }
    assert_eq!(events.len(), 3);
    assert_eq!(r.mock.seen.lock().unwrap().clone(), vec!["ask:What is 2+2?".to_owned()]);
    let _ = r.stop.send(true);
}

#[tokio::test]
async fn stopping_an_answer_ends_it_with_cancelled_and_nothing_after() {
    let r = rig(&[(Part::Knowledge, "knowledge")], true).await;
    let gw = Gateway::new(&r.root);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel) = watch::channel(false);
    let task = tokio::spawn(async move { gw.ask("long one".into(), tx, cancel).await });
    assert_eq!(rx.recv().await, Some(ChatEvent::Chunk("Four".into())));
    cancel_tx.send(true).unwrap();
    let last = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap();
    assert_eq!(last, Some(ChatEvent::Cancelled));
    task.await.unwrap();
    assert_eq!(rx.recv().await, None);
    let _ = r.stop.send(true);
}
