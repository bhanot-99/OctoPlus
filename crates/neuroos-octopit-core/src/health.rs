//! Health state and snapshot model for the OctoPit dashboard.

use neuroos_common::paths::{Part, TOTAL_RSS_BUDGET_MIB};
use neuroos_proto::v1::{HealthResponse, HealthStatus, Histogram, StatusReport};

/// Health state of a component or subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartState {
    /// Part is functioning normally.
    Ok,
    /// Part is running with degraded functionality or failed subsystem.
    Degraded,
    /// Part is stopped, unreachable, or crashed.
    Down,
    /// Part is currently initializing.
    Starting,
}

/// A subsystem health row reported by a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemRow {
    /// Name of the subsystem or sensor.
    pub name: String,
    /// Health state of the subsystem.
    pub state: PartState,
    /// Short fixed reason or detail text.
    pub detail: String,
}

/// A presentation card representing one of the 8 NeuroOS parts.
#[derive(Debug, Clone, PartialEq)]
pub struct PartCard {
    /// The NeuroOS part identity.
    pub part: neuroos_common::paths::Part,
    /// Plain user-facing title.
    pub title: &'static str,
    /// One plain sentence describing the role of this part.
    pub role: &'static str,
    /// Health state of the part (Down when not reachable).
    pub state: PartState,
    /// Whether the part is reachable by healthd.
    pub reachable: bool,
    /// Resident memory usage in bytes.
    pub rss_bytes: u64,
    /// Resident memory budget in MiB (`Part::rss_budget_mib`).
    pub budget_mib: u64,
    /// 99th percentile latency in nanoseconds.
    pub p99_ns: u64,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Subsystem status rows reported by this part.
    pub subsystems: Vec<SubsystemRow>,
}

impl PartCard {
    /// Ratio of resident memory usage to memory budget.
    ///
    /// Clamped to `0.0..=2.0`. Returns `0.0` if `budget_mib` is zero.
    #[must_use]
    pub fn rss_fraction(&self) -> f64 {
        if self.budget_mib == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let budget_bytes = (self.budget_mib as f64) * 1_048_576.0;
        #[allow(clippy::cast_precision_loss)]
        let fraction = (self.rss_bytes as f64) / budget_bytes;
        if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 2.0)
        }
    }
}

/// Top-level summary headline for the dashboard overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Headline {
    /// Every part is healthy.
    Healthy,
    /// First non-Ok part in `Part::ALL` order requiring attention.
    Attention {
        /// The part requiring attention.
        part: neuroos_common::paths::Part,
        /// The current state of the part.
        state: PartState,
    },
    /// No parts known at all.
    Unreachable,
}

/// Point-in-time snapshot of the whole system health.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Overall system health state.
    pub overall: PartState,
    /// All 8 parts in `Part::ALL` order.
    pub parts: Vec<PartCard>,
    /// Number of parts in `PartState::Ok`.
    pub healthy: usize,
    /// Total number of parts (always 8).
    pub total: usize,
    /// Total resident memory across all parts in MiB.
    pub total_rss_mib: u64,
    /// Total system resident memory budget in MiB (`TOTAL_RSS_BUDGET_MIB`).
    pub total_budget_mib: u64,
    /// Timestamp when report was generated in nanoseconds.
    pub generated_at_ns: u64,
}

