//! Implementation of `neuroosctl status` (FR-CLI-01, FR-CLI-02, design.md §7).

use std::path::Path;
use std::time::Duration;

use neuroos_common::paths::{self, Part, TOTAL_RSS_BUDGET_MIB};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{HealthResponse, HealthStatus, StatusReport, StatusRequest};
use serde::Serialize;

use crate::style::Styler;

/// JSON schema representation of `neuroosctl status --json`.
#[derive(Debug, Serialize)]
pub struct StatusJson {
    pub version: String,
    pub overall: String,
    pub healthy_count: usize,
    pub total_count: usize,
    pub total_rss_mib: u64,
    pub total_budget_mib: u64,
    pub components: Vec<ComponentJson>,
}

/// JSON representation of a single component.
#[derive(Debug, Serialize)]
pub struct ComponentJson {
    pub name: String,
    pub status: String,
    pub reachable: bool,
    pub rss_bytes: u64,
    pub budget_mib: u64,
    pub p99_ns: u64,
    pub uptime_ns: u64,
    pub cgroup_memory_bytes: u64,
    pub cgroup_cpu_usage_ns: u64,
    /// The part's own health rows (for C1: one per sensor), with fixed-text details.
    pub subsystems: Vec<SubsystemJson>,
    /// The part's latency histograms, so a report can show the whole curve, not one number.
    pub histograms: Vec<HistogramJson>,
}

/// JSON representation of one latency histogram.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HistogramJson {
    pub name: String,
    pub sample_count: u64,
    pub p99_ns: u64,
    pub upper_bounds_ns: Vec<u64>,
    pub counts: Vec<u64>,
}

