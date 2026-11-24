//! Session history tracking and prompt context rendering for OctoPlus (C5).
//!
//! Implements FR-KNO-09 and ADR-0005 session history layout.
//! Pure, deterministic management of multi-turn conversational context with follow-up window expiry,
//! answer length bounds, and conservative token budget rendering.

use std::collections::VecDeque;

use octoplus_taint::TaintFlags;

/// Configuration for a conversation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    /// Maximum number of finished turns to retain in memory.
    pub max_turns: usize,
    /// Follow-up expiry window in nanoseconds.
    pub window_ns: u64,
    /// Conservative byte-per-token ratio for budget estimation.
    pub bytes_per_token: u32,
}

/// A single recorded conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Turn {
    question: String,
    answer: String,
    taint: TaintFlags,
    timestamp_ns: u64,
}

/// Truncates a string to at most `max_bytes` on a valid UTF-8 character boundary.
fn truncate_to_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// In-memory conversation session maintaining recent turns and follow-up expiry.
#[derive(Debug, Clone)]
pub struct Session {
    cfg: SessionConfig,
    turns: VecDeque<Turn>,
    last_activity_ns: u64,
}

impl Session {
    /// Creates a new empty `Session` with the given configuration.
    #[must_use]
    pub fn new(cfg: SessionConfig) -> Self {
        Self {
            cfg,
            turns: VecDeque::new(),
            last_activity_ns: 0,
        }
    }

    /// Records one finished turn at `now_ns`. Keeps only the newest `max_turns`.
    ///
    /// The answer stored is the first ~600 bytes (on a char boundary).
    pub fn push(&mut self, question: &str, answer: &str, taint: TaintFlags, now_ns: u64) {
        self.last_activity_ns = now_ns;
        if self.cfg.max_turns == 0 {
            self.turns.clear();
            return;
        }

        let ans_stored = truncate_to_boundary(answer, 600);
        self.turns.push_back(Turn {
            question: question.to_string(),
            answer: ans_stored.to_string(),
            taint,
            timestamp_ns: now_ns,
        });

        while self.turns.len() > self.cfg.max_turns {
            self.turns.pop_front();
        }
    }

    /// Drops everything when `now_ns - last_activity_ns > window_ns` (the follow-up window closed).
    ///
    /// `now_ns` before `last_activity` (clock jump) must not panic and must not drop.
    pub fn expire(&mut self, now_ns: u64) {
        if self.turns.is_empty() {
            return;
        }
        if now_ns > self.last_activity_ns {
            let elapsed = now_ns - self.last_activity_ns;
            if elapsed > self.cfg.window_ns {
                self.clear();
            }
        }
    }

    /// Clears all session turns and resets activity (e.g. on pause).
    pub fn clear(&mut self) {
        self.turns.clear();
        self.last_activity_ns = 0;
    }

    /// Changes the follow-up window (typed turns use a longer one than voice turns).
    ///
    /// Does not drop anything by itself.
    pub fn set_window_ns(&mut self, window_ns: u64) {
        self.cfg.window_ns = window_ns;
    }

    /// Union taint of ALL stored turns (not just the rendered ones):
    /// the context taint C6 must see for a dispatch (FR-KNO-07).
    #[must_use]
    pub fn taint(&self) -> TaintFlags {
        self.turns
            .iter()
            .fold(TaintFlags::empty(), |acc, t| acc.merge(t.taint))
    }

    /// Number of turns currently retained in the session.
    #[must_use]
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// True if the session contains no stored turns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    /// Chronological text `"User: ...\nAssistant: ...\n"` for as many of the NEWEST turns as fit `budget_tokens`
    /// (estimated with `bytes_per_token`).
    ///
    /// - An older turn is dropped whole before a newer one is cut.
    /// - If even the newest turn does not fit, its answer is truncated (never its question first).
    /// - Returns the text and the union taint of exactly the turns rendered.
    /// - Text is neutralised via [`crate::sanitize::neutralise`].
    #[must_use]
    pub fn render(&self, budget_tokens: u32) -> (String, TaintFlags) {
        if budget_tokens == 0 || self.turns.is_empty() {
            return (String::new(), TaintFlags::empty());
        }

        let n = self.turns.len();

        // Pre-render each turn in neutralised form
        let rendered_turns: Vec<String> = self
            .turns
            .iter()
            .map(|t| {
                let q_clean = crate::sanitize::neutralise(&t.question);
                let a_clean = crate::sanitize::neutralise(&t.answer);
                format!("User: {q_clean}\nAssistant: {a_clean}\n")
            })
            .collect();

        // 1. Try to include as many of the newest whole turns as fit budget_tokens
        for start_idx in 0..n {
            let mut combined = String::new();
            for item in &rendered_turns[start_idx..n] {
                combined.push_str(item);
            }
            let est = crate::tokens::estimate(&combined, self.cfg.bytes_per_token);
            if est <= budget_tokens {
                let taint_union = self
                    .turns
                    .range(start_idx..n)
                    .fold(TaintFlags::empty(), |acc, t| acc.merge(t.taint));
                return (combined, taint_union);
            }
        }

        // 2. Even the newest turn alone does not fit whole.
        // Truncate the newest turn's answer (never its question first).
        let newest = match self.turns.back() {
            Some(t) => t,
            None => return (String::new(), TaintFlags::empty()),
        };

        let q_clean = crate::sanitize::neutralise(&newest.question);
        let a_clean = crate::sanitize::neutralise(&newest.answer);

        let prefix = format!("User: {q_clean}\nAssistant: ");
        let suffix = "\n";

        let bpt = if self.cfg.bytes_per_token == 0 {
            1
        } else {
            self.cfg.bytes_per_token as usize
        };
        let max_bytes = (budget_tokens as usize).saturating_mul(bpt);
        let base_len = prefix.len() + suffix.len();

        if base_len <= max_bytes {
            let avail_answer_bytes = max_bytes - base_len;
            let a_trunc = truncate_to_boundary(&a_clean, avail_answer_bytes);
            let rendered = format!("{prefix}{a_trunc}{suffix}");
            (rendered, newest.taint)
        } else {
            // Even with an empty answer, the question prefix does not fit in budget_tokens.
            // We never truncate the question first, so 0 turns fit within budget.
            (String::new(), TaintFlags::empty())
        }
    }
}
