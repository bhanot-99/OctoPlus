//! The storage engine: the database, the vector matrix and the embedder, owned together
//! (AP-3). Ingest, queries and lifecycle jobs are methods on [`Store`]; the IPC handler and the
//! tests drive nothing else.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use rusqlite::{Connection, params};

use crate::db::{Db, DbError, MatrixOp, PostCommit};
use crate::embed::Embedder;
use crate::vectors::VectorIndex;

/// Encodes a vector for the `chunks.vector` column (little-endian f32).
#[must_use]
pub fn vector_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Decodes a stored vector; `None` if the blob is not a whole number of floats.
#[must_use]
pub fn blob_to_vector(b: &[u8]) -> Option<Vec<f32>> {
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let (words, _) = b.as_chunks::<4>();
    Some(words.iter().map(|w| f32::from_le_bytes(*w)).collect())
}

/// Applies matrix changes after a commit. Poison is ignored on purpose: a panicked reader must
/// not stop the writer from keeping the matrix in step with the table.
fn apply_ops(index: &RwLock<VectorIndex>, ops: Vec<MatrixOp>) {
    let mut idx = index.write().unwrap_or_else(PoisonError::into_inner);
    for op in ops {
        match op {
            MatrixOp::Upsert { id, vector } => {
                let _ = idx.replace(id, &vector);
            }
            MatrixOp::Remove { id } => {
                idx.remove(id);
            }
        }
    }
}

/// Where C3 keeps its files (Architecture §7.1).
#[derive(Debug, Clone)]
pub struct StorePaths {
    /// The SQLite file, in `storage/`.
    pub db: PathBuf,
    /// The backups folder.
    pub backups: PathBuf,
}

impl StorePaths {
    /// `<root>/storage/neuroos.sqlite3` and `<root>/backups` for a data root such as
    /// `~/.local/share/neuroos`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            db: root.join("storage").join("neuroos.sqlite3"),
            backups: root.join("backups"),
        }
    }
}

/// C3's engine.
pub struct Store {
    pub(crate) db: Arc<Db>,
    pub(crate) index: Arc<RwLock<VectorIndex>>,
    pub(crate) embedder: Arc<dyn Embedder>,
    pub(crate) cfg: StorageConfig,
    pub(crate) paths: StorePaths,
    pub(crate) health: Arc<Health>,
    pub(crate) live: Mutex<crate::ingest::live::LiveState>,
    /// Chunks not embedded by the current model (kept fresh by the gauges and re-embedding).
    pub(crate) stale: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens the database at `paths.db`, loads the vector matrix for the current model, and records
    /// the model id in `index_meta` (FR-STO-12).
    ///
    /// # Errors
    /// [`DbError`] if the database cannot be opened or read.
    pub async fn open(
        paths: &StorePaths,
        cfg: StorageConfig,
        embedder: Arc<dyn Embedder>,
        health: Arc<Health>,
    ) -> Result<Self, DbError> {
        let index = Arc::new(RwLock::new(VectorIndex::new(embedder.dim())));
        let hook_index = Arc::clone(&index);
        let hook: PostCommit = Arc::new(move |ops| apply_ops(&hook_index, ops));
        let db = Arc::new(Db::open(&paths.db, cfg.reader_pool, cfg.write_queue, hook)?);
        let store = Self {
            db,
            index,
            embedder,
            cfg,
            paths: paths.clone(),
            health,
            live: Mutex::new(crate::ingest::live::LiveState::default()),
            stale: std::sync::atomic::AtomicU64::new(0),
        };
        let store = Arc::new(store);
        Arc::clone(&store).load_vectors().await?;
        store.record_model().await?;
        store.refresh_gauges();
        Arc::into_inner(store).ok_or(DbError::WriterGone)
    }

