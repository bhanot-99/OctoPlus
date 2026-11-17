//! Tests for `neuroosctl` CLI commands and formatting (design.md §7, FR-CLI-01..02).

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::server::{Server, ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    AnswerChunk, AnswerDone, ErrorCode, HealthResponse, HealthStatus, Histogram, PartStatus, Route,
    StatusReport,
};
use neuroosctl::ask::{AskOptions, run_ask, run_ask_internal};
use neuroosctl::status::{extract_p99_ns, format_p99, format_uptime, run_status};
use neuroosctl::style::{ColorChoice, Styler};
use tempfile::tempdir;

#[test]
fn test_format_uptime() {
    assert_eq!(format_uptime(0), "0s");
    assert_eq!(format_uptime(45), "45s");
    assert_eq!(format_uptime(125), "2m 05s");
    assert_eq!(format_uptime(3665), "1h 01m");
    assert_eq!(format_uptime(90060), "1d 01h");
    assert_eq!(format_uptime(273600), "3d 04h");
}

#[test]
fn test_format_p99_and_extraction() {
    assert_eq!(format_p99("inference", 0), "—");
    assert_eq!(format_p99("inference", 15_500_000), "15.5 ms/tok");
    assert_eq!(format_p99("storage", 450_000), "450 µs");
    assert_eq!(format_p99("storage", 12_300_000), "12.3 ms");

    let resp_empty = HealthResponse::default();
    assert_eq!(extract_p99_ns(&resp_empty), 0);

    let resp_with_hist = HealthResponse {
        histograms: vec![Histogram {
            name: "inference_decode_latency".to_string(),
            upper_bounds_ns: vec![1_000_000, 2_000_000, 5_000_000],
            counts: vec![10, 80, 10, 0],
            sample_count: 100,
            sum_ns: 180_000_000,
        }],
        ..Default::default()
    };
    assert_eq!(extract_p99_ns(&resp_with_hist), 5_000_000);

    let resp_other_hist = HealthResponse {
        histograms: vec![Histogram {
            name: "misc_metric".to_string(),
            upper_bounds_ns: vec![1_000_000, 3_000_000],
            counts: vec![50, 50, 0],
            sample_count: 100,
            sum_ns: 150_000_000,
        }],
        ..Default::default()
    };
    assert_eq!(extract_p99_ns(&resp_other_hist), 3_000_000);
}

#[test]
fn test_styler_color_modes() {
    let uncolored = Styler::new(ColorChoice::Never);
    assert!(!uncolored.is_enabled());
    assert_eq!(uncolored.ok("✔ OK"), "✔ OK");
    assert_eq!(uncolored.degraded("▲ DEGRADED"), "▲ DEGRADED");
    assert_eq!(uncolored.down("✖ DOWN"), "✖ DOWN");
    assert_eq!(uncolored.bold("TITLE"), "TITLE");
    assert_eq!(uncolored.dim("subtle"), "subtle");
    assert_eq!(uncolored.error("failed"), "error: failed");
    assert_eq!(uncolored.hint("check logs"), "hint: check logs");

    let colored = Styler::new(ColorChoice::Always);
    assert!(colored.is_enabled());
    assert!(colored.ok("✔ OK").contains("\x1b[32m"));
    assert!(colored.degraded("▲ DEGRADED").contains("\x1b[33m"));
    assert!(colored.down("✖ DOWN").contains("\x1b[31m"));
    assert!(colored.bold("TITLE").contains("\x1b[1m"));
    assert!(colored.dim("subtle").contains("\x1b[2m"));
    assert!(colored.error("failed").contains("\x1b[31;1merror:"));
    assert!(colored.hint("check").contains("\x1b[2mhint:"));
}

/// Mock server for healthd status socket.
#[derive(Debug, Clone)]
struct MockHealthdHandler {
    report: StatusReport,
}

impl Handler for MockHealthdHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::StatusRequest(_) => Ok(Body::StatusReport(self.report.clone())),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

async fn start_mock_healthd(
    runtime_root: PathBuf,
    report: StatusReport,
) -> tokio::sync::oneshot::Sender<()> {
    let sock = paths::socket_path(&runtime_root, Part::Healthd, "healthd");
    if let Some(parent) = sock.parent() {
        ensure_private_dir(parent).unwrap();
    }
    let mut scfg = ServerConfig::new(1024 * 1024, Duration::from_millis(500));
    scfg.policy = neuroos_ipc::peer::PeerPolicy::uids(vec![neuroos_ipc::peer::current_uid()]);
    let server = Server::bind(&sock, scfg).await.unwrap();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        server
            .serve(Arc::new(MockHealthdHandler { report }), async move {
                let _ = stop_rx.await;
            })
            .await;
    });

    stop_tx
}

