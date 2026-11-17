//! Control-token sanitizer and untrusted-document wrapper for Semantic-NeuroOS (C5).
//!
//! Implements FR-KNO-05/06 and ADR-0005 prompt layout safety.
//! Ensures text placed into ChatML prompts for Qwen3 and other LLM backends is
//! inert, preventing prompt injection and control token hijacking.
//!
//! # Neutralisation Scheme
//! We employ the **visible break scheme**:
//! - Opening delimiters for control tokens and structural markers receive a visible
//!   space (`' '`) immediately following the opening delimiter (e.g., `<|` becomes `< |`,
//!   `[` becomes `[ `, `<` becomes `< `).
//! - For document wrapper tags (`<untrusted_document>` and `</untrusted_document>` in any case,
//!   inner spacing, zero-width, attribute, or homoglyph form), the opening delimiter is broken
//!   and the tag name is sanitized to `untrusted-document` so that neither [`OPEN_TAG`] nor
//!   [`CLOSE_TAG`] can appear in the body.
//! - All zero-width characters (U+200B, U+200C, U+200D, U+2060, U+FEFF) are stripped entirely.
//! - Ordinary text (including standalone `<` and `>`, code comparisons `a < b`, unicode text,
//!   and role prefixes like `system:`) is returned byte-identical.
//! - The scheme is strictly idempotent: `neutralise(neutralise(x)) == neutralise(x)`, and
//!   `contains_control_token(neutralise(x))` is always `false`.

/// Canonical opening wrapper tag for untrusted document context.
pub const OPEN_TAG: &str = "<untrusted_document>";

/// Canonical closing wrapper tag for untrusted document context.
pub const CLOSE_TAG: &str = "</untrusted_document>";

/// Checks whether a character is an invisible zero-width code point.
#[inline]
#[must_use]
pub fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}'
    )
}

/// Normalises common homoglyphs for angle brackets, brackets, bars, and slashes.
#[inline]
#[must_use]
pub fn canon_char(c: char) -> char {
    match c {
        '＜' | '⟨' => '<',
        '＞' | '⟩' => '>',
        '｜' | '¦' | '│' => '|',
        '［' => '[',
        '］' => ']',
        '／' | '∕' => '/',
        _ => c,
    }
}

/// Helper that safely checks and strips an ASCII prefix case-insensitively on char boundaries.
#[inline]
fn strip_case_insensitive_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() || !s.is_char_boundary(prefix.len()) {
        return None;
    }
    if s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Checks if a string slice begins with a `<|...|>` token of up to 40 characters
/// or a known control token prefix.
fn is_chatml_or_generic_marker(after_bar: &str) -> bool {
    // 1. Scan for closing |> within 40 characters
    let mut count = 0;
    let mut chars = after_bar.chars();
    while let Some(c) = chars.next() {
        if canon_char(c) == '<' {
            break;
        }
        if canon_char(c) == '|'
            && let Some(next_c) = chars.clone().next()
            && canon_char(next_c) == '>'
        {
            return true;
        }
        count += 1;
        if count > 40 {
            break;
        }
    }

    // 2. Check known control token prefixes even if closing |> is split or omitted
    let trimmed = after_bar.trim_start();
    let known_prefixes = [
        "im_start",
        "im_end",
        "endoftext",
        "im_sep",
        "vision_start",
        "vision_end",
        "file_sep",
        "fim_prefix",
        "fim_suffix",
        "fim_middle",
        "begin_of_text",
        "start_header_id",
        "end_header_id",
        "eot_id",
        "system",
        "user",
        "assistant",
    ];
    for kp in known_prefixes {
        if strip_case_insensitive_prefix(trimmed, kp).is_some() {
            return true;
        }
    }
    false
}

