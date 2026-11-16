//! Turns filter [`Output`]s into database changes. One batch is one writer job, so a batch lands
//! completely or not at all, and the vector matrix follows the commit (ADR-0004).

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::{CounterRow, FocusSegment, HealthPoint, Item, Output};
use crate::db::{DbError, MatrixOp};
use crate::store::vector_to_blob;

/// An output plus the embedding computed for its text (promoted items only).
#[derive(Debug, Clone)]
pub struct Prepared {
    /// What to store.
    pub output: Output,
    /// Embedding of an item's text and the model that made it; `None` stores text only (stale).
    pub vector: Option<(Vec<f32>, String)>,
}

/// First 8 bytes of SHA-256 of the text, as the de-duplication key.
#[must_use]
pub fn text_hash(text: &str) -> i64 {
    let d = Sha256::digest(text.as_bytes());
    i64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}

pub(crate) fn signed(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn taint_bits(item: &Item) -> i64 {
    i64::from(item.taint.bits())
}

/// Inserts or merges an item. The same (entity, text) is one chunk: taint is unioned (R0-4),
/// the time moves forward, and a vector is only added, never lost.
///
/// # Errors
/// [`DbError`] on any SQL failure.
pub fn upsert_item(
    c: &Connection,
    it: &Item,
    source: &str,
    vector: Option<&(Vec<f32>, String)>,
) -> Result<Option<MatrixOp>, DbError> {
    let entity: i64 = c.query_row(
        "INSERT INTO entities (domain, kind, label, app_id, taint, first_seen_ns, last_seen_ns)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT (domain, kind, label) DO UPDATE SET
             last_seen_ns = MAX(last_seen_ns, excluded.last_seen_ns),
             taint = taint | excluded.taint,
             app_id = CASE WHEN excluded.app_id != '' THEN excluded.app_id ELSE app_id END
         RETURNING id",
        params![
            it.domain as i32,
            it.kind,
            it.label,
            it.app_id,
            taint_bits(it),
            signed(it.at_ns)
        ],
        |r| r.get(0),
    )?;
    let hash = text_hash(&it.text);
    let existing: Option<(i64, bool)> = c
        .query_row(
            "SELECT id, vector IS NOT NULL FROM chunks WHERE entity_id = ?1 AND text_hash = ?2",
            params![entity, hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let blob = vector.map(|(v, m)| (vector_to_blob(v), m.as_str()));
    if let Some((id, has_vector)) = existing {
        c.execute(
            "UPDATE chunks SET at_ns = MAX(at_ns, ?2), taint = taint | ?3 WHERE id = ?1",
            params![id, signed(it.at_ns), taint_bits(it)],
        )?;
        if let (false, Some((b, m)), Some((v, _))) = (has_vector, &blob, vector) {
            c.execute(
                "UPDATE chunks SET vector = ?2, embed_model = ?3 WHERE id = ?1",
                params![id, b, m],
            )?;
            return Ok(Some(MatrixOp::Upsert {
                id: u64::try_from(id).unwrap_or(0),
                vector: v.clone(),
            }));
        }
        return Ok(None);
    }
    c.execute(
        "INSERT INTO chunks (entity_id, domain, text, text_hash, taint, at_ns, source, app_id, vector, embed_model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            entity,
            it.domain as i32,
            it.text,
            hash,
            taint_bits(it),
            signed(it.at_ns),
            source,
            it.app_id,
            blob.as_ref().map(|(b, _)| b.clone()),
            blob.as_ref().map_or("", |(_, m)| m),
        ],
    )?;
    let id = c.last_insert_rowid();
    Ok(vector.map(|(v, _)| MatrixOp::Upsert {
        id: u64::try_from(id).unwrap_or(0),
        vector: v.clone(),
    }))
}

fn insert_segment(c: &Connection, s: &FocusSegment) -> Result<(), DbError> {
    c.execute(
        "INSERT INTO focus_history (app_id, title, pid, start_ns, end_ns, dwell_ns, taint) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
        params![s.app_id, s.title, s.pid, signed(s.start_ns), signed(s.end_ns), signed(s.dwell_ns)],
    )?;
    Ok(())
}

fn insert_health(c: &Connection, h: &HealthPoint) -> Result<(), DbError> {
    c.execute(
        "INSERT INTO health_samples (metric, value, at_ns, rollup, vmin, vmax, n) VALUES (?1, ?2, ?3, 0, ?2, ?2, 1)",
        params![h.metric, h.value, signed(h.at_ns)],
    )?;
    Ok(())
}

fn upsert_counter(c: &Connection, r: &CounterRow) -> Result<(), DbError> {
    c.execute(
        "INSERT INTO event_counters (key, domain, app_id, occurrences, dwell_ns, first_ns, last_ns, promoted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (key) DO UPDATE SET occurrences = excluded.occurrences, dwell_ns = excluded.dwell_ns,
             last_ns = excluded.last_ns, promoted = excluded.promoted",
        params![
            r.key,
            r.domain,
            r.app_id,
            signed(r.occurrences),
            signed(r.dwell_ns),
            signed(r.first_ns),
            signed(r.last_ns),
            i32::from(r.promoted)
        ],
    )?;
    Ok(())
}

/// Removes an entity whose source is gone, with its chunks; returns the matrix removals.
fn source_gone(
    c: &Connection,
    domain: i32,
    kind: &str,
    label: &str,
) -> Result<Vec<MatrixOp>, DbError> {
    let ids: Vec<i64> = {
        let mut s = c.prepare(
            "SELECT id FROM chunks WHERE entity_id IN (SELECT id FROM entities WHERE domain = ?1 AND kind = ?2 AND label = ?3)",
        )?;
        s.query_map(params![domain, kind, label], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    c.execute(
        "DELETE FROM entities WHERE domain = ?1 AND kind = ?2 AND label = ?3",
        params![domain, kind, label],
    )?;
    Ok(ids
        .into_iter()
        .filter_map(|id| u64::try_from(id).ok())
        .map(|id| MatrixOp::Remove { id })
        .collect())
}

/// Applies a whole batch inside the writer's savepoint.
///
/// # Errors
/// [`DbError`]; the writer then rolls the whole batch back.
pub fn apply_batch(
    c: &Connection,
    batch: &[Prepared],
    source: &str,
) -> Result<Vec<MatrixOp>, DbError> {
    let mut ops = Vec::new();
    for p in batch {
        match &p.output {
            Output::Promote(it) => ops.extend(upsert_item(c, it, source, p.vector.as_ref())?),
            Output::Segment(s) => insert_segment(c, s)?,
            Output::Health(h) => insert_health(c, h)?,
            Output::Counter(r) => upsert_counter(c, r)?,
            Output::SourceGone {
                domain,
                kind,
                label,
            } => ops.extend(source_gone(c, *domain as i32, kind, label)?),
            Output::Demoted(_) => {}
        }
    }
    Ok(ops)
}
