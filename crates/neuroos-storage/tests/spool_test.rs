//! Tests for `neuroos_storage::spool`.
//!
//! Satisfies FR-STO-10, security.md §4.2, rules.md §6, §7, §8:
//! (1) valid_name tables: good names, bad names (empty, too long, '/', '..', starts with '.', wrong extension, non-ASCII, NUL).
//! (2) parse_spool tables:
//!     - valid document;
//!     - oversized payload by 1 byte vs exactly max;
//!     - invalid UTF-8;
//!     - unknown key (e.g. `{"taint": 0}`);
//!     - duplicate key;
//!     - missing fields (each of the 5 in turn);
//!     - version variants (0, 2, "1", 1.0);
//!     - huge nesting (> 4);
//!     - URL > 2048 chars, title > 512 chars;
//!     - empty text ("" and whitespace);
//!     - control characters stripped (\0, \x07, \r stripped; \n and \t kept).
//! (3) proptest on parse_spool: random bytes never panic; whenever Ok, doc.url.chars().count() <= 2048,
//!     doc.title.chars().count() <= 512, !doc.text.trim().is_empty(), and no field contains control
//!     characters other than \n and \t.
//! (4) chunk_text tables:
//!     - paragraph breaks;
//!     - long sentence with sentence ends (. ! ? \n);
//!     - no spaces (hard cut);
//!     - multi-byte characters (e.g. 'é', '🦀') at the cut boundary;
//!     - exactly max_chars;
//!     - max_chars = 1;
//!     - empty and whitespace.
//! (5) proptest on chunk_text: for arbitrary text and random max_chars (1..=200), assert all chunks
//!     have chars().count() <= max_chars, no chunk is empty, each chunk is trimmed, and concatenating
//!     chunks with single spaces preserves all non-whitespace characters in exact order.
//! (6) negative control: a broken chunker that slices by raw bytes instead of chars fails the
//!     multi-byte non-whitespace equality check on multi-byte input, proving the check can fail (rules.md §8.9 / §7).

use neuroos_storage::spool::{
    MAX_TITLE_CHARS, MAX_URL_CHARS, SpoolDoc, SpoolError, chunk_text, parse_spool, valid_name,
};
use proptest::prelude::*;

// -----------------------------------------------------------------------------
// (1) valid_name tables
// -----------------------------------------------------------------------------

#[test]
fn test_valid_name_good_names() {
    assert!(valid_name("doc.json"));
    assert!(valid_name("fetch_123.json"));
    assert!(valid_name("a-b_c.1.json"));
    assert!(valid_name("0.json"));
    assert!(valid_name("A.B-C_123.json"));

    // Exactly 128 characters: 123 'a's + ".json" = 128
    let max_len_name = format!("{}.json", "a".repeat(123));
    assert_eq!(max_len_name.len(), 128);
    assert!(valid_name(&max_len_name));
}

#[test]
fn test_valid_name_bad_names() {
    // Empty
    assert!(!valid_name(""));

    // Exceeds 128 characters: 124 'a's + ".json" = 129
    let over_len_name = format!("{}.json", "a".repeat(124));
    assert_eq!(over_len_name.len(), 129);
    assert!(!valid_name(&over_len_name));

    // Path traversal / slashes
    assert!(!valid_name("a/b.json"));
    assert!(!valid_name("/tmp/doc.json"));
    assert!(!valid_name("dir\\doc.json"));

    // Spaces
    assert!(!valid_name("a b.json"));
    assert!(!valid_name(" doc.json"));
    assert!(!valid_name("doc.json "));

    // Double dots (traversal patterns)
    assert!(!valid_name("..json"));
    assert!(!valid_name("a..b.json"));
    assert!(!valid_name("../doc.json"));

    // Starts with dot
    assert!(!valid_name(".hidden.json"));
    assert!(!valid_name(".json"));

    // Wrong or missing extension
    assert!(!valid_name("doc.txt"));
    assert!(!valid_name("doc.JSON"));
    assert!(!valid_name("doc.json.bak"));
    assert!(!valid_name("doc_json"));
    assert!(!valid_name("doc."));

    // Non-ASCII
    assert!(!valid_name("dóc.json"));
    assert!(!valid_name("🦀.json"));
    assert!(!valid_name("中文.json"));

    // NUL byte and special characters
    assert!(!valid_name("doc\0.json"));
    assert!(!valid_name("doc?.json"));
    assert!(!valid_name("doc*.json"));
    assert!(!valid_name("doc$.json"));
    assert!(!valid_name("doc;rm.json"));
}

