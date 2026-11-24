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

/// Longest tag (with its attributes) read as one tag; a longer one is treated as plain text.
const MAX_TAG_BYTES: usize = 256 * 1024;

/// Elements whose whole content is dropped; an unclosed one drops the rest of the page.
const HARD_SKIP: &[&str] = &["script", "style", "noscript", "svg", "template"];

/// Page furniture (menus, bars, forms). Dropped when closed; an unclosed one is left alone.
const SOFT_SKIP: &[&str] = &["nav", "header", "footer", "aside", "form", "select"];

/// Opening tags that start a new line of text.
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "li", "tr",
];

/// Byte length of the tag at the start of `rest` (which begins with `<`), including its `>`.
///
/// A `>` inside a quoted attribute value (`class="[&>a]:w-full"`, `data-mw='{"a":"<ref>"}'`)
/// does not end the tag. A quote only opens right after `=`, so a stray quote in broken markup
/// cannot swallow the page; if no end is found that way, the first `>` is used.
fn tag_len(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let limit = bytes.len().min(MAX_TAG_BYTES);
    let mut quote: Option<u8> = None;
    let mut prev = 0u8;
    for (i, &b) in bytes.iter().enumerate().take(limit).skip(1) {
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if (b == b'"' || b == b'\'') && prev == b'=' {
                    quote = Some(b);
                } else if b == b'>' {
                    return Some(i + 1);
                }
                if !b.is_ascii_whitespace() {
                    prev = b;
                }
            }
        }
    }
    rest.get(..limit)
        .and_then(|head| head.find('>'))
        .map(|i| i + 1)
}

/// Byte offset just past the closing tag `</name ...>` in `hay`, or `None` when there is none.
fn closing_tag_end(hay: &str, name: &str) -> Option<usize> {
    let bytes = hay.as_bytes();
    let n = name.len();
    let mut from = 0;
    while let Some(i) = hay.get(from..)?.find("</") {
        let at = from + i;
        let cand = bytes.get(at + 2..at + 2 + n)?;
        let boundary = bytes
            .get(at + 2 + n)
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        if cand.eq_ignore_ascii_case(name.as_bytes()) && boundary {
            return Some(hay.get(at..)?.find('>').map_or(hay.len(), |g| at + g + 1));
        }
        from = at + 2;
    }
    None
}

/// Decodes the common character references; anything else is left as written.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.get(1..).and_then(|t| {
            let semi = t.get(..12.min(t.len()))?.find(';')?;
            let name = &t[..semi];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => {
                    let num = name.strip_prefix('#')?;
                    let v = match num.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(v).filter(|c| !c.is_control())
                }
            }?;
            Some((ch, semi + 2))
        });
        match decoded {
            Some((ch, used)) => {
                out.push(ch);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Appends page text: control characters dropped, whitespace folded.
fn push_text(text: &mut String, last_ws: &mut bool, chunk: &str) {
    for c in decode_entities(chunk).chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            continue;
        }
        if c.is_whitespace() {
            if !*last_ws {
                text.push(if c == '\n' { '\n' } else { ' ' });
                *last_ws = true;
            }
        } else {
            text.push(c);
            *last_ws = false;
        }
    }
}

