//! Integration tests for `neuroos-healthd` (FR-HLT-01..05, NFR-REL-04).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use neuroos_common::config::Config;
use neuroos_common::paths::{self, Part};
use neuroos_health::{CommonArgs, Health, ServiceBuilder};
use neuroos_healthd::cgroup::CgroupReader;
use neuroos_healthd::scraper::{HealthdState, scrape_cycle, scrape_remote_part};
use neuroos_healthd::soak::SoakEngine;
use neuroos_ipc::server::{Server, ServerConfig, ensure_private_dir};
use neuroos_ipc::{Client, Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    HealthRequest, HealthResponse, HealthStatus, Histogram, Ping, StatusRequest,
};
use tempfile::tempdir;
use tokio::sync::RwLock;

/// Mock health server handler.
#[derive(Debug, Clone)]
struct MockHealthHandler {
    resp: HealthResponse,
}

impl Handler for MockHealthHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::HealthRequest(_) => Ok(Body::HealthResponse(self.resp.clone())),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

async fn start_mock_server(
    sock_path: PathBuf,
    resp: HealthResponse,
) -> tokio::sync::oneshot::Sender<()> {
    if let Some(parent) = sock_path.parent() {
        ensure_private_dir(parent).unwrap();
    }
    let mut scfg = ServerConfig::new(1024 * 1024, Duration::from_millis(500));
    scfg.policy = neuroos_ipc::peer::PeerPolicy::uids(vec![neuroos_ipc::peer::current_uid()]);
    let server = Server::bind(&sock_path, scfg).await.unwrap();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        server
            .serve(Arc::new(MockHealthHandler { resp }), async move {
                let _ = stop_rx.await;
            })
            .await;
    });

    stop_tx
}

#[tokio::test]
async fn test_scrape_remote_part_success_and_failure() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().to_path_buf();

    let sock_path = paths::health_socket(&runtime_root, Part::Storage);

    let mock_resp = HealthResponse {
        component: "neuroos-storage".to_string(),
        version: "0.1.0".to_string(),
        status: HealthStatus::Ok as i32,
        started_at_ns: 1_000_000,
        sampled_at_ns: 1_000_100,
        rss_bytes: 50_000_000,
        subsystems: Vec::new(),
        counters: Vec::new(),
        gauges: Vec::new(),
        histograms: Vec::new(),
    };

    let stop_tx = start_mock_server(sock_path, mock_resp).await;

    // Successful scrape
    let (reachable, resp) = scrape_remote_part(
        &runtime_root,
        Part::Storage,
        Duration::from_millis(500),
        1024 * 1024,
    )
    .await;

    assert!(reachable);
    assert_eq!(resp.status, HealthStatus::Ok as i32);
    assert_eq!(resp.rss_bytes, 50_000_000);

    let _ = stop_tx.send(());

    // Failed scrape (server stopped / missing)
    let (reachable_down, resp_down) = scrape_remote_part(
        &runtime_root,
        Part::Voice,
        Duration::from_millis(100),
        1024 * 1024,
    )
    .await;

    assert!(!reachable_down);
    assert_eq!(resp_down.status, HealthStatus::Down as i32);
}