impl Snapshot {
    /// Converts a protobuf `StatusReport` from healthd into an OctoPit `Snapshot`.
    #[must_use]
    pub fn from_report(report: &StatusReport) -> Snapshot {
        let mut parts = Vec::with_capacity(Part::ALL.len());

        for &part in &Part::ALL {
            let part_name = part.name();
            let status_opt = report.parts.iter().find(|p| {
                p.part == part_name || p.part.strip_prefix("neuroos-") == Some(part_name)
            });

            let (title, role) = part_title_and_role(part);
            let budget_mib = part.rss_budget_mib();

            let card = match status_opt {
                Some(ps) => {
                    let reachable = ps.reachable;
                    let last = ps.last.as_ref();
                    let rss_bytes = last.map_or(0, |l| l.rss_bytes);
                    let p99_ns = last.map_or(0, extract_p99_ns);
                    let subsystems = extract_subsystems(last);

                    let (state, uptime_secs) = if !reachable {
                        (PartState::Down, 0)
                    } else if let Some(l) = last {
                        let st = match HealthStatus::try_from(l.status) {
                            Ok(HealthStatus::Ok) => PartState::Ok,
                            Ok(HealthStatus::Degraded) => PartState::Degraded,
                            Ok(HealthStatus::Starting) => PartState::Starting,
                            _ => PartState::Down,
                        };
                        let up = if l.started_at_ns > 0 && report.generated_at_ns >= l.started_at_ns
                        {
                            (report.generated_at_ns - l.started_at_ns) / 1_000_000_000
                        } else {
                            0
                        };
                        (st, up)
                    } else {
                        (PartState::Down, 0)
                    };

                    PartCard {
                        part,
                        title,
                        role,
                        state,
                        reachable,
                        rss_bytes,
                        budget_mib,
                        p99_ns,
                        uptime_secs,
                        subsystems,
                    }
                }
                None => PartCard {
                    part,
                    title,
                    role,
                    state: PartState::Down,
                    reachable: false,
                    rss_bytes: 0,
                    budget_mib,
                    p99_ns: 0,
                    uptime_secs: 0,
                    subsystems: Vec::new(),
                },
            };

            parts.push(card);
        }

        let healthy = parts.iter().filter(|p| p.state == PartState::Ok).count();
        let total = parts.len();

        let total_rss_bytes = parts
            .iter()
            .fold(0u64, |acc, p| acc.saturating_add(p.rss_bytes));
        let total_rss_mib = total_rss_bytes / (1024 * 1024);

        let overall = if report.parts.is_empty() {
            PartState::Down
        } else {
            match HealthStatus::try_from(report.overall) {
                Ok(HealthStatus::Ok) => PartState::Ok,
                Ok(HealthStatus::Degraded) => PartState::Degraded,
                Ok(HealthStatus::Starting) => PartState::Starting,
                _ => PartState::Down,
            }
        };

        Snapshot {
            overall,
            parts,
            healthy,
            total,
            total_rss_mib,
            total_budget_mib: TOTAL_RSS_BUDGET_MIB,
            generated_at_ns: report.generated_at_ns,
        }
    }

    /// Creates an offline snapshot where all parts are Down and unreachable.
    ///
    /// Used when healthd cannot be reached over IPC.
    #[must_use]
    pub fn offline() -> Snapshot {
        let parts = Part::ALL
            .iter()
            .map(|&part| {
                let (title, role) = part_title_and_role(part);
                PartCard {
                    part,
                    title,
                    role,
                    state: PartState::Down,
                    reachable: false,
                    rss_bytes: 0,
                    budget_mib: part.rss_budget_mib(),
                    p99_ns: 0,
                    uptime_secs: 0,
                    subsystems: Vec::new(),
                }
            })
            .collect();

        Snapshot {
            overall: PartState::Down,
            parts,
            healthy: 0,
            total: Part::ALL.len(),
            total_rss_mib: 0,
            total_budget_mib: TOTAL_RSS_BUDGET_MIB,
            generated_at_ns: 0,
        }
    }

    /// Computes the top-level headline for this snapshot.
    #[must_use]
    pub fn headline(&self) -> Headline {
        if self.parts.is_empty() || self.parts.iter().all(|p| !p.reachable) {
            Headline::Unreachable
        } else if let Some(bad) = self.parts.iter().find(|p| p.state != PartState::Ok) {
            Headline::Attention {
                part: bad.part,
                state: bad.state,
            }
        } else {
            Headline::Healthy
        }
    }
}

fn part_title_and_role(part: Part) -> (&'static str, &'static str) {
    match part {
        Part::Healthd => (
            "Health monitor",
            "Watches every part and restarts what fails.",
        ),
        Part::Storage => (
            "Memory store",
            "Keeps your notes and history on this machine.",
        ),
        Part::Inference => ("Language model", "Writes the answers, fully offline."),
        Part::Monitor => (
            "Desktop sensor",
            "Notices what you are doing so help can be timely.",
        ),
        Part::Kernel => (
            "Safety kernel",
            "Checks every action and asks you before anything risky.",
        ),
        Part::Knowledge => (
            "Assistant",
            "Understands your question and picks how to answer.",
        ),
        Part::Voice => ("Voice", "Listens for the wake word and speaks answers."),
        Part::Fetcher => (
            "Web fetcher",
            "The only part allowed on the network, one approved request at a time.",
        ),
    }
}

