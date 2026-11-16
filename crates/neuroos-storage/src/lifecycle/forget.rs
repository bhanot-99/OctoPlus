//! Forget and purge (FR-STO-13, FR-PRV-03, FR-PRV-06; P4-S09). Forget removes rows, text index
//! entries and vectors in one transaction, then scrubs every existing backup at once; purge
//! removes everything. Deleted content is overwritten (`secure_delete`) and the text index is
//! merged so removed words do not linger in its segments.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neuroos_proto::v1::{ForgetRequest, ForgetResult};
use rusqlite::{Connection, params};

use super::backup;
use crate::db::{DbError, MatrixOp};
use crate::ingest::apply::signed;
use crate::store::Store;

/// What to forget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Start of the time range (inclusive).
    pub from_ns: i64,
    /// End of the time range (inclusive); `i64::MAX` if open.
    pub to_ns: i64,
    /// Only this application; empty for any.
    pub app_id: String,
}

impl Scope {
    /// From a request. At least a time bound or an app is required: "forget everything" is purge.
    ///
    /// # Errors
    /// [`DbError::Invalid`] for an empty scope or an inverted range.
    pub fn from_request(r: &ForgetRequest) -> Result<Self, DbError> {
        if r.from_ns == 0 && r.to_ns == 0 && r.app_id.is_empty() {
            return Err(DbError::Invalid(
                "forget needs a time range or an app (use purge to remove everything)",
            ));
        }
        let to = if r.to_ns == 0 {
            i64::MAX
        } else {
            signed(r.to_ns)
        };
        if signed(r.from_ns) > to {
            return Err(DbError::Invalid("the range ends before it starts"));
        }
        Ok(Self {
            from_ns: signed(r.from_ns),
            to_ns: to,
            app_id: r.app_id.clone(),
        })
    }
}

/// Counts of what one forget removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Forgotten {
    /// Rows deleted in all tables.
    pub rows: u64,
    /// Chunks that had a vector.
    pub vectors: u64,
}

