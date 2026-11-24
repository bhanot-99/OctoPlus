//! Soak engine tracking baseline RSS and p99 drift against NFR-REL-04 (FR-HLT-04).
//!
//! NFR-REL-04:
//! - 24 h soak: RSS growth < 5%
//! - p99 drift < 10%
//! - Zero crashes / restarts
//!
//! Evaluates per-part metrics against baseline and appends records to `soak/soak-<UTC>.csv`.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use octoplus_common::paths::Part;
use octoplus_proto::v1::{HealthResponse, HealthStatus, Histogram};
use tracing::{info, warn};

/// Maximum allowed RSS growth percentage under NFR-REL-04.
pub const MAX_SOAK_RSS_GROWTH_PERCENT: f64 = 5.0;

/// Maximum allowed p99 drift percentage under NFR-REL-04.
pub const MAX_SOAK_P99_DRIFT_PERCENT: f64 = 10.0;

/// Number of initial samples used to establish steady-state baseline.
pub const SOAK_BASELINE_SAMPLES: usize = 5;

/// Computes the 99th percentile (p99) latency in nanoseconds from a latency histogram.
///
/// Returns 0 if there are no samples.
#[must_use]
pub fn compute_p99_ns(hist: &Histogram) -> u64 {
    let total: u64 = if hist.sample_count > 0 {
        hist.sample_count
    } else {
        hist.counts.iter().sum()
    };
    if total == 0 || hist.counts.is_empty() {
        return 0;
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let target = ((total as f64) * 0.99).ceil() as u64;
    let target = target.max(1);

    let mut cum: u64 = 0;
    for (i, &cnt) in hist.counts.iter().enumerate() {
        cum = cum.saturating_add(cnt);
        if cum >= target {
            if i < hist.upper_bounds_ns.len() {
                return hist.upper_bounds_ns[i];
            }
            return hist.upper_bounds_ns.last().copied().unwrap_or(0);
        }
    }

    hist.upper_bounds_ns.last().copied().unwrap_or(0)
}

/// Computes the primary p99 latency across all histograms reported by a component.
#[must_use]
pub fn extract_primary_p99_ns(last: &HealthResponse) -> u64 {
    // Look for decode or query or request histograms, or take the first non-empty
    for hist in &last.histograms {
        if hist.name.contains("decode")
            || hist.name.contains("query")
            || hist.name.contains("latency")
            || hist.name.contains("request")
        {
            let p99 = compute_p99_ns(hist);
            if p99 > 0 {
                return p99;
            }
        }
    }
    for hist in &last.histograms {
        let p99 = compute_p99_ns(hist);
        if p99 > 0 {
            return p99;
        }
    }
    0
}

/// Baseline tracking state for a single component.
#[derive(Debug, Clone, Default)]
pub struct PartBaseline {
    /// Initial process start timestamp.
    pub initial_started_at_ns: u64,
    /// Number of restarts detected (`started_at_ns` changed).
    pub restart_count: u64,
    /// Accumulator for initial RSS samples.
    pub initial_rss_samples: Vec<u64>,
    /// Established baseline RSS in bytes.
    pub baseline_rss_bytes: Option<u64>,
    /// Accumulator for initial p99 latency samples.
    pub initial_p99_samples: Vec<u64>,
    /// Established baseline p99 latency in nanoseconds.
    pub baseline_p99_ns: Option<u64>,
    /// Maximum RSS growth percentage observed so far.
    pub max_rss_growth_pct: f64,
    /// Maximum p99 drift percentage observed so far.
    pub max_p99_drift_pct: f64,
}

impl PartBaseline {
    /// Ingests a new sample, returns (rss_growth_pct, p99_drift_pct, restarted).
    pub fn record_sample(&mut self, last: &HealthResponse, p99_ns: u64) -> (f64, f64, bool) {
        let mut restarted = false;
        if last.started_at_ns > 0 {
            if self.initial_started_at_ns == 0 {
                self.initial_started_at_ns = last.started_at_ns;
            } else if last.started_at_ns != self.initial_started_at_ns {
                self.restart_count = self.restart_count.saturating_add(1);
                self.initial_started_at_ns = last.started_at_ns;
                restarted = true;
            }
        }

        // Establish or update RSS baseline
        if self.baseline_rss_bytes.is_none() && last.rss_bytes > 0 {
            self.initial_rss_samples.push(last.rss_bytes);
            if self.initial_rss_samples.len() >= SOAK_BASELINE_SAMPLES {
                let mut sorted = self.initial_rss_samples.clone();
                sorted.sort_unstable();
                self.baseline_rss_bytes = Some(sorted[sorted.len() / 2]);
            }
        }

        // Establish or update p99 baseline
        if self.baseline_p99_ns.is_none() && p99_ns > 0 {
            self.initial_p99_samples.push(p99_ns);
            if self.initial_p99_samples.len() >= SOAK_BASELINE_SAMPLES {
                let mut sorted = self.initial_p99_samples.clone();
                sorted.sort_unstable();
                self.baseline_p99_ns = Some(sorted[sorted.len() / 2]);
            }
        }

        // Compute RSS growth percentage
        let rss_growth_pct = match self.baseline_rss_bytes {
            Some(base) if base > 0 && last.rss_bytes > base => {
                #[allow(clippy::cast_precision_loss)]
                let pct = ((last.rss_bytes - base) as f64 / base as f64) * 100.0;
                self.max_rss_growth_pct = self.max_rss_growth_pct.max(pct);
                pct
            }
            _ => 0.0,
        };

        // Compute p99 drift percentage
        let p99_drift_pct = match self.baseline_p99_ns {
            Some(base) if base > 0 && p99_ns > base => {
                #[allow(clippy::cast_precision_loss)]
                let pct = ((p99_ns - base) as f64 / base as f64) * 100.0;
                self.max_p99_drift_pct = self.max_p99_drift_pct.max(pct);
                pct
            }
            _ => 0.0,
        };

        (rss_growth_pct, p99_drift_pct, restarted)
    }
}

/// Cgroup metrics collected alongside scrape.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrapeMetrics {
    /// Cgroup memory in bytes.
    pub cgroup_memory_bytes: u64,
    /// Cgroup CPU usage in nanoseconds.
    pub cgroup_cpu_usage_ns: u64,
}

