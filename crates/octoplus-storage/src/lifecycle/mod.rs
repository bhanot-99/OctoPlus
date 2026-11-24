//! Lifecycle jobs (FR-STO-06, -11, -12, -13; Architecture §7.5): garbage collection and the chunk
//! cap, health roll-up, backup, re-embedding, forget and purge. SQL here is pure functions over a
//! connection and a time argument, so every boundary is testable with a fake "now" (AB-13).

pub mod backup;
pub mod forget;
pub mod gc;
pub mod reembed;
pub mod rollup;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use octoplus_proto::v1::{LifecycleJob, LifecycleResult};

use crate::db::DbError;
use crate::store::Store;

impl Store {
    /// Garbage collection at `now_ns`: expire by retention, enforce the cap, compact (FR-STO-06, FR-STO-11).
    ///
    /// # Errors
    /// [`DbError`] on failure.
    pub async fn gc(self: &Arc<Self>, now_ns: u64) -> Result<gc::GcStats, DbError> {
        let cfg = self.cfg.clone();
        let stats = Arc::new(std::sync::Mutex::new(gc::GcStats::default()));
        let s2 = Arc::clone(&stats);
        self.db
            .writer()
            .exec(move |c| {
                let (st, ops) = gc::run_gc(c, now_ns, &cfg)?;
                *s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = st;
                Ok(ops)
            })
            .await?;
        self.db
            .writer()
            .maintain(|c| Ok(c.execute_batch("PRAGMA incremental_vacuum")?))
            .await?;
        self.refresh_gauges();
        let st = *stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(st)
    }

    /// Runs a lifecycle job named in a request and times it (PRD §5.3).
    ///
    /// # Errors
    /// [`DbError::Invalid`] for an unknown job; [`DbError`] if the job fails.
    pub async fn run_job(
        self: &Arc<Self>,
        job: LifecycleJob,
        now_ns: u64,
    ) -> Result<LifecycleResult, DbError> {
        let started = Instant::now();
        let items = AtomicU64::new(0);
        match job {
            LifecycleJob::Gc => {
                let st = self.gc(now_ns).await?;
                items.store(
                    st.chunks + st.focus_rows + st.health_rows + st.nudge_rows + st.entities,
                    Ordering::Relaxed,
                );
            }
            LifecycleJob::Backup => {
                self.backup(now_ns).await?;
                items.store(1, Ordering::Relaxed);
            }
            LifecycleJob::Rollup => items.store(self.rollup(now_ns).await?, Ordering::Relaxed),
            LifecycleJob::Purge => {
                self.purge().await?;
                items.store(1, Ordering::Relaxed);
            }
            LifecycleJob::Unspecified => return Err(DbError::Invalid("unknown lifecycle job")),
        }
        Ok(LifecycleResult {
            job: job as i32,
            duration_ns: u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            items: items.load(Ordering::Relaxed),
        })
    }
}
