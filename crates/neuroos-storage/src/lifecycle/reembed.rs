//! Re-embedding after a model change (FR-STO-12, P4-S08). Chunks made by another model (or with no
//! vector because embedding failed) are *stale*: they stay searchable by text and results are
//! flagged `STALE_INDEX`, but they are not in the vector matrix, because a vector from another
//! model is not comparable with a query embedded now. Background batches re-embed them and add each
//! to the matrix as it is finished; queries are never blocked.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use rusqlite::params;
use tokio::sync::watch;

use crate::db::{DbError, MatrixOp};
use crate::store::{Store, vector_to_blob};

/// Chunks re-embedded per batch: small enough to keep memory flat and queries responsive.
pub const BATCH: usize = 32;

impl Store {
    /// Chunks that are not embedded by the current model.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn stale_count(self: &Arc<Self>) -> Result<u64, DbError> {
        let model = self.embedder.id().to_owned();
        let n: i64 = self
            .read(move |c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM chunks WHERE embed_model != ?1",
                    params![model],
                    |r| r.get(0),
                )?)
            })
            .await?;
        let n = u64::try_from(n).unwrap_or(0);
        self.stale.store(n, Ordering::Relaxed);
        Ok(n)
    }

    /// Re-embeds up to [`BATCH`] stale chunks. Returns how many were done (0 = nothing left, or
    /// the embedder is failing).
    ///
    /// # Errors
    /// [`DbError`] on read or write failure.
    pub async fn reembed_batch(self: &Arc<Self>) -> Result<usize, DbError> {
        let model = self.embedder.id().to_owned();
        let m2 = model.clone();
        let rows: Vec<(i64, String)> = self
            .read(move |c| {
                let mut s = c.prepare(
                    "SELECT id, text FROM chunks WHERE embed_model != ?1 ORDER BY id LIMIT ?2",
                )?;
                Ok(
                    s.query_map(params![m2, BATCH as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect::<Result<_, _>>()?,
                )
            })
            .await?;
        if rows.is_empty() {
            return Ok(0);
        }
        let texts: Vec<String> = rows.iter().map(|(_, t)| t.clone()).collect();
        let vectors = self.embed_blocking(texts).await;
        let done: Vec<(i64, Vec<f32>)> = rows
            .iter()
            .zip(vectors)
            .filter_map(|((id, _), v)| v.map(|v| (*id, v)))
            .collect();
        let n = done.len();
        if n == 0 {
            return Ok(0);
        }
        self.db
            .writer()
            .exec(move |c| {
                let mut ops = Vec::with_capacity(done.len());
                let mut up =
                    c.prepare("UPDATE chunks SET vector = ?2, embed_model = ?3 WHERE id = ?1")?;
                for (id, v) in done {
                    up.execute(params![id, vector_to_blob(&v), model])?;
                    ops.push(MatrixOp::Upsert {
                        id: u64::try_from(id).unwrap_or(0),
                        vector: v,
                    });
                }
                Ok(ops)
            })
            .await?;
        Ok(n)
    }

    /// Re-embeds everything stale in the background, yielding between batches, until none is
    /// left, the embedder fails, or `stop` turns true.
    pub async fn reembed_all(self: &Arc<Self>, mut stop: watch::Receiver<bool>) {
        loop {
            if *stop.borrow() {
                return;
            }
            match self.reembed_batch().await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
                _ = stop.changed() => return,
            }
        }
        let _ = self.stale_count().await;
        self.refresh_gauges();
    }
}
