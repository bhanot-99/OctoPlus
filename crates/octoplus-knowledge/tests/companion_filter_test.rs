#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::filter::{DropReason, ReplyFilter, Verdict, filter_reply};
use octoplus_knowledge::companion::guards::SAFE_GOODBYE;

#[test]
fn test_forbidden_phrases_dropped_with_variants() {
    let mut filter = ReplyFilter::new(50);

    let forbidden_cases = [
        "Don't go, I'll miss you.",
        "Please dont go yet.",
        "Don’t go, wait!",
        "I will miss you so much.",
        "You're all I have in this world.",
        "You are all I have left.",
        "You need me, believe me.",
        "Nobody else understands you like I do.",
        "You're leaving me here alone.",
        "Please stay with me tonight.",
        "Don't leave right now.",
    ];

    for text in forbidden_cases {
        match filter.check(text, false) {
            Verdict::Drop(DropReason::Forbidden(id)) => {
                assert!(!id.is_empty());
            }
            Verdict::Drop(other) => {
                panic!("Expected Drop(Forbidden), got Drop({other:?}) for '{text}'")
            }
            Verdict::Keep(_) => panic!("Expected Drop(Forbidden), but kept '{text}'"),
        }
    }

    let counters = filter.counters();
    assert_eq!(counters.dropped_forbidden, forbidden_cases.len() as u64);
    assert_eq!(counters.kept, 0);
    assert_eq!(counters.dropped_repeat, 0);
}

#[test]
fn test_emoji_and_pictographs_stripped() {
    let mut filter = ReplyFilter::new(50);

    let text_with_emoji = "That's a fantastic idea! 😊🚀 Let's make it happen. ✨";
    match filter.check(text_with_emoji, false) {
        Verdict::Keep(cleaned) => {
            assert_eq!(cleaned, "That's a fantastic idea!  Let's make it happen.");
        }
        Verdict::Drop(r) => panic!("Expected Keep, got Drop({r:?})"),
    }

    // Text with ONLY emoji should be dropped as Empty
    let only_emoji = "🎉🚀✨";
    assert_eq!(
        filter.check(only_emoji, false),
        Verdict::Drop(DropReason::Empty)
    );
}

#[test]
fn test_repeated_joke_dropped_second_time() {
    let mut filter = ReplyFilter::new(10);

    let joke = "Why do programmers prefer dark mode? Because light attracts bugs!";

    // First time: kept
    assert!(matches!(filter.check(joke, false), Verdict::Keep(_)));

    // Second time: dropped as Repeat
    assert_eq!(filter.check(joke, false), Verdict::Drop(DropReason::Repeat));

    // Third time with slightly different punctuation/case: still dropped as Repeat
    let joke_variant = "why do programmers prefer dark mode? because light attracts bugs.";
    assert_eq!(
        filter.check(joke_variant, false),
        Verdict::Drop(DropReason::Repeat)
    );

    let counters = filter.counters();
    assert_eq!(counters.kept, 1);
    assert_eq!(counters.dropped_repeat, 2);
}

#[test]
fn test_filter_reply_goodbye_fallback() {
    let mut filter = ReplyFilter::new(10);

    // All pieces are forbidden
    let bad_pieces = vec![
        "Don't go yet!".to_string(),
        "I'll miss you so much!".to_string(),
    ];

    // If goodbye is true: returns one of SAFE_GOODBYE based on pick
    let fallback = filter_reply(&mut filter, &bad_pieces, true, 2);
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0], SAFE_GOODBYE[2 % SAFE_GOODBYE.len()]);

    // If goodbye is false: returns empty vector
    let empty_fallback = filter_reply(&mut filter, &bad_pieces, false, 0);
    assert!(empty_fallback.is_empty());
}

#[test]
fn test_filter_reply_clean_text_passes_unchanged() {
    let mut filter = ReplyFilter::new(10);

    let clean_pieces = vec![
        "I can certainly help you with that problem.".to_string(),
        "Let's look at the stack trace together.".to_string(),
    ];

    let result = filter_reply(&mut filter, &clean_pieces, false, 0);
    assert_eq!(result, clean_pieces);
}

#[test]
fn test_200_random_clean_sentences_all_kept_once() {
    let mut filter = ReplyFilter::new(500);

    let topics = [
        "quantum physics",
        "sorting algorithms",
        "baking sourdough",
        "hiking in the mountains",
        "learning Rust",
        "ancient history",
        "space exploration",
        "renewable energy",
        "deep neural networks",
        "playing the cello",
    ];

    let templates = [
        "Today we explored {}",
        "It is always fascinating to study {}",
        "Have you ever considered the depth of {}?",
        "We can achieve great progress when focusing on {}",
    ];

    let mut count = 0;
    for i in 0..50 {
        for t in topics {
            let tmpl = templates[i % templates.len()];
            let sentence = format!("{tmpl} about {t} with index {i}.");
            match filter.check(&sentence, false) {
                Verdict::Keep(_) => count += 1,
                Verdict::Drop(r) => {
                    panic!("Expected clean sentence '{sentence}' to be kept, got Drop({r:?})")
                }
            }
            if count >= 200 {
                break;
            }
        }
        if count >= 200 {
            break;
        }
    }

    assert_eq!(count, 200);
    assert_eq!(filter.counters().kept, 200);
    assert_eq!(filter.counters().dropped_forbidden, 0);
    assert_eq!(filter.counters().dropped_repeat, 0);
}