/// Applies a forget to one database (the live one or a backup). Returns counts and the ids of
/// the chunks removed, for the vector matrix.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn apply(c: &Connection, s: &Scope) -> Result<(Forgotten, Vec<i64>), DbError> {
    let any_app = s.app_id.is_empty();
    let mut out = Forgotten::default();
    let ids: Vec<(i64, bool)> = {
        let mut st = c.prepare(
            "SELECT id, vector IS NOT NULL FROM chunks WHERE at_ns BETWEEN ?1 AND ?2 AND (?3 = '' OR app_id = ?3)",
        )?;
        st.query_map(params![s.from_ns, s.to_ns, s.app_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<Result<_, _>>()?
    };
    let mut del = c.prepare("DELETE FROM chunks WHERE id = ?1")?;
    for (id, has_vector) in &ids {
        del.execute(params![id])?;
        out.rows += 1;
        out.vectors += u64::from(*has_vector);
    }
    out.rows += c.execute(
        "DELETE FROM focus_history WHERE end_ns >= ?1 AND start_ns <= ?2 AND (?3 = '' OR app_id = ?3)",
        params![s.from_ns, s.to_ns, s.app_id],
    )? as u64;
    out.rows += c.execute(
        "DELETE FROM event_counters WHERE last_ns BETWEEN ?1 AND ?2 AND (?3 = '' OR app_id = ?3)",
        params![s.from_ns, s.to_ns, s.app_id],
    )? as u64;
    if any_app {
        out.rows += c.execute(
            "DELETE FROM health_samples WHERE at_ns BETWEEN ?1 AND ?2",
            params![s.from_ns, s.to_ns],
        )? as u64;
        out.rows += c.execute(
            "DELETE FROM nudge_log WHERE at_ns BETWEEN ?1 AND ?2",
            params![s.from_ns, s.to_ns],
        )? as u64;
    }
    out.rows += super::gc::drop_empty_entities(c)?;
    // Entities that kept chunks outside the range must not keep a "last seen" inside it.
    c.execute(
        "UPDATE entities SET last_seen_ns = (SELECT MAX(at_ns) FROM chunks WHERE entity_id = entities.id)
         WHERE last_seen_ns BETWEEN ?1 AND ?2 AND (?3 = '' OR app_id = ?3)",
        params![s.from_ns, s.to_ns, s.app_id],
    )?;
    Ok((out, ids.into_iter().map(|(id, _)| id).collect()))
}

/// Merges the text index so removed words are gone from its segments, then compacts the file.
fn compact(c: &Connection) -> Result<(), DbError> {
    c.execute(
        "INSERT INTO chunks_fts (chunks_fts) VALUES ('optimize')",
        [],
    )?;
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    c.execute_batch("PRAGMA incremental_vacuum")?;
    Ok(())
}

/// Applies the forget to every finished backup. Returns how many backups were scrubbed.
fn scrub_backups(root: &Path, s: &Scope) -> Result<u64, DbError> {
    let mut n = 0;
    for dir in backup::list(root)? {
        let file = dir.join(backup::BACKUP_DB);
        if !file.exists() {
            continue;
        }
        let c = Connection::open(&file)?;
        c.pragma_update(None, "secure_delete", true)?;
        c.execute_batch("BEGIN IMMEDIATE")?;
        apply(&c, s)?;
        c.execute_batch("COMMIT")?;
        compact(&c)?;
        c.execute_batch("VACUUM")?;
        n += 1;
    }
    Ok(n)
}

impl Store {
    /// Forgets what the request names, in one transaction, then scrubs every backup (P4-S09).
    ///
    /// # Errors
    /// [`DbError::Invalid`] for an empty scope; [`DbError`] on failure (the live database is
    /// unchanged if its transaction fails).
    pub async fn forget(self: &Arc<Self>, r: ForgetRequest) -> Result<ForgetResult, DbError> {
        let scope = Scope::from_request(&r)?;
        let (live_app, live_from, live_to) = (scope.app_id.clone(), scope.from_ns, scope.to_ns);
        let rows = Arc::new(AtomicU64::new(0));
        let vectors = Arc::new(AtomicU64::new(0));
        let (r2, v2, sc) = (Arc::clone(&rows), Arc::clone(&vectors), scope.clone());
        self.db
            .writer()
            .exec(move |c| {
                let (counts, ids) = apply(c, &sc)?;
                r2.store(counts.rows, Ordering::Relaxed);
                v2.store(counts.vectors, Ordering::Relaxed);
                Ok(ids
                    .into_iter()
                    .filter_map(|i| u64::try_from(i).ok())
                    .map(|id| MatrixOp::Remove { id })
                    .collect())
            })
            .await?;
        self.db.writer().maintain(compact).await?;
        let root = self.paths.backups.clone();
        tokio::task::spawn_blocking(move || scrub_backups(&root, &scope))
            .await
            .map_err(|_| DbError::WriterGone)??;
        {
            let mut live = self
                .live
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            live.forget(
                &live_app,
                u64::try_from(live_from).unwrap_or(0),
                u64::try_from(live_to).unwrap_or(u64::MAX),
            );
        }
        self.forget_epoch.fetch_add(1, Ordering::SeqCst);
        self.refresh_gauges();
        Ok(ForgetResult {
            rows_deleted: rows.load(Ordering::Relaxed),
            vectors_deleted: vectors.load(Ordering::Relaxed),
        })
    }

    /// Removes every byte of user data (FR-PRV-06): all rows, the vector matrix, every backup.
    ///
    /// # Errors
    /// [`DbError`] on failure.
    pub async fn purge(self: &Arc<Self>) -> Result<(), DbError> {
        let ids = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
        let ids2 = Arc::clone(&ids);
        self.db
            .writer()
            .exec(move |c| {
                let all: Vec<i64> = {
                    let mut s = c.prepare("SELECT id FROM chunks")?;
                    s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
                };
                for t in [
                    "chunks",
                    "entities",
                    "focus_history",
                    "event_counters",
                    "health_samples",
                    "profile_facts",
                    "nudge_log",
                ] {
                    c.execute(&format!("DELETE FROM {t}"), [])?;
                }
                *ids2
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = all.clone();
                Ok(all
                    .into_iter()
                    .filter_map(|i| u64::try_from(i).ok())
                    .map(|id| MatrixOp::Remove { id })
                    .collect())
            })
            .await?;
        self.db
            .writer()
            .maintain(|c| {
                compact(c)?;
                c.execute_batch("VACUUM")?;
                Ok(())
            })
            .await?;
        if let Ok(rd) = std::fs::read_dir(&self.paths.backups) {
            for e in rd.filter_map(Result::ok) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        self.live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.forget_epoch.fetch_add(1, Ordering::SeqCst);
        self.refresh_gauges();
        Ok(())
    }
}
