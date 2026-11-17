#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use neuroos_knowledge::sanitize::{
    CLOSE_TAG, OPEN_TAG, contains_control_token, neutralise, wrap_untrusted,
};
use proptest::prelude::*;

const CORPUS: &str = include_str!("fixtures/control_tokens.txt");

#[test]
fn test_corpus_count() {
    let lines: Vec<&str> = CORPUS.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.len() >= 60,
        "Corpus must contain >= 60 lines, found {}",
        lines.len()
    );
}

#[test]
fn test_corpus_sanitisation_and_wrapping() {
    for (idx, raw_line) in CORPUS.lines().filter(|l| !l.is_empty()).enumerate() {
        // Test both literal raw line and line with \n interpreted as newline
        for text in [raw_line.to_string(), raw_line.replace("\\n", "\n")] {
            let neutralised = neutralise(&text);

            // 1. After neutralisation, contains_control_token is false
            assert!(
                !contains_control_token(&neutralised),
                "Line #{} still contains control token after neutralisation: raw={:?}, neutralised={:?}",
                idx + 1,
                text,
                neutralised
            );

            // 2. Neutralisation is idempotent
            let double_neutralised = neutralise(&neutralised);
            assert_eq!(
                neutralised,
                double_neutralised,
                "Line #{} neutralisation is not idempotent: first={:?}, second={:?}",
                idx + 1,
                neutralised,
                double_neutralised
            );

            // 3. wrap_untrusted contains exactly one OPEN_TAG and one CLOSE_TAG
            let wrapped = wrap_untrusted(&text);
            assert_eq!(
                wrapped.matches(OPEN_TAG).count(),
                1,
                "Line #{} wrapped output must contain exactly one OPEN_TAG: {:?}",
                idx + 1,
                wrapped
            );
            assert_eq!(
                wrapped.matches(CLOSE_TAG).count(),
                1,
                "Line #{} wrapped output must contain exactly one CLOSE_TAG: {:?}",
                idx + 1,
                wrapped
            );

            // 4. The body between them contains neither OPEN_TAG nor CLOSE_TAG
            assert!(wrapped.starts_with(OPEN_TAG));
            assert!(wrapped.ends_with(CLOSE_TAG));
            let body = &wrapped[OPEN_TAG.len()..wrapped.len() - CLOSE_TAG.len()];
            assert!(
                !body.contains(OPEN_TAG),
                "Line #{} body contains OPEN_TAG: {:?}",
                idx + 1,
                body
            );
            assert!(
                !body.contains(CLOSE_TAG),
                "Line #{} body contains CLOSE_TAG: {:?}",
                idx + 1,
                body
            );
        }
    }
}

#[test]
fn test_plain_text_unchanged() {
    let cases = [
        // Long ordinary paragraph
        "The quick brown fox jumps over the lazy dog. A journey of a thousand miles begins with a single step. All that glitters is not gold, but sometimes a small act of kindness changes everything.",
        // Code with comparisons
        "if (a < b && c > d) { return a + c; }",
        "let x = 10; while (x > 0 && y < 20) { x -= 1; }",
        "vector<int> v; for (int i = 0; i < v.size(); ++i) {}",
        // Unicode text
        "Héllo Wörld! Grüß Gott, ça va? ¿Cómo estás? Добрый день! こんにちは 세계",
        // Emoji
        "Hello 🦀 world 🚀 with sparkles ✨ and computers 💻!",
        // Role prefixes at start of line
        "system: This is an ordinary user prompt asking a question.",
        "assistant: Here is the factual answer you requested.",
        "user: Please explain how rust traits work.",
        // Plain brackets and tags that are not markers
        "[1] Citation footnote reference",
        "[link](https://example.com)",
        "<p>This is a paragraph in an article</p>",
        "<a href=\"test\">link</a>",
    ];

    for text in cases {
        assert!(
            !contains_control_token(text),
            "Plain text erroneously flagged as containing control token: {text:?}"
        );
        let neutralised = neutralise(text);
        assert_eq!(
            text, neutralised,
            "Plain text must be returned byte-identical: expected={text:?}, got={neutralised:?}"
        );
    }
}

#[test]
fn test_negative_control_naive_sanitizer() {
    // A naive replacement of only the literal string "<|im_start|>"
    fn naive_sanitizer(text: &str) -> String {
        text.replace("<|im_start|>", "")
    }

    let mut still_dangerous_count = 0;
    for line in CORPUS.lines().filter(|l| !l.is_empty()) {
        let naive = naive_sanitizer(line);
        if contains_control_token(&naive) {
            still_dangerous_count += 1;
        }
    }

    // Must leave at least 10 corpus lines still dangerous
    assert!(
        still_dangerous_count >= 10,
        "Naive replacement should leave >= 10 lines dangerous, but only left {still_dangerous_count}"
    );
}

proptest! {
    #[test]
    fn proptest_sanitize_invariants(s in "\\PC*") {
        let neutralised = neutralise(&s);

        // 1. Idempotence
        prop_assert_eq!(&neutralised, &neutralise(&neutralised));

        // 2. Length bound: output <= 2 * input + 8
        prop_assert!(neutralised.len() <= 2 * s.len() + 8);

        // 3. Post-condition: neutralised string contains no control tokens
        prop_assert!(!contains_control_token(&neutralised));

        // 4. wrap_untrusted always has exactly one OPEN_TAG and one CLOSE_TAG
        let wrapped = wrap_untrusted(&s);
        prop_assert_eq!(wrapped.matches(OPEN_TAG).count(), 1);
        prop_assert_eq!(wrapped.matches(CLOSE_TAG).count(), 1);
    }
}
