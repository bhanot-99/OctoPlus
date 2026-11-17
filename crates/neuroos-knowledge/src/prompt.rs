//! Prompt assembly (FR-KNO-05/06/07, ADR-0005 decision 3, Architecture §9.2).
//!
//! Pure: the same inputs give the same prompt, nothing is read or timed here (AB-13). The
//! layout, in order:
//!
//! ```text
//! <|im_start|>system   persona and rules (constant: the cacheable stable prefix)   <|im_end|>
//! <|im_start|>user
//!   things the user asked me to remember        (facts block)
//!   the window the user is looking at           (focus block)
//!   the conversation so far                     (history block)
//!   documents, each in an untrusted wrapper     (evidence block)
//!   the question
//! <|im_end|>
//! <|im_start|>assistant
//! ```
//!
//! Everything that is not our own template is neutralised first, so outside text can never
//! add a role marker or close a wrapper (FR-KNO-06). The prompt's taint is the union of the
//! taint of every part that went in (FR-KNO-07, R0-4); nothing here ever lowers it.

use neuroos_proto::v1::Evidence;
use neuroos_taint::TaintFlags;

use crate::sanitize::{CLOSE_TAG, OPEN_TAG, neutralise, wrap_untrusted};
use crate::tokens::{estimate, truncate_to};

/// Longest question kept, in tokens. ADR-0005 counts the question against the history block
/// on the first turn, but a typed question must not be cut to 48 tokens: this cap leaves the
/// rest of the context window (2,048) untouched and the history gets what remains of its own
/// budget.
pub const MAX_QUESTION_TOKENS: u32 = 256;

/// Everything the assembler needs for one turn. Text fields are raw: the assembler
/// neutralises them.
#[derive(Debug, Clone)]
pub struct PromptInput<'a> {
    /// Personal facts already chosen by relevance, one per element.
    pub facts: &'a [String],
    /// Union taint of those facts.
    pub facts_taint: TaintFlags,
    /// The focused window line ("app: title"), if the turn needed one.
    pub focus: Option<&'a str>,
    /// Union taint of the focus text (titles can come from web pages).
    pub focus_taint: TaintFlags,
    /// Session history, rendered by the session, with its union taint.
    pub history: &'a str,
    /// Union taint of the history.
    pub history_taint: TaintFlags,
    /// Evidence after selection (cutoff, no duplicates, taint present).
    pub evidence: &'a [Evidence],
    /// Union taint of that evidence.
    pub evidence_taint: TaintFlags,
    /// The user's question.
    pub question: &'a str,
}

/// Token budgets for the blocks and the estimate ratio (all from `[knowledge]` config).
#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    /// Facts block.
    pub facts: u32,
    /// Focus block.
    pub focus: u32,
    /// History block.
    pub history: u32,
    /// Evidence block.
    pub evidence: u32,
    /// Bytes per estimated token.
    pub bytes_per_token: u32,
}

/// The assembled prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Full ChatML text for C4.
    pub text: String,
    /// Length, in characters, of the constant system block (FR-INF-06).
    pub stable_prefix_chars: u32,
    /// Union of the taint of everything in the prompt, sent to C6 (FR-KNO-07).
    pub taint: TaintFlags,
    /// How many evidence items made it in whole or truncated.
    pub evidence_used: usize,
}

/// The constant first block.
#[must_use]
pub fn system_block() -> String {
    format!("<|im_start|>system\n{}<|im_end|>\n", crate::PERSONA)
}

/// Neutralises, then cuts to a token budget.
fn fit(text: &str, budget: u32, bpt: u32) -> String {
    let clean = neutralise(text);
    truncate_to(&clean, budget, bpt).to_string()
}

fn facts_block(input: &PromptInput<'_>, b: &Budgets) -> String {
    let mut body = String::new();
    for f in input.facts {
        let line = format!("- {}\n", neutralise(f.trim()));
        if estimate(&body, b.bytes_per_token) + estimate(&line, b.bytes_per_token) > b.facts {
            break;
        }
        body.push_str(&line);
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("What the user asked me to remember:\n{body}\n")
    }
}

fn focus_block(input: &PromptInput<'_>, b: &Budgets) -> String {
    match input.focus {
        Some(f) if !f.trim().is_empty() => {
            format!(
                "The window the user was looking at: {}\n\n",
                fit(f.trim(), b.focus, b.bytes_per_token)
            )
        }
        _ => String::new(),
    }
}

fn history_block(input: &PromptInput<'_>, b: &Budgets, question_tokens: u32) -> String {
    let room = b.history.saturating_sub(question_tokens);
    let text = fit(input.history.trim(), room, b.bytes_per_token);
    if text.is_empty() {
        String::new()
    } else {
        format!("The conversation so far:\n{text}\n\n")
    }
}

/// Evidence documents, each wrapped whole; the body is cut so the wrapper is never split.
fn evidence_block(input: &PromptInput<'_>, b: &Budgets) -> (String, usize) {
    let overhead = estimate(&format!("{OPEN_TAG}\n\n{CLOSE_TAG}\n"), b.bytes_per_token);
    let mut out = String::new();
    let mut used = 0usize;
    let mut left = b.evidence;
    for ev in input.evidence {
        if left <= overhead {
            break;
        }
        let body = fit(ev.text.trim(), left - overhead, b.bytes_per_token);
        if body.is_empty() {
            continue;
        }
        let wrapped = format!("{}\n", wrap_untrusted(&body));
        let cost = estimate(&wrapped, b.bytes_per_token);
        if cost > left {
            break;
        }
        left -= cost;
        out.push_str(&wrapped);
        used += 1;
    }
    (out, used)
}

/// Builds the prompt. The taint of the result is the union of all input taints plus
/// `USER_PROVIDED` for the question itself.
#[must_use]
pub fn assemble(input: &PromptInput<'_>, budgets: &Budgets) -> Prompt {
    let system = system_block();
    let stable_prefix_chars = u32::try_from(system.chars().count()).unwrap_or(u32::MAX);

    let question = fit(
        input.question.trim(),
        MAX_QUESTION_TOKENS,
        budgets.bytes_per_token,
    );
    let q_tokens = estimate(&question, budgets.bytes_per_token);

    let facts = facts_block(input, budgets);
    let focus = focus_block(input, budgets);
    let history = history_block(input, budgets, q_tokens);
    let (evidence, evidence_used) = evidence_block(input, budgets);

    // Conservative on purpose (R0-4, FR-KNO-07): the union of everything handed to the
    // assembler, even a block that was dropped or cut to nothing. Over-tainting only makes C6
    // stricter; under-tainting would let cut-off hostile text lower a tier.
    let taint = TaintFlags::USER_PROVIDED
        .merge(input.facts_taint)
        .merge(input.focus_taint)
        .merge(input.history_taint)
        .merge(input.evidence_taint);

    let text = format!(
        "{system}<|im_start|>user\n{facts}{focus}{history}{evidence}{question}<|im_end|>\n<|im_start|>assistant\n"
    );
    Prompt {
        text,
        stable_prefix_chars,
        taint,
        evidence_used,
    }
}
