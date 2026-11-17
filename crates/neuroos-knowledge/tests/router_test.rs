#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use neuroos_knowledge::router::{ActionHint, RouteInput, RouteKind, normalise, route};
use proptest::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    text: String,
    expected: String,
    nudge_window_open: bool,
    best_score: Option<f64>,
}

const FIXTURES_RAW: &str = include_str!("fixtures/router.jsonl");

fn parse_fixtures() -> Vec<Fixture> {
    FIXTURES_RAW
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str::<Fixture>(line).expect("valid fixture line JSON"))
        .collect()
}

fn parse_expected_route(expected: &str) -> RouteKind {
    match expected {
        "nudge_reply" => RouteKind::NudgeReply,
        "action" => RouteKind::Action,
        "desktop" => RouteKind::Desktop,
        "general" => RouteKind::General,
        other => panic!("unknown expected route kind in fixture: {other}"),
    }
}

#[test]
fn test_fixtures_count_and_balance() {
    let fixtures = parse_fixtures();

    // Must have >= 120 lines
    assert!(
        fixtures.len() >= 120,
        "expected >= 120 fixtures, found {}",
        fixtures.len()
    );

    let mut nudge_count = 0;
    let mut action_count = 0;
    let mut desktop_count = 0;
    let mut general_count = 0;

    for f in &fixtures {
        match f.expected.as_str() {
            "nudge_reply" => nudge_count += 1,
            "action" => action_count += 1,
            "desktop" => desktop_count += 1,
            "general" => general_count += 1,
            other => panic!("unexpected route kind: {other}"),
        }
    }

    // Must have >= 10 NudgeReply lines
    assert!(
        nudge_count >= 10,
        "expected >= 10 nudge_reply fixtures, got {nudge_count}"
    );

    // Balanced distribution
    assert!(nudge_count >= 20, "nudge count: {nudge_count}");
    assert!(action_count >= 20, "action count: {action_count}");
    assert!(desktop_count >= 20, "desktop count: {desktop_count}");
    assert!(general_count >= 20, "general count: {general_count}");
}

#[test]
fn test_fixtures_accuracy_100_percent() {
    let fixtures = parse_fixtures();
    let cutoff = 0.7;

    for (idx, f) in fixtures.iter().enumerate() {
        let input = RouteInput {
            text: &f.text,
            nudge_window_open: f.nudge_window_open,
            retrieval_best_score: f.best_score,
            cutoff,
        };
        let decision = route(&input);
        let expected = parse_expected_route(&f.expected);

        assert_eq!(
            decision.route,
            expected,
            "Fixture #{} failed: text={:?}, expected={:?}, got={:?} (decision: {:?})",
            idx + 1,
            f.text,
            expected,
            decision.route,
            decision
        );
    }
}

#[test]
fn test_all_action_hints() {
    let cases = [
        ("pause", ActionHint::Pause),
        ("please pause", ActionHint::Pause),
        ("stop listening for a while", ActionHint::Pause),
        ("mute", ActionHint::Pause),
        ("forget what I did today", ActionHint::Forget),
        ("delete what I did", ActionHint::Forget),
        ("forget everything from yesterday", ActionHint::Forget),
        ("forget that session", ActionHint::Forget),
        ("delete my browser history", ActionHint::Forget),
        ("forget that", ActionHint::ForgetFact),
        ("forget that fact", ActionHint::ForgetFact),
        ("forget that my dog is named rover", ActionHint::ForgetFact),
        ("forget my favorite color", ActionHint::ForgetFact),
        ("remember that I prefer dark mode", ActionHint::Remember),
        ("remember my wifi password is secret", ActionHint::Remember),
        ("clean up disk", ActionHint::Cleanup),
        ("free up disk space", ActionHint::Cleanup),
        ("empty the trash", ActionHint::Cleanup),
        ("search the web for rust", ActionHint::Search),
        ("look up online how to bake bread", ActionHint::Search),
        ("open this link", ActionHint::Fetch),
        ("fetch this url", ActionHint::Fetch),
        ("add to my note buy milk", ActionHint::AppendNote),
        ("write in my notes doctor visit", ActionHint::AppendNote),
        ("append to my notes meeting", ActionHint::AppendNote),
        ("search my notes for passport", ActionHint::NotesSearch),
        ("find in my notes flight number", ActionHint::NotesSearch),
        ("what is on my calendar", ActionHint::CalendarRead),
        ("my schedule", ActionHint::CalendarRead),
        ("what is using disk space", ActionHint::DiskReport),
    ];

    for (text, expected_hint) in cases {
        let input = RouteInput {
            text,
            nudge_window_open: false,
            retrieval_best_score: None,
            cutoff: 0.7,
        };
        let decision = route(&input);
        assert_eq!(
            decision.route,
            RouteKind::Action,
            "text={text:?} expected Action route"
        );
        assert_eq!(
            decision.action_hint,
            Some(expected_hint),
            "text={text:?} expected hint {expected_hint:?}"
        );
    }
}

