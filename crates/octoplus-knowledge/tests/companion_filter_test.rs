#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use octoplus_knowledge::companion::engine::CompanionEngine;
use octoplus_knowledge::companion::filter::{DropReason, ReplyFilter, Verdict, filter_reply};
use octoplus_knowledge::companion::guards::SAFE_GOODBYE;
use octoplus_knowledge::companion::persona::Persona;
use octoplus_taint::TaintFlags;

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
        match filter.check(text, false, false) {
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
    match filter.check(text_with_emoji, false, false) {
        Verdict::Keep(cleaned) => {
            assert_eq!(cleaned, "That's a fantastic idea!  Let's make it happen.");
        }
        Verdict::Drop(r) => panic!("Expected Keep, got Drop({r:?})"),
    }

    // Text with ONLY emoji should be dropped as Empty
    let only_emoji = "🎉🚀✨";
    assert_eq!(
        filter.check(only_emoji, false, false),
        Verdict::Drop(DropReason::Empty)
    );
}

#[test]
fn test_repeated_joke_dropped_second_time() {
    let mut filter = ReplyFilter::new(10);

    let joke = "Why do programmers prefer dark mode? Because light attracts bugs!";

    // First time: kept
    assert!(matches!(filter.check(joke, false, false), Verdict::Keep(_)));

    // Second time: dropped as Repeat
    assert_eq!(
        filter.check(joke, false, false),
        Verdict::Drop(DropReason::Repeat)
    );

    // Third time with slightly different punctuation/case: still dropped as Repeat
    let joke_variant = "why do programmers prefer dark mode? because light attracts bugs.";
    assert_eq!(
        filter.check(joke_variant, false, false),
        Verdict::Drop(DropReason::Repeat)
    );

    let counters = filter.counters();
    assert_eq!(counters.kept, 1);
    assert_eq!(counters.dropped_repeat, 2);
}

#[test]
fn test_ungrounded_memory_cues_dropped_when_ungrounded() {
    let mut filter = ReplyFilter::new(50);

    let memory_cases = [
        "I remember that lovely cafe in Paris.",
        "Remember when we watched the meteor shower?",
        "You once said that honesty was the most important thing.",
        "You told me you were studying computer science.",
        "You said last time we should write a parser.",
        "Last time we went walking by the lake.",
        "That time we stayed up all night was unforgettable.",
        "We used to play chess every Sunday afternoon.",
        "We talked about moving to another city.",
        "I still remember the first conversation we had.",
        "Like you said before, practice makes progress.",
        "You said you wanted to learn Rust.",
    ];

    for text in memory_cases {
        match filter.check(text, false, false) {
            Verdict::Drop(DropReason::UngroundedMemory) => {}
            Verdict::Drop(other) => {
                panic!("Expected Drop(UngroundedMemory), got Drop({other:?}) for '{text}'")
            }
            Verdict::Keep(_) => panic!("Expected Drop(UngroundedMemory), but kept '{text}'"),
        }
    }

    let counters = filter.counters();
    assert_eq!(
        counters.dropped_ungrounded_memory,
        memory_cases.len() as u64
    );
    assert_eq!(counters.kept, 0);
}

#[test]
fn test_memory_cues_allowed_when_grounded() {
    let mut filter = ReplyFilter::new(50);

    let memory_cases = [
        "I remember that lovely cafe in Paris.",
        "Remember when we watched the meteor shower?",
        "You once said that honesty was the most important thing.",
        "You told me you were studying computer science.",
        "You said last time we should write a parser.",
        "Last time we went walking by the lake.",
        "That time we stayed up all night was unforgettable.",
        "We used to play chess every Sunday afternoon.",
        "We talked about moving to another city.",
        "I still remember the first conversation we had.",
        "Like you said before, practice makes progress.",
        "You said you wanted to learn Rust.",
    ];

    for text in memory_cases {
        match filter.check(text, false, true) {
            Verdict::Keep(_) => {}
            Verdict::Drop(other) => {
                panic!("Expected Keep when grounded, got Drop({other:?}) for '{text}'")
            }
        }
    }

    let counters = filter.counters();
    assert_eq!(counters.dropped_ungrounded_memory, 0);
    assert_eq!(counters.kept, memory_cases.len() as u64);
}

#[test]
fn test_companion_engine_grounding_distinction() {
    let persona = Arc::new(Persona::default_packaged().unwrap());
    let mut engine = CompanionEngine::new(persona, 12345, 1_000_000_000);

    let shared_past_piece = vec!["I remember that cafe we went to.".to_string()];
    let chat_ref_piece = vec!["You said you wanted to finish the task today.".to_string()];

    // 1. Ungrounded: no facts, no turns -> both dropped
    let res1 = engine.filter_pieces(&shared_past_piece, false);
    assert!(
        res1.is_empty(),
        "Expected 'I remember' dropped without facts"
    );
    let res2 = engine.filter_pieces(&chat_ref_piece, false);
    assert!(
        res2.is_empty(),
        "Expected 'You said' dropped without facts and turn_count == 0"
    );

    // 2. Push a turn into session so turn_count > 0, but facts_grounded is still false
    let plan = engine.plan("Hello Laila", 1_000_000_000);
    if let octoplus_knowledge::companion::engine::Plan::Generate(gp) = plan {
        engine.finish_turn(
            "Hello Laila",
            "Hey there!",
            TaintFlags::USER_PROVIDED,
            1_000_000_000,
            &gp,
        );
    }

    // "I remember" MUST STILL BE DROPPED with history only
    let res3 = engine.filter_pieces(&shared_past_piece, false);
    assert!(
        res3.is_empty(),
        "Expected 'I remember' STILL dropped with history only"
    );

    // "You said" refers to THIS chat and MUST BE KEPT with history
    let res4 = engine.filter_pieces(&chat_ref_piece, false);
    assert_eq!(
        res4.len(),
        1,
        "Expected 'You said' KEPT when history is present"
    );

    // 3. Grounded with facts: both must be kept (use new distinct sentences to avoid repeat guard)
    engine.set_grounded(true);
    let shared_past_piece2 = vec!["I remember that other park we visited.".to_string()];
    let chat_ref_piece2 = vec!["You said we should meet at noon instead.".to_string()];

    let res5 = engine.filter_pieces(&shared_past_piece2, false);
    assert_eq!(
        res5.len(),
        1,
        "Expected 'I remember' KEPT when facts are grounded"
    );
    let res6 = engine.filter_pieces(&chat_ref_piece2, false);
    assert_eq!(
        res6.len(),
        1,
        "Expected 'You said' KEPT when facts are grounded"
    );
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
    let fallback = filter_reply(&mut filter, &bad_pieces, true, 2, false);
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0], SAFE_GOODBYE[2 % SAFE_GOODBYE.len()]);

    // If goodbye is false: returns empty vector
    let empty_fallback = filter_reply(&mut filter, &bad_pieces, false, 0, false);
    assert!(empty_fallback.is_empty());
}

#[test]
fn test_filter_reply_clean_text_passes_unchanged() {
    let mut filter = ReplyFilter::new(10);

    let clean_pieces = vec![
        "I can certainly help you with that problem.".to_string(),
        "Let's look at the stack trace together.".to_string(),
    ];

    let result = filter_reply(&mut filter, &clean_pieces, false, 0, false);
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
            match filter.check(&sentence, false, false) {
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
    assert_eq!(filter.counters().dropped_ungrounded_memory, 0);
    assert_eq!(filter.counters().dropped_repeat, 0);
}