// -----------------------------------------------------------------------------
// (2) parse_spool tables
// -----------------------------------------------------------------------------

fn make_valid_json(url: &str, title: &str, fetched_at_ns: u64, text: &str) -> String {
    serde_json::json!({
        "version": 1,
        "url": url,
        "title": title,
        "fetched_at_ns": fetched_at_ns,
        "text": text
    })
    .to_string()
}

#[test]
fn test_parse_spool_valid_document() {
    let json = make_valid_json(
        "https://example.com/article",
        "Interesting Article",
        1_700_000_000_123_456_789,
        "First line of article.\nSecond line.",
    );
    let doc = parse_spool(json.as_bytes(), 100_000).expect("valid spool should parse");

    assert_eq!(
        doc,
        SpoolDoc {
            url: "https://example.com/article".to_string(),
            title: "Interesting Article".to_string(),
            fetched_at_ns: 1_700_000_000_123_456_789,
            text: "First line of article.\nSecond line.".to_string(),
        }
    );
}

#[test]
fn test_parse_spool_oversized_payload() {
    let json = make_valid_json("http://u", "t", 1, "body");
    let bytes = json.as_bytes();
    let len = bytes.len() as u64;

    // Exactly at max_bytes succeeds
    assert!(parse_spool(bytes, len).is_ok());

    // 1 byte below payload length fails with TooLarge
    assert_eq!(parse_spool(bytes, len - 1), Err(SpoolError::TooLarge));

    // Oversized check occurs before UTF-8 check
    let invalid_utf8 = [0xff, 0xff, 0xff, 0xff];
    assert_eq!(parse_spool(&invalid_utf8, 3), Err(SpoolError::TooLarge));
}

#[test]
fn test_parse_spool_invalid_utf8() {
    let invalid_utf8 = [0xff, 0xfe, 0xfd];
    assert_eq!(parse_spool(&invalid_utf8, 1000), Err(SpoolError::NotUtf8));

    // Invalid UTF-8 inside what looks like JSON
    let mut bytes = make_valid_json("http://u", "t", 1, "body").into_bytes();
    bytes[5] = 0xff;
    assert_eq!(parse_spool(&bytes, 1000), Err(SpoolError::NotUtf8));
}

#[test]
fn test_parse_spool_unknown_key_rejected() {
    // Explicit check for forbidden "taint" field from security.md §4.2
    let json_taint =
        r#"{"version":1,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b","taint":0}"#;
    assert_eq!(
        parse_spool(json_taint.as_bytes(), 1000),
        Err(SpoolError::UnknownKey)
    );

    let json_extra =
        r#"{"version":1,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b","extra":"val"}"#;
    assert_eq!(
        parse_spool(json_extra.as_bytes(), 1000),
        Err(SpoolError::UnknownKey)
    );
}

#[test]
fn test_parse_spool_duplicate_keys_rejected() {
    let json_dup_version =
        r#"{"version":1,"version":1,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_dup_version.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let json_dup_url = r#"{"version":1,"url":"http://u1","url":"http://u2","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_dup_url.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );
}

