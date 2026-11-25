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

#[test]
fn questions_about_her_and_short_factual_questions_are_recognised() {
    for t in [
        "tell me about yourself",
        "Tell me about yourself.",
        "tell me about you",
        "Tell me about you.",
        "who are you",
        "introduce yourself",
        "describe yourself please",
        "what do you do",
        "What do you do?",
        "what do you do for work",
    ] {
        assert!(
            octoplus_knowledge::companion::guards::asks_about_her(t),
            "expected asks_about_her to fire for: {t}"
        );
    }
    for t in [
        "tell me about your past",
        "tell me about your day",
        "tell me about your life",
        "tell me about your family",
        "tell me about your hobbies",
        "what do you do all day",
        "what do you do for fun",
        "what do you do when you are sad",
        "tell me about the weather",
        "what is the capital of France",
        "I was telling my sister about yourself and the long story of how it all started last year",
        "how are you",
    ] {
        assert!(
            !octoplus_knowledge::companion::guards::asks_about_her(t),
            "expected asks_about_her NOT to fire for: {t}"
        );
    }
    for t in [
        "what is the capital of France",
        "how many legs does a spider have",
        "who wrote hamlet",
        "when did the war end",
        "is the earth round",
        "what is two plus two",
    ] {
        assert!(
            octoplus_knowledge::companion::guards::is_short_factual_question(t),
            "{t}"
        );
    }
    for t in [
        "what do you think about love",
        "how are you feeling today",
        "tell me a story",
        "I had a long day at work and I just want to talk about it for a while",
        "what should I do about my boss",
    ] {
        assert!(
            !octoplus_knowledge::companion::guards::is_short_factual_question(t),
            "{t}"
        );
    }
}

#[test]
fn machinery_and_typing_talk_is_recognised_and_ordinary_talk_is_not() {
    use octoplus_knowledge::companion::guards::meta_leak;
    for s in [
        "I'm in heart mode now, softer and slower.",
        "Mode changed to logic: clear, step-by-step.",
        "That was a typo from a quick thought.",
        "I accidentally typed HM instead of hmm.",
        "Sorry, I was typing too fast.",
        "Mode switched to logic.",
    ] {
        assert!(meta_leak(s), "{s}");
    }
    for s in [
        "Paris is the capital of France.",
        "I like the way the heart of the city feels at night.",
        "My head says be precise, my heart says be present.",
        "The model of the car is old.",
        "She is a logical person.",
        "That sounds like a good plan.",
    ] {
        assert!(!meta_leak(s), "{s}");
    }
}

#[test]
fn voice_complaints_past_chat_questions_and_joke_requests_are_recognised() {
    use octoplus_knowledge::companion::guards::{
        asks_about_past_chats, asks_for_joke, voice_complaint,
    };
    for t in [
        "why are you saying HM instead of hmm",
        "your voice sounded strange just now",
        "you are pronouncing that wrong",
        "why are you spelling out letters",
        "your voice is weird today",
    ] {
        assert!(voice_complaint(t), "{t}");
    }
    for t in [
        "I like your voice",
        "what is the capital of France",
        "tell me a story about a strange voice in the forest that kept calling out to travellers at night",
        "instead of tea I want coffee but that is a very long sentence about my preferences today",
    ] {
        assert!(!voice_complaint(t), "{t}");
    }
    for t in [
        "do you remember what we talked about yesterday",
        "do you remember what I said last time",
        "did we talk about this before",
    ] {
        assert!(asks_about_past_chats(t), "{t}");
    }
    for t in [
        "do you remember how to spell necessary",
        "what did we just say",
        "hello",
    ] {
        assert!(!asks_about_past_chats(t), "{t}");
    }
    for t in [
        "tell me a joke",
        "make me laugh",
        "say something funny",
        "got a joke",
    ] {
        assert!(asks_for_joke(t), "{t}");
    }
    for t in [
        "what is the capital of France",
        "this is no joke at all and I mean it seriously",
    ] {
        assert!(!asks_for_joke(t), "{t}");
    }
}