#[tokio::test]
async fn test_run_status_success_and_json() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().to_path_buf();

    let mut parts = Vec::new();
    for part in Part::ALL {
        parts.push(PartStatus {
            part: part.name().to_string(),
            reachable: true,
            last: Some(HealthResponse {
                component: format!("neuroos-{}", part.name()),
                version: "0.1.0".to_string(),
                status: HealthStatus::Ok as i32,
                started_at_ns: 1_000_000_000,
                sampled_at_ns: 2_000_000_000,
                rss_bytes: 10 * 1024 * 1024,
                subsystems: Vec::new(),
                counters: Vec::new(),
                gauges: Vec::new(),
                histograms: vec![Histogram {
                    name: "request_latency".to_string(),
                    upper_bounds_ns: vec![10_000_000],
                    counts: vec![5, 0],
                    sample_count: 5,
                    sum_ns: 25_000_000,
                }],
            }),
            last_scrape_ns: 2_000_000_000,
            cgroup_memory_bytes: 10 * 1024 * 1024,
            cgroup_cpu_usage_ns: 50_000_000,
        });
    }

    let report = StatusReport {
        overall: HealthStatus::Ok as i32,
        parts,
        generated_at_ns: 2_000_000_000,
    };

    let stop_tx = start_mock_healthd(runtime_root.clone(), report).await;
    let styler = Styler::new(ColorChoice::Always);

    // Test text status with color (exit code 0)
    let exit_code = run_status(&runtime_root, false, styler).await;
    assert_eq!(exit_code, 0);

    // Test json status (exit code 0)
    let json_exit = run_status(&runtime_root, true, styler).await;
    assert_eq!(json_exit, 0);

    let _ = stop_tx.send(());
}

#[tokio::test]
async fn test_run_status_degraded_down_starting_and_unreachable() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().to_path_buf();
    let styler = Styler::new(ColorChoice::Never);

    // Unreachable daemon returns exit code 1
    let exit_unreachable = run_status(&runtime_root, false, styler).await;
    assert_eq!(exit_unreachable, 1);

    // Degraded / starting / down states
    let report = StatusReport {
        overall: HealthStatus::Degraded as i32,
        parts: vec![
            PartStatus {
                part: "inference".to_string(),
                reachable: true,
                last: Some(HealthResponse {
                    component: "neuroos-inference".to_string(),
                    version: "0.1.0".to_string(),
                    status: HealthStatus::Degraded as i32,
                    started_at_ns: 1_000_000_000,
                    sampled_at_ns: 2_000_000_000,
                    rss_bytes: 2900 * 1024 * 1024,
                    subsystems: Vec::new(),
                    counters: Vec::new(),
                    gauges: Vec::new(),
                    histograms: vec![Histogram {
                        name: "decode_latency".to_string(),
                        upper_bounds_ns: vec![80_000_000],
                        counts: vec![10, 0],
                        sample_count: 10,
                        sum_ns: 800_000_000,
                    }],
                }),
                last_scrape_ns: 2_000_000_000,
                cgroup_memory_bytes: 2900 * 1024 * 1024,
                cgroup_cpu_usage_ns: 100_000_000,
            },
            PartStatus {
                part: "voice".to_string(),
                reachable: false,
                last: Some(HealthResponse {
                    component: "neuroos-voice".to_string(),
                    version: "0.1.0".to_string(),
                    status: HealthStatus::Down as i32,
                    started_at_ns: 0,
                    sampled_at_ns: 2_000_000_000,
                    rss_bytes: 0,
                    subsystems: Vec::new(),
                    counters: Vec::new(),
                    gauges: Vec::new(),
                    histograms: Vec::new(),
                }),
                last_scrape_ns: 2_000_000_000,
                cgroup_memory_bytes: 0,
                cgroup_cpu_usage_ns: 0,
            },
            PartStatus {
                part: "storage".to_string(),
                reachable: true,
                last: Some(HealthResponse {
                    component: "neuroos-storage".to_string(),
                    version: "0.1.0".to_string(),
                    status: HealthStatus::Starting as i32,
                    started_at_ns: 1_900_000_000,
                    sampled_at_ns: 2_000_000_000,
                    rss_bytes: 5 * 1024 * 1024,
                    subsystems: Vec::new(),
                    counters: Vec::new(),
                    gauges: Vec::new(),
                    histograms: Vec::new(),
                }),
                last_scrape_ns: 2_000_000_000,
                cgroup_memory_bytes: 5 * 1024 * 1024,
                cgroup_cpu_usage_ns: 10_000_000,
            },
            PartStatus {
                part: "kernel".to_string(),
                reachable: true,
                last: Some(HealthResponse {
                    component: "neuroos-kernel".to_string(),
                    version: "0.1.0".to_string(),
                    status: HealthStatus::Unspecified as i32,
                    started_at_ns: 0,
                    sampled_at_ns: 2_000_000_000,
                    rss_bytes: 0,
                    subsystems: Vec::new(),
                    counters: Vec::new(),
                    gauges: Vec::new(),
                    histograms: Vec::new(),
                }),
                last_scrape_ns: 2_000_000_000,
                cgroup_memory_bytes: 0,
                cgroup_cpu_usage_ns: 0,
            },
            PartStatus {
                part: "monitor".to_string(),
                reachable: false,
                last: None,
                last_scrape_ns: 2_000_000_000,
                cgroup_memory_bytes: 0,
                cgroup_cpu_usage_ns: 0,
            },
        ],
        generated_at_ns: 2_000_000_000,
    };

    let stop_tx = start_mock_healthd(runtime_root.clone(), report).await;
    let exit_degraded = run_status(&runtime_root, false, styler).await;
    assert_eq!(exit_degraded, 3);
    let _ = stop_tx.send(());
}

