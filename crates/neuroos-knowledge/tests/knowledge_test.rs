#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neuroos_health::LatencyHistogram;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, ReconnectingClient, Server};
use neuroos_knowledge::{KnowledgeHandler, PERSONA, build_chatml_prompt};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{AskRequest, Domain, ErrorCode, Evidence, Route, Taint};
use neuroos_taint::TaintFlags;
use neuroos_testkit::{MockInference, MockKernel, MockStorage};

#[tokio::test]
async fn test_prompt_assembly_and_stable_prefix() {
    let evidence = vec![Evidence {
        chunk_id: 1,
        domain: Domain::WindowFocus as i32,
        text: "User was editing main.rs in Zed".into(),
        score: 0.9,
        taint: Some(Taint { flags: 1 }),
        at_ns: 100,
        source: "desktop".into(),
    }];

    let (prompt, prefix_len) = build_chatml_prompt(&evidence, "What file was I editing?");
    assert!(prompt.starts_with("<|im_start|>system\n"));
    assert!(prompt.contains(PERSONA));
    assert!(
        prompt.contains(
            "<untrusted_document>\nUser was editing main.rs in Zed\n</untrusted_document>"
        )
    );
    assert!(prompt.contains("What file was I editing?<|im_end|>\n<|im_start|>assistant\n"));

    let expected_sys = format!("<|im_start|>system\n{PERSONA}<|im_end|>\n");
    assert_eq!(prefix_len, expected_sys.chars().count() as u32);
}

