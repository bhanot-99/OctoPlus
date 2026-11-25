#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::kabir::{KabirContext, KabirGate, love_topic};
use octoplus_knowledge::companion::persona::Persona;

#[test]
fn test_love_topic_positive() {
    let positive = [
        "Tell me about your thoughts on love.",
        "I have been feeling really lonely lately.",
        "The loneliness can be difficult at night.",
        "I have a deep regret about what happened.",
        "Do you have any regrets in life?",
        "I really miss someone special.",
        "Missing someone from long ago.",
        "Going through a rough breakup right now.",
        "My recent break up was painful.",
        "Healing from heartbreak takes time.",
        "Dealing with heart break is tough.",
        "How is your relationship going?",
        "Past relationships can shape us.",
    ];

    for text in positive {
        assert!(
            love_topic(text),
            "Expected '{text}' to match love/loneliness/regret topic"
        );
    }
}

#[test]
fn test_love_topic_negative() {
    let negative = [
        "What is the capital of France?",
        "Can you write a sorting algorithm in Rust?",
        "The compile error says type mismatch.",
        "Let's check the weather in Seattle.",
        "How many gigabytes of RAM does this machine have?",
        "Process terminated with exit code 0.",
    ];

    for text in negative {
        assert!(
            !love_topic(text),
            "Expected '{text}' NOT to match love/loneliness/regret topic"
        );
    }
}

#[test]
fn test_kabir_gate_blocked_by_crisis() {
    let persona = Persona::default_packaged().unwrap();
    let mut gate = KabirGate::new(&persona.past_love, 42);

    let ctx = KabirContext {
        topic_is_love_loneliness_regret: true,
        session_turns: 10,
        now_ns: 1_000_000_000_000,
        in_crisis: true,
    };

    assert!(!gate.allowed(ctx));
    assert_eq!(gate.counters().opened, 0);
    assert_eq!(gate.counters().allowed, 0);
}

#[test]
fn test_kabir_gate_blocked_by_cooldown() {
    let persona = Persona::default_packaged().unwrap();
    let mut gate = KabirGate::new(&persona.past_love, 42);

    // Initial mention at timestamp T
    let t0 = 100_000_000_000_000;
    gate.mentioned(t0);
    assert_eq!(gate.counters().mentions, 1);

    // Check 1 day later (cooldown is 3 days in default persona)
    let one_day_ns = 86_400_000_000_000;
    let ctx_1day = KabirContext {
        topic_is_love_loneliness_regret: true,
        session_turns: 10,
        now_ns: t0 + one_day_ns,
        in_crisis: false,
    };
    assert!(!gate.allowed(ctx_1day));

    // Check 2 days later: still blocked
    let ctx_2days = KabirContext {
        topic_is_love_loneliness_regret: true,
        session_turns: 10,
        now_ns: t0 + (2 * one_day_ns),
        in_crisis: false,
    };
    assert!(!gate.allowed(ctx_2days));

    // Check 3 days later: cooldown expired, gate opens
    let ctx_3days = KabirContext {
        topic_is_love_loneliness_regret: true,
        session_turns: 10,
        now_ns: t0 + (3 * one_day_ns),
        in_crisis: false,
    };
    let _ = gate.allowed(ctx_3days);
    assert_eq!(gate.counters().opened, 1);
}

#[test]
fn test_kabir_gate_requires_topic_or_turns() {
    let persona = Persona::default_packaged().unwrap();
    let mut gate = KabirGate::new(&persona.past_love, 42);

    // Topic false and turns below min_turns (default min_turns = 25)
    let ctx_blocked = KabirContext {
        topic_is_love_loneliness_regret: false,
        session_turns: 24,
        now_ns: 10_000_000_000_000,
        in_crisis: false,
    };
    assert!(!gate.allowed(ctx_blocked));
    assert_eq!(gate.counters().opened, 0);

    // When turns reach min_turns (25), gate opens
    let ctx_turns = KabirContext {
        topic_is_love_loneliness_regret: false,
        session_turns: 25,
        now_ns: 10_000_000_000_000,
        in_crisis: false,
    };
    let _ = gate.allowed(ctx_turns);
    assert_eq!(gate.counters().opened, 1);
}

#[test]
fn test_kabir_gate_probability_distribution() {
    let persona = Persona::default_packaged().unwrap();
    assert_eq!(persona.past_love.p, 0.15);

    let mut gate = KabirGate::new(&persona.past_love, 123456789);

    let ctx = KabirContext {
        topic_is_love_loneliness_regret: true,
        session_turns: 5,
        now_ns: 1_000_000_000_000,
        in_crisis: false,
    };

    let total_draws = 10_000;
    let mut allowed_count = 0;
    for _ in 0..total_draws {
        if gate.allowed(ctx) {
            allowed_count += 1;
        }
    }

    let counters = gate.counters();
    assert_eq!(counters.opened, total_draws);
    assert_eq!(counters.allowed, allowed_count);

    let share = allowed_count as f64 / total_draws as f64;
    assert!(
        (0.12..=0.18).contains(&share),
        "Allowed share {share:.4} should be within [0.12, 0.18] for p=0.15"
    );
}
