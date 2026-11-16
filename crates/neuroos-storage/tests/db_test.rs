//! W1: schema, migrations, the single writer and the reader pool (FR-STO-04, ADR-0004).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use neuroos_storage::db::{Db, DbError, LATEST_VERSION, MatrixOp, PostCommit};
use rusqlite::{Connection, params};

type Seen = Arc<Mutex<Vec<MatrixOp>>>;

fn open(dir: &tempfile::TempDir) -> (Db, Seen) {
    let seen: Seen = Arc::default();
    let sink = Arc::clone(&seen);
    let hook: PostCommit = Arc::new(move |ops| sink.lock().unwrap().extend(ops));
    (
        Db::open(&dir.path().join("neuroos.sqlite3"), 3, 64, hook).unwrap(),
        seen,
    )
}

fn add_entity(c: &Connection, label: &str) -> Result<i64, DbError> {
    c.execute(
        "INSERT INTO entities (domain, kind, label, taint, first_seen_ns, last_seen_ns) VALUES (1, 'app', ?1, 0, 1, 1)",
        params![label],
    )?;
    Ok(c.last_insert_rowid())
}

fn add_chunk(c: &Connection, entity: i64, text: &str, n: i64) -> Result<i64, DbError> {
    c.execute(
        "INSERT INTO chunks (entity_id, domain, text, text_hash, taint, at_ns, source) VALUES (?1, 1, ?2, ?3, 0, ?3, 'test')",
        params![entity, text, n],
    )?;
    Ok(c.last_insert_rowid())
}

#[test]
fn a_new_database_is_migrated_and_a_second_open_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (db, _) = open(&dir);
        let v: i64 = db
            .read(|c| {
                Ok(
                    c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(v, LATEST_VERSION);
        let mode: String = db
            .read(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(mode, "wal");
        assert!(db.integrity_ok().unwrap());
    }
    let (db, _) = open(&dir);
    let n: i64 = db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 1, "reopening applies nothing twice");
}

#[test]
fn a_database_from_a_newer_build_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (db, _) = open(&dir);
        db.writer()
            .exec_blocking(|c| {
                c.execute("INSERT INTO schema_migrations VALUES (99, 'future', 0)", [])?;
                Ok(vec![])
            })
            .unwrap();
    }
    let hook: PostCommit = Arc::new(|_| {});
    let err = Db::open(&dir.path().join("neuroos.sqlite3"), 1, 8, hook).unwrap_err();
    assert!(matches!(err, DbError::TooNew { found: 99, .. }), "{err}");
}

#[test]
fn every_taint_column_refuses_null_and_out_of_range_values() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(&dir);
    let bad = [
        "INSERT INTO entities (domain, kind, label, taint, first_seen_ns, last_seen_ns) VALUES (1, 'a', 'b', NULL, 1, 1)",
        "INSERT INTO entities (domain, kind, label, taint, first_seen_ns, last_seen_ns) VALUES (1, 'a', 'b', 16, 1, 1)",
        "INSERT INTO entities (domain, kind, label, taint, first_seen_ns, last_seen_ns) VALUES (1, 'a', 'b', -1, 1, 1)",
        "INSERT INTO focus_history (app_id, title, start_ns, end_ns, dwell_ns, taint) VALUES ('a', 't', 1, 2, 1, NULL)",
        "INSERT INTO profile_facts (text, created_ns, last_used_ns, taint) VALUES ('x', 1, 1, NULL)",
        "INSERT INTO profile_facts (text, created_ns, last_used_ns, taint) VALUES ('x', 1, 1, 99)",
    ];
    for sql in bad {
        db.writer()
            .exec_blocking(move |c| {
                assert!(c.execute(sql, []).is_err(), "accepted: {sql}");
                Ok(vec![])
            })
            .unwrap();
    }
    // and a chunk with no taint
    db.writer()
        .exec_blocking(|c| {
            let e = add_entity(c, "x")?;
            let r = c.execute(
                "INSERT INTO chunks (entity_id, domain, text, text_hash, taint, at_ns, source) VALUES (?1, 1, 't', 1, NULL, 1, 's')",
                params![e],
            );
            assert!(r.is_err());
            Ok(vec![])
        })
        .unwrap();
}