fn extract_subsystems(resp: Option<&HealthResponse>) -> Vec<SubsystemRow> {
    resp.map(|r| {
        r.subsystems
            .iter()
            .map(|s| {
                let state = match HealthStatus::try_from(s.status) {
                    Ok(HealthStatus::Ok) => PartState::Ok,
                    Ok(HealthStatus::Degraded) => PartState::Degraded,
                    Ok(HealthStatus::Starting) => PartState::Starting,
                    _ => PartState::Down,
                };
                SubsystemRow {
                    name: s.name.clone(),
                    state,
                    detail: s.detail.clone(),
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

fn extract_p99_ns(resp: &HealthResponse) -> u64 {
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

fn compute_p99_ns(hist: &Histogram) -> u64 {
    let total: u64 = if hist.sample_count > 0 {
        hist.sample_count
    } else {
        hist.counts
            .iter()
            .fold(0u64, |acc, &c| acc.saturating_add(c))
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

#[cfg(test)]
mod tests {
    use super::*;
    use neuroos_proto::v1::{PartStatus, SubsystemHealth};

    fn make_test_part(
        part: Part,
        reachable: bool,
        status: HealthStatus,
        rss_bytes: u64,
        started_at_ns: u64,
    ) -> PartStatus {
        PartStatus {
            part: part.name().to_string(),
            reachable,
            last: Some(HealthResponse {
                component: format!("neuroos-{}", part.name()),
                version: "0.1.0".to_string(),
                status: status as i32,
                started_at_ns,
                sampled_at_ns: 2_000_000_000,
                rss_bytes,
                subsystems: vec![SubsystemHealth {
                    name: "sub1".to_string(),
                    status: HealthStatus::Ok as i32,
                    detail: "all good".to_string(),
                }],
                counters: Vec::new(),
                gauges: Vec::new(),
                histograms: Vec::new(),
            }),
            last_scrape_ns: 2_000_000_000,
            cgroup_memory_bytes: 0,
            cgroup_cpu_usage_ns: 0,
        }
    }

    #[test]
    fn test_all_ok_report() {
        let report = StatusReport {
            overall: HealthStatus::Ok as i32,
            parts: Part::ALL
                .iter()
                .map(|&p| {
                    make_test_part(p, true, HealthStatus::Ok, 10 * 1024 * 1024, 1_000_000_000)
                })
                .collect(),
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.overall, PartState::Ok);
        assert_eq!(snapshot.parts.len(), 8);
        assert_eq!(snapshot.healthy, 8);
        assert_eq!(snapshot.total, 8);
        assert_eq!(snapshot.headline(), Headline::Healthy);

        for (card, &expected_part) in snapshot.parts.iter().zip(Part::ALL.iter()) {
            assert_eq!(card.part, expected_part);
            assert_eq!(card.state, PartState::Ok);
            assert!(card.reachable);
            assert_eq!(card.uptime_secs, 1);
            assert_eq!(card.subsystems.len(), 1);
            assert_eq!(card.subsystems[0].state, PartState::Ok);
        }
    }

    #[test]
    fn test_one_degraded_part() {
        let mut parts: Vec<_> = Part::ALL
            .iter()
            .map(|&p| make_test_part(p, true, HealthStatus::Ok, 10 * 1024 * 1024, 1_000_000_000))
            .collect();
        // Modify Inference (index 2 in Part::ALL: Healthd, Storage, Inference)
        parts[2] = make_test_part(
            Part::Inference,
            true,
            HealthStatus::Degraded,
            10 * 1024 * 1024,
            1_000_000_000,
        );

        let report = StatusReport {
            overall: HealthStatus::Degraded as i32,
            parts,
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.overall, PartState::Degraded);
        assert_eq!(snapshot.healthy, 7);
        assert_eq!(
            snapshot.headline(),
            Headline::Attention {
                part: Part::Inference,
                state: PartState::Degraded,
            }
        );
    }

    #[test]
    fn test_part_with_last_none() {
        let mut parts: Vec<_> = Part::ALL
            .iter()
            .map(|&p| make_test_part(p, true, HealthStatus::Ok, 10 * 1024 * 1024, 1_000_000_000))
            .collect();
        // Storage has last: None
        parts[1] = PartStatus {
            part: Part::Storage.name().to_string(),
            reachable: true,
            last: None,
            last_scrape_ns: 0,
            cgroup_memory_bytes: 0,
            cgroup_cpu_usage_ns: 0,
        };

        let report = StatusReport {
            overall: HealthStatus::Degraded as i32,
            parts,
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.healthy, 7);
        assert_eq!(snapshot.parts[1].state, PartState::Down);
        assert_eq!(snapshot.parts[1].rss_bytes, 0);
        assert_eq!(snapshot.parts[1].uptime_secs, 0);
        assert!(snapshot.parts[1].subsystems.is_empty());
        assert_eq!(
            snapshot.headline(),
            Headline::Attention {
                part: Part::Storage,
                state: PartState::Down,
            }
        );
    }

    #[test]
    fn test_empty_report() {
        let report = StatusReport {
            overall: 0,
            parts: Vec::new(),
            generated_at_ns: 1_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.overall, PartState::Down);
        assert_eq!(snapshot.parts.len(), 8);
        assert_eq!(snapshot.healthy, 0);
        assert_eq!(snapshot.total, 8);
        assert_eq!(snapshot.total_rss_mib, 0);
        for card in &snapshot.parts {
            assert_eq!(card.state, PartState::Down);
            assert!(!card.reachable);
        }
        assert_eq!(snapshot.headline(), Headline::Unreachable);
    }

    #[test]
    fn test_unreachable_part() {
        let mut parts: Vec<_> = Part::ALL
            .iter()
            .map(|&p| make_test_part(p, true, HealthStatus::Ok, 10 * 1024 * 1024, 1_000_000_000))
            .collect();
        // Monitor has reachable: false even though last is Some
        parts[3] = make_test_part(
            Part::Monitor,
            false,
            HealthStatus::Ok,
            10 * 1024 * 1024,
            1_000_000_000,
        );

        let report = StatusReport {
            overall: HealthStatus::Degraded as i32,
            parts,
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.parts[3].state, PartState::Down);
        assert!(!snapshot.parts[3].reachable);
        assert_eq!(snapshot.parts[3].uptime_secs, 0);
        assert_eq!(
            snapshot.headline(),
            Headline::Attention {
                part: Part::Monitor,
                state: PartState::Down,
            }
        );
    }

    #[test]
    fn test_p99_picked_from_right_histogram() {
        let mut part_status = make_test_part(
            Part::Storage,
            true,
            HealthStatus::Ok,
            10 * 1024 * 1024,
            1_000_000_000,
        );
        if let Some(ref mut last) = part_status.last {
            last.histograms = vec![
                Histogram {
                    name: "garbage_collection_duration".to_string(),
                    upper_bounds_ns: vec![100, 200],
                    counts: vec![10, 0],
                    sample_count: 10,
                    sum_ns: 1000,
                },
                Histogram {
                    name: "query_latency_ns".to_string(),
                    upper_bounds_ns: vec![500, 1500, 3000],
                    counts: vec![90, 9, 1],
                    sample_count: 100,
                    sum_ns: 60000,
                },
            ];
        }

        let report = StatusReport {
            overall: HealthStatus::Ok as i32,
            parts: vec![part_status],
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        let storage_card = snapshot.parts.iter().find(|p| p.part == Part::Storage);
        assert!(storage_card.is_some());
        if let Some(card) = storage_card {
            // p99 for query_latency_ns (target = 99): 90+9 = 99 -> index 1 -> upper_bound 1500
            assert_eq!(card.p99_ns, 1500);
        }
    }

    #[test]
    fn test_rss_fraction_clamping() {
        let card_zero_budget = PartCard {
            part: Part::Storage,
            title: "Memory store",
            role: "Keeps notes.",
            state: PartState::Ok,
            reachable: true,
            rss_bytes: 100_000_000,
            budget_mib: 0,
            p99_ns: 0,
            uptime_secs: 0,
            subsystems: Vec::new(),
        };
        assert_eq!(card_zero_budget.rss_fraction(), 0.0);

        let card_normal = PartCard {
            part: Part::Storage,
            title: "Memory store",
            role: "Keeps notes.",
            state: PartState::Ok,
            reachable: true,
            rss_bytes: 160 * 1024 * 1024,
            budget_mib: 320,
            p99_ns: 0,
            uptime_secs: 0,
            subsystems: Vec::new(),
        };
        assert!((card_normal.rss_fraction() - 0.5).abs() < 1e-6);

        let card_overflow = PartCard {
            part: Part::Storage,
            title: "Memory store",
            role: "Keeps notes.",
            state: PartState::Ok,
            reachable: true,
            rss_bytes: 1000 * 1024 * 1024,
            budget_mib: 320,
            p99_ns: 0,
            uptime_secs: 0,
            subsystems: Vec::new(),
        };
        assert_eq!(card_overflow.rss_fraction(), 2.0);

        let card_u64_max = PartCard {
            part: Part::Storage,
            title: "Memory store",
            role: "Keeps notes.",
            state: PartState::Ok,
            reachable: true,
            rss_bytes: u64::MAX,
            budget_mib: 320,
            p99_ns: 0,
            uptime_secs: 0,
            subsystems: Vec::new(),
        };
        assert_eq!(card_u64_max.rss_fraction(), 2.0);
    }

    #[test]
    fn test_parts_always_has_8_entries_in_order() {
        let report = StatusReport {
            overall: HealthStatus::Ok as i32,
            parts: vec![make_test_part(
                Part::Voice,
                true,
                HealthStatus::Ok,
                10 * 1024 * 1024,
                1_000_000_000,
            )],
            generated_at_ns: 2_000_000_000,
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.parts.len(), 8);
        for (card, &expected_part) in snapshot.parts.iter().zip(Part::ALL.iter()) {
            assert_eq!(card.part, expected_part);
        }

        // Voice was reported
        let voice_card = &snapshot.parts[6]; // Part::Voice is index 6
        assert_eq!(voice_card.part, Part::Voice);
        assert!(voice_card.reachable);
        assert_eq!(voice_card.state, PartState::Ok);

        // Others are Down and not reachable
        let healthd_card = &snapshot.parts[0];
        assert_eq!(healthd_card.part, Part::Healthd);
        assert!(!healthd_card.reachable);
        assert_eq!(healthd_card.state, PartState::Down);
    }

    #[test]
    fn test_snapshot_offline() {
        let snapshot = Snapshot::offline();
        assert_eq!(snapshot.overall, PartState::Down);
        assert_eq!(snapshot.parts.len(), 8);
        assert_eq!(snapshot.healthy, 0);
        assert_eq!(snapshot.total, 8);
        assert_eq!(snapshot.total_rss_mib, 0);
        assert_eq!(snapshot.total_budget_mib, TOTAL_RSS_BUDGET_MIB);
        for (card, &expected_part) in snapshot.parts.iter().zip(Part::ALL.iter()) {
            assert_eq!(card.part, expected_part);
            assert_eq!(card.state, PartState::Down);
            assert!(!card.reachable);
        }
        assert_eq!(snapshot.headline(), Headline::Unreachable);
    }

    #[test]
    fn test_edge_values_do_not_panic() {
        let mut part = make_test_part(Part::Healthd, true, HealthStatus::Ok, u64::MAX, u64::MAX);
        if let Some(ref mut l) = part.last {
            l.histograms.push(Histogram {
                name: "request".to_string(),
                upper_bounds_ns: vec![u64::MAX],
                counts: vec![u64::MAX],
                sample_count: u64::MAX,
                sum_ns: u64::MAX,
            });
        }
        let report = StatusReport {
            overall: HealthStatus::Ok as i32,
            parts: vec![part],
            generated_at_ns: 0, // generated < started
        };

        let snapshot = Snapshot::from_report(&report);
        assert_eq!(snapshot.parts[0].uptime_secs, 0);
        assert_eq!(snapshot.parts[0].p99_ns, u64::MAX);
    }
}