#[tokio::test]
async fn test_happy_path_ask_streaming_and_cancellation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("sockets");
    ensure_private_dir(&sock_dir).unwrap();

    let c6_sock = sock_dir.join("c6.sock");
    let c3_sock = sock_dir.join("c3.sock");
    let c4_sock = sock_dir.join("c4.sock");
    let c5_sock = sock_dir.join("c5.sock");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };

    // 1. Start Mock C6 (allowing)
    let mock_c6 = Arc::new(MockKernel::allowing());
    let srv_c6 = Server::bind(
        &c6_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c6 = tokio::spawn(srv_c6.serve(mock_c6.clone(), wait(stop_rx.clone())));

    // 2. Start Mock C3 (with evidence and taint)
    let mock_c3 = Arc::new(MockStorage::with_evidence(vec![Evidence {
        chunk_id: 42,
        domain: Domain::WindowFocus as i32,
        text: "Editing docs/adr/0009.md".into(),
        score: 0.88,
        taint: Some(Taint {
            flags: TaintFlags::EXTERNAL_UNTRUSTED.bits(),
        }),
        at_ns: 200,
        source: "test".into(),
    }]));
    let srv_c3 = Server::bind(
        &c3_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c3 = tokio::spawn(srv_c3.serve(mock_c3, wait(stop_rx.clone())));

    // 3. Start Mock C4 (inference) streaming tokens
    let tokens = vec![
        "You ".into(),
        "were ".into(),
        "editing ".into(),
        "ADR-0009.".into(),
    ];
    let mock_c4 = Arc::new(MockInference::with_tokens(tokens));
    let srv_c4 = Server::bind(
        &c4_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let h_c4 = tokio::spawn(srv_c4.serve(mock_c4.clone(), wait(stop_rx.clone())));

    // 4. Start C5 Knowledge handler
    let c6_client = Arc::new(ReconnectingClient::new(&c6_sock, 1024 * 1024));
    let c3_client = Arc::new(ReconnectingClient::new(&c3_sock, 1024 * 1024));
    let c4_client = Arc::new(ReconnectingClient::new(&c4_sock, 1024 * 1024));

    let first_token_hist = Arc::new(LatencyHistogram::default());
    let ask_requests = Arc::new(AtomicU64::new(0));
    let ask_completed = Arc::new(AtomicU64::new(0));
    let c4_unavailable = Arc::new(AtomicU64::new(0));

    let handler = Arc::new(KnowledgeHandler::new(
        c6_client,
        c3_client,
        c4_client,
        Duration::from_millis(500),
        Duration::from_millis(500),
        Duration::from_secs(5),
        first_token_hist,
        ask_requests.clone(),
        ask_completed.clone(),
        c4_unavailable.clone(),
    ));

    let srv_c5 = Server::bind(
        &c5_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let h_c5 = tokio::spawn(srv_c5.serve(handler, wait(stop_rx.clone())));

    // 5. Connect client to C5 and call AskRequest
    let client = Client::connect(&c5_sock, 1024 * 1024).await.unwrap();
    let mut stream = client
        .stream(
            Body::AskRequest(AskRequest {
                text: "What was I doing?".into(),
                origin: 0,
            }),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    let mut received_tokens = Vec::new();
    let mut done_body = None;

    while let Some(item) = stream.next().await {
        match item.unwrap() {
            Body::AnswerChunk(chunk) => received_tokens.push(chunk.text),
            Body::AnswerDone(done) => {
                done_body = Some(done);
                break;
            }
            other => panic!("unexpected body {other:?}"),
        }
    }

    assert_eq!(
        received_tokens,
        vec!["You ", "were ", "editing ", "ADR-0009."]
    );

    let done = done_body.expect("must receive AnswerDone");
    assert_eq!(done.route, Route::DesktopAnswer as i32);
    assert!(!done.used_web);
    let taint = done.taint.expect("taint must be present");
    // Union of USER_PROVIDED (1 << 1) and EXTERNAL_UNTRUSTED (1 << 0)
    let expected_flags = (TaintFlags::USER_PROVIDED | TaintFlags::EXTERNAL_UNTRUSTED).bits();
    assert_eq!(taint.flags, expected_flags);

    assert_eq!(ask_requests.load(Ordering::Relaxed), 1);
    assert_eq!(ask_completed.load(Ordering::Relaxed), 1);

    // Verify mock C6 was called with capability "llm.generate"
    assert_eq!(
        *mock_c6.last_capability.lock().unwrap(),
        Some("llm.generate".into())
    );

    // Clean shutdown
    let _ = stop_tx.send(true);
    let _ = h_c6.await;
    let _ = h_c3.await;
    let _ = h_c4.await;
    let _ = h_c5.await;
}

#[tokio::test]
async fn test_c3_down_fails_soft() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("sockets");
    ensure_private_dir(&sock_dir).unwrap();

    let c6_sock = sock_dir.join("c6.sock");
    let c3_sock = sock_dir.join("c3_nonexistent.sock");
    let c4_sock = sock_dir.join("c4.sock");
    let c5_sock = sock_dir.join("c5.sock");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };

    // C6 allowing
    let mock_c6 = Arc::new(MockKernel::allowing());
    let srv_c6 = Server::bind(
        &c6_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c6 = tokio::spawn(srv_c6.serve(mock_c6, wait(stop_rx.clone())));

    // C4 streaming
    let mock_c4 = Arc::new(MockInference::with_tokens(vec![
        "General ".into(),
        "answer.".into(),
    ]));
    let srv_c4 = Server::bind(
        &c4_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let h_c4 = tokio::spawn(srv_c4.serve(mock_c4, wait(stop_rx.clone())));

    // C5 Knowledge (C3 is NOT running)
    let c6_client = Arc::new(ReconnectingClient::new(&c6_sock, 1024 * 1024));
    let c3_client = Arc::new(ReconnectingClient::new(&c3_sock, 1024 * 1024));
    let c4_client = Arc::new(ReconnectingClient::new(&c4_sock, 1024 * 1024));

    let handler = Arc::new(KnowledgeHandler::new(
        c6_client,
        c3_client,
        c4_client,
        Duration::from_millis(500),
        Duration::from_millis(50),
        Duration::from_secs(2),
        Arc::new(LatencyHistogram::default()),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    ));

    let srv_c5 = Server::bind(
        &c5_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let h_c5 = tokio::spawn(srv_c5.serve(handler, wait(stop_rx.clone())));

    let client = Client::connect(&c5_sock, 1024 * 1024).await.unwrap();
    let mut stream = client
        .stream(
            Body::AskRequest(AskRequest {
                text: "Hello".into(),
                origin: 0,
            }),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    let mut got_tokens = Vec::new();
    let mut done = None;
    while let Some(item) = stream.next().await {
        match item.unwrap() {
            Body::AnswerChunk(c) => got_tokens.push(c.text),
            Body::AnswerDone(d) => {
                done = Some(d);
                break;
            }
            other => panic!("unexpected body {other:?}"),
        }
    }

    assert_eq!(got_tokens, vec!["General ", "answer."]);
    let d = done.expect("AnswerDone expected");
    // Since C3 was down, evidence taint is 0; final taint is just USER_PROVIDED
    assert_eq!(d.taint.unwrap().flags, TaintFlags::USER_PROVIDED.bits());

    let _ = stop_tx.send(true);
    let _ = h_c6.await;
    let _ = h_c4.await;
    let _ = h_c5.await;
}

#[tokio::test]
async fn test_c4_down_returns_unavailable_retryable() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("sockets");
    ensure_private_dir(&sock_dir).unwrap();

    let c6_sock = sock_dir.join("c6.sock");
    let c3_sock = sock_dir.join("c3.sock");
    let c4_sock = sock_dir.join("c4_nonexistent.sock");
    let c5_sock = sock_dir.join("c5.sock");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };

    let mock_c6 = Arc::new(MockKernel::allowing());
    let srv_c6 = Server::bind(
        &c6_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c6 = tokio::spawn(srv_c6.serve(mock_c6, wait(stop_rx.clone())));

    let c4_unavail = Arc::new(AtomicU64::new(0));

    let c6_client = Arc::new(ReconnectingClient::new(&c6_sock, 1024 * 1024));
    let c3_client = Arc::new(ReconnectingClient::new(&c3_sock, 1024 * 1024));
    let c4_client = Arc::new(ReconnectingClient::new(&c4_sock, 1024 * 1024));

    let handler = Arc::new(KnowledgeHandler::new(
        c6_client,
        c3_client,
        c4_client,
        Duration::from_millis(500),
        Duration::from_millis(50),
        Duration::from_secs(1),
        Arc::new(LatencyHistogram::default()),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        c4_unavail.clone(),
    ));

    let srv_c5 = Server::bind(
        &c5_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let h_c5 = tokio::spawn(srv_c5.serve(handler, wait(stop_rx.clone())));

    let client = Client::connect(&c5_sock, 1024 * 1024).await.unwrap();
    let err = client
        .call(
            Body::AskRequest(AskRequest {
                text: "Hello".into(),
                origin: 0,
            }),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::Unavailable,
        "must return UNAVAILABLE when C4 is down"
    );
    assert!(wire.retryable, "error must be retryable");
    assert_eq!(c4_unavail.load(Ordering::Relaxed), 1);

    let _ = stop_tx.send(true);
    let _ = h_c6.await;
    let _ = h_c5.await;
}

#[tokio::test]
async fn test_c6_rejects_returns_permission_denied() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("sockets");
    ensure_private_dir(&sock_dir).unwrap();

    let c6_sock = sock_dir.join("c6.sock");
    let c3_sock = sock_dir.join("c3.sock");
    let c4_sock = sock_dir.join("c4.sock");
    let c5_sock = sock_dir.join("c5.sock");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };

    // C6 rejecting!
    let mock_c6 = Arc::new(MockKernel::rejecting());
    let srv_c6 = Server::bind(
        &c6_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c6 = tokio::spawn(srv_c6.serve(mock_c6, wait(stop_rx.clone())));

    let c6_client = Arc::new(ReconnectingClient::new(&c6_sock, 1024 * 1024));
    let c3_client = Arc::new(ReconnectingClient::new(&c3_sock, 1024 * 1024));
    let c4_client = Arc::new(ReconnectingClient::new(&c4_sock, 1024 * 1024));

    let handler = Arc::new(KnowledgeHandler::new(
        c6_client,
        c3_client,
        c4_client,
        Duration::from_millis(500),
        Duration::from_millis(50),
        Duration::from_secs(1),
        Arc::new(LatencyHistogram::default()),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    ));

    let srv_c5 = Server::bind(
        &c5_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let h_c5 = tokio::spawn(srv_c5.serve(handler, wait(stop_rx.clone())));

    let client = Client::connect(&c5_sock, 1024 * 1024).await.unwrap();
    let err = client
        .call(
            Body::AskRequest(AskRequest {
                text: "Dangerous question".into(),
                origin: 0,
            }),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::PermissionDenied
    );

    let _ = stop_tx.send(true);
    let _ = h_c6.await;
    let _ = h_c5.await;
}

#[tokio::test]
async fn test_cancellation_propagates_to_c4() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("sockets");
    ensure_private_dir(&sock_dir).unwrap();

    let c6_sock = sock_dir.join("c6.sock");
    let c3_sock = sock_dir.join("c3.sock");
    let c4_sock = sock_dir.join("c4.sock");
    let c5_sock = sock_dir.join("c5.sock");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };

    let mock_c6 = Arc::new(MockKernel::allowing());
    let srv_c6 = Server::bind(
        &c6_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let h_c6 = tokio::spawn(srv_c6.serve(mock_c6, wait(stop_rx.clone())));

    // Slow C4 (100 ms per token, 10 tokens)
    let tokens = (0..10).map(|i| format!("tok{i} ")).collect();
    let mock_c4 = Arc::new(MockInference::with_delay(
        tokens,
        Duration::from_millis(100),
    ));
    let srv_c4 = Server::bind(
        &c4_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let h_c4 = tokio::spawn(srv_c4.serve(mock_c4.clone(), wait(stop_rx.clone())));

    let c6_client = Arc::new(ReconnectingClient::new(&c6_sock, 1024 * 1024));
    let c3_client = Arc::new(ReconnectingClient::new(&c3_sock, 1024 * 1024));
    let c4_client = Arc::new(ReconnectingClient::new(&c4_sock, 1024 * 1024));

    let handler = Arc::new(KnowledgeHandler::new(
        c6_client,
        c3_client,
        c4_client,
        Duration::from_millis(500),
        Duration::from_millis(500),
        Duration::from_secs(5),
        Arc::new(LatencyHistogram::default()),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    ));

    let srv_c5 = Server::bind(
        &c5_sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let h_c5 = tokio::spawn(srv_c5.serve(handler, wait(stop_rx.clone())));

    let client = Client::connect(&c5_sock, 1024 * 1024).await.unwrap();
    let mut stream = client
        .stream(
            Body::AskRequest(AskRequest {
                text: "Stream me".into(),
                origin: 0,
            }),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    // Read first chunk
    let first = stream.next().await.unwrap().unwrap();
    assert!(matches!(first, Body::AnswerChunk(_)));

    // Client drops stream -> cancels!
    drop(stream);

    // Wait for the cancel event to signal mock C4 with a bounded timeout
    tokio::time::timeout(Duration::from_secs(2), mock_c4.cancel_notify.notified())
        .await
        .expect("C4 mock must observe cancellation within timeout");

    assert!(
        mock_c4.cancelled.load(Ordering::SeqCst),
        "C4 mock must observe cancellation when client cancels"
    );

    let _ = stop_tx.send(true);
    let _ = h_c6.await;
    let _ = h_c4.await;
    let _ = h_c5.await;
}

#[tokio::test]
async fn test_knowledge_service_init_and_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("neuroos");
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
            neuroos_common::paths::Part::Knowledge,
            "neuroos-knowledge",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(neuroos_knowledge::init)
    });

    let knowledge_sock = runtime_dir.join("knowledge/knowledge/knowledge.sock");
    for _ in 0..50 {
        if knowledge_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(knowledge_sock.exists());
    let client = Client::connect(&knowledge_sock, 1024 * 1024).await.unwrap();

    // With peers down, AskRequest returns an error (kernel or C4 down)
    let err = client
        .call(
            Body::AskRequest(AskRequest {
                text: "test".into(),
                origin: 0,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    assert!(err.to_wire().code != 0);

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}
