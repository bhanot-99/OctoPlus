//! Strict parser and text chunker for spool files from the fetcher (C7).
//!
//! Satisfies FR-STO-10 and security.md §4.2:
//! - Validates spool filenames against safe alphanumeric patterns.
//! - Enforces strict schema, UTF-8 validity, size limits, and forbidden unknown keys.
//! - Strips control characters (except \n and \t) and NUL bytes.
//! - Splits text into bounded chunks without slicing UTF-8 boundaries.

use serde::Deserializer;
use std::collections::HashSet;

/// Maximum characters allowed in a spool document URL.
pub const MAX_URL_CHARS: usize = 2048;

/// Maximum characters allowed in a spool document title.
pub const MAX_TITLE_CHARS: usize = 512;

/// A parsed document from the fetcher spool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolDoc {
    /// Document source URL.
    pub url: String,
    /// Document title.
    pub title: String,
    /// Fetch timestamp in nanoseconds since Unix epoch.
    pub fetched_at_ns: u64,
    /// Document body text.
    pub text: String,
}

/// Errors returned by [`parse_spool`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpoolError {
    /// Payload exceeds configured max_bytes limit.
    #[error("payload exceeds maximum size")]
    TooLarge,
    /// Payload is not valid UTF-8.
    #[error("payload is not valid UTF-8")]
    NotUtf8,
    /// JSON syntax error, wrong field types, duplicate keys, or invalid nesting.
    #[error("malformed JSON payload")]
    BadJson,
    /// Version field is not the integer 1.
    #[error("unsupported schema version")]
    WrongVersion,
    /// Object contains an unrecognized key.
    #[error("unknown key in payload")]
    UnknownKey,
    /// A required field is missing from the payload.
    #[error("required field missing")]
    MissingField,
    /// URL or title exceeds character length limit.
    #[error("field exceeds maximum allowed length")]
    FieldTooLong,
    /// Document text is empty or contains only whitespace.
    #[error("document text is empty or whitespace only")]
    Empty,
}

/// File names C7 may use: 1 to 128 characters of [A-Za-z0-9._-], ending in ".json",
/// not starting with '.', no "..".
///
/// Satisfies security.md §4.2 path traversal and shell safety.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if !name.ends_with(".json") {
        return false;
    }
    if name.starts_with('.') || name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Parses a spool file payload according to Format v1.
///
/// Satisfies FR-STO-10 and security.md §4.2:
/// - Rejects payloads larger than `max_bytes` before parsing (`TooLarge`).
/// - Rejects invalid UTF-8 (`NotUtf8`).
/// - Rejects duplicate keys or nesting deeper than 4 (`BadJson`).
/// - Requires exactly `"version": 1`, `"url"`, `"title"`, `"fetched_at_ns"`, `"text"`.
/// - Rejects unknown keys (`UnknownKey`, e.g. forbidden `"taint"` field).
/// - Strips control characters (except `\n` and `\t`) and NUL bytes.
/// - Enforces length caps on URL (2048) and title (512) (`FieldTooLong`).
/// - Rejects empty/whitespace text (`Empty`).
pub fn parse_spool(bytes: &[u8], max_bytes: u64) -> Result<SpoolDoc, SpoolError> {
    if bytes.len() as u64 > max_bytes {
        return Err(SpoolError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SpoolError::NotUtf8)?;
    if !check_nesting(text, 4) {
        return Err(SpoolError::BadJson);
    }

    let mut de = serde_json::Deserializer::from_str(text);
    let (raw_keys, map) = de
        .deserialize_map(StrictMapVisitor)
        .map_err(|_| SpoolError::BadJson)?;
    de.end().map_err(|_| SpoolError::BadJson)?;

    let mut seen_keys = HashSet::with_capacity(raw_keys.len());
    for k in &raw_keys {
        if !seen_keys.insert(k) {
            return Err(SpoolError::BadJson);
        }
    }

    const ALLOWED_KEYS: &[&str] = &["version", "url", "title", "fetched_at_ns", "text"];
    for (k, _) in &map {
        if !ALLOWED_KEYS.contains(&k.as_str()) {
            return Err(SpoolError::UnknownKey);
        }
    }

    for &k in ALLOWED_KEYS {
        if !map.iter().any(|(key, _)| key == k) {
            return Err(SpoolError::MissingField);
        }
    }

    validate_and_build_doc(&map)
}

struct StrictMapVisitor;

impl<'de> serde::de::Visitor<'de> for StrictMapVisitor {
    type Value = (Vec<String>, Vec<(String, serde_json::Value)>);

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: serde::de::MapAccess<'de>,
    {
        let mut raw_keys = Vec::new();
        let mut entries = Vec::new();
        while let Some(key) = access.next_key::<String>()? {
            raw_keys.push(key.clone());
            let val: serde_json::Value = access.next_value()?;
            entries.push((key, val));
        }
        Ok((raw_keys, entries))
    }
}

fn check_nesting(s: &str, max_depth: usize) -> bool {
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escape = false;

    for c in s.chars() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' | '[' => {
                    depth = depth.saturating_add(1);
                    if depth > max_depth {
                        return false;
                    }
                }
                '}' | ']' => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }
    true
}

