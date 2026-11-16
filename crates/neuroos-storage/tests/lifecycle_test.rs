//! W9 / P4-S07, S08, S09: retention, the chunk cap, roll-up, backups and restore, re-embedding,
//! forget (live database *and* backups) and purge.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_proto::v1::{Domain, ForgetRequest, LifecycleJob};
use neuroos_storage::db::DbError;
use neuroos_storage::embed::{DIM, EmbedError, Embedder, HashEmbedder};
use neuroos_storage::ingest::{FocusSegment, HealthPoint, Item, Output};
use neuroos_storage::lifecycle::backup;
use neuroos_storage::store::{Store, StorePaths};
use neuroos_taint::TaintFlags;
use proptest::prelude::*;

const S: u64 = 1_000_000_000;
const DAY: u64 = 86_400 * S;
const NOW: u64 = 1_000 * DAY;

async fn open_with(dir: &Path, cfg: StorageConfig, e: Arc<dyn Embedder>) -> Arc<Store> {
    Arc::new(
        Store::open(
            &StorePaths::under(dir),
            cfg,
            e,
            Health::new("neuroos-storage", "test"),
        )
        .await
        .unwrap(),
    )
}

async fn open(dir: &Path) -> Arc<Store> {
    open_with(dir, StorageConfig::default(), Arc::new(HashEmbedder)).await
}

fn item(domain: Domain, label: &str, text: &str, at: u64, app: &str) -> Output {
    Output::Promote(Item {
        domain,
        kind: "k".into(),
        label: label.into(),
        app_id: app.into(),
        text: text.into(),
        taint: TaintFlags::empty(),
        at_ns: at,
    })
}

async fn count(s: &Arc<Store>, table: &'static str) -> i64 {
    s.read(move |c| Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?))
        .await
        .unwrap()
}

// ---------------------------------------------------------------- retention (P4-S07)

