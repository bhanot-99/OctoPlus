//! Personal facts relevance scoring and selection for Semantic-NeuroOS (C5).
//!
//! Implements FR-PRO-03.
//! Selects user personal facts relevant to a question within a conservative token budget,
//! using content-word overlap with stop-word removal and simple plural/`'s` folding.

use std::collections::HashSet;

use neuroos_taint::TaintFlags;

/// A stored personal fact about the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// Unique fact identifier (monotonically increasing; newer facts have larger ids).
    pub id: u64,
    /// Human-readable fact statement.
    pub text: String,
    /// Provenance taint of the fact.
    pub taint: TaintFlags,
}

/// Alphabetically sorted list of common English stop words (>= 60 words).
const STOP_WORDS: &[&str] = &[
    "a",
    "about",
    "above",
    "after",
    "again",
    "against",
    "all",
    "almost",
    "also",
    "am",
    "an",
    "and",
    "any",
    "are",
    "aren't",
    "arent",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "by",
    "can",
    "can't",
    "cannot",
    "cant",
    "could",
    "couldn't",
    "couldnt",
    "did",
    "didn't",
    "didnt",
    "do",
    "does",
    "doesn't",
    "doesnt",
    "doing",
    "don't",
    "dont",
    "down",
    "during",
    "each",
    "few",
    "for",
    "from",
    "further",
    "had",
    "hadn't",
    "hadnt",
    "has",
    "hasn't",
    "hasnt",
    "have",
    "haven't",
    "havent",
    "having",
    "he",
    "he'd",
    "he'll",
    "he's",
    "hed",
    "hell",
    "her",
    "here",
    "here's",
    "heres",
    "hers",
    "herself",
    "him",
    "himself",
    "his",
    "how",
    "how's",
    "hows",
    "i",
    "i'd",
    "i'll",
    "i'm",
    "i've",
    "id",
    "if",
    "im",
    "in",
    "into",
    "is",
    "isn't",
    "isnt",
    "it",
    "it's",
    "its",
    "itself",
    "ive",
    "just",
    "let's",
    "lets",
    "me",
    "more",
    "most",
    "must",
    "mustn't",
    "mustnt",
    "my",
    "myself",
    "no",
    "nor",
    "not",
    "now",
    "of",
    "off",
    "on",
    "once",
    "only",
    "or",
    "other",
    "ought",
    "our",
    "ours",
    "ourselves",
    "out",
    "over",
    "own",
    "same",
    "shall",
    "shan't",
    "she",
    "she'd",
    "she'll",
    "she's",
    "should",
    "shouldn't",
    "shouldnt",
    "so",
    "some",
    "such",
    "than",
    "that",
    "that's",
    "thats",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "there's",
    "theres",
    "these",
    "they",
    "they'd",
    "they'll",
    "they're",
    "they've",
    "this",
    "those",
    "through",
    "to",
    "too",
    "under",
    "until",
    "up",
    "us",
    "very",
    "was",
    "wasn't",
    "wasnt",
    "we",
    "we'd",
    "we'll",
    "we're",
    "we've",
    "were",
    "weren't",
    "werent",
    "what",
    "what's",
    "whats",
    "when",
    "when's",
    "whens",
    "where",
    "where's",
    "wheres",
    "which",
    "while",
    "who",
    "who's",
    "whos",
    "whom",
    "why",
    "why's",
    "whys",
    "will",
    "with",
    "won't",
    "wont",
    "would",
    "wouldn't",
    "wouldnt",
    "yes",
    "you",
    "you'd",
    "you'll",
    "you're",
    "you've",
    "your",
    "yours",
    "yourself",
    "yourselves",
];

/// Checks if a lowercase word is in the stop words list.
fn is_stop_word(w: &str) -> bool {
    STOP_WORDS.binary_search(&w).is_ok()
}

/// Extracts content words: lower-case with stop words removed and simple plural/`'s` folded.
fn extract_content_words(text: &str) -> HashSet<String> {
    let mut words = HashSet::new();
    for raw in text.split(|c: char| c.is_whitespace() || (c.is_ascii_punctuation() && c != '\'')) {
        let mut w = raw.trim_matches('\'').to_lowercase();
        if w.is_empty() {
            continue;
        }
        if is_stop_word(&w) {
            continue;
        }
        if w.ends_with("'s") || w.ends_with("s'") {
            w.truncate(w.len() - 2);
        } else if w.ends_with('s') && !w.ends_with("ss") && w.len() > 2 {
            w.pop();
        }
        w.retain(|c| c.is_alphanumeric());
        if w.len() <= 1 || is_stop_word(&w) {
            continue;
        }
        words.insert(w);
    }
    words
}

#[derive(Debug)]
struct ScoredCandidate<'a> {
    idx: usize,
    score: usize,
    fact: &'a Fact,
}

/// Chooses the facts relevant to `question` within `budget_tokens`.
///
/// - Lower-case word overlap with common English stop words (>= 60) removed.
/// - Simple plural/`'s` folding.
/// - Score = number of shared content words (ties: newer `id` first).
/// - A fact with score 0 is NEVER chosen, so an unrelated question gets no facts.
/// - Total rendered size (`"- {text}\n"` per fact) stays within `budget_tokens`
///   (a fact that does not fit is skipped, a later smaller one may still fit).
/// - Returns the chosen facts' indices into `facts` in score order and the union taint of the chosen ones only.
#[must_use]
pub fn pick(
    facts: &[Fact],
    question: &str,
    budget_tokens: u32,
    bytes_per_token: u32,
) -> (Vec<usize>, TaintFlags) {
    if facts.is_empty() || budget_tokens == 0 {
        return (Vec::new(), TaintFlags::empty());
    }

    let q_words = extract_content_words(question);
    if q_words.is_empty() {
        return (Vec::new(), TaintFlags::empty());
    }

    // Score facts by shared content words (filter out score == 0)
    let mut candidates: Vec<ScoredCandidate<'_>> = facts
        .iter()
        .enumerate()
        .filter_map(|(idx, fact)| {
            let f_words = extract_content_words(&fact.text);
            let score = q_words.intersection(&f_words).count();
            if score > 0 {
                Some(ScoredCandidate { idx, score, fact })
            } else {
                None
            }
        })
        .collect();

    // Sort by score descending; ties broken by newer id first, then stable index
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.fact.id.cmp(&a.fact.id))
            .then_with(|| a.idx.cmp(&b.idx))
    });

    let mut chosen_indices = Vec::new();
    let mut accumulated_rendered = String::new();
    let mut total_taint = TaintFlags::empty();

    for cand in candidates {
        let rendered_fact = format!("- {}\n", cand.fact.text);
        let mut candidate_text = accumulated_rendered.clone();
        candidate_text.push_str(&rendered_fact);

        let est = crate::tokens::estimate(&candidate_text, bytes_per_token);
        if est <= budget_tokens {
            accumulated_rendered = candidate_text;
            chosen_indices.push(cand.idx);
            total_taint = total_taint.merge(cand.fact.taint);
        }
        // If it doesn't fit, it is skipped; a later smaller fact may still fit.
    }

    (chosen_indices, total_taint)
}
