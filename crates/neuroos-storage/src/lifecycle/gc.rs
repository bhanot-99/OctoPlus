//! Retention and the chunk cap (FR-STO-06, FR-STO-11, PRD §6.5).

use neuroos_common::config::StorageConfig;
use neuroos_proto::v1::Domain;
use rusqlite::{Connection, params};

use crate::db::{DbError, MatrixOp};

/// Nanoseconds in a day.
const DAY_NS: i64 = 86_400_000_000_000;

/// What one GC pass removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcStats {
    /// Chunks deleted.
    pub chunks: u64,
    /// Entities deleted (left without chunks).
    pub entities: u64,
    /// Focus-history rows deleted.
    pub focus_rows: u64,
    /// Raw or summary health rows deleted.
    pub health_rows: u64,
    /// Nudge-log rows deleted.
    pub nudge_rows: u64,
}

/// Domains whose data lives as long as its source (PRD §6.5): never expired by age.
const PERMANENT: [Domain; 4] = [
    Domain::Notes,
    Domain::GitActivity,
    Domain::Calendar,
    Domain::UserProfile,
];

fn permanent_list() -> String {
    PERMANENT
        .iter()
        .map(|d| (*d as i32).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// How many days a domain's chunks are kept; `None` = while the source exists.
#[must_use]
pub fn retention_days(domain: Domain, cfg: &StorageConfig) -> Option<u32> {
    match domain {
        Domain::WindowFocus
        | Domain::AppLifecycle
        | Domain::IdlePresence
        | Domain::ProcessActivity
        | Domain::BuildJob => Some(cfg.retention_focus_days),
        Domain::MediaPlayback => Some(cfg.retention_media_days),
        Domain::VoiceInteraction | Domain::AssistantActions => Some(cfg.retention_voice_days),
        Domain::ExternalDocuments => Some(cfg.retention_web_days),
        Domain::SystemHealth => Some(cfg.retention_health_raw_days),
        Domain::Notes
        | Domain::GitActivity
        | Domain::Calendar
        | Domain::UserProfile
        | Domain::Unspecified => None,
    }
}

fn ids_to_ops(ids: &[i64]) -> Vec<MatrixOp> {
    ids.iter()
        .filter_map(|id| u64::try_from(*id).ok())
        .map(|id| MatrixOp::Remove { id })
        .collect()
}

/// Deletes chunks by id and returns the matrix removals.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn delete_chunks(c: &Connection, ids: &[i64]) -> Result<Vec<MatrixOp>, DbError> {
    let mut stmt = c.prepare("DELETE FROM chunks WHERE id = ?1")?;
    for id in ids {
        stmt.execute(params![id])?;
    }
    Ok(ids_to_ops(ids))
}

/// Deletes entities that no longer have any chunk.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn drop_empty_entities(c: &Connection) -> Result<u64, DbError> {
    let n = c.execute(
        "DELETE FROM entities WHERE id NOT IN (SELECT DISTINCT entity_id FROM chunks)",
        [],
    )?;
    Ok(n as u64)
}

/// Deletes the oldest chunks until at most `cap` remain: non-permanent domains first, oldest first
/// (FR-STO-06), then permanent ones as a last resort so the cap always holds.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn trim_to_cap(c: &Connection, cap: usize) -> Result<Vec<MatrixOp>, DbError> {
    let count: i64 = c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let over = count - i64::try_from(cap).unwrap_or(i64::MAX);
    if over <= 0 {
        return Ok(Vec::new());
    }
    let perm = permanent_list();
    let sql = format!(
        "SELECT id FROM chunks ORDER BY (domain IN ({perm})) ASC, at_ns ASC, id ASC LIMIT ?1"
    );
    let ids: Vec<i64> = {
        let mut s = c.prepare(&sql)?;
        s.query_map(params![over], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    let ops = delete_chunks(c, &ids)?;
    drop_empty_entities(c)?;
    Ok(ops)
}

/// One GC pass at time `now_ns`: expire by row time per domain (PRD §6.5), expire focus history,
/// nudge log and health rows, drop orphan entities and stale counters, then enforce the cap.
///
/// # Errors
/// [`DbError`] on SQL failure.
pub fn run_gc(
    c: &Connection,
    now_ns: u64,
    cfg: &StorageConfig,
) -> Result<(GcStats, Vec<MatrixOp>), DbError> {
    let now = i64::try_from(now_ns).unwrap_or(i64::MAX);
    let mut stats = GcStats::default();
    let mut ops = Vec::new();
    for d in 1..=14 {
        let Ok(domain) = Domain::try_from(d) else {
            continue;
        };
        let Some(days) = retention_days(domain, cfg) else {
            continue;
        };
        let cutoff = now.saturating_sub(i64::from(days).saturating_mul(DAY_NS));
        let ids: Vec<i64> = {
            let mut s = c.prepare("SELECT id FROM chunks WHERE domain = ?1 AND at_ns < ?2")?;
            s.query_map(params![d, cutoff], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        stats.chunks += ids.len() as u64;
        ops.extend(delete_chunks(c, &ids)?);
    }
    let focus_cut = now.saturating_sub(i64::from(cfg.retention_focus_days).saturating_mul(DAY_NS));
    stats.focus_rows = c.execute(
        "DELETE FROM focus_history WHERE end_ns < ?1",
        params![focus_cut],
    )? as u64;
    let nudge_cut = now.saturating_sub(i64::from(cfg.retention_nudge_days).saturating_mul(DAY_NS));
    stats.nudge_rows = c.execute(
        // Switch rows (ENABLED = 10, DISABLED = 9) are settings, not history: they stay.
        "DELETE FROM nudge_log WHERE at_ns < ?1 AND outcome NOT IN (9, 10)",
        params![nudge_cut],
    )? as u64;
    let roll_cut =
        now.saturating_sub(i64::from(cfg.retention_health_rollup_days).saturating_mul(DAY_NS));
    stats.health_rows = c.execute(
        "DELETE FROM health_samples WHERE rollup = 1 AND at_ns < ?1",
        params![roll_cut],
    )? as u64;
    // Counters of things never promoted and not seen for a retention period are just clutter.
    c.execute(
        "DELETE FROM event_counters WHERE promoted = 0 AND last_ns < ?1",
        params![focus_cut],
    )?;
    stats.entities = drop_empty_entities(c)?;
    ops.extend(trim_to_cap(c, cfg.chunk_cap)?);
    Ok((stats, ops))
}
