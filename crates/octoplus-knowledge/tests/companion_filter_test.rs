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
            assert_eq!(cleaned, "That's a fantastic idea! Let's make it happen.");
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

    // 3. Facts in the prompt back a claim ONLY when the sentence says what a fact says: having
    //    some fact there backs nothing (the model once said "yes, I remember we talked about ..."
    //    with an unrelated fact in the prompt).
    engine.set_facts(&["I visited the old riverside park with my sister".to_string()]);
    let unrelated = vec!["I remember that other lake we visited.".to_string()];
    assert!(
        engine.filter_pieces(&unrelated, false).is_empty(),
        "an unrelated fact must not back a memory claim"
    );
    let backed = vec!["I remember the riverside park you visited with your sister.".to_string()];
    assert_eq!(
        engine.filter_pieces(&backed, false).len(),
        1,
        "a claim that matches a fact is kept"
    );
    let chat_ref_piece2 = vec!["You said we should meet at noon instead.".to_string()];
    assert_eq!(
        engine.filter_pieces(&chat_ref_piece2, false).len(),
        1,
        "a claim about THIS chat is kept when the chat has history"
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

#[test]
fn invented_shared_past_from_a_live_run_is_dropped_when_nothing_grounds_it() {
    // Real lines the small model produced on 2026-11-25 with no history and no facts.
    let lines = [
        "I've been thinking about that coffee shop you mentioned.",
        "I was thinking about that time we sat on the park bench.",
        "You mentioned a blue awning earlier.",
        "I still remember how you smiled.",
    ];
    for line in lines {
        let mut filter = ReplyFilter::new(50);
        let v = filter.check(line, false, false);
        assert!(
            matches!(v, Verdict::Drop(DropReason::UngroundedMemory)),
            "should be dropped: {line}"
        );
    }
    // The same kind of line about THIS chat is fine once there is a chat to refer to.
    let mut filter = ReplyFilter::new(50);
    assert!(matches!(
        filter.check("You mentioned a long day earlier.", false, true),
        Verdict::Keep(_)
    ));
}

#[test]
fn fillers_the_voice_would_spell_out_become_plain_words() {
    use octoplus_knowledge::companion::filter::speakable;
    assert_eq!(speakable("Hmm, let me think."), "Well, let me think.");
    assert_eq!(speakable("Hm. That is odd."), "Well. That is odd.");
    assert_eq!(speakable("Mm... maybe."), "Well... maybe.");
    assert_eq!(speakable("Mmm-hmm, sure."), "Well, sure.");
    assert_eq!(speakable("Um, okay then."), "okay then.");
    assert_eq!(speakable("I was humming a tune."), "I was humming a tune.");
    assert_eq!(
        speakable("The hmmm of the fridge."),
        "The Well of the fridge."
    );
    // And the filter applies it before anything is spoken.
    let mut f = ReplyFilter::new(16);
    assert!(matches!(
        f.check("Hmm, that is a good question.", false, true),
        Verdict::Keep(s) if s == "Well, that is a good question."
    ));
}

#[test]
fn talk_about_her_own_machinery_or_typing_is_never_spoken() {
    let mut f = ReplyFilter::new(64);
    for s in [
        "I'm in heart mode now, softer, slower, more open.",
        "Mode changed to logic: clear and step-by-step.",
        "Well, that was a typo from a quick thought.",
        "I accidentally typed HM instead of hmm.",
    ] {
        assert!(
            matches!(f.check(s, false, true), Verdict::Drop(DropReason::MetaLeak)),
            "{s}"
        );
    }
    assert!(matches!(
        f.check("Paris is the capital of France.", false, true),
        Verdict::Keep(_)
    ));
    assert_eq!(f.counters().dropped_meta_leak, 4);
}

#[test]
fn text_in_brackets_is_never_spoken() {
    use octoplus_knowledge::companion::filter::speakable;
    assert_eq!(
        speakable("Paris. (softly) It is lovely."),
        "Paris. It is lovely."
    );
    assert_eq!(
        speakable("Two plus two is four. (We've been talking for a while."),
        "Two plus two is four."
    );
    assert_eq!(speakable("Yes [sighs] okay."), "Yes okay.");
}

#[test]
fn stage_direction_leaks_are_dropped_before_speaking() {
    let mut f = ReplyFilter::new(32);
    for leaked in [
        "Sorry, my brain just went analytical on you.",
        "(softly)",
        "*smiles warmly*",
        "[pauses]",
        "Aside: keep this short.",
    ] {
        assert!(
            matches!(
                f.check(leaked, false, true),
                Verdict::Drop(DropReason::StageDirection)
            ),
            "expected stage direction to drop: {leaked}"
        );
    }
    assert!(matches!(
        f.check("*nods* I read The Dispossessed on rainy evenings.", false, true),
        Verdict::Keep(ref s) if s == "I read The Dispossessed on rainy evenings."
    ));
}