#[test]
fn test_parse_spool_missing_fields() {
    let missing_version = r#"{"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(missing_version.as_bytes(), 1000),
        Err(SpoolError::MissingField)
    );

    let missing_url = r#"{"version":1,"title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(missing_url.as_bytes(), 1000),
        Err(SpoolError::MissingField)
    );

    let missing_title = r#"{"version":1,"url":"http://u","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(missing_title.as_bytes(), 1000),
        Err(SpoolError::MissingField)
    );

    let missing_fetched = r#"{"version":1,"url":"http://u","title":"t","text":"b"}"#;
    assert_eq!(
        parse_spool(missing_fetched.as_bytes(), 1000),
        Err(SpoolError::MissingField)
    );

    let missing_text = r#"{"version":1,"url":"http://u","title":"t","fetched_at_ns":1}"#;
    assert_eq!(
        parse_spool(missing_text.as_bytes(), 1000),
        Err(SpoolError::MissingField)
    );
}

#[test]
fn test_parse_spool_version_variants() {
    let json_v0 = r#"{"version":0,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v0.as_bytes(), 1000),
        Err(SpoolError::WrongVersion)
    );

    let json_v2 = r#"{"version":2,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v2.as_bytes(), 1000),
        Err(SpoolError::WrongVersion)
    );

    let json_v_neg = r#"{"version":-1,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v_neg.as_bytes(), 1000),
        Err(SpoolError::WrongVersion)
    );

    let json_v_str = r#"{"version":"1","url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v_str.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let json_v_float =
        r#"{"version":1.0,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v_float.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let json_v_null =
        r#"{"version":null,"url":"http://u","title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(json_v_null.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );
}

