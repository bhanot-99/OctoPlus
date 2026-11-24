#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::tokens::{estimate, truncate_to};
use proptest::prelude::*;

#[test]
fn test_estimate_tables() {
    // Empty text
    assert_eq!(estimate("", 4), 0);
    assert_eq!(estimate("", 0), 0);
    assert_eq!(estimate("", 1), 0);

    // bytes_per_token == 0 treated as 1
    assert_eq!(estimate("a", 0), 1);
    assert_eq!(estimate("abc", 0), 3);

    // Standard cases
    assert_eq!(estimate("a", 4), 1);
    assert_eq!(estimate("ab", 4), 1);
    assert_eq!(estimate("abc", 4), 1);
    assert_eq!(estimate("abcd", 4), 1);
    assert_eq!(estimate("abcde", 4), 2);
    assert_eq!(estimate("12345678", 4), 2);
    assert_eq!(estimate("123456789", 4), 3);

    // Unicode multi-byte (each 🦀 is 4 bytes)
    assert_eq!(estimate("🦀", 4), 1);
    assert_eq!(estimate("🦀🦀", 4), 2);
    assert_eq!(estimate("🦀", 3), 2); // ceil(4 / 3) = 2
}

#[test]
fn test_truncate_to_tables() {
    assert_eq!(truncate_to("", 10, 4), "");
    assert_eq!(truncate_to("hello world", 0, 4), "");
    assert_eq!(truncate_to("hello world", 1, 4), "hell");
    assert_eq!(truncate_to("hello world", 2, 4), "hello wo");
    assert_eq!(truncate_to("hello world", 3, 4), "hello world");
    assert_eq!(truncate_to("hello world", 100, 4), "hello world");

    // Zero bytes per token treated as 1
    assert_eq!(truncate_to("hello", 3, 0), "hel");

    // Multi-byte Unicode boundary handling
    // "🦀🦀🦀" has chars at byte offsets 0, 4, 8, 12
    assert_eq!(truncate_to("🦀🦀🦀", 1, 4), "🦀"); // max 4 bytes -> 1 emoji
    assert_eq!(truncate_to("🦀🦀🦀", 2, 4), "🦀🦀"); // max 8 bytes -> 2 emoji
    assert_eq!(truncate_to("🦀🦀🦀", 1, 3), ""); // max 3 bytes cannot fit 4-byte emoji
    assert_eq!(truncate_to("🦀🦀🦀", 2, 3), "🦀"); // max 6 bytes fits 1 emoji (4 bytes)
    assert_eq!(truncate_to("🦀🦀🦀", 3, 3), "🦀🦀"); // max 9 bytes fits 2 emoji (8 bytes)

    // Mixed ASCII and UTF-8
    let mixed = "Café au lait"; // 'é' is 2 bytes (bytes 3..5)
    assert_eq!(truncate_to(mixed, 1, 4), "Caf"); // 3 bytes (boundary at 3, 'é' ends at 5)
    assert_eq!(truncate_to(mixed, 2, 4), "Café au");
}

proptest! {
    #[test]
    fn proptest_tokens_invariants(
        text in "\\PC*",
        budget in 0u32..500,
        bpt in 0u32..20,
    ) {
        let result = truncate_to(&text, budget, bpt);

        // 1. Result is a prefix of text
        prop_assert!(text.starts_with(result));

        // 2. Result is on a valid char boundary
        prop_assert!(text.is_char_boundary(result.len()));

        // 3. Estimate of result is <= budget
        prop_assert!(estimate(result, bpt) <= budget);

        // 4. Zero budget gives ""
        if budget == 0 {
            prop_assert_eq!(result, "");
        }

        // 5. Appending next char exceeds budget (or text ended)
        if result.len() < text.len() {
            let next_char = text[result.len()..].chars().next().unwrap();
            let with_next = &text[..result.len() + next_char.len_utf8()];
            prop_assert!(estimate(with_next, bpt) > budget);
        }

        // 6. Estimate monotonicity in length
        prop_assert!(estimate(result, bpt) <= estimate(&text, bpt));
    }
}
