//! Deictic and time reference parsing for Semantic-NeuroOS (C5).
//!
//! Implements FR-KNO-01 and FR-KNO-04.
//! Pure, deterministic classification of deictic references and temporal anchors.

use crate::router;

/// Temporal anchor indicating when in time the user query is directed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// No temporal anchor or deictic focus identified.
    None,
    /// Explicit "now" or deictic reference without a specific time word.
    Now,
    /// Relative past offset in seconds (e.g., 60 for 1 minute ago).
    SecondsAgo(u64),
    /// Historical lookback range in seconds (e.g., 3600 for earlier / past hour).
    Range(u64),
}

/// Deictic and temporal classification output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeicticInfo {
    /// True if the turn refers to the user's screen or activity.
    pub deictic: bool,
    /// Temporal focus for retrieval context.
    pub anchor: Anchor,
}

/// Parses an English number word (up to twenty) or digits (up to 999).
fn parse_number(word: &str) -> Option<u64> {
    if let Ok(n) = word.parse::<u64>() {
        return if n <= 999 { Some(n) } else { None };
    }
    match word {
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "thirteen" => Some(13),
        "fourteen" => Some(14),
        "fifteen" => Some(15),
        "sixteen" => Some(16),
        "seventeen" => Some(17),
        "eighteen" => Some(18),
        "nineteen" => Some(19),
        "twenty" => Some(20),
        _ => None,
    }
}

/// Parses relative offset expressions like `<N> minute(s) ago` or `just now`.
fn parse_seconds_ago(norm: &str) -> Option<Anchor> {
    if router::contains_word(norm, "just now") {
        return Some(Anchor::SecondsAgo(30));
    }

    let words: Vec<&str> = norm.split_whitespace().collect();
    for (i, &w) in words.iter().enumerate() {
        if w == "ago" && i >= 2 {
            let unit = words[i - 1];
            let num_word = words[i - 2];
            if let Some(n) = parse_number(num_word) {
                match unit {
                    "minute" | "minutes" => {
                        return Some(Anchor::SecondsAgo(n.saturating_mul(60)));
                    }
                    "hour" | "hours" => {
                        return Some(Anchor::SecondsAgo(n.saturating_mul(3600)));
                    }
                    "second" | "seconds" => {
                        return Some(Anchor::SecondsAgo(n));
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

/// Determines the temporal anchor from normalised text and deictic status.
fn parse_anchor(norm: &str, deictic: bool) -> Anchor {
    // 1. Relative seconds ago
    if let Some(anchor) = parse_seconds_ago(norm) {
        return anchor;
    }

    // 2. Past ranges
    // 1 hour range (3,600s)
    if router::contains_word(norm, "last hour")
        || router::contains_word(norm, "past hour")
        || router::contains_word(norm, "earlier")
        || router::contains_word(norm, "recently")
    {
        return Anchor::Range(3600);
    }

    // 1 day range (86,400s)
    if router::contains_word(norm, "this morning")
        || router::contains_word(norm, "this afternoon")
        || router::contains_word(norm, "this evening")
        || router::contains_word(norm, "today")
    {
        return Anchor::Range(86400);
    }

    // 2 day range (172,800s)
    if router::contains_word(norm, "yesterday") || router::contains_word(norm, "last night") {
        return Anchor::Range(172800);
    }

    // 7 day range (604,800s)
    if router::contains_word(norm, "this week") || router::contains_word(norm, "last week") {
        return Anchor::Range(604800);
    }

    // 3. Explicit now phrases
    if router::contains_word(norm, "right now")
        || router::contains_word(norm, "at the moment")
        || router::contains_word(norm, "currently")
        || router::contains_word(norm, "now")
    {
        return Anchor::Now;
    }

    // 4. Deictic with no explicit time word defaults to Now
    if deictic {
        return Anchor::Now;
    }

    // 5. No time word and not deictic
    Anchor::None
}

/// Parses deictic status and temporal anchor from input text.
#[must_use]
pub fn parse(text: &str) -> DeicticInfo {
    let norm = router::normalise(text);
    let deictic = router::detect_deictic(&norm);
    let anchor = parse_anchor(&norm, deictic);
    DeicticInfo { deictic, anchor }
}