#[tokio::test]
async fn chunks_expire_per_domain_exactly_at_the_retention_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    let rows = [
        (Domain::WindowFocus, 30),
        (Domain::MediaPlayback, 14),
        (Domain::VoiceInteraction, 90),
        (Domain::ExternalDocuments, 30),
    ];
    let mut outs = Vec::new();
    for (d, days) in rows {
        let cutoff = NOW - days * DAY;
        outs.push(item(
            d,
            &format!("{d:?}-old"),
            &format!("{d:?} too old"),
            cutoff - 1,
            "",
        ));
        outs.push(item(
            d,
            &format!("{d:?}-edge"),
            &format!("{d:?} on the edge"),
            cutoff,
            "",
        ));
        outs.push(item(
            d,
            &format!("{d:?}-new"),
            &format!("{d:?} recent"),
            NOW,
            "",
        ));
    }
    for d in [Domain::Notes, Domain::GitActivity, Domain::Calendar] {
        outs.push(item(
            d,
            "ancient",
            &format!("{d:?} ancient but its source exists"),
            1,
            "",
        ));
    }
    s.ingest(outs).await.unwrap();
    assert_eq!(count(&s, "chunks").await, 15);
    let st = s.gc(NOW).await.unwrap();
    assert_eq!(st.chunks, 4, "one too-old chunk per expiring domain");
    assert_eq!(count(&s, "chunks").await, 11);
    let texts: Vec<String> = s
        .read(|c| {
            Ok(c.prepare("SELECT text FROM chunks")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .await
        .unwrap();
    assert!(texts.iter().all(|t| !t.contains("too old")));
    assert!(
        texts.iter().any(|t| t.contains("Notes ancient")),
        "notes live while the source exists"
    );
    assert_eq!(s.vectors_len(), 11, "the matrix followed the table");
    assert_eq!(
        count(&s, "entities").await,
        11,
        "entities left without chunks are gone"
    );
}

#[tokio::test]
async fn focus_history_nudge_log_and_health_follow_their_own_retention() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    let old = NOW - 31 * DAY;
    s.ingest(vec![
        Output::Segment(FocusSegment {
            app_id: "a".into(),
            title: "t".into(),
            pid: 0,
            start_ns: old - 10 * S,
            end_ns: old,
            dwell_ns: 10 * S,
        }),
        Output::Segment(FocusSegment {
            app_id: "b".into(),
            title: "t".into(),
            pid: 0,
            start_ns: NOW - 10 * S,
            end_ns: NOW,
            dwell_ns: 10 * S,
        }),
    ])
    .await
    .unwrap();
    s.db()
        .writer()
        .exec(move |c| {
            c.execute("INSERT INTO nudge_log (rule_id, outcome, at_ns) VALUES ('r', 1, ?1)", [i64::try_from(NOW - 91 * DAY).unwrap()])?;
            c.execute("INSERT INTO nudge_log (rule_id, outcome, at_ns) VALUES ('r', 1, ?1)", [i64::try_from(NOW - 89 * DAY).unwrap()])?;
            c.execute("INSERT INTO health_samples (metric, value, at_ns, rollup, vmin, vmax, n) VALUES ('m', 1, ?1, 1, 1, 1, 60)", [i64::try_from(NOW - 31 * DAY).unwrap()])?;
            c.execute("INSERT INTO health_samples (metric, value, at_ns, rollup, vmin, vmax, n) VALUES ('m', 1, ?1, 1, 1, 1, 60)", [i64::try_from(NOW - 29 * DAY).unwrap()])?;
            Ok(vec![])
        })
        .await
        .unwrap();
    let st = s.gc(NOW).await.unwrap();
    assert_eq!((st.focus_rows, st.nudge_rows, st.health_rows), (1, 1, 1));
    assert_eq!(
        (
            count(&s, "focus_history").await,
            count(&s, "nudge_log").await,
            count(&s, "health_samples").await
        ),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn the_chunk_cap_holds_keeps_the_newest_and_spares_permanent_domains() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = StorageConfig {
        chunk_cap: 100,
        ..Default::default()
    };
    let s = open_with(dir.path(), cfg, Arc::new(HashEmbedder)).await;
    let mut outs: Vec<Output> = (0..60)
        .map(|i| {
            item(
                Domain::Notes,
                &format!("n{i}"),
                &format!("note number {i}"),
                1 + i,
                "",
            )
        })
        .collect();
    outs.extend((0..80).map(|i| {
        item(
            Domain::WindowFocus,
            &format!("w{i}"),
            &format!("window number {i}"),
            1000 + i,
            "app",
        )
    }));
    s.ingest(outs).await.unwrap();
    assert_eq!(
        count(&s, "chunks").await,
        100,
        "the cap holds right after ingest"
    );
    assert_eq!(s.vectors_len(), 100);
    let notes: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM chunks WHERE domain = ?1",
                [Domain::Notes as i32],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(
        notes, 60,
        "notes (permanent) are only trimmed when nothing else is left"
    );
    let oldest: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT MIN(at_ns) FROM chunks WHERE domain = ?1",
                [Domain::WindowFocus as i32],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(oldest, 1040, "the 40 oldest window chunks went first");
    // Only permanent data left: the cap still holds, oldest first.
    let cfg = StorageConfig {
        chunk_cap: 10,
        ..Default::default()
    };
    let dir2 = tempfile::tempdir().unwrap();
    let s2 = open_with(dir2.path(), cfg, Arc::new(HashEmbedder)).await;
    s2.ingest(
        (0..25)
            .map(|i| {
                item(
                    Domain::Notes,
                    &format!("n{i}"),
                    &format!("note {i}"),
                    i + 1,
                    "",
                )
            })
            .collect(),
    )
    .await
    .unwrap();
    assert_eq!(count(&s2, "chunks").await, 10);
    let min: i64 = s2
        .read(|c| Ok(c.query_row("SELECT MIN(at_ns) FROM chunks", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(min, 16);
}

// ---------------------------------------------------------------- roll-up

#[tokio::test]
async fn raw_health_older_than_the_window_becomes_hourly_summaries_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    let hour = 3600 * S;
    let base = (NOW - 8 * DAY) / hour * hour; // a whole hour 8 days ago
    let mut outs: Vec<Output> = [10.0, 20.0, 60.0]
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Output::Health(HealthPoint {
                metric: "cpu".into(),
                value: *v,
                at_ns: base + 5 * S + i as u64 * S,
            })
        })
        .collect();
    outs.push(Output::Health(HealthPoint {
        metric: "cpu".into(),
        value: 99.0,
        at_ns: NOW - S,
    }));
    s.ingest(outs).await.unwrap();
    assert_eq!(s.rollup(NOW).await.unwrap(), 3);
    let (n, rolled, mean, min, max, cnt): (i64, i64, f64, f64, f64, i64) = s
        .read(|c| Ok(c.query_row("SELECT COUNT(*), SUM(rollup), AVG(value), MIN(vmin), MAX(vmax), MAX(n) FROM health_samples WHERE at_ns < ?1", [i64::try_from(NOW - DAY).unwrap()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?))
        .await
        .unwrap();
    assert_eq!(
        (n, rolled, cnt),
        (1, 1, 3),
        "one summary row for three raw samples"
    );
    assert!((mean - 30.0).abs() < 1e-9 && (min - 10.0).abs() < 1e-9 && (max - 60.0).abs() < 1e-9);
    assert_eq!(
        count(&s, "health_samples").await,
        2,
        "the recent raw sample is untouched"
    );
    assert_eq!(
        s.rollup(NOW).await.unwrap(),
        0,
        "running it again changes nothing"
    );
    assert_eq!(count(&s, "health_samples").await, 2);
}

// ---------------------------------------------------------------- backups (P4-S07)

#[tokio::test]
async fn a_backup_is_a_complete_restorable_database_and_old_ones_rotate() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    s.ingest(
        (0..20)
            .map(|i| {
                item(
                    Domain::Notes,
                    &format!("n{i}"),
                    &format!("restorable note {i}"),
                    i + 1,
                    "",
                )
            })
            .collect(),
    )
    .await
    .unwrap();
    let b = s.backup(NOW).await.unwrap();
    assert!(b.ends_with(backup::stamp(NOW)));
    let file = b.join(backup::BACKUP_DB);
    let conn = rusqlite::Connection::open(&file).unwrap();
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 20);
    drop(conn);
    // End to end restore: put the backup where a fresh C3 expects its database and open it.
    let restore = tempfile::tempdir().unwrap();
    let paths = StorePaths::under(restore.path());
    std::fs::create_dir_all(paths.db.parent().unwrap()).unwrap();
    std::fs::copy(&file, &paths.db).unwrap();
    let r = Arc::new(
        Store::open(
            &paths,
            StorageConfig::default(),
            Arc::new(HashEmbedder),
            Health::new("c3", "t"),
        )
        .await
        .unwrap(),
    );
    assert_eq!(count(&r, "chunks").await, 20);
    assert_eq!(
        r.vectors_len(),
        20,
        "the vector matrix is rebuilt from the restored table"
    );
    // Rotation: keep the newest 8 of 11.
    for i in 1..=10_u64 {
        s.backup(NOW + i * 10 * S).await.unwrap();
    }
    let kept = backup::list(&StorePaths::under(dir.path()).backups).unwrap();
    assert_eq!(kept.len(), 8);
    assert!(
        kept[0].ends_with(backup::stamp(NOW + 100 * S)),
        "newest first"
    );
    assert!(
        !kept.iter().any(|p| p.ends_with(backup::stamp(NOW))),
        "the oldest rotated away"
    );
}

#[tokio::test]
async fn a_crashed_backup_leaves_older_backups_intact_and_is_swept() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    s.ingest(vec![item(Domain::Notes, "n", "keep me", 1, "")])
        .await
        .unwrap();
    let good = s.backup(NOW).await.unwrap();
    let root = StorePaths::under(dir.path()).backups;
    // A killed run leaves a half-written temporary folder.
    let crashed = root.join(".tmp-20991231T000000Z");
    std::fs::create_dir_all(&crashed).unwrap();
    std::fs::write(crashed.join(backup::BACKUP_DB), b"half a database").unwrap();
    assert_eq!(
        backup::list(&root).unwrap().len(),
        1,
        "a temporary folder is never a backup"
    );
    s.backup(NOW + 100 * S).await.unwrap();
    assert!(!crashed.exists(), "swept at the next run");
    assert!(good.join(backup::BACKUP_DB).exists());
    // A failing backup (the folder cannot be created) reports an error and changes nothing.
    let broken = tempfile::tempdir().unwrap();
    let s2 = open(broken.path()).await;
    std::fs::write(
        StorePaths::under(broken.path()).backups,
        b"a file where a folder should be",
    )
    .unwrap();
    assert!(s2.backup(NOW).await.is_err());
}

// ---------------------------------------------------------------- re-embed (P4-S08)

struct OtherModel;

impl Embedder for OtherModel {
    fn id(&self) -> &str {
        "other-v2"
    }

    fn dim(&self) -> usize {
        DIM
    }

    fn embed(&self, t: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        HashEmbedder.embed(t)
    }
}

#[tokio::test]
async fn a_model_change_re_embeds_in_the_background_without_losing_text_search() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = open(dir.path()).await;
        s.ingest(
            (0..70)
                .map(|i| {
                    item(
                        Domain::Notes,
                        &format!("n{i}"),
                        &format!("quarterly report number {i}"),
                        i + 1,
                        "",
                    )
                })
                .collect(),
        )
        .await
        .unwrap();
        assert_eq!((s.vectors_len(), s.stale_count().await.unwrap()), (70, 0));
    }
    let s = open_with(dir.path(), StorageConfig::default(), Arc::new(OtherModel)).await;
    assert_eq!(
        s.stale_count().await.unwrap(),
        70,
        "every chunk was made by the old model"
    );
    assert_eq!(
        s.vectors_len(),
        0,
        "old vectors are not mixed with the new model's space"
    );
    let fts: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'quarterly'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(fts, 70, "text search keeps working during the swap");
    let (_tx, rx) = tokio::sync::watch::channel(false);
    s.reembed_all(rx).await;
    assert_eq!((s.stale_count().await.unwrap(), s.vectors_len()), (0, 70));
    let meta: String = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT value FROM index_meta WHERE key = 'embedding_model'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(meta, "other-v2");
}

