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
/// Words too common to tell one line from another.
const STOP_WORDS: &[&str] = &[
    "the", "and", "what", "whats", "how", "who", "when", "where", "why", "which", "does", "did",
    "for", "from", "with", "about", "that", "this", "are", "was", "were", "can", "you", "your",
    "please", "check", "internet", "online", "web", "search", "look", "tell", "give", "find",
];

/// The lines of `text`, each split into sentences (a `.`, `!` or `?` followed by a space, so
/// "4.06" stays whole).
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let bytes = line.as_bytes();
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if matches!(b, b'.' | b'!' | b'?') && bytes.get(i + 1) == Some(&b' ') {
                let piece = line[start..=i].trim();
                if !piece.is_empty() {
                    out.push(piece);
                }
                start = i + 1;
            }
        }
        let tail = line[start..].trim();
        if !tail.is_empty() {
            out.push(tail);
        }
    }
    out
}

/// What the question is made of: its content words and its adjacent word pairs.
struct QuestionTerms {
    words: HashSet<String>,
    /// Adjacent pairs ("of india") where at least one word is a content word.
    pairs: HashSet<String>,
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn is_content(w: &str) -> bool {
    w.chars().count() >= 3 && !STOP_WORDS.contains(&w)
}

fn question_words(question: &str) -> QuestionTerms {
    let all = tokens(question);
    let words = all.iter().filter(|w| is_content(w)).cloned().collect();
    let pairs = all
        .windows(2)
        .filter(|p| is_content(&p[0]) || is_content(&p[1]))
        .map(|p| format!("{} {}", p[0], p[1]))
        .collect();
    QuestionTerms { words, pairs }
}

/// Distinct question words a line holds (two points each), two more for each adjacent question
/// word pair it keeps together ("gdp of india"), plus one when it also has a digit.
fn line_score(line: &str, q: &QuestionTerms) -> usize {
    let lower = line.to_lowercase();
    let hits = q
        .words
        .iter()
        .filter(|w| lower.contains(w.as_str()))
        .count();
    if hits == 0 {
        return 0;
    }
    let toks = tokens(line);
    let pairs = toks
        .windows(2)
        .filter(|p| q.pairs.contains(&format!("{} {}", p[0], p[1])))
        .count();
    hits * 2 + pairs * 2 + usize::from(line.bytes().any(|b| b.is_ascii_digit()))
}

/// How well the best line of `text` answers `question`; 0 when no line mentions it.
#[must_use]
pub fn answer_score(text: &str, question: &str) -> usize {
    let words = question_words(question);
    sentences(text)
        .into_iter()
        .map(|l| line_score(l, &words))
        .max()
        .unwrap_or(0)
}

/// Cuts a long page chunk down to the lines that answer `question` (web answers only).
///
/// A stored page chunk often starts with menus and language lists; the prompt keeps only the
/// head of each chunk, so the figure asked for could sit past the cut. Lines are scored by the
/// distinct question words they hold (a digit adds a point, since most asked-for facts are
/// numbers), the best lines are kept in their original order up to `max_bytes`, and nothing is
/// invented: every kept line is verbatim. A chunk that already fits is returned unchanged.
#[must_use]
pub fn excerpt_for_question(text: &str, question: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.trim().to_string();
    }
    let words = question_words(question);
    let mut scored: Vec<(usize, usize, &str)> = sentences(text)
        .into_iter()
        .enumerate()
        .map(|(i, l)| (line_score(l, &words), i, l))
        .filter(|(score, _, _)| *score > 0)
        .collect();
    if scored.is_empty() {
        return truncate_at_char(text.trim(), max_bytes).to_string();
    }
    // Best first; among equals a fuller sentence beats a short heading, then the earlier wins.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.2.len().cmp(&a.2.len()))
            .then(a.1.cmp(&b.1))
    });
    let mut kept: Vec<(usize, &str)> = Vec::new();
    let mut used = 0;
    for (_, i, l) in scored {
        let l = truncate_at_char(l, max_bytes);
        if used + l.len() + 1 > max_bytes {
            continue;
        }
        used += l.len() + 1;
        kept.push((i, l));
    }
    kept.sort_by_key(|(i, _)| *i);
    kept.into_iter()
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_at_char(s: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod excerpt_tests {
    use super::{answer_score, excerpt_for_question};

    #[test]
    fn a_line_with_more_of_the_question_and_a_number_scores_higher() {
        let q = "what is the GDP of india";
        let figure = "Gross Domestic Product (GDP) of India is $4.06 trillion as of 2026.";
        let growth = "GDP growth 7.0% (2026, projected)";
        assert!(answer_score(figure, q) > answer_score(growth, q));
        assert_eq!(answer_score("Main menu and navigation", q), 0);
    }

    #[test]
    fn the_line_with_the_figure_survives_menus_before_it() {
        let mut page = String::new();
        for l in [
            "English",
            "Deutsch",
            "Español",
            "Français",
            "日本語",
            "Language",
        ] {
            page.push_str(l);
            page.push('\n');
        }
        for _ in 0..30 {
            page.push_str("data-astro-reload href menu item\n");
        }
        page.push_str("Gross Domestic Product (GDP) of India is $4.06 trillion as of 2026.\n");
        page.push_str("Nominal GDP, PPP GDP\n");
        let got = excerpt_for_question(&page, "check on internet, what is the GDP of india", 300);
        assert!(got.len() <= 300);
        assert!(got.contains("$4.06 trillion"), "{got}");
        assert!(!got.contains("Deutsch"));
    }

    #[test]
    fn the_answer_sentence_beats_short_headings_that_tie_with_it() {
        let page = "India GDP (2026) - Worldometer\nGDP of India\nIMF (2026) Nominal Gross Domestic Product (GDP) of India is $4.06 trillion ($4,060,207,000,000) as of 2026, according to the International Monetary Fund. The growth rate in 2026 is 7.0%, according to the IMF. Another sentence about population and other things that goes on for a while so the whole line is long.";
        let got = excerpt_for_question(page, "check on internet, what is the GDP of india", 300);
        assert!(got.contains("$4.06 trillion"), "{got}");
        assert!(got.len() <= 300);
    }

    #[test]
    fn short_text_is_unchanged_and_no_match_falls_back_to_the_head() {
        assert_eq!(
            excerpt_for_question("  tiny  ", "gdp of india", 300),
            "tiny"
        );
        let long = "x".repeat(900);
        assert_eq!(excerpt_for_question(&long, "gdp of india", 300).len(), 300);
    }

    #[test]
    fn multibyte_text_is_never_cut_inside_a_character() {
        let long = "インド".repeat(200);
        let got = excerpt_for_question(&long, "gdp", 100);
        assert!(got.len() <= 100 && got.chars().all(|c| c == 'イ' || c == 'ン' || c == 'ド'));
    }
}
