//! Streamed companion reply validation and safety scrubbing (Phase 12).
//!
//! Validates streamed output sentence by sentence:
//! 1. Strips emoji and pictographs.
//! 2. Trims whitespace and drops empty sentences.
//! 3. Detects forbidden attachment/manipulative phrases.
//! 4. Detects ungrounded memory and shared-past claims unless grounded.
//! 5. Detects repeated conversational lines via RepeatGuard.
//! 6. Provides safe goodbye fallback when goodbye sentences are dropped.

use crate::companion::guards::{self, SAFE_GOODBYE};
use crate::companion::repeat::RepeatGuard;
use crate::speech::strip_pictographs;

/// Cues claiming a shared past or personal memory that require explicit facts in prompt.
pub const PAST_MEMORY_CUES: &[&str] = &[
    "i remember",
    "remember when",
    "you once said",
    "you said last time",
    "last time we",
    "that time we",
    "we used to",
    "i still remember",
];

/// Cues that refer to earlier discussion in this conversation.
pub const CHAT_MEMORY_CUES: &[&str] = &[
    "you said",
    "you told me",
    "we talked about",
    "like you said before",
];

/// Normalizes text for memory cue matching by stripping apostrophes,
/// converting other punctuation to spaces, lowercasing, and normalizing whitespace.
pub fn normalize_for_memory(text: &str) -> String {
    let mut s = String::with_capacity(text.len() + 2);
    s.push(' ');
    for c in text.chars() {
        if c == '\'' || c == '’' {
            continue;
        } else if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else {
            s.push(' ');
        }
    }
    s.push(' ');

    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(" {collapsed} ")
}

/// Returns true if the text contains a shared past memory cue.
pub fn has_past_memory_cue(text: &str) -> bool {
    let norm = normalize_for_memory(text);
    PAST_MEMORY_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Returns true if the text contains a chat memory cue.
pub fn has_chat_memory_cue(text: &str) -> bool {
    let norm = normalize_for_memory(text);
    CHAT_MEMORY_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Returns true if the text contains any memory cue (shared past or chat).
pub fn has_memory_cue(text: &str) -> bool {
    has_past_memory_cue(text) || has_chat_memory_cue(text)
}

/// Reason why a reply sentence was dropped by the filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    /// Contained a forbidden manipulative or guilt-trip phrase.
    Forbidden(&'static str),
    /// Contained an ungrounded claim of memory or shared past.
    UngroundedMemory,
    /// Duplicate sentence detected by the repeat guard.
    Repeat,
    /// Sentence was empty or contained only pictographs/whitespace.
    Empty,
}

/// Result of evaluating a reply sentence against the safety filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Sentence passed all safety checks and was retained.
    Keep(String),
    /// Sentence failed a safety check and was dropped.
    Drop(DropReason),
}

/// Counters tracking reply filtering actions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplyFilterCounters {
    /// Number of sentences kept.
    pub kept: u64,
    /// Number of sentences dropped due to forbidden phrases.
    pub dropped_forbidden: u64,
    /// Number of sentences dropped due to ungrounded memory claims.
    pub dropped_ungrounded_memory: u64,
    /// Number of sentences dropped due to repeat detection.
    pub dropped_repeat: u64,
}

/// Streamed reply filter checking sentences before speech synthesis.
#[derive(Debug)]
pub struct ReplyFilter {
    repeat_guard: RepeatGuard,
    counters: ReplyFilterCounters,
}

impl ReplyFilter {
    /// Creates a new reply filter with the specified repeat buffer capacity.
    #[must_use]
    pub fn new(repeat_capacity: usize) -> Self {
        Self {
            repeat_guard: RepeatGuard::new(repeat_capacity),
            counters: ReplyFilterCounters::default(),
        }
    }

    /// Checks a single candidate sentence.
    ///
    /// Cleans pictographs, validates against forbidden phrases, checks ungrounded memory
    /// cues, checks the repeat guard, and updates evaluation counters.
    pub fn check(&mut self, sentence: &str, _goodbye: bool, grounded: bool) -> Verdict {
        let cleaned = strip_pictographs(sentence);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return Verdict::Drop(DropReason::Empty);
        }

        if let Some(id) = guards::forbidden_phrase(trimmed) {
            self.counters.dropped_forbidden += 1;
            return Verdict::Drop(DropReason::Forbidden(id));
        }

        if !grounded && has_memory_cue(trimmed) {
            self.counters.dropped_ungrounded_memory += 1;
            return Verdict::Drop(DropReason::UngroundedMemory);
        }

        if self.repeat_guard.is_repeat_and_remember(trimmed) {
            self.counters.dropped_repeat += 1;
            return Verdict::Drop(DropReason::Repeat);
        }

        self.counters.kept += 1;
        Verdict::Keep(trimmed.to_string())
    }

    /// Returns the current evaluation counters.
    #[must_use]
    pub fn counters(&self) -> ReplyFilterCounters {
        self.counters
    }
}

/// Filters a slice of streamed reply pieces.
///
/// If all sentences are dropped and `goodbye` is true, returns a safe goodbye line
/// indexed by `pick % SAFE_GOODBYE.len()`. If not goodbye, returns an empty vector.
pub fn filter_reply(
    filter: &mut ReplyFilter,
    pieces: &[String],
    goodbye: bool,
    pick: usize,
    grounded: bool,
) -> Vec<String> {
    let mut kept = Vec::new();
    for p in pieces {
        if let Verdict::Keep(clean) = filter.check(p, goodbye, grounded) {
            kept.push(clean);
        }
    }

    if kept.is_empty() {
        if goodbye {
            let idx = if SAFE_GOODBYE.is_empty() {
                0
            } else {
                pick % SAFE_GOODBYE.len()
            };
            vec![SAFE_GOODBYE[idx].to_string()]
        } else {
            Vec::new()
        }
    } else {
        kept
    }
}
