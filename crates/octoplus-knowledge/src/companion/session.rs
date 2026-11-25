//! Multi-turn companion conversation memory and rolling summary (Phase 12).
//!
//! Keeps a rolling summary of older conversation turns plus the most recent raw turns.

use std::collections::VecDeque;

use octoplus_taint::TaintFlags;

use crate::tokens::estimate;

/// Nanoseconds in one minute.
const NS_PER_MINUTE: u64 = 60_000_000_000;

/// Maximum character length for the rolling summary.
const MAX_SUMMARY_CHARS: usize = 600;

/// Maximum characters kept for a turn sentence in the summary.
const MAX_SUMMARY_LINE_CHARS: usize = 80;

/// A recorded raw conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawTurn {
    question: String,
    answer: String,
}

/// In-memory companion session managing conversational history and rolling summaries.
#[derive(Debug, Clone)]
pub struct CompanionSession {
    max_minutes: u32,
    keep_turns: usize,
    bytes_per_token: u32,
    raw_turns: VecDeque<RawTurn>,
    summary_lines: VecDeque<String>,
    total_turns: usize,
    started_ns: u64,
    last_activity_ns: u64,
    taint: TaintFlags,
}

fn first_sentence(s: &str) -> &str {
    let s = s.trim();
    if s.is_empty() {
        return "";
    }
    for (i, c) in s.char_indices() {
        if c == '.' || c == '?' || c == '!' || c == '\n' {
            return s[..i + c.len_utf8()].trim();
        }
    }
    s
}

fn cut_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

impl CompanionSession {
    /// Creates a new companion conversation session.
    #[must_use]
    pub fn new(max_minutes: u32, keep_turns: usize, bytes_per_token: u32) -> Self {
        Self {
            max_minutes,
            keep_turns,
            bytes_per_token,
            raw_turns: VecDeque::new(),
            summary_lines: VecDeque::new(),
            total_turns: 0,
            started_ns: 0,
            last_activity_ns: 0,
            taint: TaintFlags::empty(),
        }
    }

    /// Records one finished turn at `now_ns`.
    ///
    /// When more than `keep_turns` exist, folds the oldest turn into the rolling summary.
    pub fn push(&mut self, question: &str, answer: &str, taint: TaintFlags, now_ns: u64) {
        if self.started_ns == 0 {
            self.started_ns = now_ns;
        }
        self.last_activity_ns = now_ns;
        self.total_turns += 1;
        self.taint = self.taint.merge(taint);

        self.raw_turns.push_back(RawTurn {
            question: question.to_string(),
            answer: answer.to_string(),
        });

        while self.raw_turns.len() > self.keep_turns {
            if let Some(old) = self.raw_turns.pop_front() {
                let q_sent = cut_chars(first_sentence(&old.question), MAX_SUMMARY_LINE_CHARS);
                let a_sent = cut_chars(first_sentence(&old.answer), MAX_SUMMARY_LINE_CHARS);
                let line = format!("owner: {q_sent}; me: {a_sent}");
                self.summary_lines.push_back(line);
                self.trim_summary();
            }
        }
    }

    fn trim_summary(&mut self) {
        while !self.summary_lines.is_empty() {
            let total: usize = self
                .summary_lines
                .iter()
                .map(|l| l.chars().count())
                .sum::<usize>()
                + self.summary_lines.len().saturating_sub(1);
            if total <= MAX_SUMMARY_CHARS {
                break;
            }
            self.summary_lines.pop_front();
        }
    }

    /// Returns true if the session has been idle longer than `max_minutes`.
    #[must_use]
    pub fn expired(&self, now_ns: u64) -> bool {
        if self.last_activity_ns == 0 {
            return false;
        }
        if now_ns > self.last_activity_ns {
            let elapsed = now_ns - self.last_activity_ns;
            elapsed > u64::from(self.max_minutes) * NS_PER_MINUTE
        } else {
            false
        }
    }

    /// Returns total turns pushed to this session.
    #[must_use]
    pub fn turn_count(&self) -> usize {
        self.total_turns
    }

    /// Returns timestamp in nanoseconds of the first pushed turn.
    #[must_use]
    pub fn started_ns(&self) -> u64 {
        self.started_ns
    }

    /// Returns elapsed minutes since session start.
    #[must_use]
    pub fn elapsed_minutes(&self, now_ns: u64) -> u32 {
        if self.started_ns == 0 || now_ns <= self.started_ns {
            0
        } else {
            ((now_ns - self.started_ns) / NS_PER_MINUTE) as u32
        }
    }

    /// Returns cumulative taint flags of all pushed turns.
    #[must_use]
    pub fn taint(&self) -> TaintFlags {
        self.taint
    }

    /// Clears conversation turns, summaries, and resets session state.
    pub fn clear(&mut self) {
        self.raw_turns.clear();
        self.summary_lines.clear();
        self.total_turns = 0;
        self.started_ns = 0;
        self.last_activity_ns = 0;
        self.taint = TaintFlags::empty();
    }

    /// Renders the conversation history formatted within `budget_tokens`.
    ///
    /// Formats the summary first ("Earlier in this chat: ...") followed by the raw turns.
    /// If total estimate exceeds budget, drops context from the oldest side.
    #[must_use]
    pub fn render(&self, budget_tokens: u32) -> (String, TaintFlags) {
        if budget_tokens == 0 {
            return (String::new(), self.taint);
        }

        let mut blocks = Vec::new();
        if !self.summary_lines.is_empty() {
            let body = self
                .summary_lines
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            blocks.push(format!("Earlier in this chat:\n{body}\n\n"));
        }

        for turn in &self.raw_turns {
            if turn.question.is_empty() {
                // A line she opened the chat with: nothing from the owner came before it.
                blocks.push(format!("You: {}\n\n", turn.answer));
            } else {
                blocks.push(format!(
                    "Owner: {}\nYou: {}\n\n",
                    turn.question, turn.answer
                ));
            }
        }

        if blocks.is_empty() {
            return (String::new(), self.taint);
        }

        // Try including from newest blocks backwards towards oldest
        let mut included = Vec::new();
        let mut total_tokens = 0;

        for block in blocks.iter().rev() {
            let cost = estimate(block, self.bytes_per_token);
            if total_tokens + cost <= budget_tokens {
                included.push(block.as_str());
                total_tokens += cost;
            } else {
                break;
            }
        }

        if included.is_empty() {
            // Even the newest block did not fit whole; truncate it from the oldest side
            if let Some(last_block) = blocks.last() {
                let max_bytes =
                    (budget_tokens as usize).saturating_mul(self.bytes_per_token as usize);
                if last_block.len() <= max_bytes {
                    return (last_block.clone(), self.taint);
                }
                let mut start = last_block.len().saturating_sub(max_bytes);
                while start < last_block.len() && !last_block.is_char_boundary(start) {
                    start += 1;
                }
                return (last_block[start..].to_string(), self.taint);
            }
            return (String::new(), self.taint);
        }

        included.reverse();
        (included.join(""), self.taint)
    }
}
