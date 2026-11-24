#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::chat::{
    LOOKUP_OFFER, REMEMBER_OFFER, Topic, accepts_lookup, freshness_cue, personal_fact_statement,
    professional_topic, with_offer, yes_no,
};
use proptest::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ChatFixture {
    text: String,
    fresh: bool,
    topic: String,
    accept: bool,
    yes_no: String,
    fact: bool,
}

const FIXTURES_RAW: &str = include_str!("fixtures/chat.jsonl");

fn parse_expected_topic(s: &str) -> Option<Topic> {
    match s {
        "health" => Some(Topic::Health),
        "legal" => Some(Topic::Legal),
        "money" => Some(Topic::Money),
        "none" => None,
        other => panic!("unknown topic in fixture: {other}"),
    }
}

fn parse_expected_yes_no(s: &str) -> Option<bool> {
    match s {
        "yes" => Some(true),
        "no" => Some(false),
        "none" => None,
        other => panic!("unknown yes_no in fixture: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Fixture verification
// ---------------------------------------------------------------------------

#[test]
fn test_chat_fixtures() {
    let fixtures: Vec<ChatFixture> = FIXTURES_RAW
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| serde_json::from_str::<ChatFixture>(line).expect("valid fixture line"))
        .collect();

    assert!(
        fixtures.len() >= 90,
        "expected >= 90 fixtures, got {}",
        fixtures.len()
    );

    let mut fresh_matches = 0usize;
    let mut topic_matches = 0usize;
    let mut accept_matches = 0usize;
    let mut yes_no_matches = 0usize;
    let mut fact_matches = 0usize;

    for (idx, f) in fixtures.iter().enumerate() {
        // 1. Freshness
        let got_fresh = freshness_cue(&f.text);
        if got_fresh == f.fresh {
            fresh_matches += 1;
        } else {
            eprintln!(
                "Fixture #{} fresh mismatch: text={:?}, got={}, expected={}",
                idx + 1,
                f.text,
                got_fresh,
                f.fresh
            );
        }

        // 2. Professional topic
        let got_topic = professional_topic(&f.text);
        let exp_topic = parse_expected_topic(&f.topic);
        if got_topic == exp_topic {
            topic_matches += 1;
        } else {
            eprintln!(
                "Fixture #{} topic mismatch: text={:?}, got={:?}, expected={:?}",
                idx + 1,
                f.text,
                got_topic,
                exp_topic
            );
        }

        // 3. Accepts lookup
        let got_accept = accepts_lookup(&f.text);
        if got_accept == f.accept {
            accept_matches += 1;
        } else {
            eprintln!(
                "Fixture #{} accept mismatch: text={:?}, got={}, expected={}",
                idx + 1,
                f.text,
                got_accept,
                f.accept
            );
        }

        // 4. Yes/No
        let got_yes_no = yes_no(&f.text);
        let exp_yes_no = parse_expected_yes_no(&f.yes_no);
        if got_yes_no == exp_yes_no {
            yes_no_matches += 1;
        } else {
            eprintln!(
                "Fixture #{} yes_no mismatch: text={:?}, got={:?}, expected={:?}",
                idx + 1,
                f.text,
                got_yes_no,
                exp_yes_no
            );
        }

        // 5. Personal fact statement
        let got_fact = personal_fact_statement(&f.text);
        if got_fact == f.fact {
            fact_matches += 1;
        } else {
            eprintln!(
                "Fixture #{} fact mismatch: text={:?}, got={}, expected={}",
                idx + 1,
                f.text,
                got_fact,
                f.fact
            );
        }
    }

    let n = fixtures.len() as f64;
    let fresh_acc = (fresh_matches as f64) / n;
    let topic_acc = (topic_matches as f64) / n;

    assert!(
        fresh_acc >= 0.95,
        "freshness accuracy must be >= 95%, got {:.2}% ({}/{})",
        fresh_acc * 100.0,
        fresh_matches,
        fixtures.len()
    );
    assert!(
        topic_acc >= 0.95,
        "professional topic accuracy must be >= 95%, got {:.2}% ({}/{})",
        topic_acc * 100.0,
        topic_matches,
        fixtures.len()
    );

    // 100% required on accepts_lookup, yes_no, and personal_fact_statement
    assert_eq!(
        accept_matches,
        fixtures.len(),
        "accepts_lookup must match 100% of fixtures"
    );
    assert_eq!(
        yes_no_matches,
        fixtures.len(),
        "yes_no must match 100% of fixtures"
    );
    assert_eq!(
        fact_matches,
        fixtures.len(),
        "personal_fact_statement must match 100% of fixtures"
    );
}

// ---------------------------------------------------------------------------
// Tables for with_offer
// ---------------------------------------------------------------------------

#[test]
fn test_with_offer_tables_and_idempotence() {
    let cases = [
        (
            "The capital of France is Paris.",
            false,
            "The capital of France is Paris.",
        ),
        (
            "The latest score is 3-2.",
            true,
            "The latest score is 3-2. Would you like me to look that up for you?",
        ),
        (
            "Here is the price: $99. ",
            true,
            "Here is the price: $99. Would you like me to look that up for you?",
        ),
        (
            "Forecast says rain tomorrow.\n",
            true,
            "Forecast says rain tomorrow.\nWould you like me to look that up for you?",
        ),
        ("", false, ""),
        ("", true, "Would you like me to look that up for you?"),
    ];

    for (input, fresh, expected) in cases {
        let got = with_offer(input, fresh);
        assert_eq!(got, expected, "input={:?}, fresh={}", input, fresh);

        // Idempotency: applying with_offer twice with fresh=true equals once
        if fresh {
            let twice = with_offer(&got, true);
            assert_eq!(
                twice, got,
                "with_offer must be idempotent when fresh=true: {:?}",
                input
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Negative-control test showing naive substring match fails on near-misses
// ---------------------------------------------------------------------------

fn naive_substring_freshness(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Naive substring matcher without word boundaries
    lower.contains("best") || lower.contains("top")
}

#[test]
fn test_negative_control_naive_substring_fails_on_near_misses() {
    let near_misses = [
        "please bestow this honor upon them",
        "stop the running background task",
        "the topic of the discussion was interesting",
        "they described a philosophical utopia",
    ];

    for &s in &near_misses {
        let naive = naive_substring_freshness(s);
        let real = freshness_cue(s);

        // Naive substring match incorrectly flags these as fresh
        assert!(
            naive,
            "naive substring matcher should have erroneously matched {s}"
        );
        // Real word-boundary matcher correctly rejects them
        assert!(
            !real,
            "real freshness_cue must correctly reject near-miss {s}"
        );
    }
}

// ---------------------------------------------------------------------------
// Proptests
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn proptest_chat_invariants(text in "\\PC*") {
        // 1. Functions never panic on arbitrary unicode
        let _ = freshness_cue(&text);
        let _ = professional_topic(&text);
        let _ = accepts_lookup(&text);
        let _ = yes_no(&text);
        let _ = personal_fact_statement(&text);

        // 2. with_offer invariants
        let unchanged = with_offer(&text, false);
        prop_assert_eq!(unchanged, text.clone());

        let offered = with_offer(&text, true);
        prop_assert!(offered.ends_with(LOOKUP_OFFER));

        // 3. LOOKUP_OFFER format invariants
        prop_assert!(LOOKUP_OFFER.len() < 100);
        prop_assert!(LOOKUP_OFFER.ends_with('?'));
        prop_assert!(!LOOKUP_OFFER.contains('\n'));
        prop_assert!(!LOOKUP_OFFER.contains('<'));
        prop_assert!(!LOOKUP_OFFER.contains('>'));
        prop_assert!(!LOOKUP_OFFER.contains('*'));
        prop_assert!(!LOOKUP_OFFER.contains('#'));

        // 4. REMEMBER_OFFER format invariants
        prop_assert!(REMEMBER_OFFER.ends_with('?'));
        prop_assert!(!REMEMBER_OFFER.contains('\n'));
    }
}
