//! Hybrid search (FR-STO-05, P4-S03): the query is embedded, scanned exactly against the in-memory
//! matrix, matched against the text index, and the two rankings are fused (reciprocal rank
//! fusion). Taint rides on every result; results that are not from the current embedding model
//! carry `STALE_INDEX`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use neuroos_proto::v1::{Domain, Evidence, HybridResult, QueryHybrid, Taint};
use neuroos_taint::TaintFlags;
use rusqlite::{Connection, params_from_iter};

use crate::db::DbError;
use crate::fusion::{fts_match, rrf};
use crate::store::Store;

/// Results returned when the request does not say.
const DEFAULT_TOP_K: u32 = 5;

struct Row {
    domain: i32,
    text: String,
    taint: u32,
    at_ns: u64,
    source: String,
    stale: bool,
}

fn fetch_rows(c: &Connection, ids: &[u64], model: &str) -> Result<HashMap<u64, Row>, DbError> {
    let mut out = HashMap::new();
    let mut stmt = c.prepare(
        "SELECT domain, text, taint, at_ns, source, embed_model FROM chunks WHERE id = ?1",
    )?;
    for id in ids {
        let Ok(key) = i64::try_from(*id) else {
            continue;
        };
        let row = stmt.query_row([key], |r| {
            Ok(Row {
                domain: r.get(0)?,
                text: r.get(1)?,
                taint: u32::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                at_ns: u64::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                source: r.get(4)?,
                stale: r.get::<_, String>(5)? != model,
            })
        });
        if let Ok(row) = row {
            out.insert(*id, row);
        }
    }
    Ok(out)
}

/// Chunk ids matching `expr` in the text index, best first, restricted to `domains` if any.
fn text_hits(
    c: &Connection,
    expr: &str,
    domains: &[i32],
    limit: usize,
) -> Result<Vec<u64>, DbError> {
    let mut sql = String::from(
        "SELECT chunks_fts.rowid FROM chunks_fts JOIN chunks ON chunks.id = chunks_fts.rowid WHERE chunks_fts MATCH ?1",
    );
    if !domains.is_empty() {
        sql.push_str(" AND chunks.domain IN (");
        sql.push_str(&domains.iter().map(|_| "?").collect::<Vec<_>>().join(","));
        sql.push(')');
    }
    sql.push_str(" ORDER BY rank LIMIT ");
    sql.push_str(&limit.to_string());
    let mut stmt = c.prepare(&sql)?;
    let mut args: Vec<rusqlite::types::Value> = vec![expr.to_owned().into()];
    args.extend(
        domains
            .iter()
            .map(|d| rusqlite::types::Value::from(i64::from(*d))),
    );
    let ids = stmt
        .query_map(params_from_iter(args), |r| r.get::<_, i64>(0))?
        .filter_map(Result::ok)
        .filter_map(|i| u64::try_from(i).ok())
        .collect();
    Ok(ids)
}

impl Store {
    /// Answers a hybrid query (FR-STO-05).
    ///
    /// # Errors
    /// [`DbError`] on read failure. An embedding failure is not an error: the answer then comes
    /// from the text index alone and is flagged `stale_index`.
    pub async fn hybrid(self: &Arc<Self>, q: QueryHybrid) -> Result<HybridResult, DbError> {
        let top_k = usize::try_from(if q.top_k == 0 {
            DEFAULT_TOP_K
        } else {
            q.top_k.min(self.cfg.query_top_k_max)
        })
        .unwrap_or(5);
        let domains: Vec<i32> = q.domains.clone();
        if q.text.trim().is_empty() {
            return Ok(HybridResult {
                evidence: Vec::new(),
                stale_index: false,
            });
        }
        let candidates =
            self.cfg.query_candidates.max(top_k) * if domains.is_empty() { 1 } else { 4 };
        let query_vec = self
            .embed_blocking(vec![q.text.clone()])
            .await
            .into_iter()
            .next()
            .flatten();
        // Exact scan on the blocking pool (it uses several threads of its own).
        let me = Arc::clone(self);
        let scored: Vec<(u64, f32)> = match query_vec {
            Some(v) => tokio::task::spawn_blocking(move || {
                me.index
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .top_k(&v, candidates, me.cfg.scan_threads)
            })
            .await
            .map_err(|_| DbError::WriterGone)?,
            None => Vec::new(),
        };
        let cosine: HashMap<u64, f32> = scored.iter().copied().collect();
        let expr = fts_match(&q.text);
        let model = self.embedder.id().to_owned();
        let (rk, doms) = (self.cfg.rrf_k, domains.clone());
        let vec_ids: Vec<u64> = scored.iter().map(|(i, _)| *i).collect();
        let (text_ids, rows) = self
            .read(move |c| {
                let text_ids = match &expr {
                    Some(e) => text_hits(c, e, &doms, candidates)?,
                    None => Vec::new(),
                };
                let mut all = vec_ids.clone();
                all.extend(text_ids.iter().copied());
                all.sort_unstable();
                all.dedup();
                Ok((text_ids, fetch_rows(c, &all, &model)?))
            })
            .await?;
        // Domain filter for vector candidates (the matrix knows no domains).
        let keep = |id: &u64| {
            rows.get(id)
                .is_some_and(|r| domains.is_empty() || domains.contains(&r.domain))
        };
        let vec_ranked: Vec<u64> = scored.iter().map(|(i, _)| *i).filter(keep).collect();
        let text_ranked: Vec<u64> = text_ids.into_iter().filter(keep).collect();
        let fused = rrf(&[&vec_ranked, &text_ranked], rk);
        let best = 2.0 / (f64::from(rk) + 1.0);
        let stale_any = self.stale.load(Ordering::Relaxed) > 0;
        let mut evidence: Vec<Evidence> = Vec::new();
        let mut by_text: HashMap<String, usize> = HashMap::new();
        for (id, fused_score) in fused {
            let Some(row) = rows.get(&id) else { continue };
            let score = cosine
                .get(&id)
                .map_or(fused_score / best, |c| f64::from(*c));
            if score < q.min_score {
                continue;
            }
            let mut flags = TaintFlags::from_bits_truncate(row.taint);
            if row.stale {
                flags |= TaintFlags::STALE_INDEX;
            }
            // The same text from two entities is one piece of evidence; taint is unioned (R0-4).
            if let Some(&i) = by_text.get(&row.text) {
                let merged = TaintFlags::from_bits_truncate(
                    evidence[i].taint.as_ref().map_or(0, |t| t.flags),
                ) | flags;
                evidence[i].taint = Some(Taint {
                    flags: merged.bits(),
                });
                continue;
            }
            by_text.insert(row.text.clone(), evidence.len());
            evidence.push(Evidence {
                chunk_id: id,
                domain: Domain::try_from(row.domain).unwrap_or(Domain::Unspecified) as i32,
                text: row.text.clone(),
                score,
                taint: Some(Taint {
                    flags: flags.bits(),
                }),
                at_ns: row.at_ns,
                source: row.source.clone(),
            });
            if evidence.len() >= top_k {
                break;
            }
        }
        let stale_index = stale_any
            || evidence.iter().any(|e| {
                e.taint
                    .as_ref()
                    .is_some_and(|t| t.flags & TaintFlags::STALE_INDEX.bits() != 0)
            });
        Ok(HybridResult {
            evidence,
            stale_index,
        })
    }
}