fn parse_version(val: Option<&serde_json::Value>) -> Result<(), SpoolError> {
    let num = val.and_then(|v| v.as_number()).ok_or(SpoolError::BadJson)?;
    if !num.is_i64() && !num.is_u64() {
        return Err(SpoolError::BadJson);
    }
    if num.as_i64() != Some(1) && num.as_u64() != Some(1) {
        return Err(SpoolError::WrongVersion);
    }
    Ok(())
}

fn validate_and_build_doc(map: &[(String, serde_json::Value)]) -> Result<SpoolDoc, SpoolError> {
    let mut version_val = None;
    let mut url_val = None;
    let mut title_val = None;
    let mut fetched_val = None;
    let mut text_val = None;

    for (k, v) in map {
        match k.as_str() {
            "version" => version_val = Some(v),
            "url" => url_val = Some(v),
            "title" => title_val = Some(v),
            "fetched_at_ns" => fetched_val = Some(v),
            "text" => text_val = Some(v),
            _ => return Err(SpoolError::UnknownKey),
        }
    }

    parse_version(version_val)?;

    let raw_url = url_val
        .and_then(|v| v.as_str())
        .ok_or(SpoolError::BadJson)?;
    let raw_title = title_val
        .and_then(|v| v.as_str())
        .ok_or(SpoolError::BadJson)?;
    let fetched_at_ns = fetched_val
        .and_then(|v| v.as_u64())
        .ok_or(SpoolError::BadJson)?;
    let raw_text = text_val
        .and_then(|v| v.as_str())
        .ok_or(SpoolError::BadJson)?;

    let url = clean_control_chars(raw_url);
    if url.chars().count() > MAX_URL_CHARS {
        return Err(SpoolError::FieldTooLong);
    }

    let title = clean_control_chars(raw_title);
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(SpoolError::FieldTooLong);
    }

    let text = clean_control_chars(raw_text);
    if text.trim().is_empty() {
        return Err(SpoolError::Empty);
    }

    Ok(SpoolDoc {
        url,
        title,
        fetched_at_ns,
        text,
    })
}

fn clean_control_chars(s: &str) -> String {
    s.chars()
        .filter(|&c| c != '\0' && (!c.is_control() || c == '\n' || c == '\t'))
        .collect()
}

/// Splits text into chunks of at most `max_chars` characters without splitting a UTF-8 character.
///
/// Preference hierarchy:
/// 1. Paragraph breaks (`"\n\n"`).
/// 2. Sentence ends (`". "`, `"! "`, `"? "`, `"\n"`).
/// 3. Whitespace characters.
/// 4. Hard cut at `max_chars`.
///
/// Chunks are trimmed and non-empty. Concatenating with single spaces preserves all
/// non-whitespace characters in order. `max_chars` of 0 is treated as 1.
#[must_use]
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut rem = text.trim_start();

    while !rem.is_empty() {
        let char_count = rem.chars().count();
        if char_count <= max_chars {
            let trimmed = rem.trim();
            if !trimmed.is_empty() {
                chunks.push(trimmed.to_string());
            }
            break;
        }

        let (cut, next) = find_cut_and_next(rem, max_chars);
        let Some(chunk_slice) = rem.get(..cut) else {
            break;
        };
        let trimmed = chunk_slice.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
        let Some(next_slice) = rem.get(next..) else {
            break;
        };
        rem = next_slice.trim_start();
    }

    chunks
}

fn find_cut_and_next(s: &str, max_chars: usize) -> (usize, usize) {
    let window_byte_len = s
        .char_indices()
        .nth(max_chars)
        .map_or(s.len(), |(idx, _)| idx);
    let Some(window) = s.get(..window_byte_len) else {
        return (window_byte_len, window_byte_len);
    };

    if let Some(pos) = window.rfind("\n\n")
        && window.get(..pos).is_some_and(|sub| !sub.trim().is_empty())
    {
        return (pos, pos.saturating_add(2));
    }

    if let Some(pair) = find_best_sentence(window) {
        return pair;
    }

    if let Some(pair) = find_best_ws(window) {
        return pair;
    }

    (window_byte_len, window_byte_len)
}

fn find_best_sentence(window: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for delim in [". ", "! ", "? "] {
        if let Some(pos) = window.rfind(delim) {
            let cut = pos.saturating_add(1);
            let next = pos.saturating_add(2);
            if window.get(..cut).is_some_and(|sub| !sub.trim().is_empty())
                && best.is_none_or(|(b, _)| cut > b)
            {
                best = Some((cut, next));
            }
        }
    }
    if let Some(pos) = window.rfind('\n') {
        let cut = pos;
        let next = pos.saturating_add(1);
        if window.get(..cut).is_some_and(|sub| !sub.trim().is_empty())
            && best.is_none_or(|(b, _)| cut > b)
        {
            best = Some((cut, next));
        }
    }
    best
}

fn find_best_ws(window: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (b_idx, c) in window.char_indices() {
        if c.is_whitespace() {
            let cut = b_idx;
            let next = b_idx.saturating_add(c.len_utf8());
            if window.get(..cut).is_some_and(|sub| !sub.trim().is_empty()) {
                best = Some((cut, next));
            }
        }
    }
    best
}