#[test]
fn test_normalise_edge_cases() {
    // Empty
    assert_eq!(normalise(""), "");
    assert_eq!(normalise("   "), "");

    // Only punctuation
    assert_eq!(normalise("..."), "");
    assert_eq!(normalise(",,, --- !?"), "");
    assert_eq!(normalise(";;; ::: ()[]{}"), "");

    // Unicode
    assert_eq!(normalise("Héllo, Wörld! 123"), "héllo wörld 123");
    assert_eq!(normalise("¿Cómo estás?"), "cómo estás");
    assert_eq!(normalise("Café — au lait!"), "café au lait");

    // Hyphenated & punctuation folding to single spaces
    assert_eq!(normalise("forget-me-not"), "forget me not");
    assert_eq!(normalise("don't stop"), "don t stop");
    assert_eq!(
        normalise("\tfoo...bar\n\nbaz   qux--123"),
        "foo bar baz qux 123"
    );
    assert_eq!(normalise("  multiple   spaces  "), "multiple spaces");
}

#[test]
fn test_negative_control_broken_matcher() {
    // A deliberately broken classifier that matches action keywords without word boundaries or imperative structure
    fn broken_classifier(text: &str) -> &'static str {
        let action_words = [
            "pause", "forget", "remember", "clean", "disk", "search", "note", "fetch", "empty",
        ];
        let lower = text.to_lowercase();
        if action_words.iter().any(|w| lower.contains(w)) {
            "action"
        } else {
            "general"
        }
    }

    let fixtures = parse_fixtures();
    let mut broken_failures = 0;

    for f in &fixtures {
        let broken_route = broken_classifier(&f.text);
        if broken_route != f.expected.as_str() {
            broken_failures += 1;
        }
    }

    // Must fail on at least one fixture (especially near-misses like "the forget-me-not flower")
    assert!(
        broken_failures >= 10,
        "broken classifier should fail on several near-miss fixtures, but only failed on {broken_failures}"
    );

    // Specifically test near-miss examples
    assert_eq!(
        broken_classifier("the forget-me-not flower is blue"),
        "action"
    );
    assert_eq!(
        broken_classifier("I remember when we visited London"),
        "action"
    );
    assert_eq!(broken_classifier("it will be easy to forget"), "action");
}

proptest! {
    #[test]
    fn proptest_never_panics(
        text in "\\PC*",
        nudge_open in any::<bool>(),
        score in proptest::option::of(any::<f64>()),
        cutoff in any::<f64>(),
    ) {
        let input = RouteInput {
            text: &text,
            nudge_window_open: nudge_open,
            retrieval_best_score: score,
            cutoff,
        };
        let _decision = route(&input);
    }

    #[test]
    fn proptest_deterministic(
        text in "\\PC*",
        nudge_open in any::<bool>(),
        score in proptest::option::of(any::<f64>()),
        cutoff in any::<f64>(),
    ) {
        let input1 = RouteInput {
            text: &text,
            nudge_window_open: nudge_open,
            retrieval_best_score: score,
            cutoff,
        };
        let input2 = RouteInput {
            text: &text,
            nudge_window_open: nudge_open,
            retrieval_best_score: score,
            cutoff,
        };
        prop_assert_eq!(route(&input1), route(&input2));
    }

    #[test]
    fn proptest_no_nudge_when_window_closed(
        text in "\\PC*",
        score in proptest::option::of(any::<f64>()),
        cutoff in any::<f64>(),
    ) {
        let input = RouteInput {
            text: &text,
            nudge_window_open: false,
            retrieval_best_score: score,
            cutoff,
        };
        let decision = route(&input);
        prop_assert_ne!(decision.route, RouteKind::NudgeReply);
    }

    #[test]
    fn proptest_no_action_from_text_without_letters(
        text in "[0-9 \\p{P}\\p{S}]*",
        nudge_open in any::<bool>(),
        score in proptest::option::of(any::<f64>()),
        cutoff in any::<f64>(),
    ) {
        // Digits, spaces and symbols alone never name a skill.
        let decision = route(&RouteInput {
            text: &text,
            nudge_window_open: nudge_open,
            retrieval_best_score: score,
            cutoff,
        });
        prop_assert_ne!(decision.route, RouteKind::Action);
        prop_assert_eq!(decision.action_hint, None);
    }
}