/// Turns an HTML page into its readable text and title.
///
/// Scripts, styles, comments, menus and page furniture are dropped; a `>` inside a quoted
/// attribute does not end a tag (live: Tailwind class lists and wiki markup filled the stored
/// lead of two pages and the figure asked for never reached the answer).
pub fn extract_html(raw_html: &str, fallback_title: &str) -> Result<ExtractedDoc, ExtractError> {
    if raw_html.len() > MAX_TEXT_CHARS {
        return Err(ExtractError::TooLarge);
    }

    let mut title = String::new();
    let mut text = String::with_capacity(raw_html.len() / 2);
    let mut in_title = false;
    let mut last_ws = false;
    let mut pos = 0;

    while pos < raw_html.len() {
        let rest = &raw_html[pos..];
        let Some(lt) = rest.find('<') else {
            if in_title {
                title.push_str(&decode_entities(rest));
            } else {
                push_text(&mut text, &mut last_ws, rest);
            }
            break;
        };
        let chunk = &rest[..lt];
        if in_title {
            title.push_str(&decode_entities(chunk));
        } else {
            push_text(&mut text, &mut last_ws, chunk);
        }
        pos += lt;
        let rest = &raw_html[pos..];

        if rest.starts_with("<!--") {
            pos += rest.find("-->").map_or(rest.len(), |e| e + 3);
            continue;
        }
        // A tag name starts with a letter ("</x" or "<!doctype" too); "a < b" is text.
        let b = rest.as_bytes();
        let starts_tag = match b.get(1) {
            Some(c) if c.is_ascii_alphabetic() || *c == b'!' => true,
            Some(b'/') => b.get(2).is_some_and(u8::is_ascii_alphabetic),
            _ => false,
        };
        let Some(len) = starts_tag.then(|| tag_len(rest)).flatten() else {
            // A "<" with no tag after it (as in "a < b"): plain text.
            push_text(&mut text, &mut last_ws, "<");
            pos += 1;
            continue;
        };
        let inner = &rest[1..len - 1];
        pos += len;

        let closing = inner.starts_with('/');
        let name = inner
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if closing {
            if name == "title" {
                in_title = false;
            }
            continue;
        }
        if in_title {
            continue;
        }
        let self_closing = inner.trim_end().ends_with('/');
        if name == "title" {
            in_title = true;
        } else if HARD_SKIP.contains(&name.as_str()) && !self_closing {
            pos += closing_tag_end(&raw_html[pos..], &name).unwrap_or(raw_html.len() - pos);
        } else if SOFT_SKIP.contains(&name.as_str()) && !self_closing {
            if let Some(end) = closing_tag_end(&raw_html[pos..], &name) {
                pos += end;
            }
        } else if BLOCK_TAGS.contains(&name.as_str()) && !last_ws && !text.is_empty() {
            text.push('\n');
            last_ws = true;
        }
    }

    let clean_title: String = title.chars().filter(|c| !c.is_control()).collect();
    let clean_title: String = clean_title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    let final_title = if clean_title.is_empty() {
        fallback_title.to_string()
    } else {
        clean_title
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

    #[test]
    fn a_greater_than_inside_a_quoted_attribute_does_not_leak_text() {
        let html = r#"<div class="[&>a]:w-full px-2 text-sm" data-x='{"a":"<ref>"}'>Visible</div><p>India GDP is $4.06 trillion</p>"#;
        let doc = extract_html(html, "f").unwrap();
        assert_eq!(doc.text, "Visible\nIndia GDP is $4.06 trillion");
    }

    #[test]
    fn comments_menus_and_script_with_a_less_than_are_dropped() {
        let html = "<!-- a > b --><nav><a>Home</a> <a>Deutsch</a></nav>\
            <script>if (a < b) { x = '</div>'; }</script><main><p>Real text</p></main>\
            <footer>Copyright</footer>";
        let doc = extract_html(html, "f").unwrap();
        assert_eq!(doc.text, "Real text");
    }

    #[test]
    fn an_unclosed_menu_is_kept_and_an_unclosed_script_ends_the_page() {
        let doc = extract_html("<nav>Menu <p>Body text", "f").unwrap();
        assert!(doc.text.contains("Body text"));
        assert_eq!(
            extract_html("<p>Before</p><script>never closed <p>After", "f")
                .unwrap()
                .text,
            "Before"
        );
    }

    #[test]
    fn a_stray_quote_cannot_swallow_the_page() {
        let doc = extract_html("<a href=\"x>link</a><p>Still here</p>", "f").unwrap();
        assert!(doc.text.contains("Still here"));
    }

    #[test]
    fn entities_are_decoded_and_a_bare_ampersand_is_kept() {
        let doc = extract_html(
            "<title>A &amp; B</title><p>R&D &lt;5&gt; &#36;4 &#x24;5 &nbsp;x</p>",
            "f",
        )
        .unwrap();
        assert_eq!(doc.title, "A & B");
        assert_eq!(doc.text, "R&D <5> $4 $5 x");
    }

    #[test]
    fn a_lone_less_than_is_text() {
        let doc = extract_html("<p>1 < 2 and 3 > 2</p>", "f").unwrap();
        assert_eq!(doc.text, "1 < 2 and 3 > 2");
    }
}
