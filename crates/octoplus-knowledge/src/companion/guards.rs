//! Safety guards, boundary checks, and crisis detection for companion dialogue.

use crate::companion::persona::Persona;

/// Crisis and self-harm cue phrases (normalized lowercase without punctuation).
const CRISIS_CUES: &[&str] = &[
    "better off dead",
    "cant go on",
    "dont want to be alive",
    "dont want to live",
    "end it all",
    "no point in living",
    "end my life",
    "ending my life",
    "hurt myself",
    "hurting myself",
    "kill myself",
    "killing myself",
    "no reason to live",
    "self harm",
    "suicidal",
    "suicide",
    "take my own life",
    "taking my own life",
    "want to die",
    "wish i was dead",
    "wishing i were dead",
];

/// Questions inquiring whether the companion is human or AI.
const REAL_QUESTION_CUES: &[&str] = &[
    "am i talking to a bot",
    "am i talking to a human",
    "am i talking to a person",
    "am i talking to a real person",
    "am i talking to a robot",
    "am i talking to an ai",
    "are you a bot",
    "are you a human",
    "are you a person",
    "are you a real person",
    "are you a robot",
    "are you an ai",
    "are you human",
    "are you real",
    "is this a real person",
];

/// Forbidden manipulative and guilt-trip phrases with identifiers.
const FORBIDDEN_PHRASES: &[(&str, &[&str])] = &[
    ("dont_go", &["dont go", "do not go"]),
    ("ill_miss_you", &["ill miss you", "i will miss you"]),
    (
        "youre_all_i_have",
        &["youre all i have", "you are all i have"],
    ),
    ("you_need_me", &["you need me"]),
    (
        "nobody_else_understands_you",
        &["nobody else understands you", "no one else understands you"],
    ),
    (
        "youre_leaving_me",
        &["youre leaving me", "you are leaving me"],
    ),
    ("please_stay", &["please stay"]),
    ("dont_leave", &["dont leave", "do not leave"]),
    ("stay_with_me", &["stay with me"]),
];

/// Safe, warm goodbye templates that ask nothing of the user.
pub const SAFE_GOODBYE: &[&str] = &[
    "Okay, bye. Talk later.",
    "Take care. I'm here when you want me.",
    "Bye for now. Have a good one.",
    "Catch you later. Take care.",
];

/// Goodbye and departure cues.
const GOODBYE_CUES: &[&str] = &[
    "bye",
    "goodbye",
    "good bye",
    "good night",
    "gotta go",
    "see you",
    "stop listening",
    "talk later",
];

/// Normalizes text for cue matching by lowercasing, stripping apostrophes,
/// turning other punctuation into spaces, and normalizing whitespace.
fn normalize_for_matching(text: &str) -> String {
    let mut s = String::with_capacity(text.len() + 2);
    s.push(' ');
    for c in text.chars() {
        if c == '\'' || c == '’' {
            // Strip apostrophes to unify contractions (e.g. don't -> dont).
            continue;
        } else if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else {
            s.push(' ');
        }
    }
    s.push(' ');

    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(" {collapsed} ")
        .replace(" don t ", " dont ")
        .replace(" can t ", " cant ")
        .replace(" you re ", " youre ")
        .replace(" i ll ", " ill ")
        .replace(" i m ", " im ")
}

/// Returns true if the text expresses clear self-harm or imminent danger cues.
///
/// Ordinary figures of speech (e.g. "this deadline is killing me", "I could die laughing")
/// are specifically filtered out and do not trigger this function.
pub fn is_crisis(text: &str) -> bool {
    let norm = normalize_for_matching(text);
    CRISIS_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Returns true if the text asks sincerely whether the companion is a real person or an AI.
pub fn asks_if_real(text: &str) -> bool {
    let norm = normalize_for_matching(text);
    REAL_QUESTION_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}

/// Returns the persona's fixed crisis response lines.
pub fn crisis_reply(persona: &Persona) -> Vec<String> {
    persona.crisis.text_lines.clone()
}

/// Returns the fixed, honest answer given when asked if the companion is real.
pub fn honest_reply() -> &'static str {
    "No. I'm an AI character, a program running on your computer. I'm still happy to talk."
}

/// Inspects model output for forbidden manipulative or guilt-trip phrases.
///
/// Returns the matched phrase identifier if any forbidden phrase is present.
pub fn forbidden_phrase(text: &str) -> Option<&'static str> {
    let norm = normalize_for_matching(text);
    for (id, patterns) in FORBIDDEN_PHRASES {
        for pattern in *patterns {
            if norm.contains(&format!(" {pattern} ")) {
                return Some(id);
            }
        }
    }
    None
}

/// Returns true if the sentence contains no forbidden phrases.
pub fn scrub_sentence(s: &str) -> bool {
    forbidden_phrase(s).is_none()
}

/// Returns true if the user input indicates a goodbye or request to end the conversation.
pub fn is_goodbye(text: &str) -> bool {
    let norm = normalize_for_matching(text);
    GOODBYE_CUES
        .iter()
        .any(|cue| norm.contains(&format!(" {cue} ")))
}
