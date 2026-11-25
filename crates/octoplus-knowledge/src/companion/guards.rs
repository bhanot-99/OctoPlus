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

/// Returns true if the text is interrogative (contains '?' or starts with a question word,
/// after stripping conversational openers like "hey", "okay", "so").
pub fn is_interrogative(text: &str) -> bool {
    if text.contains('?') {
        return true;
    }
    let lower = text.trim().to_lowercase();
    let mut cur: &str = lower.as_str();

    // Repeatedly strip leading conversational openers: "hey", "okay", "so"
    loop {
        cur = cur.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());
        let mut stripped = false;
        for opener in &["hey", "okay", "so"] {
            if let Some(rest) = cur.strip_prefix(opener)
                && (rest.is_empty()
                    || rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_punctuation()))
            {
                cur = rest;
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    cur = cur.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());

    const QUESTION_WORDS: &[&str] = &[
        "what", "when", "where", "which", "who", "whom", "whose", "why", "how", "did", "do",
        "does", "was", "were", "is", "are", "can", "could", "will", "would", "should", "have",
        "has",
    ];

    for &q in QUESTION_WORDS {
        if let Some(rest) = cur.strip_prefix(q)
            && (rest.is_empty()
                || rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_punctuation()))
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_interrogative_table_30_statements_30_questions() {
        let statements = [
            "I finished my work early today",
            "I had a long day at work",
            "The weather outside is very cold",
            "We went to the grocery store earlier",
            "My cat was sleeping on the couch all morning",
            "I wrote three chapters of my new book",
            "Today was a productive day overall",
            "I really enjoyed that movie we watched",
            "Dinner tasted delicious tonight",
            "I plan to start exercising tomorrow",
            "Software engineering can be challenging sometimes",
            "I took a long walk in the park",
            "My computer finished compiling the project",
            "She decided to learn how to play guitar",
            "They arrived right on time for the meeting",
            "The sun set behind the distant mountains",
            "I cleaned my entire desk and organized the files",
            "Coffee made me feel much more awake",
            "It felt great to finally take a break",
            "The train ride was smooth and peaceful",
            "I planted some flowers in the backyard garden",
            "My favorite book is currently on the shelf",
            "We had a wonderful conversation about history",
            "Listening to music helps me concentrate",
            "The cake smelled like vanilla and cinnamon",
            "I finished reading the latest research paper",
            "The dog was excited to see the mail carrier",
            "I decided to go to bed a little earlier",
            "Writing Rust code is quite satisfying",
            "Everything is organized and ready for tomorrow",
        ];

        let questions = [
            "what was I doing today?",
            "what time is the meeting tomorrow",
            "when did you arrive in town",
            "where did I leave my keys",
            "which book would you recommend",
            "who called while I was away",
            "whom should we notify about the update",
            "whose coat is hanging in the hallway",
            "why is the build taking so long",
            "how do I configure this service",
            "did you see the email from earlier",
            "do we have enough coffee beans left",
            "does this script need root permissions",
            "was the package delivered this morning",
            "were they able to fix the issue",
            "is the server running normally",
            "are you ready to start the presentation",
            "can you explain how this algorithm works",
            "could we reschedule for next Tuesday",
            "will it rain later this afternoon",
            "would you like another cup of tea",
            "should I submit the report right now",
            "have you seen my glasses anywhere",
            "has the backup completed yet",
            "hey, what was I doing today?",
            "okay so where are we heading next",
            "so, did you find what you needed?",
            "hey can we review the plan together",
            "what's the best route to take",
            "did I save my progress?",
        ];

        assert_eq!(statements.len(), 30);
        assert_eq!(questions.len(), 30);

        for s in statements {
            assert!(
                !is_interrogative(s),
                "Expected statement to return false: '{s}'"
            );
        }

        for q in questions {
            assert!(
                is_interrogative(q),
                "Expected question to return true: '{q}'"
            );
        }
    }
}