    /// Fills the matrix with every stored vector made by the current model. Vectors from another
    /// model are not comparable with a query embedded now, so they stay out until re-embedded.
    async fn load_vectors(self: Arc<Self>) -> Result<(), DbError> {
        let model = self.embedder.id().to_owned();
        let me = Arc::clone(&self);
        let rows: Vec<(u64, Vec<u8>)> = tokio::task::spawn_blocking(move || {
            me.db.read(|c| {
                let mut s = c.prepare(
                    "SELECT id, vector FROM chunks WHERE vector IS NOT NULL AND embed_model = ?1",
                )?;
                let rows = s
                    .query_map(params![model], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
                    })?
                    .filter_map(Result::ok)
                    .filter_map(|(id, b)| u64::try_from(id).ok().map(|id| (id, b)))
                    .collect();
                Ok(rows)
            })
        })
        .await
        .map_err(|_| DbError::WriterGone)??;
        let mut idx = self.index.write().unwrap_or_else(PoisonError::into_inner);
        for (id, blob) in rows {
            if let Some(v) = blob_to_vector(&blob) {
                let _ = idx.replace(id, &v);
            }
        }
        Ok(())
    }

    async fn record_model(&self) -> Result<(), DbError> {
        let (id, dim) = (
            self.embedder.id().to_owned(),
            self.embedder.dim().to_string(),
        );
        self.db.writer().exec(move |c| {
            for (k, v) in [("embedding_model", id), ("embedding_dim", dim)] {
                c.execute(
                    "INSERT INTO index_meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![k, v],
                )?;
            }
            Ok(vec![])
        }).await
    }

    /// Updates the live picture from one C1 event (open window, idle, media, recent samples).
    pub fn observe(&self, ev: &neuroos_proto::v1::MonitorEvent) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(ev);
    }

    /// The database.
    #[must_use]
    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    /// The configuration in force.
    #[must_use]
    pub fn config(&self) -> &StorageConfig {
        &self.cfg
    }

    /// The embedder.
    #[must_use]
    pub fn embedder(&self) -> &Arc<dyn Embedder> {
        &self.embedder
    }

    /// Vectors currently in the matrix.
    #[must_use]
    pub fn vectors_len(&self) -> usize {
        self.index
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Updates the health gauges from the database (cheap counts).
    pub fn refresh_gauges(&self) {
        let model = self.embedder.id().to_owned();
        let counts: Result<(i64, i64), DbError> = self.db.read(|c| {
            let chunks = c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
            let stale = c.query_row(
                "SELECT COUNT(*) FROM chunks WHERE embed_model != ?1",
                params![model],
                |r| r.get(0),
            )?;
            Ok((chunks, stale))
        });
        if let Ok((chunks, stale)) = counts {
            self.stale.store(
                u64::try_from(stale).unwrap_or(0),
                std::sync::atomic::Ordering::Relaxed,
            );
            #[allow(clippy::cast_precision_loss)]
            {
                self.health.gauge("chunks", chunks as f64);
                self.health.gauge("chunks_cap", self.cfg.chunk_cap as f64);
                self.health.gauge("stale_chunks", stale as f64);
                self.health.gauge("vectors", self.vectors_len() as f64);
            }
        }
    }

    /// Runs `f` on a reader connection on the blocking pool.
    ///
    /// # Errors
    /// Whatever `f` returns, or [`DbError::WriterGone`] if the pool task failed.
    pub async fn read<T: Send + 'static>(
        self: &Arc<Self>,
        f: impl FnOnce(&Connection) -> Result<T, DbError> + Send + 'static,
    ) -> Result<T, DbError> {
        let me = Arc::clone(self);
        tokio::task::spawn_blocking(move || me.db.read(f))
            .await
            .map_err(|_| DbError::WriterGone)?
    }
}

impl Store {
    /// Reads the saved promotion counters so a restart keeps its progress.
    ///
    /// # Errors
    /// [`DbError`] on read failure.
    pub async fn load_counters(
        self: &Arc<Self>,
    ) -> Result<Vec<crate::ingest::CounterRow>, DbError> {
        self.read(|c| {
            let mut s = c.prepare(
                "SELECT key, domain, app_id, occurrences, dwell_ns, first_ns, last_ns, promoted FROM event_counters",
            )?;
            let rows = s
                .query_map([], |r| {
                    Ok(crate::ingest::CounterRow {
                        key: r.get(0)?,
                        domain: r.get(1)?,
                        app_id: r.get(2)?,
                        occurrences: u64::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                        dwell_ns: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                        first_ns: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                        last_ns: u64::try_from(r.get::<_, i64>(6)?).unwrap_or(0),
                        promoted: r.get::<_, i64>(7)? != 0,
                    })
                })?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .await
    }
}