#[test]
fn test_parse_spool_wrong_field_types() {
    let bad_url = r#"{"version":1,"url":123,"title":"t","fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(bad_url.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let bad_title = r#"{"version":1,"url":"u","title":["t"],"fetched_at_ns":1,"text":"b"}"#;
    assert_eq!(
        parse_spool(bad_title.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let bad_fetched = r#"{"version":1,"url":"u","title":"t","fetched_at_ns":"1","text":"b"}"#;
    assert_eq!(
        parse_spool(bad_fetched.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let bad_text = r#"{"version":1,"url":"u","title":"t","fetched_at_ns":1,"text":{"nested":1}}"#;
    assert_eq!(
        parse_spool(bad_text.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );
}

#[test]
fn test_parse_spool_huge_nesting_rejected() {
    // Nesting depth 5 exceeds max depth 4
    let json_nested = r#"[[[[[1]]]]]"#;
    assert_eq!(
        parse_spool(json_nested.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );

    let json_nested_obj = r#"{"a":{"b":{"c":{"d":{"e":1}}}}}"#;
    assert_eq!(
        parse_spool(json_nested_obj.as_bytes(), 1000),
        Err(SpoolError::BadJson)
    );
}

#[test]
fn test_parse_spool_field_length_limits() {
    // URL <= 2048 chars is fine, 2049 is FieldTooLong
    let url_2048 = "a".repeat(MAX_URL_CHARS);
    let json_ok = make_valid_json(&url_2048, "title", 1, "text");
    assert!(parse_spool(json_ok.as_bytes(), 100_000).is_ok());

    let url_2049 = "a".repeat(MAX_URL_CHARS + 1);
    let json_url_long = make_valid_json(&url_2049, "title", 1, "text");
    assert_eq!(
        parse_spool(json_url_long.as_bytes(), 100_000),
        Err(SpoolError::FieldTooLong)
    );

    // Title <= 512 chars is fine, 513 is FieldTooLong
    let title_512 = "t".repeat(MAX_TITLE_CHARS);
    let json_ok = make_valid_json("http://u", &title_512, 1, "text");
    assert!(parse_spool(json_ok.as_bytes(), 100_000).is_ok());

    let title_513 = "t".repeat(MAX_TITLE_CHARS + 1);
    let json_title_long = make_valid_json("http://u", &title_513, 1, "text");
    assert_eq!(
        parse_spool(json_title_long.as_bytes(), 100_000),
        Err(SpoolError::FieldTooLong)
    );
}

#[test]
fn test_parse_spool_empty_text_rejected() {
    let json_empty = make_valid_json("http://u", "t", 1, "");
    assert_eq!(
        parse_spool(json_empty.as_bytes(), 1000),
        Err(SpoolError::Empty)
    );

    let json_spaces = make_valid_json("http://u", "t", 1, "   \t\n  \r\n  ");
    assert_eq!(
        parse_spool(json_spaces.as_bytes(), 1000),
        Err(SpoolError::Empty)
    );

    // Text consisting solely of control characters (stripped out) becomes empty
    let json_controls = make_valid_json("http://u", "t", 1, "\x00\x07\x08\r");
    assert_eq!(
        parse_spool(json_controls.as_bytes(), 1000),
        Err(SpoolError::Empty)
    );
}

#[test]
fn test_parse_spool_control_characters_stripped() {
    let url_with_ctrl = "https://example.com/a\x00\x07\x08b\rc";
    let title_with_ctrl = "Title\x00\x07with\rCR and\tTab and\nNewline";
    let text_with_ctrl = "Body text\x00with\x07NUL and bell\r\nNew line\tTabbed.";

    let json = make_valid_json(url_with_ctrl, title_with_ctrl, 100, text_with_ctrl);
    let doc = parse_spool(json.as_bytes(), 1000).expect("should parse after stripping controls");

    assert_eq!(doc.url, "https://example.com/abc");
    assert_eq!(doc.title, "TitlewithCR and\tTab and\nNewline");
    assert_eq!(doc.text, "Body textwithNUL and bell\nNew line\tTabbed.");
}

// -----------------------------------------------------------------------------
// (3) proptest on parse_spool
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn proptest_parse_spool_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048),
        max_bytes in 0u64..4096u64,
    ) {
        // Must never panic on arbitrary input
        if let Ok(doc) = parse_spool(&bytes, max_bytes) {
            prop_assert!(doc.url.chars().count() <= MAX_URL_CHARS);
            prop_assert!(doc.title.chars().count() <= MAX_TITLE_CHARS);
            prop_assert!(!doc.text.trim().is_empty());

            for (field_name, field) in [("url", &doc.url), ("title", &doc.title), ("text", &doc.text)] {
                prop_assert!(!field.contains('\0'), "{field_name} contains NUL");
                for c in field.chars() {
                    prop_assert!(
                        c == '\n' || c == '\t' || !c.is_control(),
                        "{field_name} contains invalid control char: {c:?}"
                    );
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// (4) chunk_text tables
// -----------------------------------------------------------------------------

#[test]
fn test_chunk_text_paragraph_breaks() {
    let text = "Paragraph one content here.\n\nParagraph two content here.\n\nParagraph three.";
    let chunks = chunk_text(text, 35);

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], "Paragraph one content here.");
    assert_eq!(chunks[1], "Paragraph two content here.");
    assert_eq!(chunks[2], "Paragraph three.");
}

#[test]
fn test_chunk_text_sentence_ends() {
    let text = "First sentence. Second sentence! Third question? Fourth\nFifth sentence.";
    let chunks = chunk_text(text, 18);

    assert_eq!(chunks.len(), 5);
    assert_eq!(chunks[0], "First sentence.");
    assert_eq!(chunks[1], "Second sentence!");
    assert_eq!(chunks[2], "Third question?");
    assert_eq!(chunks[3], "Fourth");
    assert_eq!(chunks[4], "Fifth sentence.");
}

#[test]
fn test_chunk_text_hard_cut_no_spaces() {
    let text = "abcdefghijklmnopqrstuvwxyz";
    let chunks = chunk_text(text, 10);

    assert_eq!(chunks, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
}

#[test]
fn test_chunk_text_multibyte_boundary() {
    let text = "Café résumé 🦀 crab 🦀 and 汉语 words.";
    let chunks = chunk_text(text, 10);

    for c in &chunks {
        assert!(c.chars().count() <= 10);
    }

    let orig_non_ws: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let joined_non_ws: String = chunks
        .join(" ")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(orig_non_ws, joined_non_ws);
}

#[test]
fn test_chunk_text_exact_max_chars() {
    let text = "12345678901234567890";
    let chunks = chunk_text(text, 20);

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "12345678901234567890");
}

#[test]
fn test_chunk_text_max_chars_one_and_zero() {
    let text = "a b c";
    let chunks_one = chunk_text(text, 1);
    assert_eq!(chunks_one, vec!["a", "b", "c"]);

    // max_chars = 0 is treated as 1
    let chunks_zero = chunk_text(text, 0);
    assert_eq!(chunks_zero, vec!["a", "b", "c"]);

    let text_no_space = "abc";
    assert_eq!(chunk_text(text_no_space, 1), vec!["a", "b", "c"]);
    assert_eq!(chunk_text(text_no_space, 0), vec!["a", "b", "c"]);
}

#[test]
fn test_chunk_text_empty_and_whitespace() {
    assert!(chunk_text("", 50).is_empty());
    assert!(chunk_text("   \n\t  \r\n  ", 50).is_empty());
}

// -----------------------------------------------------------------------------
// (5) proptest on chunk_text
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn proptest_chunk_text_properties(
        text in "[a-zA-Z0-9 \\n\\.!?🦀é中]{0,500}",
        max_chars in 1usize..=200usize,
    ) {
        let chunks = chunk_text(&text, max_chars);

        if text.trim().is_empty() {
            prop_assert!(chunks.is_empty());
        } else {
            prop_assert!(!chunks.is_empty());
            for chunk in &chunks {
                prop_assert!(!chunk.is_empty());
                prop_assert_eq!(chunk, chunk.trim(), "chunk must be trimmed");
                prop_assert!(
                    chunk.chars().count() <= max_chars,
                    "chunk length {} exceeds max_chars {}",
                    chunk.chars().count(),
                    max_chars
                );
            }

            let orig_chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
            let joined = chunks.join(" ");
            let chunk_chars: Vec<char> = joined.chars().filter(|c| !c.is_whitespace()).collect();
            prop_assert_eq!(orig_chars, chunk_chars, "non-whitespace characters must be preserved in order");
        }
    }
}

// -----------------------------------------------------------------------------
// (6) negative control: broken byte-based chunker fails check (rules.md §8.9 / §7)
// -----------------------------------------------------------------------------

/// A deliberately flawed chunker that slices by raw byte count rather than char count,
/// using lossy conversion when splitting in the middle of a multi-byte sequence.
fn broken_byte_chunker(text: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + max_bytes).min(bytes.len());
        let slice = String::from_utf8_lossy(&bytes[i..end]).trim().to_string();
        if !slice.is_empty() {
            chunks.push(slice);
        }
        i = end;
    }
    chunks
}

#[test]
fn test_negative_control_broken_chunker_fails_char_preservation() {
    // Four 4-byte crab emojis: 16 bytes total.
    let text = "🦀🦀🦀🦀";
    let max_chars = 3;

    // chunk_text properly handles multi-byte UTF-8
    let good_chunks = chunk_text(text, max_chars);
    let good_joined = good_chunks.join(" ");
    let orig_chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let good_chars: Vec<char> = good_joined.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(orig_chars, good_chars);

    // Flawed byte chunker with max_bytes = 5 slices in the middle of a 4-byte sequence,
    // producing U+FFFD replacement characters.
    let bad_chunks = broken_byte_chunker(text, 5);
    let bad_joined = bad_chunks.join(" ");
    let bad_chars: Vec<char> = bad_joined.chars().filter(|c| !c.is_whitespace()).collect();

    // The broken chunker FAILS character preservation (proves the check can fail)
    assert_ne!(
        orig_chars, bad_chars,
        "broken byte chunker unexpectedly passed character preservation"
    );
    assert!(bad_chars.contains(&'\u{FFFD}'));
}
