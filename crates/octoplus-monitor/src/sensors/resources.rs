//! CPU, memory and the largest process (FR-MON-04, P3-S03). Parsers are pure; the sample is
//! collected on the blocking pool so a slow `/proc` walk never stalls the runtime (AB-11).

use std::path::{Path, PathBuf};
use std::time::Duration;

use octoplus_common::config::MonitorConfig;
use octoplus_proto::v1::ResourceSample;
use octoplus_proto::v1::monitor_event::Event;
use tokio::time::MissedTickBehavior;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// Cumulative CPU time from the aggregate `cpu` line of `/proc/stat`, in clock ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    /// Ticks spent working (everything except idle and iowait).
    pub busy: u64,
    /// All ticks.
    pub total: u64,
}

/// Parses the first (`cpu `) line of `/proc/stat`. Guest time is already inside user time,
/// so only the first eight counters are summed.
#[must_use]
pub fn parse_cpu_times(stat: &str) -> Option<CpuTimes> {
    let line = stat.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let v: Vec<u64> = parts
        .take(8)
        .map(|f| f.parse().ok())
        .collect::<Option<_>>()?;
    if v.len() < 5 {
        return None;
    }
    let total = v.iter().fold(0u64, |a, b| a.saturating_add(*b));
    let idle = v[3].saturating_add(v[4]);
    Some(CpuTimes {
        busy: total.saturating_sub(idle),
        total,
    })
}

/// CPU use between two snapshots, 0–100. `None` when time did not advance or a counter
/// went backwards (wrap, or a hot-unplugged CPU): that interval is skipped, not guessed.
#[must_use]
pub fn cpu_percent(prev: CpuTimes, cur: CpuTimes) -> Option<f64> {
    if cur.total <= prev.total || cur.busy < prev.busy {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // tick counts are far below 2^52
    let pct = (cur.busy - prev.busy) as f64 / (cur.total - prev.total) as f64 * 100.0;
    Some(pct.clamp(0.0, 100.0))
}

/// `(MemTotal, MemAvailable)` in bytes from `/proc/meminfo`.
#[must_use]
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(name))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kb.checked_mul(1024)
    };
    let total = field("MemTotal:")?;
    let avail = field("MemAvailable:")?;
    (avail <= total).then_some((total, avail))
}

/// The process using the most resident memory, with its (kernel-truncated) name. A process
/// that vanishes mid-scan is skipped.
#[must_use]
pub fn top_process(proc_root: &Path, page: usize) -> Option<(String, u64)> {
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(proc_root).ok()?.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let Ok(statm) = std::fs::read_to_string(entry.path().join("statm")) else {
            continue;
        };
        if let Some(rss) = octoplus_health::parse_statm_rss(&statm, page)
            && best.as_ref().is_none_or(|(b, _)| rss > *b)
        {
            best = Some((rss, entry.path()));
        }
    }
    let (rss, dir) = best?;
    let comm = std::fs::read_to_string(dir.join("comm")).ok()?;
    Some((comm.trim_end().to_owned(), rss))
}

/// Reads CPU counters, memory and the top process once.
fn collect(proc_root: &Path) -> Option<(CpuTimes, ResourceSample)> {
    let cpu = parse_cpu_times(&std::fs::read_to_string(proc_root.join("stat")).ok()?)?;
    let (total, avail) = parse_meminfo(&std::fs::read_to_string(proc_root.join("meminfo")).ok()?)?;
    let (name, rss) = top_process(proc_root, rustix::param::page_size()).unwrap_or_default();
    Some((
        cpu,
        ResourceSample {
            cpu_percent: 0.0,
            mem_total_bytes: total,
            mem_available_bytes: avail,
            top_process: name,
            top_process_rss_bytes: rss,
        },
    ))
}

/// Samples CPU and memory every `cpu_interval_s` (FR-MON-04).
#[derive(Debug)]
pub struct ResourcesSensor {
    proc_root: PathBuf,
    interval: Duration,
}

impl ResourcesSensor {
    /// The real `/proc`.
    #[must_use]
    pub fn new(cfg: &MonitorConfig) -> Self {
        Self::with_root(cfg, Path::new("/"))
    }

    /// A tree rooted at `root` (`<root>/proc/...`), for tests.
    #[must_use]
    pub fn with_root(cfg: &MonitorConfig, root: &Path) -> Self {
        Self {
            proc_root: root.join("proc"),
            interval: Duration::from_secs(cfg.cpu_interval_s),
        }
    }
}

impl Sensor for ResourcesSensor {
    fn name(&self) -> &'static str {
        "resources"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let mut tick = tokio::time::interval(self.interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut prev: Option<CpuTimes> = None;
        loop {
            tick.tick().await;
            let root = self.proc_root.clone();
            let sampled = tokio::task::spawn_blocking(move || collect(&root))
                .await
                .map_err(|_| SensorError::Unavailable("resource scan failed"))?;
            // One unreadable or odd reading must not end the sensor (review F6): say so and try
            // again at the next tick. A /proc that stays unreadable keeps the sensor DEGRADED.
            let Some((cpu, mut sample)) = sampled else {
                out.degraded("/proc stat or meminfo unreadable");
                continue;
            };
            // The first tick only sets the baseline: a percentage needs two readings.
            if let Some(pct) = prev.and_then(|p| cpu_percent(p, cpu)) {
                sample.cpu_percent = pct;
                // Stamped when the reading is complete: the walk of /proc is the observation.
                out.emit(out.now_ns(), Event::Resources(sample));
            } else {
                out.set_ok();
            }
            prev = Some(cpu);
        }
    }
}
