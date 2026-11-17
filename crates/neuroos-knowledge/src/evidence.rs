//! Evidence filtering and deduplication for Semantic-NeuroOS (C5).
//!
//! Implements FR-KNO-01, FR-KNO-04, and taint propagation rules (R0-4).

use std::collections::HashSet;

use neuroos_proto::v1::Evidence;
use neuroos_taint::TaintFlags;

/// Outcome of evidence selection and filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    /// Filtered and deduplicated evidence items (at most `max`).
    pub items: Vec<Evidence>,
    /// Union taint of the kept items.
    pub taint: TaintFlags,
    /// Number of items dropped due to missing or undecodable taint.
    pub dropped_no_taint: u32,
    /// Number of items dropped due to score below cutoff (including NaN).
    pub dropped_below_cutoff: u32,
    /// Number of items dropped due to duplicate `chunk_id` or duplicate normalised text.
    pub dropped_duplicate: u32,
}

/// Normalises evidence text: lower-case with whitespace collapsed to single spaces.
fn normalise_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            for lc in c.to_lowercase() {
                out.push(lc);
            }
        }
    }
    out
}

/// Keeps at most `max` items, best score first (stable for ties: smaller `chunk_id` first).
///
/// Drops, counting each reason once (evaluated in this order):
/// 1. Taint missing or not decodable by [`TaintFlags::decode`] (R0-4: missing taint is an error, never clean);
/// 2. Score < `cutoff` (NaN scores count as below cutoff);
/// 3. Duplicate `chunk_id`;
/// 4. Duplicate normalised text (lower-case, whitespace collapsed).
///
/// `taint` is the union of the kept items' taint.
#[must_use]
pub fn select(evidence: Vec<Evidence>, cutoff: f64, max: usize) -> Selected {
    let mut dropped_no_taint = 0u32;
    let mut dropped_below_cutoff = 0u32;
    let mut dropped_duplicate = 0u32;

    // Phase 1: Filter out items with missing/invalid taint and below cutoff
    let mut valid: Vec<(Evidence, TaintFlags)> = Vec::with_capacity(evidence.len());

    for ev in evidence {
        // Step 1: Check taint
        let taint_raw = ev.taint.as_ref().map(|t| t.flags);
        let taint_flags = match TaintFlags::decode(taint_raw) {
            Ok(flags) => flags,
            Err(_) => {
                dropped_no_taint += 1;
                continue;
            }
        };

        // Step 2: Check score cutoff (NaN counts as below cutoff)
        if ev.score.is_nan() || ev.score < cutoff {
            dropped_below_cutoff += 1;
            continue;
        }

        valid.push((ev, taint_flags));
    }

    // Phase 2: Sort valid items: highest score first; tie-break smaller chunk_id first
    valid.sort_by(|(a, _), (b, _)| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });

    // Phase 3: Deduplicate (by chunk_id, then by normalised text) and retain at most `max`
    let mut seen_chunk_ids = HashSet::new();
    let mut seen_texts = HashSet::new();
    let mut kept_items = Vec::with_capacity(valid.len().min(max));
    let mut total_taint = TaintFlags::empty();

    for (ev, flags) in valid {
        if seen_chunk_ids.contains(&ev.chunk_id) {
            dropped_duplicate += 1;
            continue;
        }
        let norm = normalise_text(&ev.text);
        if seen_texts.contains(&norm) {
            dropped_duplicate += 1;
            continue;
        }

        seen_chunk_ids.insert(ev.chunk_id);
        seen_texts.insert(norm);

        if kept_items.len() < max {
            total_taint = total_taint.merge(flags);
            kept_items.push(ev);
        }
    }

    Selected {
        items: kept_items,
        taint: total_taint,
        dropped_no_taint,
        dropped_below_cutoff,
        dropped_duplicate,
    }
}
