//! Speech directedness evaluation for open-microphone sessions (Phase 12, story C-S02c).
//!
//! Assesses whether incoming spoken text in an open session is directed to Laila.
//!
//! Heuristic design:
//! - Crisis cues always respond.
//! - Single filler words and empty speech are ignored.
//! - Overly long monologues (>40 words) are ignored as ambient television/background talk.
//! - Questions, direct address ("you", "your"), name mentions ("Laila", "Jarvis"), and
//!   goodbyes respond.
//! - Short answers (<=8 words) respond when the preceding companion turn was a question.
//! - First-person statements (3..=40 words) respond during the first 3 turns of a session.

use crate::companion::guards;

/// Result of evaluating whether spoken speech is directed to the companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directed {
    /// Speech is intended for the companion; generate or deliver a reply.
    Respond,
    /// Speech is not addressed to the companion; drop silently without logging or audio.
    Ignore(IgnoreReason),
}

/// Category explaining why speech was determined to be ambient or undirected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    /// Monologue longer than 40 words typical of media or external speech.
    Monologue,
    /// Ambient background talk, third-person chatter, or unaddressed speech.
    NotAddressed,
    /// Low-information conversational noise or single filler syllables.
    Filler,
}

/// Single-word vocal fillers and acoustic noise markers.
pub const FILLERS: &[&str] = &["uh", "um", "hmm", "mm", "the", "a", "ah", "er", "oh"];

/// Direct character name identifiers.
pub const NAME_CUES: &[&str] = &["laila", "jarvis"];

/// Pronouns and terms addressing the companion.
pub const ADDRESS_CUES: &[&str] = &["you", "your", "youre", "yours", "yourself"];

/// First-person singular self-reference pronouns.
pub const FIRST_PERSON_CUES: &[&str] = &["i", "my", "me", "im", "mine", "myself"];

/// Question starters and modals indicating an inquiry.
pub const QUESTION_STARTERS: &[&str] = &[
    "what",
    "why",
    "how",
    "who",
    "when",
    "where",
    "which",
    "whose",
    "whom",
    "can you",
    "could you",
    "do you",
    "are you",
    "will you",
    "would you",
    "should i",
    "should we",
    "can i",
    "could i",
    "did you",
    "have you",
    "is it",
    "is there",
    "tell me",
];

/// Extracts cleaned lowercase alphanumeric words from text.
fn extract_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            let mut s = String::new();
            for c in w.chars() {
                if c == '\'' || c == '’' {
                    continue;
                }
                if c.is_alphanumeric() {
                    s.extend(c.to_lowercase());
                }
            }
            s
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Evaluates whether incoming speech is directed to the companion.
#[must_use]
pub fn assess(text: &str, last_reply_was_question: bool, session_turns: u32) -> Directed {
    // 1. Crisis phrases ALWAYS respond
    if guards::is_crisis(text) {
        return Directed::Respond;
    }

    let words = extract_words(text);
    let word_count = words.len();

    // 2. Ignore(Filler) for empty or one-word noise
    if word_count == 0 {
        return Directed::Ignore(IgnoreReason::Filler);
    }
    if word_count == 1 && FILLERS.contains(&words[0].as_str()) {
        return Directed::Ignore(IgnoreReason::Filler);
    }

    // 3. Ignore(Monologue) for more than 40 words
    if word_count > 40 {
        return Directed::Ignore(IgnoreReason::Monologue);
    }

    // 4. Goodbyes always respond
    if guards::is_goodbye(text) {
        return Directed::Respond;
    }

    // 5. Question mark always responds
    if text.contains('?') {
        return Directed::Respond;
    }

    // 6. Starts with interrogative or modal (checking after stripping openers)
    let lower = text.trim().to_lowercase();
    let mut cur = lower.as_str();
    loop {
        cur = cur.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());
        let mut stripped = false;
        for opener in &["hey", "okay", "so", "well"] {
            if let Some(rest) = cur.strip_prefix(opener)
                && (rest.is_empty()
                    || rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_punctuation()))
            {
                cur = rest;
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    cur = cur.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());

    for &starter in QUESTION_STARTERS {
        if let Some(rest) = cur.strip_prefix(starter)
            && (rest.is_empty()
                || rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_punctuation()))
        {
            return Directed::Respond;
        }
    }

    // 7. Mentions her name or "jarvis"
    if words.iter().any(|w| NAME_CUES.contains(&w.as_str())) {
        return Directed::Respond;
    }

    // 8. Addresses her ("you", "your")
    if words.iter().any(|w| ADDRESS_CUES.contains(&w.as_str())) {
        return Directed::Respond;
    }

    // 9. Short answer (<= 8 words) when previous assistant turn ended with '?'
    if last_reply_was_question && word_count <= 8 {
        return Directed::Respond;
    }

    // 10. First-person statement (3..=40 words) in first 3 turns of session
    if session_turns < 3
        && (3..=40).contains(&word_count)
        && words
            .iter()
            .any(|w| FIRST_PERSON_CUES.contains(&w.as_str()))
    {
        return Directed::Respond;
    }

    // 11. Otherwise: not addressed
    Directed::Ignore(IgnoreReason::NotAddressed)
}
