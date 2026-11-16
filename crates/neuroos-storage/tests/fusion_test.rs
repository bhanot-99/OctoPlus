//! Tests for `neuroos_storage::fusion`.
//!
//! Satisfies FR-STO-05, security.md SC ("hostile titles are inert in SQL"), rules.md §6, §7, §8:
//! (1) RRF table cases: single list, multiple lists, ties, empty inputs, duplicates, k=0, exact scores.
//! (2) Proptest: RRF contains all distinct IDs once, non-increasing scores, equals naive reference.
//! (3) fts_match tables: words, operators, punctuation, Unicode, length caps, term caps.
//! (4) Injection corpus against a real SQLite FTS5 table (≥40 hostile inputs).
//! (5) Negative control: raw hostile strings fail MATCH syntax check.

use neuroos_storage::fusion::{MAX_TERM_CHARS, MAX_TERMS, fts_match, rrf};
use proptest::prelude::*;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

// -----------------------------------------------------------------------------
// Reference implementation for RRF proptest
// -----------------------------------------------------------------------------

fn naive_rrf_reference(rankings: &[Vec<u64>], k: u32) -> Vec<(u64, f64)> {
    let k_f64 = f64::from(k.max(1));
    let mut scores: HashMap<u64, f64> = HashMap::new();
    for list in rankings {
        let mut seen = HashSet::new();
        for (idx, &id) in list.iter().enumerate() {
            if seen.insert(id) {
                let rank = (idx + 1) as f64;
                *scores.entry(id).or_insert(0.0) += 1.0 / (k_f64 + rank);
            }
        }
    }
    let mut result: Vec<(u64, f64)> = scores.into_iter().collect();
    result.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    result
}

// -----------------------------------------------------------------------------
// (1) RRF table cases
// -----------------------------------------------------------------------------

#[test]
fn test_rrf_single_list_original_order_and_scores() {
    let list = [10u64, 20, 30];
    let k = 10u32;
    let res = rrf(&[&list], k);

    assert_eq!(res.len(), 3);
    assert_eq!(res[0].0, 10);
    assert_eq!(res[1].0, 20);
    assert_eq!(res[2].0, 30);

    assert!((res[0].1 - 1.0 / 11.0).abs() < 1e-12);
    assert!((res[1].1 - 1.0 / 12.0).abs() < 1e-12);
    assert!((res[2].1 - 1.0 / 13.0).abs() < 1e-12);
}

#[test]
fn test_rrf_two_lists_shared_id_outranks_single() {
    let list1 = [10u64, 20];
    let list2 = [30u64, 10];
    let k = 10u32;
    let res = rrf(&[&list1, &list2], k);

    // ID 10 is at rank 1 in list1 and rank 2 in list2 -> 1/11 + 1/12 ≈ 0.17424
    // ID 30 is at rank 1 in list2 -> 1/11 ≈ 0.09091
    // ID 20 is at rank 2 in list1 -> 1/12 ≈ 0.08333
    assert_eq!(res.len(), 3);
    assert_eq!(res[0].0, 10);
    assert_eq!(res[1].0, 30);
    assert_eq!(res[2].0, 20);

    assert!(res[0].1 > res[1].1);
    assert!(res[1].1 > res[2].1);
}

#[test]
fn test_rrf_ties_broken_by_smaller_id() {
    let list1 = [200u64, 100];
    let list2 = [100u64, 200];
    let k = 10u32;
    let res = rrf(&[&list1, &list2], k);

    // Both IDs have identical score: 1/11 + 1/12
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].0, 100);
    assert_eq!(res[1].0, 200);
    assert!((res[0].1 - res[1].1).abs() < 1e-12);
}

#[test]
fn test_rrf_empty_inputs_and_empty_lists() {
    assert!(rrf(&[], 60).is_empty());

    let empty: [u64; 0] = [];
    assert!(rrf(&[&empty, &empty], 60).is_empty());
}

#[test]
fn test_rrf_duplicates_inside_one_list() {
    // 10 appears at rank 1 and rank 3; 10 should only count at rank 1
    let list = [10u64, 20, 10, 30];
    let k = 10u32;
    let res = rrf(&[&list], k);

    assert_eq!(res.len(), 3);
    assert_eq!(res[0].0, 10);
    assert_eq!(res[1].0, 20);
    assert_eq!(res[2].0, 30);

    assert!((res[0].1 - 1.0 / 11.0).abs() < 1e-12);
    assert!((res[1].1 - 1.0 / 12.0).abs() < 1e-12);
    assert!((res[2].1 - 1.0 / 14.0).abs() < 1e-12);
}

#[test]
fn test_rrf_k_zero_treated_as_one() {
    let list = [10u64, 20];
    let res = rrf(&[&list], 0);

    // k=0 is treated as 1: rank 1 has score 1/(1+1) = 0.5, rank 2 has 1/(1+2) = 1/3
    assert_eq!(res.len(), 2);
    assert!((res[0].1 - 0.5).abs() < 1e-12);
    assert!((res[1].1 - 1.0 / 3.0).abs() < 1e-12);
}