/// A part's histograms with their own p99.
#[must_use]
pub fn histogram_rows(resp: Option<&HealthResponse>) -> Vec<HistogramJson> {
    resp.map(|r| {
        r.histograms
            .iter()
            .map(|h| HistogramJson {
                name: h.name.clone(),
                sample_count: h.sample_count,
                p99_ns: compute_p99_ns(h),
                upper_bounds_ns: h.upper_bounds_ns.clone(),
                counts: h.counts.clone(),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// JSON representation of one subsystem row.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SubsystemJson {
    pub name: String,
    pub status: String,
    pub detail: String,
}

/// The status word for a health status.
#[must_use]
pub fn status_word(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Ok => "OK",
        HealthStatus::Degraded => "DEGRADED",
        HealthStatus::Down => "DOWN",
        HealthStatus::Starting => "STARTING",
        HealthStatus::Unspecified => "UNSPECIFIED",
    }
}

/// A part's subsystem rows, in the order the part reported them.
#[must_use]
pub fn subsystem_rows(resp: Option<&HealthResponse>) -> Vec<SubsystemJson> {
    resp.map(|r| {
        r.subsystems
            .iter()
            .map(|s| SubsystemJson {
                name: s.name.clone(),
                status: status_word(HealthStatus::try_from(s.status).unwrap_or(HealthStatus::Down))
                    .to_owned(),
                detail: s.detail.clone(),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Formats duration in seconds into human-readable uptime (e.g. `3d 04h`, `12m 30s`).
#[must_use]
pub fn format_uptime(uptime_secs: u64) -> String {
    let days = uptime_secs / 86400;
    let hours = (uptime_secs % 86400) / 3600;
    let mins = (uptime_secs % 3600) / 60;
    let secs = uptime_secs % 60;

    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else if mins > 0 {
        format!("{mins}m {secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// Computes primary p99 latency in nanoseconds from a health response.
#[must_use]
pub fn extract_p99_ns(resp: &HealthResponse) -> u64 {
    for hist in &resp.histograms {
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
    for hist in &resp.histograms {
        let p99 = compute_p99_ns(hist);
        if p99 > 0 {
            return p99;
        }
    }
    0
}

fn compute_p99_ns(hist: &neuroos_proto::v1::Histogram) -> u64 {
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

/// Formats p99 latency for human display.
#[must_use]
pub fn format_p99(part_name: &str, p99_ns: u64) -> String {
    if p99_ns == 0 {
        return "—".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let ms = (p99_ns as f64) / 1_000_000.0;
    if part_name == "inference" {
        format!("{ms:.1} ms/tok")
    } else if ms < 1.0 {
        #[allow(clippy::cast_precision_loss)]
        let us = (p99_ns as f64) / 1_000.0;
        format!("{us:.0} µs")
    } else {
        format!("{ms:.1} ms")
    }
}

/// Fetches status report from healthd.
pub async fn fetch_status_report(
    runtime_root: &Path,
    timeout: Duration,
) -> Result<StatusReport, String> {
    let sock = paths::socket_path(runtime_root, Part::Healthd, "healthd");
    let client = Client::connect(&sock, 1024 * 1024)
        .await
        .map_err(|e| format!("connect error: {e}"))?;

    let res = client
        .call(Body::StatusRequest(StatusRequest {}), timeout)
        .await
        .map_err(|e| format!("call error: {e}"))?;

    match res {
        Body::StatusReport(rep) => Ok(rep),
        _ => Err("unexpected response frame from healthd".to_string()),
    }
}

/// Runs the `status` command, prints output to stdout/stderr, returns the appropriate exit code.
pub async fn run_status(runtime_root: &Path, json: bool, styler: Styler) -> i32 {
    let report = match fetch_status_report(runtime_root, Duration::from_secs(2)).await {
        Ok(r) => r,
        Err(_) => {
            eprintln!("{}", styler.error("could not connect to neuroos-healthd"));
            eprintln!(
                "{}",
                styler.hint("start the service with 'systemctl --user start neuroos-healthd'")
            );
            return 1;
        }
    };

    let overall_status = HealthStatus::try_from(report.overall).unwrap_or(HealthStatus::Down);

    // Compute stats across parts
    let mut healthy_count = 0;
    let total_count = report.parts.len();
    let mut total_rss_bytes = 0;
    let mut components_json = Vec::new();
    let mut first_unhealthy = None;

    for part_status in &report.parts {
        let (status, rss, started_at, p99_ns) = match &part_status.last {
            Some(l) => {
                let st = HealthStatus::try_from(l.status).unwrap_or(HealthStatus::Down);
                let p99 = extract_p99_ns(l);
                (st, l.rss_bytes, l.started_at_ns, p99)
            }
            None => (HealthStatus::Down, 0, 0, 0),
        };

        if status == HealthStatus::Ok {
            healthy_count += 1;
        } else if first_unhealthy.is_none() {
            first_unhealthy = Some(part_status.part.clone());
        }

        total_rss_bytes += rss;

        let part_obj = Part::ALL.iter().find(|p| p.name() == part_status.part);
        let budget_mib = part_obj.map_or(0, |p| p.rss_budget_mib());

        let uptime_ns = if started_at > 0 && report.generated_at_ns >= started_at {
            report.generated_at_ns - started_at
        } else {
            0
        };

        components_json.push(ComponentJson {
            name: format!("neuroos-{}", part_status.part),
            status: match status {
                HealthStatus::Ok => "OK".to_string(),
                HealthStatus::Degraded => "DEGRADED".to_string(),
                HealthStatus::Down => "DOWN".to_string(),
                HealthStatus::Starting => "STARTING".to_string(),
                HealthStatus::Unspecified => "UNSPECIFIED".to_string(),
            },
            reachable: part_status.reachable,
            rss_bytes: rss,
            budget_mib,
            p99_ns,
            uptime_ns,
            cgroup_memory_bytes: part_status.cgroup_memory_bytes,
            cgroup_cpu_usage_ns: part_status.cgroup_cpu_usage_ns,
            subsystems: subsystem_rows(part_status.last.as_ref()),
            histograms: histogram_rows(part_status.last.as_ref()),
        });
    }

    let total_rss_mib = total_rss_bytes / (1024 * 1024);

    if json {
        let out = StatusJson {
            version: env!("CARGO_PKG_VERSION").to_string(),
            overall: match overall_status {
                HealthStatus::Ok => "OK".to_string(),
                HealthStatus::Degraded => "DEGRADED".to_string(),
                HealthStatus::Down => "DOWN".to_string(),
                HealthStatus::Starting => "STARTING".to_string(),
                HealthStatus::Unspecified => "UNSPECIFIED".to_string(),
            },
            healthy_count,
            total_count,
            total_rss_mib,
            total_budget_mib: TOTAL_RSS_BUDGET_MIB,
            components: components_json,
        };
        if let Ok(json_str) = serde_json::to_string_pretty(&out) {
            println!("{json_str}");
        }
    } else {
        // Table text format
        println!(
            "NeuroOS  v{}  ·  healthy {}/{}  ·  total RSS {} / {} MiB\n",
            env!("CARGO_PKG_VERSION"),
            healthy_count,
            total_count,
            total_rss_mib,
            TOTAL_RSS_BUDGET_MIB
        );

        println!(
            "{:<20} {:<12} {:<16} {:<10} {:<10}",
            styler.bold("COMPONENT"),
            styler.bold("STATUS"),
            styler.bold("RSS / BUDGET"),
            styler.bold("P99"),
            styler.bold("UPTIME")
        );

        for part_status in &report.parts {
            let comp_name = format!("neuroos-{}", part_status.part);
            let (status, rss_bytes, started_at, p99_ns) = match &part_status.last {
                Some(l) => {
                    let st = HealthStatus::try_from(l.status).unwrap_or(HealthStatus::Down);
                    (st, l.rss_bytes, l.started_at_ns, extract_p99_ns(l))
                }
                None => (HealthStatus::Down, 0, 0, 0),
            };

            let part_obj = Part::ALL.iter().find(|p| p.name() == part_status.part);
            let budget_mib = part_obj.map_or(0, |p| p.rss_budget_mib());

            let status_cell = match status {
                HealthStatus::Ok => styler.ok("✔ OK"),
                HealthStatus::Degraded => styler.degraded("▲ DEGRADED"),
                HealthStatus::Down => styler.down("✖ DOWN"),
                HealthStatus::Starting => styler.dim("… STARTING"),
                HealthStatus::Unspecified => styler.dim("… UNKNOWN"),
            };

            let rss_mib = rss_bytes / (1024 * 1024);
            let rss_cell = if part_status.reachable && rss_bytes > 0 {
                format!("{:>4} / {:>4} MiB", rss_mib, budget_mib)
            } else {
                format!("   — / {:>4} MiB", budget_mib)
            };

            let p99_cell = format_p99(&part_status.part, p99_ns);

            let uptime_cell = if part_status.reachable
                && started_at > 0
                && report.generated_at_ns >= started_at
            {
                let uptime_secs = (report.generated_at_ns - started_at) / 1_000_000_000;
                format_uptime(uptime_secs)
            } else {
                "—".to_string()
            };

            // If styling is enabled, the ANSI escapes add extra byte length, so pad using raw string length
            let status_plain = match status {
                HealthStatus::Ok => "✔ OK",
                HealthStatus::Degraded => "▲ DEGRADED",
                HealthStatus::Down => "✖ DOWN",
                HealthStatus::Starting => "… STARTING",
                HealthStatus::Unspecified => "… UNKNOWN",
            };
            let status_pad = " ".repeat(12_usize.saturating_sub(status_plain.chars().count()));

            println!(
                "{:<20} {}{} {:<16} {:<10} {:<10}",
                comp_name, status_cell, status_pad, rss_cell, p99_cell, uptime_cell
            );
        }

        // A part's own rows that are not healthy, e.g. one sensor of C1 (never user content).
        for part_status in report.parts.iter().filter(|p| p.reachable) {
            for row in subsystem_rows(part_status.last.as_ref())
                .iter()
                .filter(|r| r.status != "OK")
            {
                println!(
                    "  neuroos-{} › {}: {} — {}",
                    part_status.part, row.name, row.status, row.detail
                );
            }
        }

        if let Some(bad_part) = first_unhealthy {
            println!(
                "\n{}",
                styler.hint(&format!(
                    "run 'systemctl --user status neuroos-{bad_part}' for details"
                ))
            );
        }
    }

    match overall_status {
        HealthStatus::Ok => 0,
        HealthStatus::Degraded
        | HealthStatus::Down
        | HealthStatus::Starting
        | HealthStatus::Unspecified => 3,
    }
}