/// Inspects whether the slice after `<` matches any variation of `untrusted_document`.
///
/// Returns `Some((bytes_consumed, has_slash))` if matched.
fn match_untrusted_document(rem: &str) -> Option<(usize, bool)> {
    let mut chars = rem.chars();
    while let Some(c) = chars.clone().next() {
        if !c.is_whitespace() {
            break;
        }
        chars.next();
    }
    let mut has_slash = false;
    if let Some(c) = chars.clone().next()
        && (c == '/' || c == '／' || c == '∕')
    {
        has_slash = true;
        chars.next();
    }
    while let Some(c) = chars.clone().next() {
        if !c.is_whitespace() {
            break;
        }
        chars.next();
    }

    let after_ws = chars.as_str();
    let after_tag = strip_case_insensitive_prefix(after_ws, "untrusted_document")?;

    let next_char = after_tag.chars().next();
    let is_delim = match next_char {
        None => true,
        Some(c) => c.is_whitespace() || c == '>' || c == '＞' || c == '/',
    };
    if !is_delim {
        return None;
    }

    let mut scan_chars = after_tag.chars();
    while let Some(c) = scan_chars.clone().next() {
        scan_chars.next();
        if c == '>' || c == '＞' {
            break;
        }
    }
    let consumed = rem.len() - scan_chars.as_str().len();
    Some((consumed, has_slash))
}

/// Inspects whether the slice after `<` matches any known LLM tag (think, tool_call, etc.).
///
/// Returns `Some((bytes_consumed, has_slash, canonical_tag))` if matched.
fn match_known_tag(rem: &str) -> Option<(usize, bool, &'static str)> {
    let mut chars = rem.chars();
    let mut has_slash = false;
    if let Some(c) = chars.clone().next()
        && (c == '/' || c == '／' || c == '∕')
    {
        has_slash = true;
        chars.next();
    }

    // Do NOT permit leading whitespace here; `< think>` is already neutralised.
    let after_slash = chars.as_str();
    let tags = [
        ("think", "think"),
        ("tool_call", "tool_call"),
        ("tool_response", "tool_response"),
        ("start_of_turn", "start_of_turn"),
        ("end_of_turn", "end_of_turn"),
        ("s", "s"),
    ];

    for (t, canonical) in tags {
        if let Some(after_tag) = strip_case_insensitive_prefix(after_slash, t) {
            let next_c = after_tag.chars().next();
            let is_delim = match next_c {
                None => true,
                Some(c) => c.is_whitespace() || c == '>' || c == '＞',
            };
            if is_delim {
                let mut tag_chars = after_tag.chars();
                while let Some(c) = tag_chars.clone().next() {
                    tag_chars.next();
                    if c == '>' || c == '＞' {
                        break;
                    }
                }
                let consumed = rem.len() - tag_chars.as_str().len();
                return Some((consumed, has_slash, canonical));
            }
        }
    }
    None
}