/// Mock knowledge server for ask requests.
#[derive(Debug, Clone)]
enum MockKnowledgeBehavior {
    Normal,
    PermissionDenied,
    InternalError,
    Slow,
}

#[derive(Debug, Clone)]
struct MockKnowledgeHandler {
    behavior: MockKnowledgeBehavior,
}

impl Handler for MockKnowledgeHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        match self.behavior {
            MockKnowledgeBehavior::PermissionDenied => Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "tier check review required",
                false,
            )),
            MockKnowledgeBehavior::InternalError => Err(HandlerError::new(
                ErrorCode::Internal,
                "model backend crashed",
                false,
            )),
            MockKnowledgeBehavior::Slow => {
                let _ = sink
                    .send(Body::AnswerChunk(AnswerChunk {
                        text: "Slow initial text...".to_string(),
                    }))
                    .await;
                // Sleep to simulate long generation
                tokio::time::sleep(Duration::from_millis(500)).await;
                Ok(Body::AnswerDone(AnswerDone {
                    route: Route::GeneralChat as i32,
                    taint: None,
                    used_web: false,
                }))
            }
            MockKnowledgeBehavior::Normal => match req.body {
                Body::AskRequest(q) => {
                    let _ = sink
                        .send(Body::AnswerChunk(AnswerChunk {
                            text: format!("Answer to: {}", q.text),
                        }))
                        .await;
                    Ok(Body::AnswerDone(AnswerDone {
                        route: Route::GeneralChat as i32,
                        taint: None,
                        used_web: false,
                    }))
                }
                _ => Err(HandlerError::unsupported()),
            },
        }
    }
}

async fn start_mock_knowledge(
    runtime_root: PathBuf,
    behavior: MockKnowledgeBehavior,
) -> tokio::sync::oneshot::Sender<()> {
    let sock = paths::socket_path(&runtime_root, Part::Knowledge, "knowledge");
    if let Some(parent) = sock.parent() {
        ensure_private_dir(parent).unwrap();
    }
    let mut scfg = ServerConfig::new(1024 * 1024, Duration::from_millis(500));
    scfg.policy = neuroos_ipc::peer::PeerPolicy::uids(vec![neuroos_ipc::peer::current_uid()]);
    let server = Server::bind(&sock, scfg).await.unwrap();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        server
            .serve(Arc::new(MockKnowledgeHandler { behavior }), async move {
                let _ = stop_rx.await;
            })
            .await;
    });

    stop_tx
}