// ---------------------------------------------------------------- forget and purge (P4-S09)

const SENTINEL: &str = "zorblax-sentinel";

async fn seed(s: &Arc<Store>) {
    let mut outs = Vec::new();
    for (app, base) in [("alpha", 100u64), ("beta", 200), ("gamma", 300)] {
        for i in 0..5u64 {
            outs.push(item(
                Domain::WindowFocus,
                &format!("{app}-{i}"),
                &format!("{SENTINEL} {app} item {i}"),
                base + i,
                app,
            ));
        }
        outs.push(Output::Segment(FocusSegment {
            app_id: app.into(),
            title: format!("{SENTINEL} {app}"),
            pid: 1,
            start_ns: base,
            end_ns: base + 4,
            dwell_ns: 4,
        }));
    }
    outs.push(Output::Health(HealthPoint {
        metric: "cpu".into(),
        value: 1.0,
        at_ns: 250,
    }));
    s.ingest(outs).await.unwrap();
}

fn db_files(db: &Path) -> Vec<std::path::PathBuf> {
    let mut v = vec![db.to_path_buf()];
    for suffix in ["-wal", "-shm"] {
        v.push(std::path::PathBuf::from(format!(
            "{}{suffix}",
            db.display()
        )));
    }
    v
}

fn file_contains(p: &Path, needle: &str) -> bool {
    std::fs::read(p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
}

fn remaining(conn: &rusqlite::Connection, app: &str) -> (i64, i64, i64) {
    let q = |sql: &str| {
        conn.query_row(sql, rusqlite::params![app], |r| r.get::<_, i64>(0))
            .unwrap()
    };
    (
        q("SELECT COUNT(*) FROM chunks WHERE app_id = ?1"),
        q("SELECT COUNT(*) FROM focus_history WHERE app_id = ?1"),
        q("SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1"),
    )
}

#[tokio::test]
async fn forget_by_app_removes_rows_text_index_vectors_and_every_backup_copy() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    seed(&s).await;
    let b1 = s.backup(NOW).await.unwrap();
    let b2 = s.backup(NOW + 10 * S).await.unwrap();
    let before = s.vectors_len();
    let r = s
        .forget(ForgetRequest {
            from_ns: 0,
            to_ns: 0,
            app_id: "beta".into(),
        })
        .await
        .unwrap();
    assert_eq!(r.vectors_deleted, 5);
    assert!(
        r.rows_deleted > 5,
        "chunks, the focus row and entities: {}",
        r.rows_deleted
    );
    assert_eq!(
        s.vectors_len(),
        before - 5,
        "the matrix lost exactly those vectors"
    );
    let live = rusqlite::Connection::open(&StorePaths::under(dir.path()).db).unwrap();
    let (lc, lf, _) = remaining(&live, "beta");
    assert_eq!((lc, lf), (0, 0));
    for db in [
        live_path(dir.path()),
        b1.join(backup::BACKUP_DB),
        b2.join(backup::BACKUP_DB),
    ] {
        let c = rusqlite::Connection::open(&db).unwrap();
        let (chunks, focus, _) = remaining(&c, "beta");
        assert_eq!((chunks, focus), (0, 0), "{}", db.display());
        let hits: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'beta'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            hits,
            0,
            "no text index entry for the forgotten app in {}",
            db.display()
        );
        let others: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE app_id IN ('alpha','gamma')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(others, 10, "everything else is intact in {}", db.display());
        let ok: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ok, "ok");
    }
    // The words themselves are gone from the raw files: not just unlinked, overwritten.
    for p in db_files(&live_path(dir.path()))
        .into_iter()
        .chain([b1.join(backup::BACKUP_DB), b2.join(backup::BACKUP_DB)])
    {
        assert!(
            !file_contains(&p, "zorblax-sentinel beta"),
            "{} still holds the forgotten text",
            p.display()
        );
    }
    assert!(
        file_contains(&b1.join(backup::BACKUP_DB), "zorblax-sentinel alpha"),
        "and the rest is still there"
    );
}

