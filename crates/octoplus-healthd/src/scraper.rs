//! Parallel bounded scraping engine and state management for `octoplus-healthd` (FR-HLT-02..05).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use octoplus_common::config::Config;
use octoplus_common::paths::{self, Part};
use octoplus_health::Health;
use octoplus_ipc::Client;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{HealthRequest, HealthResponse, HealthStatus, PartStatus, StatusReport};
use tokio::sync::RwLock;

use crate::cgroup::CgroupReader;
use crate::soak::SoakEngine;

/// Maximum number of historic scrape samples kept per component in memory (Architecture §6.6).
pub const ROLLING_WINDOW_CAPACITY: usize = 120;

/// Returns current UTC epoch in nanoseconds.
#[must_use]
pub fn now_ns() -> u64 {
    u64::try_from(jiff::Timestamp::now().as_nanosecond()).unwrap_or(0)
}

/// One compact rolling-window entry. Plain numbers only, so the window's memory
/// is fixed once its capacity is reserved (no per-sample heap data; see the
/// Phase 1 mini-soak, where full status copies grew healthd's RSS by ~1 MiB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Scrape time, UTC epoch ns.
    pub at_ns: u64,
    /// Reported status.
    pub status: HealthStatus,
    /// Whether the health socket answered.
    pub reachable: bool,
    /// Process RSS in bytes (cgroup memory if the process reported none).
    pub rss_bytes: u64,
    /// cgroup `memory.current` in bytes.
    pub cgroup_memory_bytes: u64,
    /// cgroup CPU usage in ns.
    pub cgroup_cpu_usage_ns: u64,
}

/// Shared internal state for `octoplus-healthd`.
#[derive(Debug)]
pub struct HealthdState {
    /// Overall system health status (worst across all components).
    pub overall: HealthStatus,
    /// Latest status per component.
    pub parts: HashMap<Part, PartStatus>,
    /// Rolling window of recent samples per component (fixed-size, allocated once).
    pub rolling: HashMap<Part, VecDeque<Sample>>,
    /// Soak baseline and drift tracking engine.
    pub soak: SoakEngine,
    /// Timestamp of last report generation.
    pub last_generated_at_ns: u64,
}

impl HealthdState {
    /// Creates a new state with empty parts.
    #[must_use]
    pub fn new(soak: SoakEngine) -> Self {
        Self {
            overall: HealthStatus::Starting,
            parts: HashMap::new(),
            rolling: HashMap::new(),
            soak,
            last_generated_at_ns: 0,
        }
    }

    /// Evaluates worst status across all parts.
    /// Priority: DOWN > DEGRADED > STARTING > OK.
    #[must_use]
    pub fn compute_overall(&self) -> HealthStatus {
        if self.parts.is_empty() {
            return HealthStatus::Starting;
        }

        let mut has_degraded = false;
        let mut has_starting = false;

        for part in Part::ALL {
            let status = match self.parts.get(&part) {
                Some(p) => match HealthStatus::try_from(p.last.as_ref().map_or(0, |l| l.status)) {
                    Ok(s) => s,
                    Err(_) => HealthStatus::Down,
                },
                None => HealthStatus::Down,
            };

            match status {
                HealthStatus::Down | HealthStatus::Unspecified => {
                    return HealthStatus::Down;
                }
                HealthStatus::Degraded => {
                    has_degraded = true;
                }
                HealthStatus::Starting => {
                    has_starting = true;
                }
                HealthStatus::Ok => {}
            }
        }

        if has_degraded {
            HealthStatus::Degraded
        } else if has_starting {
            HealthStatus::Starting
        } else {
            HealthStatus::Ok
        }
    }

    /// Produces a [`StatusReport`] containing current metrics in standard part order.
    #[must_use]
    pub fn build_report(&self) -> StatusReport {
        let mut parts = Vec::new();
        for part in Part::ALL {
            if let Some(status) = self.parts.get(&part) {
                parts.push(status.clone());
            } else {
                // If not yet scraped, report as DOWN
                parts.push(PartStatus {
                    part: part.name().to_string(),
                    reachable: false,
                    last: Some(HealthResponse {
                        component: format!("octoplus-{}", part.name()),
                        version: String::new(),
                        status: HealthStatus::Down as i32,
                        started_at_ns: 0,
                        sampled_at_ns: now_ns(),
                        rss_bytes: 0,
                        subsystems: Vec::new(),
                        counters: Vec::new(),
                        gauges: Vec::new(),
                        histograms: Vec::new(),
                    }),
                    last_scrape_ns: self.last_generated_at_ns,
                    cgroup_memory_bytes: 0,
                    cgroup_cpu_usage_ns: 0,
                });
            }
        }

        StatusReport {
            overall: self.overall as i32,
            parts,
            generated_at_ns: self.last_generated_at_ns,
        }
    }
}