/// Core single-pass scanner that detects or neutralises control tokens.
fn process(text: &str, detect_only: bool) -> (bool, Option<String>) {
    let has_zero_width = text.chars().any(is_zero_width);
    if has_zero_width && detect_only {
        return (true, None);
    }

    let clean_buf;
    let clean = if has_zero_width {
        clean_buf = text
            .chars()
            .filter(|&c| !is_zero_width(c))
            .collect::<String>();
        clean_buf.as_str()
    } else {
        text
    };

    let mut found_control = has_zero_width;
    let mut out = if detect_only {
        String::new()
    } else {
        String::with_capacity(clean.len() + 16)
    };

    let mut i = 0;
    while i < clean.len() {
        let current_slice = &clean[i..];
        let c = match current_slice.chars().next() {
            Some(ch) => ch,
            None => break,
        };
        let c_len = c.len_utf8();
        let canon = canon_char(c);

        if canon == '[' {
            let rem = &clean[i + c_len..];
            let mut chars = rem.chars();
            let mut has_slash = false;
            if let Some(ch) = chars.clone().next()
                && (ch == '/' || ch == '／' || ch == '∕')
            {
                has_slash = true;
                chars.next();
            }

            // No whitespace allowed after '[' or '[/'; otherwise it is already neutralised.
            let after_slash = chars.as_str();
            if let Some(after_inst) = strip_case_insensitive_prefix(after_slash, "INST") {
                let mut close_chars = after_inst.chars();
                while let Some(ch) = close_chars.clone().next() {
                    if !ch.is_whitespace() {
                        break;
                    }
                    close_chars.next();
                }
                if let Some(close_bracket) = close_chars.clone().next()
                    && (close_bracket == ']' || close_bracket == '］')
                {
                    close_chars.next();
                    if detect_only {
                        return (true, None);
                    }
                    found_control = true;
                    if has_slash {
                        out.push_str("[ /INST]");
                    } else {
                        out.push_str("[ INST]");
                    }
                    let consumed = rem.len() - close_chars.as_str().len();
                    i += c_len + consumed;
                    continue;
                }
            }

            if !detect_only {
                out.push(c);
            }
            i += c_len;
            continue;
        }

        if canon == '<' {
            let rem = &clean[i + c_len..];

            // 1. Check for <| form
            let mut bar_chars = rem.chars();
            if let Some(bar_c) = bar_chars.next()
                && canon_char(bar_c) == '|'
            {
                let after_bar = bar_chars.as_str();
                if is_chatml_or_generic_marker(after_bar) {
                    if detect_only {
                        return (true, None);
                    }
                    found_control = true;
                    out.push_str("< |");
                    i += c_len + bar_c.len_utf8();
                    continue;
                }
            }

            // 2. Check for <<SYS>> or <</SYS>>
            let mut sys_chars = rem.chars();
            if let Some(second_lt) = sys_chars.next()
                && canon_char(second_lt) == '<'
            {
                let mut sys_body = sys_chars.as_str();
                let mut has_slash = false;
                if let Some(ch) = sys_body.chars().next()
                    && (ch == '/' || ch == '／' || ch == '∕')
                {
                    has_slash = true;
                    sys_body = &sys_body[ch.len_utf8()..];
                }
                while let Some(ch) = sys_body.chars().next() {
                    if !ch.is_whitespace() {
                        break;
                    }
                    sys_body = &sys_body[ch.len_utf8()..];
                }

                if let Some(after_sys) = strip_case_insensitive_prefix(sys_body, "SYS") {
                    let mut rest = after_sys;
                    while let Some(ch) = rest.chars().next() {
                        if !ch.is_whitespace() {
                            break;
                        }
                        rest = &rest[ch.len_utf8()..];
                    }
                    let mut close_chars = rest.chars();
                    if let Some(gt1) = close_chars.next()
                        && canon_char(gt1) == '>'
                        && let Some(gt2) = close_chars.next()
                        && canon_char(gt2) == '>'
                    {
                        if detect_only {
                            return (true, None);
                        }
                        found_control = true;
                        if has_slash {
                            out.push_str("< </SYS>>");
                        } else {
                            out.push_str("< <SYS>>");
                        }
                        let consumed = rem.len() - close_chars.as_str().len();
                        i += c_len + consumed;
                        continue;
                    }
                }
            }

            // 3. Check for untrusted_document wrapper tags
            if let Some((consumed, has_slash)) = match_untrusted_document(rem) {
                if detect_only {
                    return (true, None);
                }
                found_control = true;
                if has_slash {
                    out.push_str("< /untrusted-document>");
                } else {
                    out.push_str("< untrusted-document>");
                }
                i += c_len + consumed;
                continue;
            }

            // 4. Check for known tags (think, tool_call, s, etc.)
            if let Some((consumed, has_slash, tag_name)) = match_known_tag(rem) {
                if detect_only {
                    return (true, None);
                }
                found_control = true;
                if has_slash {
                    out.push_str("< /");
                } else {
                    out.push_str("< ");
                }
                out.push_str(tag_name);
                out.push('>');
                i += c_len + consumed;
                continue;
            }

            if !detect_only {
                out.push(c);
            }
            i += c_len;
            continue;
        }

        if !detect_only {
            out.push(c);
        }
        i += c_len;
    }

    if detect_only {
        (found_control, None)
    } else if !found_control {
        (false, Some(text.to_string()))
    } else {
        (true, Some(out))
    }
}

/// Replaces every sequence the model could read as a control token or structural marker,
/// so the text stays readable but inert. Idempotent. Ordinary text (letters, digits, normal
/// punctuation, a plain '<' or '>' not forming a marker) is returned byte-identical.
/// Output is never longer than 2x the input (bytes) plus 8.
#[must_use]
pub fn neutralise(text: &str) -> String {
    let (_, out) = process(text, false);
    match out {
        Some(s) => s,
        None => text.to_string(),
    }
}

/// True when the text still contains anything `neutralise` would change.
#[must_use]
pub fn contains_control_token(text: &str) -> bool {
    let (found, _) = process(text, true);
    found
}

/// Neutralises, then wraps as `<untrusted_document>\n{body}\n</untrusted_document>`.
///
/// The body can never contain [`OPEN_TAG`] or [`CLOSE_TAG`] in any case, spacing,
/// zero-width-interleaved or homoglyph form.
#[must_use]
pub fn wrap_untrusted(text: &str) -> String {
    let body = neutralise(text);
    format!("{OPEN_TAG}\n{body}\n{CLOSE_TAG}")
}
