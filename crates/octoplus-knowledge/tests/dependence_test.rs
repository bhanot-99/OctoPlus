#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::dependence::{DependenceGuard, GENTLE_LINES, no_one_else};
use octoplus_knowledge::companion::guards::scrub_sentence;
use octoplus_knowledge::companion::persona::Persona;

const NS_PER_MINUTE: u64 = 60_000_000_000;
const NS_PER_DAY: u64 = 86_400_000_000_000;

#[test]
fn test_no_one_else_cues() {
    let positive = [
        "I feel like I have no one to talk to.",
        "There is nobody else who understands.",
        "I have no one else in my corner.",
        "You're all I have right now.",
        "You are all I have in this city.",
        "youre all i have to talk with",
    ];

    for text in positive {
        assert!(
            no_one_else(text),
            "Expected '{text}' to match no_one_else cue"
        );
    }

    let negative = [
        "I talked to my brother earlier today.",
        "There is someone knocking at the door.",
        "Anyone else having this compiler error?",
        "Let's review the code together.",
    ];

    for text in negative {
        assert!(
            !no_one_else(text),
            "Expected '{text}' NOT to match no_one_else cue"
        );
    }
}

#[test]
fn test_gentle_lines_are_safe_and_clean() {
    assert_eq!(GENTLE_LINES.len(), 3);
    for line in GENTLE_LINES {
        assert!(
            scrub_sentence(line),
            "Gentle line '{line}' must not contain any forbidden phrase"
        );
    }
}

#[test]
fn test_dependence_guard_90_minutes_daily_limit() {
    let persona = Persona::default_packaged().unwrap();
    let mut guard = DependenceGuard::new(&persona.dependence_guard);

    let day1_ns = NS_PER_DAY;
    // Record three 30-minute sessions totaling 90 minutes
    guard.record_session(day1_ns, day1_ns + 30 * NS_PER_MINUTE);
    guard.record_session(day1_ns + 40 * NS_PER_MINUTE, day1_ns + 70 * NS_PER_MINUTE);
    guard.record_session(day1_ns + 80 * NS_PER_MINUTE, day1_ns + 110 * NS_PER_MINUTE);

    // At 90 minutes of talk, should trigger once
    let check_time = day1_ns + 120 * NS_PER_MINUTE;
    assert!(
        guard.should_speak(check_time, false),
        "Expected dependence guard to trigger at 90 minutes talk limit"
    );

    // Second check on the same day must be refused (max_per_day = 1)
    assert!(
        !guard.should_speak(check_time + 10 * NS_PER_MINUTE, false),
        "Second check on the same day must be refused"
    );

    // Next day (day 2): limit resets
    let day2_ns = 2 * NS_PER_DAY;
    guard.record_session(day2_ns, day2_ns + 90 * NS_PER_MINUTE);
    assert!(
        guard.should_speak(day2_ns + 100 * NS_PER_MINUTE, false),
        "Next day should allow intervention again"
    );
}

#[test]
fn test_dependence_guard_three_sessions_in_hour() {
    let persona = Persona::default_packaged().unwrap();
    let mut guard = DependenceGuard::new(&persona.dependence_guard);

    let base_time = NS_PER_DAY + 10 * 3_600_000_000_000; // 10:00 AM on Day 1

    // Record three 15-minute sessions within 55 minutes
    // (threshold in default config: 3 sessions in an hour of at least 15 minutes)
    guard.record_session(base_time, base_time + 15 * NS_PER_MINUTE);
    guard.record_session(
        base_time + 18 * NS_PER_MINUTE,
        base_time + 33 * NS_PER_MINUTE,
    );
    guard.record_session(
        base_time + 36 * NS_PER_MINUTE,
        base_time + 51 * NS_PER_MINUTE,
    );

    let check_time = base_time + 55 * NS_PER_MINUTE;
    assert!(
        guard.should_speak(check_time, false),
        "Expected 3 sessions in an hour to trigger dependence guard"
    );
}

#[test]
fn test_dependence_guard_owner_expressed_isolation() {
    let persona = Persona::default_packaged().unwrap();
    let mut guard = DependenceGuard::new(&persona.dependence_guard);

    let now_ns = NS_PER_DAY + 10 * NS_PER_MINUTE;
    // With zero previous talk time, saying nobody else triggers immediate gentle reminder
    assert!(
        guard.should_speak(now_ns, true),
        "Isolation cue should trigger dependence intervention"
    );

    // Subsequent trigger on same day is capped
    assert!(!guard.should_speak(now_ns + 5 * NS_PER_MINUTE, true));
}
