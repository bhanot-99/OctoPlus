//! Hybrid search rank fusion and FTS5 query sanitisation.
//!
//! Satisfies FR-STO-05 and security.md SC ("hostile titles are inert in SQL"):
//! - Reciprocal Rank Fusion (`rrf`) combines ranking lists deterministically.
//! - FTS query sanitiser (`fts_match`) converts arbitrary untrusted text into safe, quoted FTS5 terms.

use std::collections::{HashMap, HashSet};

/// Maximum number of terms preserved in an FTS5 MATCH expression.
pub const MAX_TERMS: usize = 12;

/// Maximum number of characters preserved per term in an FTS5 MATCH expression.
pub const MAX_TERM_CHARS: usize = 40;

/// Reciprocal rank fusion.
///
/// `rankings` are lists of ids, best first. An id's score is the sum over the lists it
/// appears in of `1.0 / (k as f64 + rank as f64)`, rank counting from 1.
/// Returns every distinct id with its fused score, best first; ties broken by the
/// SMALLER id. An id repeated inside one list counts only at its first (best) position.
/// `k >= 1` (a k of 0 is treated as 1).
///
/// Implements FR-STO-05 (hybrid retrieval ranking combination).
#[must_use]
pub fn rrf(rankings: &[&[u64]], k: u32) -> Vec<(u64, f64)> {
    let k_f64 = f64::from(k.max(1));
    // Terms per id, summed in a fixed order below: float addition is not associative, and two ids
    // with the same set of ranks must get bit-identical scores so the id tie-break decides.
    let mut terms: HashMap<u64, Vec<f64>> = HashMap::new();

    for list in rankings {
        let mut seen = HashSet::with_capacity(list.len());
        for (idx, &id) in list.iter().enumerate() {
            if seen.insert(id) {
                let rank = (idx.saturating_add(1)) as f64;
                let rrf_score = 1.0f64 / (k_f64 + rank);
                terms.entry(id).or_default().push(rrf_score);
            }
        }
    }

    let mut result: Vec<(u64, f64)> = terms
        .into_iter()
        .map(|(id, mut t)| {
            t.sort_unstable_by(f64::total_cmp);
            (id, t.iter().sum())
        })
        .collect();
    // Mathematically equal sums built from different ranks (1/30 + 1/6 = 1/5) differ in the last
    // bit; ordering uses the score on a 1e-12 grid so those tie and the smaller id wins. A grid
    // key (not a tolerance comparison) keeps the order total, which `sort` requires.
    #[allow(clippy::cast_possible_truncation)]
    let key = |s: f64| (s * 1e12).round() as i64;
    result.sort_unstable_by(|a, b| key(b.1).cmp(&key(a.1)).then_with(|| a.0.cmp(&b.0)));
    // Tied scores come out identical, so "non-increasing" holds exactly.
    #[allow(clippy::cast_precision_loss)]
    result.iter_mut().for_each(|r| r.1 = key(r.1) as f64 / 1e12);
    result
}

/// Turns arbitrary user text into an FTS5 MATCH expression that cannot be an operator injection.
///
/// Splits on anything that is not alphanumeric (Unicode), lower-cases, drops terms
/// shorter than 2 characters, cuts each term to 40 characters, drops duplicates
/// keeping the first occurrence, keeps at most 12 terms, wraps EVERY term in double
/// quotes (no double quote can occur inside a term because it was split out), and joins
/// with `" OR "`. Returns `None` when no term remains.
///
/// Implements security.md SC ("hostile titles are inert in SQL") and FR-STO-05.
#[must_use]
pub fn fts_match(text: &str) -> Option<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();

    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        if raw.chars().count() < 2 {
            continue;
        }
        let lower = raw.to_lowercase();
        let cut: String = lower.chars().take(MAX_TERM_CHARS).collect();
        if cut.chars().count() < 2 {
            continue;
        }
        if seen.insert(cut.clone()) {
            terms.push(cut);
            if terms.len() >= MAX_TERMS {
                break;
            }
        }
    }

    if terms.is_empty() {
        return None;
    }

    let joined = terms
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ");

    Some(joined)
}