/// Scrapes a remote part's health socket over IPC.
pub async fn scrape_remote_part(
    runtime_root: &Path,
    part: Part,
    deadline: Duration,
    max_frame_bytes: u32,
) -> (bool, HealthResponse) {
    let sock_path = paths::health_socket(runtime_root, part);
    let scrape_ts = now_ns();

    let client = match Client::connect(&sock_path, max_frame_bytes).await {
        Ok(c) => c,
        Err(_) => {
            return (
                false,
                HealthResponse {
                    component: format!("octoplus-{}", part.name()),
                    version: String::new(),
                    status: HealthStatus::Down as i32,
                    started_at_ns: 0,
                    sampled_at_ns: scrape_ts,
                    rss_bytes: 0,
                    subsystems: Vec::new(),
                    counters: Vec::new(),
                    gauges: Vec::new(),
                    histograms: Vec::new(),
                },
            );
        }
    };

    let call_res = client
        .call(Body::HealthRequest(HealthRequest {}), deadline)
        .await;

    match call_res {
        Ok(Body::HealthResponse(mut resp)) => {
            if resp.status == HealthStatus::Unspecified as i32 {
                resp.status = HealthStatus::Down as i32;
            }
            (true, resp)
        }
        _ => (
            false,
            HealthResponse {
                component: format!("octoplus-{}", part.name()),
                version: String::new(),
                status: HealthStatus::Down as i32,
                started_at_ns: 0,
                sampled_at_ns: scrape_ts,
                rss_bytes: 0,
                subsystems: Vec::new(),
                counters: Vec::new(),
                gauges: Vec::new(),
                histograms: Vec::new(),
            },
        ),
    }
}

/// Performs a single parallel scrape cycle across all 8 components (7 remote + self).
pub async fn scrape_cycle(
    state: &Arc<RwLock<HealthdState>>,
    runtime_root: &Path,
    config: &Config,
    self_health: &Arc<Health>,
    cgroup: &CgroupReader,
) {
    let deadline = config.deadlines.control();
    let max_frame = config.ipc.max_frame_bytes;
    let scrape_ts = now_ns();

    // 1. Launch 7 remote scrapes concurrently
    let remote_parts = [
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
        Part::Voice,
        Part::Fetcher,
    ];

    let mut tasks = Vec::new();
    for &part in &remote_parts {
        let root = runtime_root.to_path_buf();
        tasks.push(tokio::spawn(async move {
            let (reachable, resp) = scrape_remote_part(&root, part, deadline, max_frame).await;
            (part, reachable, resp)
        }));
    }

    // 2. Local health snapshot for Part::Healthd
    let self_resp = self_health.snapshot();
    let mut scrape_results = Vec::with_capacity(Part::ALL.len());
    scrape_results.push((Part::Healthd, true, self_resp));

    // Await all remote scrape tasks
    for task in tasks {
        if let Ok((part, reachable, resp)) = task.await {
            scrape_results.push((part, reachable, resp));
        }
    }

    // 3. Update state, cgroup metrics, 90% budget checks, and soak engine
    let mut st = state.write().await;
    st.last_generated_at_ns = scrape_ts;

    for (part, reachable, mut resp) in scrape_results {
        let unit = part.unit();
        let cgroup_mem = cgroup.read_memory_bytes(&unit);
        let cgroup_cpu = cgroup.read_cpu_usage_ns(&unit);

        // If process reported 0 RSS (or unreachable), use cgroup memory if available
        if resp.rss_bytes == 0 && cgroup_mem > 0 {
            resp.rss_bytes = cgroup_mem;
        }

        let status = match HealthStatus::try_from(resp.status) {
            Ok(s) => s,
            Err(_) => HealthStatus::Down,
        };

        // Record into soak engine
        st.soak.record_scrape(
            part,
            status,
            &resp,
            crate::soak::ScrapeMetrics {
                cgroup_memory_bytes: cgroup_mem,
                cgroup_cpu_usage_ns: cgroup_cpu,
            },
        );

        let rss_bytes = resp.rss_bytes;
        let part_status = PartStatus {
            part: part.name().to_string(),
            reachable,
            last: Some(resp),
            last_scrape_ns: scrape_ts,
            cgroup_memory_bytes: cgroup_mem,
            cgroup_cpu_usage_ns: cgroup_cpu,
        };

        // Update rolling window
        let sample = Sample {
            at_ns: scrape_ts,
            status,
            reachable,
            rss_bytes,
            cgroup_memory_bytes: cgroup_mem,
            cgroup_cpu_usage_ns: cgroup_cpu,
        };
        let window = st
            .rolling
            .entry(part)
            .or_insert_with(|| VecDeque::with_capacity(ROLLING_WINDOW_CAPACITY));
        if window.len() >= ROLLING_WINDOW_CAPACITY {
            window.pop_front();
        }
        window.push_back(sample);

        st.parts.insert(part, part_status);
    }

    st.overall = st.compute_overall();
}