#[tokio::test]
async fn test_scrape_cycle_aggregates_worst_status_and_cgroups() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().join("neuroos");
    let cgroup_root = tmp.path().join("cgroup");
    let soak_dir = tmp.path().join("soak");

    let cgroup = CgroupReader::new(cgroup_root.clone(), 1000);
    let soak = SoakEngine::new(soak_dir, false).unwrap();
    let state = Arc::new(RwLock::new(HealthdState::new(soak)));
    let self_health = Health::new("neuroos-healthd", "0.1.0");
    self_health.set_ready();

    let config = Config::from_toml_str("").unwrap();

    // Set up cgroup files for storage and inference
    let storage_cg = cgroup.unit_cgroup_dir("neuroos-storage.service");
    std::fs::create_dir_all(&storage_cg).unwrap();
    std::fs::write(storage_cg.join("memory.current"), b"20971520\n").unwrap(); // 20 MiB
    std::fs::write(storage_cg.join("cpu.stat"), b"usage_usec 42000\n").unwrap();

    // Start mock server for storage (OK)
    let storage_sock = paths::health_socket(&runtime_root, Part::Storage);
    let storage_resp = HealthResponse {
        component: "neuroos-storage".to_string(),
        version: "0.1.0".to_string(),
        status: HealthStatus::Ok as i32,
        started_at_ns: 1_000_000,
        sampled_at_ns: 1_000_100,
        rss_bytes: 20_971_520,
        subsystems: Vec::new(),
        counters: Vec::new(),
        gauges: Vec::new(),
        histograms: vec![Histogram {
            name: "storage_latency".to_string(),
            upper_bounds_ns: vec![5_000_000, 10_000_000],
            counts: vec![10, 0, 0],
            sample_count: 10,
            sum_ns: 30_000_000,
        }],
    };
    let _stop_storage = start_mock_server(storage_sock, storage_resp).await;

    // Run scrape cycle: other parts are unreachable, so overall should be DOWN
    scrape_cycle(&state, &runtime_root, &config, &self_health, &cgroup).await;

    {
        let st = state.read().await;
        assert_eq!(st.overall, HealthStatus::Down);
        let storage_status = st.parts.get(&Part::Storage).unwrap();
        assert!(storage_status.reachable);
        assert_eq!(storage_status.cgroup_memory_bytes, 20_971_520);
        assert_eq!(storage_status.cgroup_cpu_usage_ns, 42_000_000);

        let report = st.build_report();
        assert_eq!(report.overall, HealthStatus::Down as i32);
        assert_eq!(report.parts.len(), 8);
    }
}

#[tokio::test]
async fn test_90_percent_budget_marks_degraded() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().join("neuroos");
    let cgroup_root = tmp.path().join("cgroup");
    let soak_dir = tmp.path().join("soak");

    let cgroup = CgroupReader::new(cgroup_root, 1000);
    let soak = SoakEngine::new(soak_dir, false).unwrap();
    let state = Arc::new(RwLock::new(HealthdState::new(soak)));
    let self_health = Health::new("neuroos-healthd", "0.1.0");
    self_health.set_ready();

    let config = Config::from_toml_str("").unwrap();

    // Storage budget is 240 MiB. 90% is 216 MiB.
    // Set storage RSS to 220 MiB (exceeds 90%).
    let storage_sock = paths::health_socket(&runtime_root, Part::Storage);
    let high_rss = 220 * 1024 * 1024;
    let storage_resp = HealthResponse {
        component: "neuroos-storage".to_string(),
        version: "0.1.0".to_string(),
        status: HealthStatus::Ok as i32,
        started_at_ns: 1_000_000,
        sampled_at_ns: 1_000_100,
        rss_bytes: high_rss,
        subsystems: Vec::new(),
        counters: Vec::new(),
        gauges: Vec::new(),
        histograms: Vec::new(),
    };
    let _stop_storage = start_mock_server(storage_sock, storage_resp).await;

    scrape_cycle(&state, &runtime_root, &config, &self_health, &cgroup).await;

    let st = state.read().await;
    let storage_status = st.parts.get(&Part::Storage).unwrap();
    assert_eq!(
        storage_status.last.as_ref().unwrap().status,
        HealthStatus::Degraded as i32
    );
}