#[tokio::test]
async fn test_run_ask_streaming_and_errors() {
    let tmp = tempdir().unwrap();
    let styler = Styler::new(ColorChoice::Never);

    // Unreachable knowledge service returns exit code 1
    let runtime_root_down = tmp.path().join("down");
    let exit_down = run_ask(
        &runtime_root_down,
        "hello".to_string(),
        AskOptions::default(),
        styler,
    )
    .await;
    assert_eq!(exit_down, 1);

    // Successful ask streaming
    let runtime_root_ok = tmp.path().join("ok");
    let stop_tx =
        start_mock_knowledge(runtime_root_ok.clone(), MockKnowledgeBehavior::Normal).await;
    let exit_ok = run_ask(
        &runtime_root_ok,
        "what is 2+2?".to_string(),
        AskOptions::default(),
        styler,
    )
    .await;
    assert_eq!(exit_ok, 0);

    // With JSON output
    let exit_json = run_ask(
        &runtime_root_ok,
        "what is 2+2?".to_string(),
        AskOptions {
            json: true,
            ..AskOptions::default()
        },
        styler,
    )
    .await;
    assert_eq!(exit_json, 0);
    let _ = stop_tx.send(());

    // Permission denied returns exit code 4
    let runtime_root_denied = tmp.path().join("denied");
    let stop_denied = start_mock_knowledge(
        runtime_root_denied.clone(),
        MockKnowledgeBehavior::PermissionDenied,
    )
    .await;
    let exit_denied = run_ask(
        &runtime_root_denied,
        "delete data".to_string(),
        AskOptions::default(),
        styler,
    )
    .await;
    assert_eq!(exit_denied, 4);
    let _ = stop_denied.send(());

    // Internal error returns exit code 1
    let runtime_root_err = tmp.path().join("err");
    let stop_err = start_mock_knowledge(
        runtime_root_err.clone(),
        MockKnowledgeBehavior::InternalError,
    )
    .await;
    let exit_err = run_ask(
        &runtime_root_err,
        "trigger crash".to_string(),
        AskOptions::default(),
        styler,
    )
    .await;
    assert_eq!(exit_err, 1);
    let _ = stop_err.send(());

    // Cancellation test with immediate cancel signal
    let runtime_root_cancel = tmp.path().join("cancel");
    let stop_cancel =
        start_mock_knowledge(runtime_root_cancel.clone(), MockKnowledgeBehavior::Slow).await;
    let cancel_sig = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    tokio::pin!(cancel_sig);
    let exit_cancel = run_ask_internal(
        &runtime_root_cancel,
        "slow question".to_string(),
        AskOptions::default(),
        styler,
        cancel_sig,
    )
    .await;
    assert_eq!(exit_cancel, 130);
    let _ = stop_cancel.send(());
}

#[derive(Debug, Clone)]
struct MockKernelHandler {
    chain_ok: bool,
    first_bad_seq: u64,
}

impl Handler for MockKernelHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::AuditTailRequest(tail) => {
                let count = if tail.limit == 0 {
                    20
                } else {
                    tail.limit.min(1000)
                };
                let mut records = Vec::new();
                for i in 1..=count.min(2) {
                    records.push(neuroos_proto::v1::AuditRecord {
                        seq: u64::from(i),
                        at_ns: 1_700_000_000_000_000_000,
                        kind: if i == 1 {
                            "ATTEMPT".into()
                        } else {
                            "RESULT".into()
                        },
                        skill: "system.read_metadata".into(),
                        status: if i == 1 { "".into() } else { "EXECUTED".into() },
                        record_hash:
                            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                                .into(),
                        tier: "SAFE".into(),
                        taint: 0,
                        origin: "CLI".into(),
                    });
                }
                Ok(Body::AuditRecords(neuroos_proto::v1::AuditRecords {
                    records,
                }))
            }
            Body::AuditVerifyRequest(_) => Ok(Body::AuditVerifyResult(
                neuroos_proto::v1::AuditVerifyResult {
                    chain_ok: self.chain_ok,
                    records_checked: 10,
                    first_bad_seq: self.first_bad_seq,
                },
            )),
            Body::EgressListRequest(list) => {
                let count = if list.limit == 0 {
                    20
                } else {
                    list.limit.min(1000)
                };
                let mut records = Vec::new();
                for _ in 1..=count.min(2) {
                    records.push(neuroos_proto::v1::EgressRecord {
                        at_ns: 1_700_000_000_000_000_000,
                        host: "api.example.com".into(),
                        sent_text: "what is the weather today?".into(),
                        bytes_out: 1024,
                        bytes_in: 2048,
                    });
                }
                Ok(Body::EgressRecords(neuroos_proto::v1::EgressRecords {
                    records,
                }))
            }
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "unknown request",
                false,
            )),
        }
    }
}

