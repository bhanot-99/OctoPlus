//! Token budget helpers for OctoPlus (C5).
//!
//! Provides pure, deterministic token estimation and truncation based on a
//! conservative byte-per-token ratio.

/// Conservative token estimate: `ceil(bytes / bytes_per_token)`; 0 for empty; `bytes_per_token == 0` is treated as 1.
#[must_use]
pub fn estimate(text: &str, bytes_per_token: u32) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let bpt = if bytes_per_token == 0 {
        1
    } else {
        u64::from(bytes_per_token)
    };
    let bytes = text.len() as u64;
    let tokens = bytes.div_ceil(bpt);
    if tokens > u64::from(u32::MAX) {
        u32::MAX
    } else {
        tokens as u32
    }
}

/// Longest prefix of `text`, on a char boundary, whose estimate is <= `budget_tokens`.
///
/// Exact: appending the next char would exceed the budget (or the text ended).
#[must_use]
pub fn truncate_to(text: &str, budget_tokens: u32, bytes_per_token: u32) -> &str {
    if budget_tokens == 0 || text.is_empty() {
        return "";
    }
    let bpt = if bytes_per_token == 0 {
        1
    } else {
        bytes_per_token as usize
    };
    let max_bytes = (budget_tokens as usize).saturating_mul(bpt);
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