#[tokio::test]
async fn test_healthd_full_service_bootstrap_and_ipc() {
    let tmp = tempdir().unwrap();
    let runtime_dir = tmp.path().join("neuroos");
    let config_file = tmp.path().join("config.toml");
    std::fs::write(
        &config_file,
        b"[healthd]\nscrape_interval_s = 60\nsoak = true\n",
    )
    .unwrap();

    let args = CommonArgs {
        runtime_dir: Some(runtime_dir.clone()),
        config: Some(config_file),
        allow_uid: Some(neuroos_ipc::peer::current_uid()),
        soak: true,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let thread = std::thread::spawn(move || {
        ServiceBuilder::new(Part::Healthd, "neuroos-healthd-test", "0.1.0")
            .args(args)
            .skip_restrict(true)
            .external_stop(stop_rx)
            .run(neuroos_healthd::init)
    });

    let healthd_sock = paths::socket_path(&runtime_dir, Part::Healthd, "healthd");

    // Wait for healthd socket to be bound
    let mut client = None;
    for _ in 0..50 {
        if let Ok(c) = Client::connect(&healthd_sock, 1024 * 1024).await {
            client = Some(c);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let client = client.expect("failed to connect to healthd.sock");

    // Test StatusRequest -> StatusReport
    let rep_res = client
        .call(
            Body::StatusRequest(StatusRequest {}),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    match rep_res {
        Body::StatusReport(rep) => {
            assert_eq!(rep.parts.len(), 8);
            let healthd_part = rep.parts.iter().find(|p| p.part == "healthd").unwrap();
            assert!(healthd_part.reachable);
        }
        other => panic!("expected StatusReport, got {:?}", other),
    }

    // Test HealthRequest -> HealthResponse
    let h_res = client
        .call(
            Body::HealthRequest(HealthRequest {}),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    match h_res {
        Body::HealthResponse(h) => {
            assert_eq!(h.component, "neuroos-healthd-test");
        }
        other => panic!("expected HealthResponse, got {:?}", other),
    }

    // Test Ping -> Pong
    let ping_res = client
        .call(
            Body::Ping(Ping {
                payload: b"hello".to_vec(),
            }),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    match ping_res {
        Body::Pong(p) => assert_eq!(p.payload, b"hello"),
        other => panic!("expected Pong, got {:?}", other),
    }

    // Stop daemon
    let _ = stop_tx.send(());
    let _ = thread.join().unwrap();
}

#[tokio::test]
async fn test_run_scraper_loop_runs_and_terminates() {
    let tmp = tempdir().unwrap();
    let runtime_root = tmp.path().join("neuroos");
    let cgroup_root = tmp.path().join("cgroup");
    let soak_dir = tmp.path().join("soak");

    let cgroup = CgroupReader::new(cgroup_root, 1000);
    let soak = SoakEngine::new(soak_dir.clone(), false).unwrap();
    assert_eq!(soak.soak_dir(), soak_dir);

    let state = Arc::new(RwLock::new(HealthdState::new(soak)));
    let self_health = Health::new("neuroos-healthd", "0.1.0");

    let config = Config::from_toml_str("[healthd]\nscrape_interval_s = 1\n").unwrap();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

    let scraper_ctx = neuroos_healthd::scraper::ScraperContext {
        runtime_root,
        config,
        self_health,
        cgroup,
    };

    let loop_task = tokio::spawn(neuroos_healthd::scraper::run_scraper_loop(
        state.clone(),
        scraper_ctx,
        stop_rx,
    ));

    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = stop_tx.send(true);
    loop_task.await.unwrap();

    let st = state.read().await;
    assert!(st.last_generated_at_ns > 0);
}

#[test]
fn test_compute_overall_edge_cases() {
    let tmp = tempdir().unwrap();
    let soak = SoakEngine::new(tmp.path().join("soak"), false).unwrap();
    let mut state = HealthdState::new(soak);

    // Empty parts -> Starting
    assert_eq!(state.compute_overall(), HealthStatus::Starting);

    // One part starting, others OK -> Starting
    for part in Part::ALL {
        state.parts.insert(
            part,
            neuroos_proto::v1::PartStatus {
                part: part.name().to_string(),
                reachable: true,
                last: Some(HealthResponse {
                    status: HealthStatus::Ok as i32,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }
    assert_eq!(state.compute_overall(), HealthStatus::Ok);

    state
        .parts
        .get_mut(&Part::Storage)
        .unwrap()
        .last
        .as_mut()
        .unwrap()
        .status = HealthStatus::Starting as i32;
    assert_eq!(state.compute_overall(), HealthStatus::Starting);

    state
        .parts
        .get_mut(&Part::Inference)
        .unwrap()
        .last
        .as_mut()
        .unwrap()
        .status = HealthStatus::Degraded as i32;
    assert_eq!(state.compute_overall(), HealthStatus::Degraded);
}