async fn start_mock_kernel(
    runtime_root: PathBuf,
    chain_ok: bool,
    first_bad_seq: u64,
) -> tokio::sync::watch::Sender<bool> {
    let sock = paths::socket_path(&runtime_root, Part::Kernel, "kernel");
    if let Some(parent) = sock.parent() {
        ensure_private_dir(parent).unwrap();
    }
    let server = Server::bind(
        &sock,
        ServerConfig::new(1024 * 1024, Duration::from_secs(1)),
    )
    .await
    .unwrap();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let wait = async move {
        let _ = stop_rx.wait_for(|s| *s).await;
    };
    let handler = Arc::new(MockKernelHandler {
        chain_ok,
        first_bad_seq,
    });
    tokio::spawn(server.serve(handler, wait));
    tokio::time::sleep(Duration::from_millis(50)).await;
    stop_tx
}

#[tokio::test]
async fn test_audit_tail_and_verify() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().join("audit_test");
    let styler = Styler::new(ColorChoice::Never);

    // 1. Connection error when kernel not running -> exit code 1
    let exit_offline = neuroosctl::audit::run_audit(
        &runtime_root,
        neuroosctl::AuditCommands::Verify {
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_offline, 1);

    // 2. Start mock kernel with chain_ok = true
    let stop_kernel = start_mock_kernel(runtime_root.clone(), true, 0).await;

    let exit_tail = neuroosctl::audit::run_audit(
        &runtime_root,
        neuroosctl::AuditCommands::Tail {
            limit: 10,
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_tail, 0);

    let exit_tail_json = neuroosctl::audit::run_audit(
        &runtime_root,
        neuroosctl::AuditCommands::Tail {
            limit: 10,
            json: true,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_tail_json, 0);

    let exit_verify = neuroosctl::audit::run_audit(
        &runtime_root,
        neuroosctl::AuditCommands::Verify {
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_verify, 0);

    let exit_verify_json = neuroosctl::audit::run_audit(
        &runtime_root,
        neuroosctl::AuditCommands::Verify {
            json: true,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_verify_json, 0);

    let _ = stop_kernel.send(true);

    // 3. Start mock kernel with chain_ok = false, first_bad_seq = 42 -> exit code 3
    let runtime_root_broken = tmp.path().join("audit_broken");
    let stop_broken = start_mock_kernel(runtime_root_broken.clone(), false, 42).await;

    let exit_broken = neuroosctl::audit::run_audit(
        &runtime_root_broken,
        neuroosctl::AuditCommands::Verify {
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_broken, 3);

    let exit_broken_json = neuroosctl::audit::run_audit(
        &runtime_root_broken,
        neuroosctl::AuditCommands::Verify {
            json: true,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_broken_json, 3);

    let _ = stop_broken.send(true);
}

#[tokio::test]
async fn test_egress_list() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().join("egress_test");
    let styler = Styler::new(ColorChoice::Never);

    // 1. Connection error when kernel not running -> exit code 1
    let exit_offline = neuroosctl::egress::run_egress(
        &runtime_root,
        neuroosctl::EgressCommands::List {
            limit: 10,
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_offline, 1);

    // 2. Start mock kernel
    let stop_kernel = start_mock_kernel(runtime_root.clone(), true, 0).await;

    let exit_list = neuroosctl::egress::run_egress(
        &runtime_root,
        neuroosctl::EgressCommands::List {
            limit: 10,
            json: false,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_list, 0);

    let exit_list_json = neuroosctl::egress::run_egress(
        &runtime_root,
        neuroosctl::EgressCommands::List {
            limit: 10,
            json: true,
            quiet: false,
        },
        styler,
    )
    .await;
    assert_eq!(exit_list_json, 0);

    let _ = stop_kernel.send(true);
}

#[test]
fn test_audit_formatting_denied_policy_and_tainted_review() {
    let styler = Styler::new(ColorChoice::Never);

    // 1. Tainted review record: skill has base tier SAFE, but logged tier is REVIEW with taint 1
    let tainted_record = neuroos_proto::v1::AuditRecord {
        seq: 42,
        at_ns: 1_700_000_000_000_000_000,
        kind: "ATTEMPT".into(),
        skill: "system.read_metadata".into(),
        status: "".into(),
        record_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        tier: "REVIEW".into(),
        taint: 1, // EXTERNAL_UNTRUSTED bit 0
        origin: "VOICE".into(),
    };

    let formatted_tainted = neuroosctl::audit::format_audit_row(&tainted_record, &styler);
    // TIER must show [REVIEW] [WEB], NOT base tier [SAFE]
    assert!(formatted_tainted.contains("[REVIEW] [WEB]"));
    assert!(!formatted_tainted.contains("[SAFE]"));

    // 2. DENIED_POLICY status: must fit in STATUS column without overflowing into HASH
    let denied_record = neuroos_proto::v1::AuditRecord {
        seq: 43,
        at_ns: 1_700_000_000_000_000_000,
        kind: "RESULT".into(),
        skill: "system.read_metadata".into(),
        status: "DENIED_POLICY".into(),
        record_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        tier: "REVIEW".into(),
        taint: 1,
        origin: "VOICE".into(),
    };

    let formatted_denied = neuroosctl::audit::format_audit_row(&denied_record, &styler);
    assert!(formatted_denied.contains("DENIED_POLICY"));

    // Verify column alignments:
    // SEQ (0..6), WHEN (6..27), ACTION (27..37), SKILL (37..62), TIER (62..80), STATUS (80..96), HASH (96..)
    assert_eq!(&formatted_denied[0..2], "43");
    assert_eq!(&formatted_denied[27..33], "RESULT");
    assert_eq!(&formatted_denied[37..57], "system.read_metadata");
    assert_eq!(&formatted_denied[62..76], "[REVIEW] [WEB]");
    assert_eq!(&formatted_denied[80..93], "DENIED_POLICY");
    assert_eq!(&formatted_denied[96..], "012345…abcdef");
}

#[test]
fn test_audit_tail_json_serialization() {
    let json_rec = neuroosctl::audit::AuditRecordJson {
        seq: 1,
        at_ns: 1_700_000_000_000_000_000,
        kind: "RESULT".into(),
        skill: "system.read_metadata".into(),
        status: "DENIED_POLICY".into(),
        record_hash: "abcd1234abcd1234".into(),
        tier: "REVIEW".into(),
        taint: 1,
        origin: "VOICE".into(),
    };
    let tail_json = neuroosctl::audit::AuditTailJson {
        schema: 1,
        records: vec![json_rec.clone()],
    };
    let serialized = serde_json::to_string(&tail_json).unwrap();
    assert!(serialized.contains("\"schema\":1"));
    assert!(serialized.contains("\"tier\":\"REVIEW\""));
    assert!(serialized.contains("\"taint\":1"));
    assert!(serialized.contains("\"origin\":\"VOICE\""));

    let deserialized: neuroosctl::audit::AuditTailJson = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.schema, 1);
    assert_eq!(deserialized.records[0], json_rec);
}

#[test]
fn status_lists_a_parts_subsystem_rows_with_fixed_text() {
    use neuroos_proto::v1::{HealthResponse, HealthStatus, SubsystemHealth};
    use neuroosctl::status::{SubsystemJson, subsystem_rows};
    let resp = HealthResponse {
        subsystems: vec![
            SubsystemHealth {
                name: "disk".into(),
                status: HealthStatus::Ok as i32,
                detail: "ok".into(),
            },
            SubsystemHealth {
                name: "focus".into(),
                status: HealthStatus::Degraded as i32,
                detail: "compositor connection lost".into(),
            },
        ],
        ..Default::default()
    };
    let rows = subsystem_rows(Some(&resp));
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1],
        SubsystemJson {
            name: "focus".into(),
            status: "DEGRADED".into(),
            detail: "compositor connection lost".into()
        }
    );
    assert!(
        subsystem_rows(None).is_empty(),
        "an unreachable part has no rows"
    );
    let bad = HealthResponse {
        subsystems: vec![SubsystemHealth {
            name: "x".into(),
            status: 99,
            detail: String::new(),
        }],
        ..Default::default()
    };
    assert_eq!(
        subsystem_rows(Some(&bad))[0].status,
        "DOWN",
        "an unknown status is shown as DOWN"
    );
}

#[test]
fn storage_jobs_are_named_and_purge_is_not_one_of_them() {
    use neuroos_proto::v1::LifecycleJob;
    use neuroosctl::storage::job_from_name;
    assert_eq!(job_from_name("gc"), Some(LifecycleJob::Gc));
    assert_eq!(job_from_name("backup"), Some(LifecycleJob::Backup));
    assert_eq!(job_from_name("rollup"), Some(LifecycleJob::Rollup));
    for bad in ["purge", "", "GC", "gc ", "forget"] {
        assert_eq!(job_from_name(bad), None, "{bad:?}");
    }
}
