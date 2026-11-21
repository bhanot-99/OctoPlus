//! HTML Text and Metadata Extractor for C7 Fetcher (FR-FET-07, security.md §4.2).
//!
//! Parses HTML documents into clean, bounded plain text and titles:
//! - Strips executable and style blocks (`<script>`, `<style>`, `<template>`, `<svg>`, etc.).
//! - Strips HTML markup while preserving meaningful spacing and paragraph breaks.
//! - Strips non-printable control characters (except `\n` and `\t`).
//! - Enforces length boundaries: title ≤ 512 chars, body ≤ 5 MiB.

/// Maximum title length in characters matching C3 Spool format (FR-STO-10).
pub const MAX_TITLE_CHARS: usize = 512;

/// Maximum extracted text length in characters (matches 5 MiB body cap).
pub const MAX_TEXT_CHARS: usize = 5 * 1024 * 1024;

/// Result of extracting readable content from HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedDoc {
    /// Extracted document title (or fallback domain name).
    pub title: String,
    /// Extracted readable text body.
    pub text: String,
}

/// Errors occurring during HTML extraction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExtractError {
    /// Extracted text is empty or contains only whitespace.
    #[error("extracted document text is empty")]
    Empty,
    /// Input exceeds raw size boundary.
    #[error("input payload too large for text extraction")]
    TooLarge,
}

/// Simple streaming state machine for HTML tag stripping and text normalization.
pub fn extract_html(raw_html: &str, fallback_title: &str) -> Result<ExtractedDoc, ExtractError> {
    if raw_html.len() > MAX_TEXT_CHARS {
        return Err(ExtractError::TooLarge);
    }

    let mut title = String::new();
    let mut text = String::with_capacity(raw_html.len());
    let mut in_tag = false;
    let mut current_tag = String::new();
    let mut in_skip_block = false;
    let mut skip_tag_name = String::new();
    let mut in_title = false;
    let mut last_was_whitespace = false;

    let chars = raw_html.chars();

    for c in chars {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag_lower = current_tag.to_ascii_lowercase();
                let tag_trimmed = tag_lower.trim();

                if in_skip_block {
                    // Check for closing skip tag, e.g. </script> or </style>
                    if tag_trimmed.starts_with('/') {
                        let closing = tag_trimmed.strip_prefix('/').unwrap_or("").trim();
                        if closing == skip_tag_name {
                            in_skip_block = false;
                            skip_tag_name.clear();
                        }
                    }
                } else if in_title {
                    if tag_trimmed.starts_with("/title") {
                        in_title = false;
                    }
                } else {
                    // Check for opening tags that should be stripped entirely
                    let tag_name = tag_trimmed.split_whitespace().next().unwrap_or("");
                    if matches!(
                        tag_name,
                        "script" | "style" | "noscript" | "svg" | "template"
                    ) {
                        in_skip_block = true;
                        skip_tag_name = tag_name.to_string();
                    } else if tag_name == "title" {
                        in_title = true;
                    } else if matches!(
                        tag_name,
                        "p" | "div" | "br" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr"
                    ) && !last_was_whitespace
                        && !text.is_empty()
                    {
                        text.push('\n');
                        last_was_whitespace = true;
                    }
                }
                current_tag.clear();
            } else {
                current_tag.push(c);
            }
            continue;
        }

        if c == '<' {
            in_tag = true;
            current_tag.clear();
            continue;
        }

        if in_skip_block {
            continue;
        }

        if in_title {
            if title.len() < MAX_TITLE_CHARS && !c.is_control() {
                title.push(c);
            }
            continue;
        }

        // Filter control characters except \n and \t
        if c.is_control() && c != '\n' && c != '\t' {
            continue;
        }

        if c.is_whitespace() {
            if !last_was_whitespace {
                text.push(if c == '\n' { '\n' } else { ' ' });
                last_was_whitespace = true;
            }
        } else {
            text.push(c);
            last_was_whitespace = false;
        }
    }

    let clean_title = title.trim();
    let final_title = if clean_title.is_empty() {
        fallback_title.to_string()
    } else {
        clean_title.to_string()
    };

    let clean_text = text.trim();
    if clean_text.is_empty() {
        return Err(ExtractError::Empty);
    }

    Ok(ExtractedDoc {
        title: final_title,
        text: clean_text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_clean_title_and_body() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Example Domain</title>
                <style>body { background: #fff; }</style>
                <script>alert("hostile script");</script>
            </head>
            <body>
                <h1>Example Heading</h1>
                <p>This is illustrative <b>text</b> for testing.</p>
            </body>
            </html>
        "#;

        let doc = extract_html(html, "fallback.com").unwrap();
        assert_eq!(doc.title, "Example Domain");
        assert!(doc.text.contains("Example Heading"));
        assert!(doc.text.contains("This is illustrative text for testing."));
        assert!(!doc.text.contains("alert"));
        assert!(!doc.text.contains("background"));
    }

    #[test]
    fn falls_back_to_given_title_when_empty() {
        let html = "<p>Simple paragraph without header</p>";
        let doc = extract_html(html, "default.example").unwrap();
        assert_eq!(doc.title, "default.example");
        assert_eq!(doc.text, "Simple paragraph without header");
    }

    #[test]
    fn strips_null_and_control_bytes() {
        let html = "<p>Clean\0 text\x07 with control\x1b chars</p>";
        let doc = extract_html(html, "fallback").unwrap();
        assert_eq!(doc.text, "Clean text with control chars");
    }

    #[test]
    fn rejects_empty_body() {
        let html = "<html><head><script>foo();</script></head><body></body></html>";
        assert_eq!(extract_html(html, "fallback"), Err(ExtractError::Empty));
    }
}