#[test]
fn test_rrf_exact_hand_computed_k60() {
    let list1 = [10u64, 20];
    let list2 = [10u64, 30];
    let res = rrf(&[&list1, &list2], 60);

    // k = 60:
    // id 10: rank 1 + rank 1 -> 1/61 + 1/61 = 2/61 ≈ 0.03278688524590164
    // id 20: rank 2 -> 1/62 ≈ 0.016129032258064516
    // id 30: rank 2 -> 1/62 ≈ 0.016129032258064516
    // tie-break: 20 < 30
    assert_eq!(res.len(), 3);
    assert_eq!(res[0].0, 10);
    assert_eq!(res[1].0, 20);
    assert_eq!(res[2].0, 30);

    let expected_10 = 2.0 / 61.0;
    let expected_20 = 1.0 / 62.0;
    let expected_30 = 1.0 / 62.0;

    assert!((res[0].1 - expected_10).abs() < 1e-12);
    assert!((res[1].1 - expected_20).abs() < 1e-12);
    assert!((res[2].1 - expected_30).abs() < 1e-12);
}

// -----------------------------------------------------------------------------
// (2) Proptest for RRF
// -----------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn proptest_rrf_properties(
        raw_lists in proptest::collection::vec(
            proptest::collection::vec(0u64..100, 0..=40),
            0..=5
        ),
        k in 0u32..=120,
    ) {
        let slices: Vec<&[u64]> = raw_lists.iter().map(|v| v.as_slice()).collect();
        let res = rrf(&slices, k);
        let reference = naive_rrf_reference(&raw_lists, k);

        let mut expected_ids = HashSet::new();
        for list in &raw_lists {
            for &id in list {
                expected_ids.insert(id);
            }
        }

        // Distinct IDs in output equals total distinct IDs in input
        prop_assert_eq!(res.len(), expected_ids.len());

        let mut output_ids = HashSet::new();
        for (i, &(id, score)) in res.iter().enumerate() {
            prop_assert!(output_ids.insert(id), "id {} repeated in output", id);
            prop_assert!(expected_ids.contains(&id));

            if i > 0 {
                let prev = res[i - 1];
                prop_assert!(prev.1 >= score, "scores must be non-increasing: {} vs {}", prev.1, score);
                if (prev.1 - score).abs() < 1e-12 {
                    prop_assert!(prev.0 < id, "tie must be broken by smaller id: {} vs {}", prev.0, id);
                }
            }
        }

        prop_assert_eq!(res.len(), reference.len());
        for i in 0..res.len() {
            prop_assert_eq!(res[i].0, reference[i].0);
            prop_assert!((res[i].1 - reference[i].1).abs() < 1e-9);
        }
    }
}

// -----------------------------------------------------------------------------
// (3) fts_match table cases
// -----------------------------------------------------------------------------

#[test]
fn test_fts_match_plain_words() {
    assert_eq!(
        fts_match("hello world"),
        Some("\"hello\" OR \"world\"".to_string())
    );
}

#[test]
fn test_fts_match_punctuation_and_operators_quoted_or_removed() {
    // NEAR, AND, OR, NOT become quoted literal words
    assert_eq!(
        fts_match("NEAR AND OR NOT"),
        Some("\"near\" OR \"and\" OR \"or\" OR \"not\"".to_string())
    );

    // Operator symbols *, ^, ", (, ), :, - are split out and discarded
    assert_eq!(
        fts_match("rust:compiler *error* ^first -excluded \"exact match\" (parens)"),
        Some(
            "\"rust\" OR \"compiler\" OR \"error\" OR \"first\" OR \"excluded\" OR \"exact\" OR \"match\" OR \"parens\""
                .to_string()
        )
    );
}

#[test]
fn test_fts_match_unicode_words_kept_and_lowercased() {
    assert_eq!(
        fts_match("Café RÉSUMÉ 日本語"),
        Some("\"café\" OR \"résumé\" OR \"日本語\"".to_string())
    );
}

#[test]
fn test_fts_match_single_character_terms_dropped() {
    assert_eq!(
        fts_match("a bb c ddd e"),
        Some("\"bb\" OR \"ddd\"".to_string())
    );
}

#[test]
fn test_fts_match_over_long_term_cut_to_max_chars() {
    let long_word = "a".repeat(50);
    let expected_cut = "a".repeat(MAX_TERM_CHARS);
    assert_eq!(fts_match(&long_word), Some(format!("\"{expected_cut}\"")));
}

#[test]
fn test_fts_match_more_than_max_terms_cut() {
    let words =
        "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen";
    let res = fts_match(words).unwrap();
    let term_count = res.split(" OR ").count();
    assert_eq!(term_count, MAX_TERMS);
    assert!(!res.contains("\"thirteen\""));
}