#[test]
fn the_text_index_follows_insert_update_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(&dir);
    let id = Arc::new(Mutex::new(0_i64));
    let id2 = Arc::clone(&id);
    db.writer()
        .exec_blocking(move |c| {
            let e = add_entity(c, "e")?;
            *id2.lock().unwrap() = add_chunk(c, e, "compiling the rust workspace", 1)?;
            Ok(vec![])
        })
        .unwrap();
    let hits = |q: &'static str| -> i64 {
        db.read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                params![q],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    assert_eq!(hits("rust"), 1);
    let cid = *id.lock().unwrap();
    db.writer()
        .exec_blocking(move |c| {
            c.execute(
                "UPDATE chunks SET text = 'reading the sqlite manual' WHERE id = ?1",
                params![cid],
            )?;
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(
        (hits("rust"), hits("sqlite")),
        (0, 1),
        "the old words are gone, the new ones found"
    );
    db.writer()
        .exec_blocking(move |c| {
            c.execute("DELETE FROM chunks WHERE id = ?1", params![cid])?;
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(hits("sqlite"), 0);
}

#[test]
fn deleting_an_entity_removes_its_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(&dir);
    db.writer()
        .exec_blocking(|c| {
            let e = add_entity(c, "e")?;
            add_chunk(c, e, "a", 1)?;
            add_chunk(c, e, "b", 2)?;
            c.execute("DELETE FROM entities WHERE id = ?1", params![e])?;
            Ok(vec![])
        })
        .unwrap();
    let n: i64 = db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn one_failing_job_does_not_spoil_the_jobs_around_it_and_its_matrix_changes_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (db, seen) = open(&dir);
    db.writer()
        .exec_blocking(|c| {
            let e = add_entity(c, "e")?;
            add_chunk(c, e, "good one", 1)?;
            Ok(vec![MatrixOp::Remove { id: 1 }])
        })
        .unwrap();
    let err = db
        .writer()
        .exec_blocking(|c| {
            let e = add_entity(c, "f")?;
            add_chunk(c, e, "half done", 2)?;
            Err(DbError::Invalid("on purpose"))
        })
        .unwrap_err();
    assert!(matches!(err, DbError::Invalid(_)));
    db.writer()
        .exec_blocking(|c| {
            let e = add_entity(c, "g")?;
            add_chunk(c, e, "good two", 3)?;
            Ok(vec![MatrixOp::Remove { id: 3 }])
        })
        .unwrap();
    let texts: Vec<String> = db
        .read(|c| {
            let mut s = c.prepare("SELECT text FROM chunks ORDER BY id")?;
            Ok(s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert_eq!(
        texts,
        vec!["good one", "good two"],
        "the failed job's rows were rolled back"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        2,
        "only committed jobs reach the matrix"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_thousand_chunks_from_many_tasks_while_readers_query_stay_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let (db, seen) = open(&dir);
    let db = Arc::new(db);
    let mut tasks = Vec::new();
    for t in 0..8_i64 {
        let db = Arc::clone(&db);
        tasks.push(tokio::spawn(async move {
            for i in 0..1250_i64 {
                db.writer()
                    .exec(move |c| {
                        let e = add_entity(c, &format!("e{t}-{i}"))?;
                        let id = add_chunk(
                            c,
                            e,
                            &format!("chunk number {t} {i} about topic{}", i % 50),
                            t * 10_000 + i,
                        )?;
                        Ok(vec![MatrixOp::Upsert {
                            id: u64::try_from(id).unwrap(),
                            vector: vec![1.0],
                        }])
                    })
                    .await
                    .unwrap();
            }
        }));
    }
    let reader = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || {
            let mut last = 0_i64;
            for _ in 0..200 {
                let n: i64 = db
                    .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?))
                    .unwrap();
                assert!(n >= last, "a reader never sees the count go backwards");
                last = n;
            }
        })
    };
    for t in tasks {
        t.await.unwrap();
    }
    reader.await.unwrap();
    let n: i64 = db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 10_000);
    assert_eq!(seen.lock().unwrap().len(), 10_000);
    let fts: i64 = db
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'topic7'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(fts, 200);
    assert!(db.integrity_ok().unwrap());
    assert_eq!(
        db.readers().idle_count(),
        3,
        "every reader connection was returned"
    );
}
