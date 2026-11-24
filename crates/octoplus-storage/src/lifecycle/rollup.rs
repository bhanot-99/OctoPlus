//! Hourly health roll-up (PRD §6.5, Architecture §7.5): raw samples older than the raw window
//! become one summary row per metric per hour (mean, min, max, count) and the raw rows go.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, params};

use crate::db::DbError;
use crate::store::Store;

const HOUR_NS: i64 = 3_600_000_000_000;
const DAY_NS: i64 = 86_400_000_000_000;

/// Rolls up raw samples older than `raw_days` as of `now_ns`. Returns the raw rows replaced.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn rollup(c: &Connection, now_ns: u64, raw_days: u32) -> Result<u64, DbError> {
    let now = i64::try_from(now_ns).unwrap_or(i64::MAX);
    let cutoff = now.saturating_sub(i64::from(raw_days).saturating_mul(DAY_NS));
    let cut = cutoff - cutoff.rem_euclid(HOUR_NS); // whole hours only: a half-done hour stays raw
    c.execute(
        "INSERT INTO health_samples (metric, value, at_ns, rollup, vmin, vmax, n)
         SELECT metric, SUM(value * n) / SUM(n), (at_ns / ?2) * ?2, 1, MIN(vmin), MAX(vmax), SUM(n)
         FROM health_samples WHERE rollup = 0 AND at_ns < ?1 GROUP BY metric, at_ns / ?2",
        params![cut, HOUR_NS],
    )?;
    let removed = c.execute(
        "DELETE FROM health_samples WHERE rollup = 0 AND at_ns < ?1",
        params![cut],
    )?;
    Ok(removed as u64)
}

impl Store {
    /// Runs the roll-up job.
    ///
    /// # Errors
    /// [`DbError`] on failure.
    pub async fn rollup(self: &Arc<Self>, now_ns: u64) -> Result<u64, DbError> {
        let days = self.cfg.retention_health_raw_days;
        let n = Arc::new(AtomicU64::new(0));
        let n2 = Arc::clone(&n);
        self.db
            .writer()
            .exec(move |c| {
                n2.store(rollup(c, now_ns, days)?, Ordering::Relaxed);
                Ok(vec![])
            })
            .await?;
        Ok(n.load(Ordering::Relaxed))
    }
}