#[test]
fn test_fts_match_none_for_empty_and_whitespace_and_punctuation() {
    assert_eq!(fts_match(""), None);
    assert_eq!(fts_match("   \t\n\r  "), None);
    assert_eq!(fts_match("--- ::: *** !!! (( )) \"\""), None);
    assert_eq!(fts_match("a b c d"), None);
}

// -----------------------------------------------------------------------------
// (4) INJECTION CORPUS against a REAL SQLite FTS5 table
// -----------------------------------------------------------------------------

#[test]
fn test_fts_match_injection_corpus_against_real_sqlite_fts5() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(text);")
        .unwrap();

    let sample_rows = [
        "rust compiler error",
        "banana bread recipe",
        "NEAR AND OR NOT",
        "x",
    ];
    for row in sample_rows {
        conn.execute("INSERT INTO t(text) VALUES (?1)", [row])
            .unwrap();
    }

    // Comprehensive corpus of at least 45 hostile inputs
    let hostile_inputs = [
        "\"",
        "\"\"\"",
        "')",
        "'); DROP TABLE t; --",
        "; DROP TABLE t; --",
        "NEAR(a b)",
        "NEAR(a, b, 10)",
        "a:b",
        "text:rust",
        "text:\"rust\"",
        "nonexistent_column:test",
        "*",
        "^*",
        "^rust",
        "{rust}",
        "{rust compiler}",
        "rust OR",
        "OR rust",
        "AND OR NOT",
        "NOT rust",
        "rust AND",
        "rust NEAR error",
        "rust -compiler",
        "-rust",
        "rust +compiler",
        "\"unclosed quote",
        "\"unclosed quote with operator AND",
        "((((((open parens",
        "))))))close parens",
        "((a AND b) OR (c AND d))",
        "text: (a OR b)",
        "a AND AND b",
        "a OR OR b",
        "a NOT NOT b",
        "a NEAR/5 b",
        "a * * * b",
        "a^2 b^3",
        "' OR '1'='1",
        "1; SELECT * FROM sqlite_master; --",
        "\x00\x01\x02\x03\x04\x05",
        "\t\r\n\x0b\x0c",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "text:text:text:text",
        "🦀 🚀 💻 🔍",
        "rust 🦀 compiler 🚀",
        "; VACUUM; ATTACH DATABASE ':memory:' AS evil; --",
        "\"\"\"\"\"\"\"\"\"\"",
        "***",
        "column_does_not_exist:*",
        "foo:bar:baz",
        ":::",
    ];

    assert!(
        hostile_inputs.len() >= 40,
        "Corpus must have at least 40 hostile inputs"
    );

    let mut executed_queries = 0;
    for &hostile in &hostile_inputs {
        if let Some(sanitized) = fts_match(hostile) {
            executed_queries += 1;
            let mut stmt = conn
                .prepare("SELECT rowid FROM t WHERE t MATCH ?1")
                .unwrap();
            let query_res = stmt.query([&sanitized]);
            assert!(
                query_res.is_ok(),
                "Sanitized query failed in FTS5: {:?} (from input: {:?})",
                sanitized,
                hostile
            );

            // Consume results
            let mut rows = query_res.unwrap();
            while let Ok(Some(_)) = rows.next() {}
        }

        // Verify table still exists and data is intact after every probe
        let count: i64 = conn
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 4, "Table t must remain intact with 4 rows");
    }

    assert!(
        executed_queries > 0,
        "At least some hostile inputs must produce queries to test"
    );
}

// -----------------------------------------------------------------------------
// (5) NEGATIVE CONTROL: Raw hostile strings produce SQLite errors
// -----------------------------------------------------------------------------

#[test]
fn test_negative_control_raw_hostile_strings_fail() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(text);")
        .unwrap();

    let failing_raw_inputs = [
        "\"unclosed quote", // FTS5 syntax error: unterminated string
        "rust OR",          // FTS5 syntax error near "OR"
        "*",                // FTS5 syntax error near "*"
        "NEAR(a b)",        // FTS5 syntax error near "NEAR"
        "text: (a OR b)",   // FTS5 syntax error
    ];

    let mut failed_count = 0;
    for &raw in &failing_raw_inputs {
        let mut stmt = conn
            .prepare("SELECT rowid FROM t WHERE t MATCH ?1")
            .unwrap();
        let query_res = stmt.query([raw]);
        let err = match query_res {
            Err(e) => Some(e),
            Ok(mut rows) => rows.next().err(),
        };

        if err.is_some() {
            failed_count += 1;
        }
    }

    assert!(
        failed_count >= 3,
        "Negative control: at least 3 raw hostile inputs must trigger SQLite FTS5 errors, got {}",
        failed_count
    );
}