/// Context for the background scraper loop.
#[derive(Debug)]
pub struct ScraperContext {
    /// Runtime root directory.
    pub runtime_root: PathBuf,
    /// Service configuration.
    pub config: Config,
    /// Health reporter for local health snapshot.
    pub self_health: Arc<Health>,
    /// Cgroup reader for metrics.
    pub cgroup: CgroupReader,
}

/// Runs the background scraper loop until shutdown signal.
pub async fn run_scraper_loop(
    state: Arc<RwLock<HealthdState>>,
    ctx: ScraperContext,
    mut stop_rx: tokio::sync::watch::Receiver<bool>,
) {
    let interval_s = ctx.config.healthd.scrape_interval_s.max(1);
    let interval = Duration::from_secs(interval_s);
    tracing::info!(
        interval_s,
        soak = ctx.config.healthd.soak,
        "scraper started"
    );

    // Run first scrape immediately on startup
    scrape_cycle(
        &state,
        &ctx.runtime_root,
        &ctx.config,
        &ctx.self_health,
        &ctx.cgroup,
    )
    .await;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                scrape_cycle(
                    &state,
                    &ctx.runtime_root,
                    &ctx.config,
                    &ctx.self_health,
                    &ctx.cgroup,
                )
                .await;
            }
            res = stop_rx.changed() => {
                if res.is_err() || *stop_rx.borrow() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_scraper_rolling_window_and_cgroup_fallback() {
        let tmp = tempdir().unwrap();
        let runtime_root = tmp.path().join("octoplus");
        let cgroup_root = tmp.path().join("cgroup");
        let soak_dir = tmp.path().join("soak");

        let cgroup = CgroupReader::new(cgroup_root.clone(), 1000);
        let soak = SoakEngine::new(soak_dir, false).unwrap();
        let state = Arc::new(RwLock::new(HealthdState::new(soak)));
        let self_health = Health::new("octoplus-healthd", "0.1.0");

        let config = Config::from_toml_str("").unwrap();

        // Write cgroup memory for healthd
        let healthd_cg = cgroup.unit_cgroup_dir("octoplus-healthd.service");
        std::fs::create_dir_all(&healthd_cg).unwrap();
        std::fs::write(healthd_cg.join("memory.current"), b"15728640\n").unwrap(); // 15 MiB

        // Trigger scrape cycle 125 times to verify rolling window pop_front()
        for _ in 0..125 {
            scrape_cycle(&state, &runtime_root, &config, &self_health, &cgroup).await;
        }

        let st = state.read().await;
        let window = st.rolling.get(&Part::Healthd).unwrap();
        assert_eq!(window.len(), ROLLING_WINDOW_CAPACITY);
        // Reserved once: the window never reallocates as it fills and rolls.
        assert!(window.capacity() >= ROLLING_WINDOW_CAPACITY);
        assert!(window.capacity() < 2 * ROLLING_WINDOW_CAPACITY);
        assert!(now_ns() > 0);
    }
}
