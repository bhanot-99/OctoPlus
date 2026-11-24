#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use octoplus_knowledge::evidence::{Selected, select};
use octoplus_proto::v1::{Evidence, Taint};
use octoplus_taint::TaintFlags;
use proptest::prelude::*;

fn make_evidence(chunk_id: u64, score: f64, text: &str, flags: Option<u32>) -> Evidence {
    Evidence {
        chunk_id,
        domain: 0,
        text: text.to_string(),
        score,
        taint: flags.map(|f| Taint { flags: f }),
        at_ns: 1_700_000_000_000_000,
        source: "test_source".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Table tests for each drop reason
// ---------------------------------------------------------------------------

#[test]
fn test_drop_reason_missing_or_invalid_taint() {
    let ev_missing_taint = make_evidence(1, 0.9, "missing taint", None);
    let ev_unknown_bits = make_evidence(2, 0.85, "unknown bits", Some(0x8000_0000));
    let ev_valid = make_evidence(
        3,
        0.80,
        "valid item",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );

    let input = vec![ev_missing_taint, ev_unknown_bits, ev_valid.clone()];
    let res = select(input, 0.5, 10);

    assert_eq!(
        res.dropped_no_taint, 2,
        "both None and unknown bits must count as dropped_no_taint"
    );
    assert_eq!(res.dropped_below_cutoff, 0);
    assert_eq!(res.dropped_duplicate, 0);
    assert_eq!(res.items, vec![ev_valid]);
    assert_eq!(res.taint, TaintFlags::USER_PROVIDED);
}

#[test]
fn test_drop_reason_score_below_cutoff_and_nan() {
    let ev_below = make_evidence(
        1,
        0.499,
        "below cutoff",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let ev_nan = make_evidence(
        2,
        f64::NAN,
        "nan score",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let ev_at_cutoff = make_evidence(
        3,
        0.5,
        "exact cutoff",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let ev_above = make_evidence(
        4,
        0.9,
        "above cutoff",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );

    let input = vec![ev_below, ev_nan, ev_at_cutoff.clone(), ev_above.clone()];
    let res = select(input, 0.5, 10);

    assert_eq!(res.dropped_no_taint, 0);
    assert_eq!(
        res.dropped_below_cutoff, 2,
        "score < cutoff and NaN must count as dropped_below_cutoff"
    );
    assert_eq!(res.dropped_duplicate, 0);
    // Best score first: 0.9 then 0.5
    assert_eq!(res.items, vec![ev_above, ev_at_cutoff]);
}

#[test]
fn test_drop_reason_duplicate_chunk_id() {
    let ev_high = make_evidence(
        42,
        0.95,
        "first occurrence",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let ev_lower_same_id = make_evidence(
        42,
        0.70,
        "different text same id",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );

    let input = vec![ev_lower_same_id, ev_high.clone()];
    let res = select(input, 0.5, 10);

    assert_eq!(res.dropped_no_taint, 0);
    assert_eq!(res.dropped_below_cutoff, 0);
    assert_eq!(res.dropped_duplicate, 1);
    assert_eq!(res.items, vec![ev_high]);
}

#[test]
fn test_drop_reason_duplicate_normalised_text() {
    let ev_orig = make_evidence(
        10,
        0.90,
        "Hello World!",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let ev_dup = make_evidence(
        11,
        0.80,
        "  hello   world!  ",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );

    let input = vec![ev_dup, ev_orig.clone()];
    let res = select(input, 0.5, 10);

    assert_eq!(res.dropped_no_taint, 0);
    assert_eq!(res.dropped_below_cutoff, 0);
    assert_eq!(res.dropped_duplicate, 1);
    assert_eq!(res.items, vec![ev_orig]);
}

#[test]
fn test_drop_reason_evaluation_order_precedence() {
    // 1. Missing taint beats score < cutoff
    let ev_bad_taint_and_low_score = make_evidence(1, 0.1, "bad taint and low score", None);
    let res1 = select(vec![ev_bad_taint_and_low_score], 0.5, 10);
    assert_eq!(res1.dropped_no_taint, 1);
    assert_eq!(res1.dropped_below_cutoff, 0);
    assert_eq!(res1.dropped_duplicate, 0);

    // 2. Missing taint beats duplicate chunk_id
    let ev_valid = make_evidence(2, 0.9, "valid", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev_dup_id_bad_taint = make_evidence(2, 0.9, "valid duplicate id", None);
    let res2 = select(vec![ev_valid.clone(), ev_dup_id_bad_taint], 0.5, 10);
    assert_eq!(res2.dropped_no_taint, 1);
    assert_eq!(res2.dropped_below_cutoff, 0);
    assert_eq!(res2.dropped_duplicate, 0);

    // 3. Score < cutoff beats duplicate chunk_id
    let ev_dup_id_low_score = make_evidence(
        2,
        0.2,
        "valid duplicate id but low score",
        Some(TaintFlags::USER_PROVIDED.bits()),
    );
    let res3 = select(vec![ev_valid.clone(), ev_dup_id_low_score], 0.5, 10);
    assert_eq!(res3.dropped_no_taint, 0);
    assert_eq!(res3.dropped_below_cutoff, 1);
    assert_eq!(res3.dropped_duplicate, 0);

    // 4. Duplicate chunk_id AND duplicate text counts once
    let ev_dup_both = make_evidence(2, 0.8, "valid", Some(TaintFlags::USER_PROVIDED.bits()));
    let res4 = select(vec![ev_valid, ev_dup_both], 0.5, 10);
    assert_eq!(res4.dropped_duplicate, 1);
}

// ---------------------------------------------------------------------------
// Sorting, tie-breaking and max truncation
// ---------------------------------------------------------------------------

#[test]
fn test_sorting_and_tie_breaking() {
    // Scores: 0.7 (id 30), 0.9 (id 10), 0.7 (id 20), 0.7 (id 25)
    let ev1 = make_evidence(30, 0.7, "c", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev2 = make_evidence(10, 0.9, "a", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev3 = make_evidence(20, 0.7, "b", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev4 = make_evidence(25, 0.7, "d", Some(TaintFlags::USER_PROVIDED.bits()));

    let input = vec![ev1, ev2.clone(), ev3.clone(), ev4.clone()];
    let res = select(input, 0.5, 10);

    assert_eq!(res.items.len(), 4);
    assert_eq!(res.items[0], ev2); // score 0.9
    assert_eq!(res.items[1], ev3); // score 0.7, chunk_id 20
    assert_eq!(res.items[2], ev4); // score 0.7, chunk_id 25
    assert_eq!(
        res.items[3],
        make_evidence(30, 0.7, "c", Some(TaintFlags::USER_PROVIDED.bits()))
    ); // score 0.7, chunk_id 30
}

#[test]
fn test_max_truncation() {
    let ev1 = make_evidence(1, 0.9, "one", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev2 = make_evidence(2, 0.8, "two", Some(TaintFlags::EXTERNAL_UNTRUSTED.bits()));
    let ev3 = make_evidence(3, 0.7, "three", Some(TaintFlags::MODEL_GENERATED.bits()));

    let input = vec![ev1.clone(), ev2.clone(), ev3];

    // max = 0
    let res0 = select(input.clone(), 0.5, 0);
    assert!(res0.items.is_empty());
    assert_eq!(res0.taint, TaintFlags::empty());

    // max = 1
    let res1 = select(input.clone(), 0.5, 1);
    assert_eq!(res1.items, vec![ev1.clone()]);
    assert_eq!(res1.taint, TaintFlags::USER_PROVIDED);

    // max = 2
    let res2 = select(input, 0.5, 2);
    assert_eq!(res2.items, vec![ev1, ev2]);
    assert_eq!(
        res2.taint,
        TaintFlags::USER_PROVIDED.merge(TaintFlags::EXTERNAL_UNTRUSTED)
    );
}

#[test]
fn test_taint_union_propagation() {
    let ev1 = make_evidence(1, 0.9, "one", Some(TaintFlags::USER_PROVIDED.bits()));
    let ev2 = make_evidence(2, 0.8, "two", Some(TaintFlags::EXTERNAL_UNTRUSTED.bits()));
    let ev3_dropped = make_evidence(3, 0.1, "three", Some(TaintFlags::MODEL_GENERATED.bits()));

    let res = select(vec![ev1, ev2, ev3_dropped], 0.5, 10);
    assert_eq!(
        res.taint,
        TaintFlags::USER_PROVIDED.merge(TaintFlags::EXTERNAL_UNTRUSTED),
        "dropped items' taint must not leak into total taint"
    );
}

// ---------------------------------------------------------------------------
// Negative-control test asserting that a buggy selector without taint decoding fails
// ---------------------------------------------------------------------------

fn buggy_select_ignoring_taint(evidence: Vec<Evidence>, cutoff: f64, max: usize) -> Selected {
    // BUGGY: Ignores taint decoding entirely, treating missing taint as clean (violates R0-4).
    let mut valid = Vec::new();
    for ev in evidence {
        if ev.score.is_nan() || ev.score < cutoff {
            continue;
        }
        valid.push(ev);
    }
    valid.truncate(max);
    Selected {
        items: valid,
        taint: TaintFlags::empty(),
        dropped_no_taint: 0,
        dropped_below_cutoff: 0,
        dropped_duplicate: 0,
    }
}

#[test]
fn test_negative_control_buggy_selector_fails_taint_requirement() {
    // When an item has no taint, R0-4 dictates it MUST be dropped as an error, never treated as clean.
    let hostile_untracked_evidence = make_evidence(1, 0.99, "untracked source data", None);

    let real_result = select(vec![hostile_untracked_evidence.clone()], 0.5, 10);
    let buggy_result = buggy_select_ignoring_taint(vec![hostile_untracked_evidence], 0.5, 10);

    // The real implementation correctly dropped the missing-taint item
    assert_eq!(real_result.dropped_no_taint, 1);
    assert!(real_result.items.is_empty());

    // The buggy implementation failed the security invariant by keeping untracked evidence
    assert_eq!(buggy_result.dropped_no_taint, 0);
    assert_eq!(buggy_result.items.len(), 1);

    // Negative control assertion: the buggy selector diverges from the secure implementation
    assert_ne!(
        real_result, buggy_result,
        "buggy selector without taint decoding must fail security assertions"
    );
}

// ---------------------------------------------------------------------------
// Proptests: invariants over arbitrary evidence streams
// ---------------------------------------------------------------------------

fn arb_taint_flags() -> impl Strategy<Value = Option<u32>> {
    prop_oneof![
        Just(None),
        Just(Some(0)),
        Just(Some(1)),           // EXTERNAL_UNTRUSTED
        Just(Some(2)),           // USER_PROVIDED
        Just(Some(4)),           // MODEL_GENERATED
        Just(Some(8)),           // STALE_INDEX
        Just(Some(3)),           // EXTERNAL | USER
        Just(Some(0x8000_0000)), // invalid bit
    ]
}

fn arb_evidence() -> impl Strategy<Value = Evidence> {
    (
        0u64..100,
        prop_oneof![(0.0f64..1.0f64), Just(f64::NAN), Just(0.0), Just(1.0),],
        prop_oneof![
            Just("apple".to_string()),
            Just("banana".to_string()),
            Just("APPLE".to_string()),
            Just("  apple  ".to_string()),
            Just("cherry".to_string()),
        ],
        arb_taint_flags(),
    )
        .prop_map(|(chunk_id, score, text, flags)| make_evidence(chunk_id, score, &text, flags))
}

proptest! {
    #[test]
    fn proptest_evidence_selection_invariants(
        evidence in prop::collection::vec(arb_evidence(), 0..30),
        cutoff in 0.0f64..1.0f64,
        max in 0usize..15,
    ) {
        let input_len = evidence.len();
        let selected = select(evidence, cutoff, max);

        // Property 1: output length never exceeds max
        prop_assert!(selected.items.len() <= max);

        // Property 2: never contains duplicate chunk_id
        let mut seen_ids = HashSet::new();
        for item in &selected.items {
            prop_assert!(seen_ids.insert(item.chunk_id), "duplicate chunk_id: {}", item.chunk_id);
        }

        // Property 3: never contains duplicate normalised text
        let mut seen_texts = HashSet::new();
        for item in &selected.items {
            let norm: String = item.text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            prop_assert!(seen_texts.insert(norm), "duplicate text in kept items");
        }

        // Property 4: every kept item has score >= cutoff and is not NaN
        for item in &selected.items {
            prop_assert!(!item.score.is_nan());
            prop_assert!(item.score >= cutoff);
        }

        // Property 5: items are sorted descending by score, tie-broken by smaller chunk_id
        for w in selected.items.windows(2) {
            let a = &w[0];
            let b = &w[1];
            if (a.score - b.score).abs() < f64::EPSILON {
                prop_assert!(a.chunk_id <= b.chunk_id);
            } else {
                prop_assert!(a.score >= b.score);
            }
        }

        // Property 6: taint is union of kept items
        let mut expected_taint = TaintFlags::empty();
        for item in &selected.items {
            let decoded = TaintFlags::decode(item.taint.as_ref().map(|t| t.flags)).unwrap();
            expected_taint = expected_taint.merge(decoded);
        }
        prop_assert_eq!(selected.taint, expected_taint);

        // Property 7: drop counts are bounded by input length
        let total_accounted = (selected.items.len() as u32)
            + selected.dropped_no_taint
            + selected.dropped_below_cutoff
            + selected.dropped_duplicate;
        prop_assert!(total_accounted <= (input_len as u32));
    }
}
