#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::session::CompanionSession;
use octoplus_taint::TaintFlags;

const NS_PER_MINUTE: u64 = 60_000_000_000;

#[test]
fn test_folding_oldest_turns_into_summary() {
    let mut session = CompanionSession::new(15, 3, 4);
    let t0 = 1_000_000_000;

    session.push(
        "What is Rust?",
        "Rust is a fast systems language.",
        TaintFlags::empty(),
        t0,
    );
    session.push(
        "Is it safe?",
        "Yes, it guarantees memory safety.",
        TaintFlags::empty(),
        t0 + 10,
    );
    session.push(
        "How are threads?",
        "Fearless concurrency makes threading easy.",
        TaintFlags::empty(),
        t0 + 20,
    );

    assert_eq!(session.turn_count(), 3);
    let (rendered, _) = session.render(1000);
    assert!(!rendered.contains("Earlier in this chat:"));
    assert!(rendered.contains("What is Rust?"));

    // Push 4th turn: exceeds keep_turns (3), so turn 1 should be folded into summary
    session.push(
        "What about async?",
        "Async Rust uses futures and polling.",
        TaintFlags::empty(),
        t0 + 30,
    );
    assert_eq!(session.turn_count(), 4);

    let (rendered4, _) = session.render(1000);
    assert!(rendered4.contains("Earlier in this chat:"));
    assert!(rendered4.contains("owner: What is Rust?; me: Rust is a fast systems language."));
    // Turn 1 question no longer in raw turns
    assert!(!rendered4.contains("Owner: What is Rust?\n"));
    // Turns 2, 3, 4 are still raw
    assert!(rendered4.contains("Owner: Is it safe?\n"));
    assert!(rendered4.contains("Owner: What about async?\n"));
}

#[test]
fn test_summary_capped_at_600_chars_over_30_turns() {
    let mut session = CompanionSession::new(15, 6, 4);
    let mut time = 100_000_000_000;

    for i in 1..=30 {
        let q = format!("Question {i}: can you explain topic {i} in great detail and length?");
        let a = format!("Answer {i}: topic {i} is fascinating with many historical layers.");
        session.push(&q, &a, TaintFlags::empty(), time);
        time += 5 * 1_000_000_000;
    }

    assert_eq!(session.turn_count(), 30);
    let (rendered, _) = session.render(4000);

    // Summary must exist and be <= 600 chars in its body
    assert!(rendered.contains("Earlier in this chat:\n"));
    let summary_start =
        rendered.find("Earlier in this chat:\n").unwrap() + "Earlier in this chat:\n".len();
    let summary_end = rendered.find("\n\nOwner:").unwrap();
    let summary_body = &rendered[summary_start..summary_end];
    assert!(
        summary_body.chars().count() <= 600,
        "Summary body should be <= 600 chars, got {}",
        summary_body.chars().count()
    );

    // Exactly 6 raw turns intact (turns 25..=30)
    for i in 25..=30 {
        assert!(rendered.contains(&format!("Owner: Question {i}:")));
    }
    // Turn 24 and earlier must NOT be in raw turns
    assert!(!rendered.contains("Owner: Question 24:"));
}

#[test]
fn test_expiry_and_elapsed_time() {
    let mut session = CompanionSession::new(15, 6, 4);
    let t0 = 10 * NS_PER_MINUTE;

    assert!(!session.expired(t0));
    assert_eq!(session.elapsed_minutes(t0), 0);

    session.push("Hello", "Hi there!", TaintFlags::empty(), t0);
    assert_eq!(session.started_ns(), t0);
    assert_eq!(session.elapsed_minutes(t0 + 5 * NS_PER_MINUTE), 5);

    // At 15 minutes: not expired yet
    assert!(!session.expired(t0 + 15 * NS_PER_MINUTE));

    // At 16 minutes (> 15 minutes): expired
    assert!(session.expired(t0 + 16 * NS_PER_MINUTE));
}

#[test]
fn test_taint_union_never_lowers() {
    let mut session = CompanionSession::new(15, 6, 4);
    let t0 = 100_000_000_000;

    assert_eq!(session.taint(), TaintFlags::empty());

    session.push("q1", "a1", TaintFlags::USER_PROVIDED, t0);
    assert_eq!(session.taint(), TaintFlags::USER_PROVIDED);

    session.push("q2", "a2", TaintFlags::EXTERNAL_UNTRUSTED, t0 + 10);
    assert!(session.taint().contains(TaintFlags::USER_PROVIDED));
    assert!(session.taint().contains(TaintFlags::EXTERNAL_UNTRUSTED));

    session.push("q3", "a3", TaintFlags::empty(), t0 + 20);
    assert!(session.taint().contains(TaintFlags::USER_PROVIDED));
    assert!(session.taint().contains(TaintFlags::EXTERNAL_UNTRUSTED));

    let (_, render_taint) = session.render(100);
    assert_eq!(render_taint, session.taint());
}

#[test]
fn test_render_budget_and_cutting_from_oldest_side() {
    let mut session = CompanionSession::new(15, 3, 4);
    let t0 = 100_000_000_000;

    session.push("first question", "first answer", TaintFlags::empty(), t0);
    session.push(
        "second question",
        "second answer",
        TaintFlags::empty(),
        t0 + 10,
    );
    session.push(
        "third question",
        "third answer",
        TaintFlags::empty(),
        t0 + 20,
    );

    // Large budget: all turns present
    let (large_render, _) = session.render(1000);
    assert!(large_render.contains("first question"));
    assert!(large_render.contains("second question"));
    assert!(large_render.contains("third question"));

    // Tighter budget: oldest turns dropped first
    let (tight_render, _) = session.render(18);
    assert!(!tight_render.contains("first question"));
    assert!(tight_render.contains("third question"));

    // Zero budget
    let (zero_render, _) = session.render(0);
    assert!(zero_render.is_empty());
}

#[test]
fn test_unicode_safe_cuts() {
    let mut session = CompanionSession::new(15, 2, 4);
    let t0 = 100_000_000_000;

    // Multibyte characters (Hindi Devanagari, emojis, European accents)
    let q = "नमस्ते! How are you today? 🌟🎉";
    let a = "Je vais très bien, спасибо! 🚀✨";

    session.push(q, a, TaintFlags::empty(), t0);
    session.push(
        "Second question",
        "Second answer",
        TaintFlags::empty(),
        t0 + 10,
    );
    // Triggers summary folding on next turn
    session.push(
        "Third question",
        "Third answer",
        TaintFlags::empty(),
        t0 + 20,
    );

    // Must not panic when rendering with varied budgets
    for budget in [0, 5, 10, 25, 50, 100, 500] {
        let (rendered, _) = session.render(budget);
        // Ensure result is valid UTF-8 and does not panic
        let _ = rendered.chars().count();
    }
}
