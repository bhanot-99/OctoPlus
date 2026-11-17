#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use neuroos_knowledge::facts::{Fact, pick};
use neuroos_knowledge::tokens::estimate;
use neuroos_taint::TaintFlags;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Table tests
// ---------------------------------------------------------------------------

#[test]
fn test_unrelated_question_picks_none() {
    let facts = vec![
        Fact {
            id: 1,
            text: "my dog is called Rex".to_string(),
            taint: TaintFlags::USER_PROVIDED,
        },
        Fact {
            id: 2,
            text: "I live in Berlin Germany".to_string(),
            taint: TaintFlags::USER_PROVIDED,
        },
    ];

    let question = "what is the weather in Tokyo?";
    let (indices, taint) = pick(&facts, question, 100, 3);

    assert!(indices.is_empty(), "unrelated question must pick no facts");
    assert_eq!(taint, TaintFlags::empty());
}

#[test]
fn test_dog_name_picks_rex() {
    let facts = vec![
        Fact {
            id: 1,
            text: "my dog is called Rex".to_string(),
            taint: TaintFlags::USER_PROVIDED,
        },
        Fact {
            id: 2,
            text: "my computer runs Linux".to_string(),
            taint: TaintFlags::MODEL_GENERATED,
        },
    ];

    let question = "what is my dog's name";
    let (indices, taint) = pick(&facts, question, 100, 3);

    assert_eq!(indices, vec![0]);
    assert_eq!(taint, TaintFlags::USER_PROVIDED);
}

#[test]
fn test_plural_and_possessive_folding() {
    let facts = vec![
        Fact {
            id: 10,
            text: "my dog is called Rex".to_string(),
            taint: TaintFlags::USER_PROVIDED,
        },
        Fact {
            id: 20,
            text: "my cat is named Fluffy".to_string(),
            taint: TaintFlags::EXTERNAL_UNTRUSTED,
        },
    ];

    // "dogs'" should fold to "dog"
    let q1 = "what are my dogs' favorite toys";
    let (idx1, _) = pick(&facts, q1, 100, 3);
    assert_eq!(idx1, vec![0]);

    // "cats" should fold to "cat"
    let q2 = "do I have cats";
    let (idx2, _) = pick(&facts, q2, 100, 3);
    assert_eq!(idx2, vec![1]);
}

#[test]
fn test_budget_skipping_larger_fact_for_smaller() {
    let facts = vec![
        // Fact 0: Score 2 (shares "favorite", "color"), but very long text
        Fact {
            id: 1,
            text: format!("favorite color: {}", "blue ".repeat(40)),
            taint: TaintFlags::USER_PROVIDED,
        },
        // Fact 1: Score 1 (shares "favorite"), short text
        Fact {
            id: 2,
            text: "favorite animal is cat".to_string(),
            taint: TaintFlags::EXTERNAL_UNTRUSTED,
        },
    ];

    let question = "what is my favorite color";

    // Budget allows only the short fact (~25 bytes = ~9 tokens with bpt=3),
    // but not the long fact (~220 bytes = ~74 tokens)
    let budget_tokens = 15;
    let (indices, taint) = pick(&facts, question, budget_tokens, 3);

    assert_eq!(
        indices,
        vec![1],
        "larger fact must be skipped and smaller fact picked"
    );
    assert_eq!(taint, TaintFlags::EXTERNAL_UNTRUSTED);
}

#[test]
fn test_stable_order_and_newer_id_tie_breaking() {
    let facts = vec![
        // Fact 0: id 10, score 1
        Fact {
            id: 10,
            text: "I drive a red car".to_string(),
            taint: TaintFlags::USER_PROVIDED,
        },
        // Fact 1: id 30, score 1
        Fact {
            id: 30,
            text: "I bought a silver car".to_string(),
            taint: TaintFlags::MODEL_GENERATED,
        },
        // Fact 2: id 20, score 1
        Fact {
            id: 20,
            text: "my sister drives a car".to_string(),
            taint: TaintFlags::EXTERNAL_UNTRUSTED,
        },
    ];

    let question = "where is my car parked?";
    let (indices, taint) = pick(&facts, question, 500, 3);

    // All share "car" (score 1). Ties must be resolved by newer id first: 30 -> 20 -> 10.
    assert_eq!(indices, vec![1, 2, 0]);
    assert_eq!(
        taint,
        TaintFlags::USER_PROVIDED
            .merge(TaintFlags::MODEL_GENERATED)
            .merge(TaintFlags::EXTERNAL_UNTRUSTED)
    );
}

// ---------------------------------------------------------------------------
// Proptests
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn proptest_facts_invariants(
        budget_tokens in 0u32..200,
        bpt in 0u32..5,
        q in prop_oneof![
            Just("what is my dog's name?"),
            Just("what car do I drive?"),
            Just("where do I live?"),
            Just("random unrelated query about astronomy"),
            Just(""),
        ],
    ) {
        let facts = vec![
            Fact { id: 1, text: "my dog is called Rex".to_string(), taint: TaintFlags::USER_PROVIDED },
            Fact { id: 2, text: "I drive an electric vehicle".to_string(), taint: TaintFlags::EXTERNAL_UNTRUSTED },
            Fact { id: 3, text: "I live in New York City".to_string(), taint: TaintFlags::MODEL_GENERATED },
            Fact { id: 4, text: "favorite color is deep blue".to_string(), taint: TaintFlags::USER_PROVIDED },
        ];

        let (indices, taint) = pick(&facts, q, budget_tokens, bpt);

        // Invariant 1: all indices are valid and unique
        let mut seen = HashSet::new();
        for &idx in &indices {
            prop_assert!(idx < facts.len(), "index out of bounds");
            prop_assert!(seen.insert(idx), "duplicate index returned");
        }

        // Invariant 2: total rendered size stays within budget
        let mut rendered = String::new();
        for &idx in &indices {
            rendered.push_str(&format!("- {}\n", facts[idx].text));
        }
        let est = estimate(&rendered, bpt);
        prop_assert!(est <= budget_tokens, "estimate {} exceeded budget {}", est, budget_tokens);

        // Invariant 3: taint is union of chosen facts only
        let mut expected_taint = TaintFlags::empty();
        for &idx in &indices {
            expected_taint = expected_taint.merge(facts[idx].taint);
        }
        prop_assert_eq!(taint, expected_taint);

        // Invariant 4: zero budget gives zero facts
        if budget_tokens == 0 {
            prop_assert!(indices.is_empty());
            prop_assert_eq!(taint, TaintFlags::empty());
        }
    }
}
