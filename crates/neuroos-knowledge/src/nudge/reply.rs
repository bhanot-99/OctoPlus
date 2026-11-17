//! What a text reply to a nudge means (FR-NUD-09, design.md §8.4). Pure.
//!
//! The reply decides everything: a nudge itself never acts (R0-6, FR-NUD-10). Anything that is
//! not one of the four short answers below is a new question and is left to the turn engine.

use crate::router::{contains_word, normalise, strip_prefix_word};

/// The four things a user can say after a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// "Yes", "show me", "go ahead": the offered skill, if any, is requested (normal tiers apply).
    Yes,
    /// "No", "it's fine": dismissed; the cooldown runs.
    No,
    /// "Not now", "later": this rule is snoozed.
    NotNow,
    /// "Stop telling me about this": this rule is switched off until re-enabled.
    Stop,
}

/// Longest reply, in words, that can be one of the four (longer text is a new question).
const MAX_WORDS: usize = 8;

/// Phrases for "stop". A bare "stop" also means stop, but "stop listening" is a request, not a
/// reply, and is left to the router.
const STOP: &[&str] = &[
    "stop telling me about this",
    "stop telling me",
    "stop reminding me",
    "stop mentioning this",
    "don t tell me again",
    "dont tell me again",
    "don t show this again",
    "dont show this again",
    "don t mention this again",
    "never mention this again",
    "never again",
    "turn this off",
    "switch this off",
    "disable this",
    "stop it",
    "stop",
];

const NOT_NOW: &[&str] = &[
    "not now",
    "not right now",
    "not at the moment",
    "not today",
    "later",
    "maybe later",
    "remind me later",
    "snooze",
    "snooze it",
    "ask me later",
];

const NO: &[&str] = &[
    "no",
    "nope",
    "nah",
    "no thanks",
    "no thank you",
    "it s fine",
    "its fine",
    "i m fine",
    "im fine",
    "that s fine",
    "never mind",
    "nevermind",
    "dismiss",
    "dismiss it",
    "ignore it",
    "ignore this",
    "i know",
    "got it",
];

const YES: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "please do",
    "go ahead",
    "do it",
    "sounds good",
    "show me",
    "yes please",
    "please show me",
    "tell me",
    "tell me more",
];

fn matches_any(norm: &str, phrases: &[&str]) -> bool {
    phrases.contains(&norm)
}

/// Classifies `text`, or `None` when it is something else (a new question).
#[must_use]
pub fn classify(text: &str) -> Option<Reply> {
    let norm = normalise(text);
    if norm.is_empty() || norm.split(' ').count() > MAX_WORDS {
        return None;
    }
    if matches_any(&norm, STOP) {
        return Some(Reply::Stop);
    }
    // "stop <something else>" is a request, not a reply.
    if strip_prefix_word(&norm, "stop").is_some() {
        return None;
    }
    if matches_any(&norm, NOT_NOW) {
        return Some(Reply::NotNow);
    }
    if matches_any(&norm, NO) {
        return Some(Reply::No);
    }
    if matches_any(&norm, YES) {
        return Some(Reply::Yes);
    }
    // "Yes show me what is using it", "okay show me": a yes word first, then more words.
    let yes_start = ["yes", "okay", "ok", "sure", "show me"]
        .iter()
        .any(|w| strip_prefix_word(&norm, w).is_some());
    if yes_start && !contains_word(&norm, "not") && !contains_word(&norm, "no") {
        return Some(Reply::Yes);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        use Reply::*;
        let cases: &[(&str, Option<Reply>)] = &[
            ("Yes", Some(Yes)),
            ("go ahead", Some(Yes)),
            ("show me", Some(Yes)),
            ("Show me what is using it", Some(Yes)),
            ("okay show me", Some(Yes)),
            ("yes please!", Some(Yes)),
            ("No", Some(No)),
            ("it's fine", Some(No)),
            ("No thanks.", Some(No)),
            ("not now", Some(NotNow)),
            ("Later", Some(NotNow)),
            ("maybe later", Some(NotNow)),
            ("Stop telling me about this", Some(Stop)),
            ("stop", Some(Stop)),
            ("don't tell me again", Some(Stop)),
            ("stop listening for an hour", None),
            ("stop the music", None),
            ("what is the capital of France", None),
            ("yes but not now", None),
            ("", None),
            ("   ", None),
            ("yes yes yes yes yes yes yes yes yes", None),
        ];
        for (text, want) in cases {
            assert_eq!(classify(text), *want, "{text:?}");
        }
    }
}