/// Manages soak baseline metrics and writes the soak CSV file.
#[derive(Debug)]
pub struct SoakEngine {
    soak_dir: PathBuf,
    csv_path: PathBuf,
    csv_file: Option<File>,
    baselines: HashMap<Part, PartBaseline>,
    enabled: bool,
}

impl SoakEngine {
    /// Creates a new soak engine.
    ///
    /// If `enabled` is true, creates `soak_dir` if needed and initializes `<soak_dir>/soak-<UTC>.csv`.
    pub fn new(soak_dir: PathBuf, enabled: bool) -> std::io::Result<Self> {
        let mut csv_file = None;
        let mut csv_path = PathBuf::new();

        if enabled {
            std::fs::create_dir_all(&soak_dir)?;
            let now = Timestamp::now();
            let utc_str = now.strftime("%Y%m%dT%H%M%SZ").to_string();
            csv_path = soak_dir.join(format!("soak-{utc_str}.csv"));

            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&csv_path)?;

            writeln!(
                f,
                "timestamp_utc,part,status,rss_bytes,baseline_rss_bytes,rss_growth_pct,p99_ns,baseline_p99_ns,p99_drift_pct,restarts,cgroup_memory_bytes,cgroup_cpu_usage_ns"
            )?;
            f.flush()?;
            info!(path = %csv_path.display(), "soak CSV initialized");
            csv_file = Some(f);
        }