fn live_path(root: &Path) -> std::path::PathBuf {
    StorePaths::under(root).db
}

#[tokio::test]
async fn forget_by_time_range_takes_samples_and_history_too_and_empty_scopes_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    seed(&s).await;
    s.forget(ForgetRequest {
        from_ns: 150,
        to_ns: 260,
        app_id: String::new(),
    })
    .await
    .unwrap();
    assert_eq!(
        count(&s, "health_samples").await,
        0,
        "the sample at 250 was in range"
    );
    assert_eq!(
        count(&s, "chunks").await,
        10,
        "beta's chunks (200–204) are gone"
    );
    assert_eq!(count(&s, "focus_history").await, 2);
    for bad in [
        ForgetRequest {
            from_ns: 0,
            to_ns: 0,
            app_id: String::new(),
        },
        ForgetRequest {
            from_ns: 50,
            to_ns: 10,
            app_id: String::new(),
        },
    ] {
        assert!(matches!(s.forget(bad).await, Err(DbError::Invalid(_))));
    }
    assert_eq!(
        count(&s, "chunks").await,
        10,
        "a refused forget changed nothing"
    );
}

#[tokio::test]
async fn negative_control_a_forget_that_skips_the_backups_would_be_caught() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    seed(&s).await;
    let b = s.backup(NOW).await.unwrap();
    // Delete from the live database only, as a buggy forget would.
    s.db()
        .writer()
        .exec(|c| {
            c.execute("DELETE FROM chunks WHERE app_id = 'beta'", [])?;
            Ok(vec![])
        })
        .await
        .unwrap();
    let c = rusqlite::Connection::open(b.join(backup::BACKUP_DB)).unwrap();
    assert_eq!(
        remaining(&c, "beta").0,
        5,
        "the backup still holds the data: the check above would fail"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn for_any_scope_exactly_the_matching_data_disappears_live_and_in_backups(
        from in 0u64..400, len in 0u64..400, app_pick in 0usize..4,
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async {
            let apps = ["", "alpha", "beta", "gamma"];
            let (to, app) = (from + len, apps[app_pick]);
            prop_assume!(from > 0 || to > 0 || !app.is_empty());
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path()).await;
            seed(&s).await;
            let b = s.backup(NOW).await.unwrap();
            let in_scope = |a: &str, at: u64| at >= from && at <= (if to == 0 { u64::MAX } else { to }) && (app.is_empty() || a == app);
            let mut expected = 0;
            for (a, base) in [("alpha", 100u64), ("beta", 200), ("gamma", 300)] {
                expected += (0..5).filter(|i| !in_scope(a, base + i)).count();
            }
            s.forget(ForgetRequest { from_ns: from, to_ns: to, app_id: app.into() }).await.unwrap();
            let live = count(&s, "chunks").await;
            prop_assert_eq!(live, i64::try_from(expected).unwrap());
            let bc = rusqlite::Connection::open(b.join(backup::BACKUP_DB)).unwrap();
            let in_backup: i64 = bc.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
            prop_assert_eq!(in_backup, live, "the backup was scrubbed the same way");
            prop_assert_eq!(s.vectors_len(), usize::try_from(live).unwrap(), "no vector outlives its row");
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn purge_leaves_nothing_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path()).await;
    seed(&s).await;
    s.profile_add(
        neuroos_proto::v1::ProfileAdd {
            text: format!("{SENTINEL} fact"),
            taint: Some(neuroos_proto::v1::Taint { flags: 2 }),
        },
        5,
    )
    .await
    .unwrap();
    let b = s.backup(NOW).await.unwrap();
    s.purge().await.unwrap();
    for t in [
        "chunks",
        "entities",
        "focus_history",
        "event_counters",
        "health_samples",
        "profile_facts",
        "nudge_log",
    ] {
        assert_eq!(count(&s, t).await, 0, "{t}");
    }
    assert_eq!(s.vectors_len(), 0);
    assert!(!b.exists(), "backups are removed");
    for p in db_files(&live_path(dir.path())) {
        assert!(
            !file_contains(&p, SENTINEL),
            "{} still holds user text",
            p.display()
        );
    }
    let r = s.run_job(LifecycleJob::Gc, NOW).await.unwrap();
    assert_eq!(r.job, LifecycleJob::Gc as i32);
    assert!(matches!(
        s.run_job(LifecycleJob::Unspecified, NOW).await,
        Err(DbError::Invalid(_))
    ));
}
