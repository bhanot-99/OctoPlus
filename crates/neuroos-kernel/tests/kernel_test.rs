#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Server};
use neuroos_kernel::{KernelHandler, Registry};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, EvaluateRequest, Ping, Taint, Tier};
use neuroos_taint::TaintFlags;

#[tokio::test]
async fn test_kernel_evaluation_and_taint_enforcement() {
    let registry = Arc::new(Registry::load().unwrap());
    let handler = Arc::new(KernelHandler::new(registry));
    let temp_dir = tempfile::tempdir().unwrap();
    let sock_dir = temp_dir.path().join("kernel");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("kernel.sock");

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

    // 1. SAFE skill clean -> SAFE / true
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.read_metadata".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Safe as i32);
            assert!(res.allowed_without_dialog);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 2. SAFE skill + EXTERNAL_UNTRUSTED -> REVIEW / false
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.read_metadata".into(),
                context_taint: Some(Taint {
                    flags: TaintFlags::EXTERNAL_UNTRUSTED.bits(),
                }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Review as i32);
            assert!(!res.allowed_without_dialog);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 3. REVIEW skill -> REVIEW / false
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.cleanup".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Review as i32);
            assert!(!res.allowed_without_dialog);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 4. Unknown capability -> PermissionDenied error
    let err = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "unknown.capability".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::PermissionDenied,
        "unknown capability must be rejected with PermissionDenied"
    );

    // 5. shell.exec (disabled) -> PermissionDenied error
    let err = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "shell.exec".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::PermissionDenied,
        "disabled shell.exec must be rejected with PermissionDenied"
    );

    // 6. Missing context_taint MUST be rejected with InvalidRequest (rules.md §6.4)
    let err = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.read_metadata".into(),
                context_taint: None,
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
    let wire = err.to_wire();
    assert_eq!(
        ErrorCode::try_from(wire.code).unwrap(),
        ErrorCode::InvalidRequest,
        "missing context_taint must be rejected with InvalidRequest"
    );

    // 7. Invalid body (Ping)
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
async fn test_kernel_service_init_and_run() {
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime_dir = temp_dir.path().join("neuroos");
    let config_file = temp_dir.path().join("config.toml");
    std::fs::write(&config_file, b"").unwrap();
    // Never the user's real home: key, audit log and cleanup roots all live in the temp dir.
    let kpaths = neuroos_kernel::KernelPaths {
        home: temp_dir.path().to_path_buf(),
        config_dir: temp_dir.path().join("config"),
        data_dir: temp_dir.path().join("data"),
    };
    std::fs::create_dir_all(&kpaths.config_dir).unwrap();
    let key = kpaths.config_dir.join("hmac.key");
    std::fs::write(&key, [5u8; 32]).unwrap();
    std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();

    let args = neuroos_health::CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(neuroos_ipc::peer::current_uid()),
        soak: false,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        neuroos_health::ServiceBuilder::new(
            neuroos_common::paths::Part::Kernel,
            "neuroos-kernel",
            "0.1.0",
        )
        .skip_restrict(true)
        .args(args)
        .external_stop(stop_rx)
        .run(move |ctx| neuroos_kernel::init_with(ctx, &kpaths))
    });

    let kernel_sock = runtime_dir.join("kernel/kernel/kernel.sock");
    for _ in 0..50 {
        if kernel_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(kernel_sock.exists());
    let client = Client::connect(&kernel_sock, 1024 * 1024).await.unwrap();
    let res = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.read_metadata".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match res {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Safe as i32);
            assert!(res.allowed_without_dialog);
        }
        other => panic!("unexpected body {other:?}"),
    }

    let _ = stop_tx.send(());
    let res = thread.join().unwrap();
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_kernel_audit_tail_verify_and_tamper() {
    let registry = Arc::new(Registry::load().unwrap());
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_dir = temp_dir.path().join("audit");
    let audit_actor = neuroos_kernel::AuditActor::start(&audit_dir).unwrap();

    let handler = Arc::new(KernelHandler::with_audit(registry, audit_actor.clone()));
    let sock_dir = temp_dir.path().join("kernel");
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("kernel.sock");

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

    // 1. Initial audit tail and verify on empty log
    let tail_res = client
        .call(
            Body::AuditTailRequest(neuroos_proto::v1::AuditTailRequest { limit: 10 }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match tail_res {
        Body::AuditRecords(rec) => assert!(rec.records.is_empty()),
        other => panic!("unexpected body {other:?}"),
    }

    let verify_res = client
        .call(
            Body::AuditVerifyRequest(neuroos_proto::v1::AuditVerifyRequest {}),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match verify_res {
        Body::AuditVerifyResult(rep) => {
            assert!(rep.chain_ok);
            assert_eq!(rep.records_checked, 0);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 2. Append ATTEMPT and RESULT records through the actor
    let action_id = "018f2f67-8e4a-7bc9-93e8-112233445566";
    let args_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    audit_actor
        .attempt(
            1_000_000,
            action_id,
            "system.read_metadata",
            neuroos_kernel::ActionTier::Safe,
            0,
            neuroos_kernel::ActionOrigin::Cli,
            args_hash,
        )
        .await
        .unwrap();

    audit_actor
        .result(
            2_000_000,
            action_id,
            "system.read_metadata",
            neuroos_kernel::ActionTier::Safe,
            0,
            neuroos_kernel::ActionOrigin::Cli,
            args_hash,
            neuroos_kernel::ActionStatus::Executed,
        )
        .await
        .unwrap();

    // 3. Tail records via client
    let tail_res = client
        .call(
            Body::AuditTailRequest(neuroos_proto::v1::AuditTailRequest { limit: 10 }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match tail_res {
        Body::AuditRecords(rec) => {
            assert_eq!(rec.records.len(), 2);
            assert_eq!(rec.records[0].seq, 1);
            assert_eq!(rec.records[0].kind, "ATTEMPT");
            assert_eq!(rec.records[0].skill, "system.read_metadata");
            assert!(!rec.records[0].record_hash.is_empty());

            assert_eq!(rec.records[1].seq, 2);
            assert_eq!(rec.records[1].kind, "RESULT");
            assert_eq!(rec.records[1].status, "EXECUTED");
            assert!(!rec.records[1].record_hash.is_empty());
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 4. Verify integrity via client
    let verify_res = client
        .call(
            Body::AuditVerifyRequest(neuroos_proto::v1::AuditVerifyRequest {}),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match verify_res {
        Body::AuditVerifyResult(rep) => {
            assert!(rep.chain_ok);
            assert_eq!(rep.records_checked, 2);
            assert_eq!(rep.first_bad_seq, 0);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 5. Append egress record and query via client
    audit_actor
        .egress_append(
            3_000_000,
            action_id,
            "api.example.com",
            "search query",
            128,
            512,
        )
        .await
        .unwrap();

    let egress_res = client
        .call(
            Body::EgressListRequest(neuroos_proto::v1::EgressListRequest { limit: 10 }),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match egress_res {
        Body::EgressRecords(rec) => {
            assert_eq!(rec.records.len(), 1);
            assert_eq!(rec.records[0].host, "api.example.com");
            assert_eq!(rec.records[0].sent_text, "search query");
            assert_eq!(rec.records[0].bytes_out, 128);
            assert_eq!(rec.records[0].bytes_in, 512);
        }
        other => panic!("unexpected body {other:?}"),
    }

    // 6. Tamper with the action audit log on disk -> verify must report chain broken
    let action_log_file = audit_dir.join("actions.jsonl");
    let content = std::fs::read_to_string(&action_log_file).unwrap();
    // Tamper by mutating a character in line 1
    let tampered = content.replacen("system.read_metadata", "system.disk_report", 1);
    std::fs::write(&action_log_file, tampered).unwrap();

    let verify_tampered = client
        .call(
            Body::AuditVerifyRequest(neuroos_proto::v1::AuditVerifyRequest {}),
            Duration::from_millis(500),
        )
        .await
        .unwrap();

    match verify_tampered {
        Body::AuditVerifyResult(rep) => {
            assert!(!rep.chain_ok, "verification must fail after file tampering");
            assert_eq!(
                rep.first_bad_seq, 2,
                "line 2 prev_hash will mismatch tampered line 1"
            );
        }
        other => panic!("unexpected body {other:?}"),
    }

    let _ = stop_tx.send(true);
    let _ = handle.await;
}
