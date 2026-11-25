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
    "ive been thinking about that",
    "i was thinking about that",
    "the other day we",
    "when we met",
    "weve talked about",
];

/// Cues that refer to earlier discussion in this conversation.
pub const CHAT_MEMORY_CUES: &[&str] = &[
    "you said",
    "you told me",
    "we talked about",
    "like you said before",
    "you mentioned",
    "you were telling me",
    "you talked about",
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
    /// Named a topic the owner asked her to leave alone.
    AvoidedTopic,
    /// Talked about her own machinery or about typing (never spoken).
    MetaLeak,
    /// Duplicate sentence detected by the repeat guard.
    Repeat,
    /// Sentence was empty or contained only pictographs/whitespace.
    Empty,
}

impl DropReason {
    /// A fixed name for logs and counters (never any of the dropped words).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Forbidden(_) => "forbidden_phrase",
            Self::UngroundedMemory => "ungrounded_memory",
            Self::AvoidedTopic => "avoided_topic",
            Self::MetaLeak => "meta_leak",
            Self::Repeat => "repeat",
            Self::Empty => "empty",
        }
    }
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
    /// Number of sentences dropped because they named an avoided topic.
    pub dropped_avoided_topic: u64,
    /// Number of sentences dropped for talking about her own machinery or typing.
    pub dropped_meta_leak: u64,
    /// Number of sentences dropped due to repeat detection.
    pub dropped_repeat: u64,
}

/// Removes text in brackets: stage directions like "(softly)" and asides like "(Though my head
/// says ...)" are written for the page and sound wrong aloud. An unclosed bracket removes the
/// rest of the sentence.
fn without_asides(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Fillers the speech engine reads letter by letter ("H M M") or that sound wrong aloud.
const SPELLED_OUT: [&str; 9] = [
    "hmm", "hm", "hmmm", "mm", "mmm", "mhm", "mmhmm", "mmmhmm", "mmhm",
];
/// Fillers that are simply dropped.
const DROPPED: [&str; 5] = ["um", "umm", "uh", "uhm", "erm"];

/// Replaces fillers the voice cannot say with plain words ("Hmm," becomes "Well,"), and drops
/// "um" and "uh". Everything else is left exactly as written.
#[must_use]
pub fn speakable(text: &str) -> String {
    let text = without_asides(text);
    let mut out: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        let core: String = token
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect::<String>()
            .to_lowercase();
        let tail: String = token
            .chars()
            .rev()
            .take_while(|c| !c.is_alphabetic())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let head_is_clean = token.chars().take_while(|c| !c.is_alphabetic()).count() <= 1;
        if head_is_clean && SPELLED_OUT.contains(&core.as_str()) {
            out.push(format!("Well{tail}"));
        } else if head_is_clean && DROPPED.contains(&core.as_str()) {
            // dropped; its comma goes with it
        } else {
            out.push(token.to_string());
        }
    }
    out.join(" ")
}

/// Streamed reply filter checking sentences before speech synthesis.
#[derive(Debug)]
pub struct ReplyFilter {
    repeat_guard: RepeatGuard,
    counters: ReplyFilterCounters,
    avoid_topics: Vec<String>,
}

impl ReplyFilter {
    /// Creates a new reply filter with the specified repeat buffer capacity.
    #[must_use]
    pub fn new(repeat_capacity: usize) -> Self {
        Self {
            repeat_guard: RepeatGuard::new(repeat_capacity),
            counters: ReplyFilterCounters::default(),
            avoid_topics: Vec::new(),
        }
    }

    /// Topics the owner wants left alone: any sentence that names one is dropped.
    #[must_use]
    pub fn with_avoid_topics(mut self, topics: &[String]) -> Self {
        self.avoid_topics = topics
            .iter()
            .map(|t| normalize_for_memory(t).trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        self
    }

    /// Checks a single candidate sentence.
    ///
    /// Cleans pictographs, validates against forbidden phrases, checks ungrounded memory
    /// cues, checks the repeat guard, and updates evaluation counters.
    pub fn check(&mut self, sentence: &str, _goodbye: bool, grounded: bool) -> Verdict {
        let cleaned = speakable(&strip_pictographs(sentence));
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return Verdict::Drop(DropReason::Empty);
        }

        if let Some(id) = guards::forbidden_phrase(trimmed) {
            self.counters.dropped_forbidden += 1;
            return Verdict::Drop(DropReason::Forbidden(id));
        }

        if guards::meta_leak(trimmed) {
            self.counters.dropped_meta_leak += 1;
            return Verdict::Drop(DropReason::MetaLeak);
        }

        if !self.avoid_topics.is_empty() {
            let norm = normalize_for_memory(trimmed);
            if self
                .avoid_topics
                .iter()
                .any(|t| norm.contains(&format!(" {t} ")))
            {
                self.counters.dropped_avoided_topic += 1;
                return Verdict::Drop(DropReason::AvoidedTopic);
            }
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