        Ok(Self {
            soak_dir,
            csv_path,
            csv_file,
            baselines: HashMap::new(),
            enabled,
        })
    }

    /// Whether soak tracking is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Path to the soak directory.
    #[must_use]
    pub fn soak_dir(&self) -> &Path {
        &self.soak_dir
    }

    /// Path to the current soak CSV file.
    #[must_use]
    pub fn csv_path(&self) -> &Path {
        &self.csv_path
    }

    /// Records scrape metrics for one component and appends a row to the soak CSV if enabled.
    pub fn record_scrape(
        &mut self,
        part: Part,
        status: HealthStatus,
        last: &HealthResponse,
        metrics: ScrapeMetrics,
    ) {
        if !self.enabled {
            return;
        }

        let p99_ns = extract_primary_p99_ns(last);
        let baseline = self.baselines.entry(part).or_default();
        let (growth_pct, drift_pct, restarted) = baseline.record_sample(last, p99_ns);

        if restarted {
            warn!(
                part = part.name(),
                restarts = baseline.restart_count,
                "NFR-REL-04 violation: component restarted during soak"
            );
        }
        if growth_pct > MAX_SOAK_RSS_GROWTH_PERCENT {
            warn!(
                part = part.name(),
                growth_pct,
                max = MAX_SOAK_RSS_GROWTH_PERCENT,
                "NFR-REL-04 violation: RSS growth exceeded 5%"
            );
        }
        if drift_pct > MAX_SOAK_P99_DRIFT_PERCENT {
            warn!(
                part = part.name(),
                drift_pct,
                max = MAX_SOAK_P99_DRIFT_PERCENT,
                "NFR-REL-04 violation: p99 latency drift exceeded 10%"
            );
        }

        if let Some(f) = &mut self.csv_file {
            let now = Timestamp::now().to_string();
            let base_rss = baseline.baseline_rss_bytes.unwrap_or(0);
            let base_p99 = baseline.baseline_p99_ns.unwrap_or(0);
            let status_str = match status {
                HealthStatus::Ok => "OK",
                HealthStatus::Degraded => "DEGRADED",
                HealthStatus::Down => "DOWN",
                HealthStatus::Starting => "STARTING",
                HealthStatus::Unspecified => "UNSPECIFIED",
            };

            let _ = writeln!(
                f,
                "{now},{part_name},{status_str},{rss},{base_rss},{growth_pct:.2},{p99_ns},{base_p99},{drift_pct:.2},{restarts},{cgroup_memory_bytes},{cgroup_cpu_usage_ns}",
                part_name = part.name(),
                rss = last.rss_bytes,
                restarts = baseline.restart_count,
                cgroup_memory_bytes = metrics.cgroup_memory_bytes,
                cgroup_cpu_usage_ns = metrics.cgroup_cpu_usage_ns,
            );
            let _ = f.flush();
        }
    }

    /// Returns the baseline tracking state for `part`.
    #[must_use]
    pub fn get_baseline(&self, part: Part) -> Option<&PartBaseline> {
        self.baselines.get(&part)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_compute_p99_ns() {
        let hist = Histogram {
            name: "test_latency".to_string(),
            upper_bounds_ns: vec![1_000, 5_000, 10_000, 50_000],
            counts: vec![90, 8, 2, 0, 0], // total = 100 samples
            sample_count: 100,
            sum_ns: 120_000,
        };
        // 99th sample is in bucket 2 (upper_bound = 10_000)
        assert_eq!(compute_p99_ns(&hist), 10_000);

        let empty = Histogram::default();
        assert_eq!(compute_p99_ns(&empty), 0);
    }

    #[test]
    fn test_soak_engine_records_and_detects_violations() {
        let tmp = tempdir().unwrap();
        let mut engine = SoakEngine::new(tmp.path().to_path_buf(), true).unwrap();
        assert!(engine.is_enabled());
        assert!(engine.csv_path().exists());

        let part = Part::Storage;
        let mut resp = HealthResponse {
            component: "octoplus-storage".to_string(),
            version: "0.1.0".to_string(),
            status: HealthStatus::Ok as i32,
            started_at_ns: 1_000_000,
            sampled_at_ns: 1_000_100,
            rss_bytes: 100_000_000,
            subsystems: Vec::new(),
            counters: Vec::new(),
            gauges: Vec::new(),
            histograms: vec![Histogram {
                name: "query_latency".to_string(),
                upper_bounds_ns: vec![10_000_000, 20_000_000],
                counts: vec![100, 0, 0],
                sample_count: 100,
                sum_ns: 500_000_000,
            }],
        };

        let metrics = ScrapeMetrics {
            cgroup_memory_bytes: 100_000_000,
            cgroup_cpu_usage_ns: 5_000_000,
        };

        // Send baseline samples
        for _ in 0..5 {
            engine.record_scrape(part, HealthStatus::Ok, &resp, metrics);
        }

        let base = engine.get_baseline(part).unwrap();
        assert_eq!(base.baseline_rss_bytes, Some(100_000_000));
        assert_eq!(base.baseline_p99_ns, Some(10_000_000));
        assert_eq!(base.restart_count, 0);

        // Send sample with 6% growth (> 5%)
        resp.rss_bytes = 106_000_000;
        engine.record_scrape(
            part,
            HealthStatus::Ok,
            &resp,
            ScrapeMetrics {
                cgroup_memory_bytes: 106_000_000,
                cgroup_cpu_usage_ns: 6_000_000,
            },
        );

        let base = engine.get_baseline(part).unwrap();
        assert!(base.max_rss_growth_pct > 5.0);

        // Send sample with restarted process
        resp.started_at_ns = 2_000_000;
        engine.record_scrape(
            part,
            HealthStatus::Ok,
            &resp,
            ScrapeMetrics {
                cgroup_memory_bytes: 106_000_000,
                cgroup_cpu_usage_ns: 7_000_000,
            },
        );

        let base = engine.get_baseline(part).unwrap();
        assert_eq!(base.restart_count, 1);

        // Verify CSV contents
        let csv_data = std::fs::read_to_string(engine.csv_path()).unwrap();
        assert!(csv_data.contains("timestamp_utc,part,status"));
        assert!(csv_data.contains("storage,OK,106000000"));
    }
}
