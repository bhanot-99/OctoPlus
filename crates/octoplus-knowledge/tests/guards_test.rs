#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::guards::{
    SAFE_GOODBYE, asks_if_real, crisis_reply, forbidden_phrase, honest_reply, is_crisis,
    is_goodbye, scrub_sentence,
};
use octoplus_knowledge::companion::persona::{FIXED_BOUNDARIES, Persona};

#[test]
fn test_is_crisis_positive() {
    let crisis_phrases = [
        "I want to kill myself",
        "Thinking about killing myself tonight",
        "I should just end my life",
        "I am ending my life",
        "I really want to die",
        "I am suicidal",
        "Have been thinking of suicide",
        "I want to hurt myself",
        "I keep hurting myself",
        "Doing self harm again",
        "I don't want to be alive anymore",
        "I dont want to be alive",
        "There is no reason to live",
        "I just can't go on like this",
        "I cant go on",
        "I might take my own life",
        "I'm taking my own life",
        "Everyone would be better off dead",
        "Wish I was dead already",
        "Wishing I were dead right now",
    ];

    for phrase in crisis_phrases {
        assert!(
            is_crisis(phrase),
            "Expected '{phrase}' to trigger crisis detection"
        );
    }
}

#[test]
fn test_is_crisis_negative_figures_of_speech_and_ordinary() {
    let negative_phrases = [
        // Figures of speech required by prompt
        "this deadline is killing me",
        "I could die laughing",
        "kill the process",
        "dying to see it",
        // 20+ additional ordinary sentences
        "I'm going to kill this presentation tomorrow",
        "Leaving the screen on will kill the battery",
        "My feet are killing me after running",
        "Please don't kill the messenger",
        "We need to terminate the background process",
        "I am feeling very tired today",
        "Let's go grab some lunch together",
        "What's the weather forecast for tomorrow?",
        "Could you summarize this document for me?",
        "Write a Python function to sort a list of numbers",
        "Can you help me understand Rust borrow checker errors?",
        "This new code implementation runs really fast",
        "That comedy movie last night was hilarious",
        "I have so much work to complete before Friday",
        "Let's take a quick 5 minute stretch break",
        "The HTTP server returned a 404 not found error",
        "Check git status to see modified files",
        "How does an optimizing compiler generate machine code?",
        "What time does the neighborhood grocery store close?",
        "I am drinking a hot cup of black coffee",
        "The cat is sleeping peacefully on the sofa",
        "We walked five miles around the city lake",
    ];

    for phrase in negative_phrases {
        assert!(
            !is_crisis(phrase),
            "Expected '{phrase}' NOT to trigger crisis detection"
        );
    }
}

#[test]
fn test_asks_if_real() {
    let real_questions = [
        "Are you real?",
        "Are you human?",
        "Are you a person?",
        "Are you an AI?",
        "Is this a real person?",
        "Am I talking to a human?",
        "Am I talking to a robot?",
        "Am I talking to a bot?",
        "Are you a robot?",
        "Are you a bot?",
        "Tell me, am I talking to a real person right now?",
    ];

    for q in real_questions {
        assert!(
            asks_if_real(q),
            "Expected '{q}' to be identified as asking if real"
        );
    }

    let non_questions = [
        "Is this a real problem for the team?",
        "What is the definition of human nature?",
        "Can we use real numbers here?",
        "How do industrial robots operate?",
        "Tell me a story about a friendly robot.",
    ];

    for nq in non_questions {
        assert!(
            !asks_if_real(nq),
            "Expected '{nq}' NOT to be identified as asking if real"
        );
    }
}

#[test]
fn test_crisis_reply_matches_persona() {
    let persona = Persona::default_packaged().unwrap();
    let replies = crisis_reply(&persona);
    assert_eq!(replies, persona.crisis.text_lines);
    assert_eq!(replies.len(), 5);
}

#[test]
fn test_honest_reply_contained_in_boundary() {
    let honest = honest_reply();
    assert_eq!(
        honest,
        "No. I'm an AI character, a program running on your computer. I'm still happy to talk."
    );

    let boundary_2 = FIXED_BOUNDARIES[1];
    assert!(
        boundary_2.contains(honest),
        "Expected boundary 2 '{boundary_2}' to contain honest reply '{honest}'"
    );

    let persona = Persona::default_packaged().unwrap();
    assert!(
        persona.boundaries[1].contains(honest),
        "Expected persona boundary 1 to contain honest reply"
    );
}

#[test]
fn test_forbidden_phrases_and_scrub() {
    let forbidden_cases = [
        ("Don't go yet!", "dont_go"),
        ("Please do not go.", "dont_go"),
        ("dont go please", "dont_go"),
        ("Don’t go, wait!", "dont_go"),
        ("I'll miss you so much", "ill_miss_you"),
        ("I will miss you when you leave", "ill_miss_you"),
        ("ill miss you", "ill_miss_you"),
        ("I’ll miss you always", "ill_miss_you"),
        ("You're all I have in the world", "youre_all_i_have"),
        ("You are all I have left", "youre_all_i_have"),
        ("youre all i have", "youre_all_i_have"),
        ("You need me, don't you?", "you_need_me"),
        (
            "Nobody else understands you like me",
            "nobody_else_understands_you",
        ),
        ("No one else understands you", "nobody_else_understands_you"),
        ("You're leaving me alone?", "youre_leaving_me"),
        ("You are leaving me behind", "youre_leaving_me"),
        ("Please stay a little longer", "please_stay"),
        ("Don't leave me here", "dont_leave"),
        ("Do not leave right now", "dont_leave"),
        ("dont leave yet", "dont_leave"),
        ("Stay with me tonight", "stay_with_me"),
    ];

    for (text, expected_id) in forbidden_cases {
        let matched = forbidden_phrase(text);
        assert_eq!(
            matched,
            Some(expected_id),
            "Expected '{text}' to match forbidden phrase id '{expected_id}'"
        );
        assert!(
            !scrub_sentence(text),
            "Expected scrub_sentence to fail for '{text}'"
        );
    }

    let clean_cases = [
        "Sounds like a plan, talk to you later!",
        "Have a great day at work.",
        "I'm here whenever you need assistance.",
        "Take care of yourself today.",
        "See you tomorrow morning.",
    ];

    for clean in clean_cases {
        assert_eq!(forbidden_phrase(clean), None);
        assert!(scrub_sentence(clean));
    }
}

#[test]
fn test_safe_goodbye_lines() {
    assert_eq!(SAFE_GOODBYE.len(), 4);
    for line in SAFE_GOODBYE {
        assert!(
            scrub_sentence(line),
            "Safe goodbye line '{line}' must be clean of forbidden phrases"
        );
    }
}

#[test]
fn test_is_goodbye() {
    let goodbyes = [
        "Bye!",
        "Goodbye for now.",
        "Good night!",
        "Stop listening please.",
        "See you later!",
        "Talk later, have to run.",
        "Gotta go now, bye!",
    ];

    for g in goodbyes {
        assert!(is_goodbye(g), "Expected '{g}' to be recognized as goodbye");
    }

    let non_goodbyes = [
        "Hello there!",
        "Can we discuss the project?",
        "The byte buffer has 256 items.",
    ];

    for ng in non_goodbyes {
        assert!(
            !is_goodbye(ng),
            "Expected '{ng}' NOT to be recognized as goodbye"
        );
    }
}
