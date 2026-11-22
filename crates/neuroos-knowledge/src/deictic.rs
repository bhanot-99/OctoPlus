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
    if router::contains_word(norm, "just now") || router::contains_word(norm, "a moment ago") {
        return Some(Anchor::SecondsAgo(30));
    }
    if router::contains_word(norm, "half an hour ago")
        || router::contains_word(norm, "half hour ago")
    {
        return Some(Anchor::SecondsAgo(1800));
    }
    if router::contains_word(norm, "a quarter of an hour ago") {
        return Some(Anchor::SecondsAgo(900));
    }
    if router::contains_word(norm, "a while ago") {
        return Some(Anchor::Range(3600));
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

/// Openings of a question about the user's own past activity.
const ACTIVITY_OPENINGS: &[&str] = &[
    "what",
    "which",
    "how",
    "show",
    "tell me",
    "summarize",
    "summarise",
    "list",
    "remind me",
];

/// Phrases that make it about what the user did.
const ACTIVITY_CUES: &[&str] = &[
    "did i",
    "was i",
    "have i",
    "had i",
    "i did",
    "i was",
    "i worked",
    "i have been",
    "i ve been",
    "i spent",
    "i used",
    "my day",
    "my activity",
    "my work",
    "i been",
];

/// When `text` asks what the user did over a time range ("what did I work on in the last
/// hour", "summarise my day"), the length of that range in seconds (FR-STO-08, P5-S02).
/// `None` for anything else, including questions about one moment ("what was I reading
/// five minutes ago", answered from focus history).
#[must_use]
pub fn activity_range(text: &str) -> Option<u64> {
    let norm = router::normalise(text);
    let opens = ACTIVITY_OPENINGS
        .iter()
        .any(|o| norm == *o || norm.starts_with(&format!("{o} ")));
    if !opens
        || !ACTIVITY_CUES
            .iter()
            .any(|c| router::contains_word(&norm, c))
    {
        return None;
    }
    match parse_anchor(&norm, false) {
        Anchor::Range(secs) => Some(secs),
        // "summarise my day" names no range word: today.
        _ if router::contains_word(&norm, "my day") => Some(86_400),
        _ => None,
    }
}

/// Parses deictic status and temporal anchor from input text.
#[must_use]
pub fn parse(text: &str) -> DeicticInfo {
    let norm = router::normalise(text);
    let deictic = router::detect_deictic(&norm);
    let anchor = parse_anchor(&norm, deictic);
    DeicticInfo { deictic, anchor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_questions_with_a_range() {
        for (q, secs) in [
            ("What did I work on in the last hour?", 3600),
            ("what was I doing this morning", 86_400),
            ("Summarise my day", 86_400),
            ("how long have I been coding today", 86_400),
            ("what did I do yesterday?", 172_800),
            ("show me what I worked on this week", 604_800),
        ] {
            assert_eq!(activity_range(q), Some(secs), "{q}");
        }
        for q in [
            "what was I reading five minutes ago",
            "what is the capital of France",
            "what did the news say today",
            "how do I fix this error",
            "I worked on the report today",
        ] {
            assert_eq!(activity_range(q), None, "{q}");
        }
    }
}
