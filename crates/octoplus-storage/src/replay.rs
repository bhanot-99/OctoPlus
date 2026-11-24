//! The replay harness and the noise gate (KPI-8, FR-STO-01, phases.md §7). It feeds a recorded
//! dump through the *real* filter and adapters (and, for the soak, the real store) with the
//! events' own timestamps, then reports what became an entity. The noise gate is a test over
//! every committed dump; its negative control turns a stage off and must fail.

use std::collections::{BTreeSet, HashSet};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use octoplus_common::config::StorageConfig;
use octoplus_proto::dump::{DumpError, DumpReader};
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{Domain, MonitorEvent};

use crate::ingest::filter::{IngestFilter, Stages, same_program};
use crate::ingest::{Demotion, Output};
use crate::store::Store;

/// What a replay produced.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Events read.
    pub events: u64,
    /// Distinct promoted entities outside system health (KPI-8 counts these).
    pub entities: BTreeSet<(i32, String, String)>,
    /// Chunks promoted (with repeats).
    pub chunks: u64,
    /// Entities whose label is a compiler or build tool (KPI-8 wants none).
    pub compiler_entities: Vec<String>,
    /// Media entities for players that were never focused (KPI-8 wants none).
    pub passive_media_entities: Vec<String>,
    /// Entities for OctoPlus's own windows (must be none).
    pub own_window_entities: Vec<String>,
    /// Items dropped per stage.
    pub demoted_own_window: u64,
    /// Passive media dropped.
    pub demoted_passive_media: u64,
    /// Items held back by the promotion gate.
    pub demoted_below_threshold: u64,
    /// Subprocess trees folded into their root.
    pub demoted_subprocess: u64,
    /// Health points seen.
    pub health_points: u64,
}

/// Reads every event of a dump (validating it end to end).
///
/// # Errors
/// [`DumpError`] for a missing, truncated or corrupt dump.
pub fn load(path: &Path) -> Result<Vec<MonitorEvent>, DumpError> {
    let mut r = DumpReader::new(File::open(path)?)?;
    let mut v = Vec::new();
    while let Some(e) = r.next_event()? {
        v.push(e);
    }
    Ok(v)
}

/// Runs the filter over events and tallies the result. No database, no clock.
#[must_use]
pub fn replay_events(events: &[MonitorEvent], cfg: &StorageConfig, stages: Stages) -> Report {
    let mut f = IngestFilter::with_stages(cfg, stages);
    let mut rep = Report {
        events: events.len() as u64,
        ..Report::default()
    };
    let mut focused: HashSet<String> = HashSet::new();
    let build: HashSet<String> = cfg
        .build_process_names
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    let mut outs: Vec<Output> = Vec::new();
    let mut last = 0;
    for e in events {
        if let Some(Event::Focus(fc)) = &e.event {
            focused.insert(fc.app_id.to_ascii_lowercase());
        }
        outs.extend(f.push(e));
        last = e.observed_at_ns;
    }
    outs.extend(f.finish(last));
    for o in outs {
        match o {
            Output::Promote(i) => {
                rep.chunks += 1;
                if i.domain != Domain::SystemHealth {
                    rep.entities
                        .insert((i.domain as i32, i.kind.clone(), i.label.clone()));
                }
            }
            Output::Health(_) => rep.health_points += 1,
            Output::Demoted(Demotion::OwnWindow) => rep.demoted_own_window += 1,
            Output::Demoted(Demotion::PassiveMedia) => rep.demoted_passive_media += 1,
            Output::Demoted(Demotion::BelowThreshold) => rep.demoted_below_threshold += 1,
            Output::Demoted(Demotion::Subprocess) => rep.demoted_subprocess += 1,
            _ => {}
        }
    }
    for (domain, kind, label) in &rep.entities {
        let l = label.to_ascii_lowercase();
        if matches!(
            Domain::try_from(*domain),
            Ok(Domain::ProcessActivity | Domain::BuildJob)
        ) && kind != "build"
            && build.contains(&l)
        {
            rep.compiler_entities.push(label.clone());
        }
        if *domain == Domain::MediaPlayback as i32
            && !focused.iter().any(|a| same_program(label, a))
        {
            rep.passive_media_entities.push(label.clone());
        }
        if cfg
            .own_app_prefixes
            .iter()
            .any(|p| l.starts_with(&p.to_ascii_lowercase()))
        {
            rep.own_window_entities.push(label.clone());
        }
    }
    rep
}

/// Replays a dump file through the filter (see [`replay_events`]).
///
/// # Errors
/// [`DumpError`] for a bad dump.
pub fn replay_file(path: &Path, cfg: &StorageConfig, stages: Stages) -> Result<Report, DumpError> {
    Ok(replay_events(&load(path)?, cfg, stages))
}

/// Feeds events into a live store the way the pipeline does: filter, live state, batched ingest.
/// With `speed` the gaps between events are slept (scaled), so a soak runs "at 10×"; without it
/// events go in as fast as the store takes them.
///
/// # Errors
/// [`crate::db::DbError`] if a batch cannot be stored.
pub async fn replay_into_store(
    store: &Arc<Store>,
    events: &[MonitorEvent],
    speed: Option<f64>,
) -> Result<Report, crate::db::DbError> {
    let mut f = IngestFilter::new(store.config());
    let mut rep = Report {
        events: events.len() as u64,
        ..Report::default()
    };
    let mut pending: Vec<Output> = Vec::new();
    let mut prev = None;
    let batch = store.config().ingest_batch;
    let mut n = 0usize;
    for e in events {
        if let (Some(s), Some(p)) = (speed, prev) {
            let gap = e.observed_at_ns.saturating_sub(p);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let wait = (gap as f64 / s) as u64;
            if wait > 0 {
                tokio::time::sleep(Duration::from_nanos(wait)).await;
            }
        }
        prev = Some(e.observed_at_ns);
        store.observe(e);
        pending.extend(f.push(e));
        n += 1;
        if n >= batch {
            tally(&mut rep, &pending);
            store.ingest(std::mem::take(&mut pending)).await?;
            n = 0;
        }
    }
    pending.extend(f.finish(prev.unwrap_or(0)));
    tally(&mut rep, &pending);
    store.ingest(pending).await?;
    Ok(rep)
}

fn tally(rep: &mut Report, outs: &[Output]) {
    for o in outs {
        match o {
            Output::Promote(i) => {
                rep.chunks += 1;
                if i.domain != Domain::SystemHealth {
                    rep.entities
                        .insert((i.domain as i32, i.kind.clone(), i.label.clone()));
                }
            }
            Output::Health(_) => rep.health_points += 1,
            _ => {}
        }
    }
}
