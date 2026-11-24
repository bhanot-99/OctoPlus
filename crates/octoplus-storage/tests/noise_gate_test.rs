//! W11 / KPI-8: the ingest noise gate over every committed dump, with negative controls
//! (rules §8.9). The dumps are synthetic until real recordings exist (D-26); the gate runs on
//! the real ones the same way at integration.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use octoplus_common::config::StorageConfig;
use octoplus_health::Health;
use octoplus_storage::embed::HashEmbedder;
use octoplus_storage::ingest::filter::Stages;
use octoplus_storage::replay::{self, Report};
use octoplus_storage::store::{Store, StorePaths};

/// PRD KPI-8: promoted entities per dump at most this many.
const KPI8_MAX_ENTITIES: usize = 30;

fn dumps() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/monitor-dumps");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "dump"))
        .collect();
    v.sort();
    v
}

fn run(p: &std::path::Path, stages: Stages) -> Report {
    replay::replay_file(p, &StorageConfig::default(), stages).unwrap()
}

#[test]
fn every_committed_dump_passes_the_kpi_8_noise_gate() {
    let dumps = dumps();
    assert!(
        dumps.len() >= 3,
        "the gate needs its dumps: found {}",
        dumps.len()
    );
    for d in &dumps {
        let r = run(d, Stages::default());
        let name = d.file_name().unwrap().to_string_lossy();
        assert!(
            r.entities.len() <= KPI8_MAX_ENTITIES,
            "{name}: {} promoted entities: {:?}",
            r.entities.len(),
            r.entities
        );
        assert!(
            r.compiler_entities.is_empty(),
            "{name}: compiler subprocesses became entities: {:?}",
            r.compiler_entities
        );
        assert!(
            r.passive_media_entities.is_empty(),
            "{name}: passive media nodes: {:?}",
            r.passive_media_entities
        );
        assert!(
            r.own_window_entities.is_empty(),
            "{name}: OctoPlus's own windows were stored: {:?}",
            r.own_window_entities
        );
        // The gate must not pass vacuously: each dump has to exercise every stage.
        assert!(
            r.demoted_subprocess > 0 && r.demoted_passive_media > 0 && r.demoted_own_window > 0,
            "{name}: the dump does not exercise every stage: {r:?}"
        );
        assert!(
            r.entities.len() >= 3 && r.chunks > 0,
            "{name}: nothing was promoted at all"
        );
    }
}

#[test]
fn negative_controls_each_stage_switched_off_makes_the_gate_fail() {
    let dumps = dumps();
    let any = |stages: Stages, bad: fn(&Report) -> bool| dumps.iter().any(|d| bad(&run(d, stages)));
    let on = Stages::default();
    assert!(
        any(
            Stages {
                collapse: false,
                ..on
            },
            |r| !r.compiler_entities.is_empty()
        ),
        "stage 1 off must be noticed"
    );
    assert!(
        any(Stages { media: false, ..on }, |r| !r
            .passive_media_entities
            .is_empty()),
        "stage 4 off must be noticed"
    );
    assert!(
        any(
            Stages {
                own_windows: false,
                ..on
            },
            |r| !r.own_window_entities.is_empty()
        ),
        "stage 2 off must be noticed"
    );
    assert!(
        any(
            Stages {
                promote: false,
                ..on
            },
            |r| r.entities.len() > KPI8_MAX_ENTITIES
        ),
        "stage 3 off must be noticed"
    );
}

#[test]
fn a_replay_is_deterministic() {
    let d = &dumps()[0];
    assert_eq!(run(d, Stages::default()), run(d, Stages::default()));
}

#[tokio::test]
async fn replaying_into_a_real_store_stores_exactly_the_entities_the_gate_counted() {
    let d = &dumps()[0];
    let events = replay::load(d).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        Store::open(
            &StorePaths::under(dir.path()),
            StorageConfig::default(),
            Arc::new(HashEmbedder),
            Health::new("c3", "t"),
        )
        .await
        .unwrap(),
    );
    let rep = replay::replay_into_store(&store, &events, None)
        .await
        .unwrap();
    let stored: i64 = store
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(
        usize::try_from(stored).unwrap(),
        rep.entities.len(),
        "the database holds what the gate counted"
    );
    assert!(
        i64::try_from(rep.entities.len()).unwrap() <= i64::try_from(KPI8_MAX_ENTITIES).unwrap()
    );
    let health: i64 = store
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM health_samples", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert!(
        health > 1000,
        "samples bypass the gate and are all stored: {health}"
    );
    let focus: i64 = store
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM focus_history", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert!(focus > 20, "focus history keeps the segments");
    assert!(store.db().integrity_ok().unwrap());
}
