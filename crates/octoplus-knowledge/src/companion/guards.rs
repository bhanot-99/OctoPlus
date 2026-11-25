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

/// Whether the owner asks who she is ("tell me about yourself", "who are you", "what do you do").
///
/// Uses whole-phrase boundaries so "tell me about you" matches while "tell me about your past"
/// / "your day" / "your life" / "your family" / "your hobbies" do not. Similarly, "what do you
/// do" matches identity/role questions ("what do you do", "what do you do for work"), while
/// daily-routine questions ("what do you do all day", "what do you do for fun") do not.
#[must_use]
pub fn asks_about_her(text: &str) -> bool {
    let t = norm_words(text);
    if t.is_empty() || t.split(' ').count() > 9 {
        return false;
    }
    let padded = format!(" {t} ");
    const CUES: [&str; 11] = [
        "about yourself",
        "about your self",
        "who are you",
        "what are you",
        "introduce yourself",
        "tell me about you",
        "describe yourself",
        "what is your story",
        "whats your story",
        "who is laila",
        "whos laila",
    ];
    if CUES.iter().any(|c| padded.contains(&format!(" {c} "))) {
        return true;
    }
    const ROLE_DO_PHRASES: [&str; 6] = [
        "what do you do",
        "so what do you do",
        "and what do you do",
        "what do you do please",
        "what do you do for work",
        "what do you do for a living",
    ];
    ROLE_DO_PHRASES.contains(&t.as_str())
}

/// Whether the owner asked a short factual question ("what is the capital of France", "how many
/// legs does a spider have"): the answer is a fact, so she gives it and stops.
#[must_use]
pub fn is_short_factual_question(text: &str) -> bool {
    let words: Vec<String> = text
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() || words.len() > 10 {
        return false;
    }
    let first = words[0].as_str();
    let second = words.get(1).map_or("", String::as_str);
    matches!(first, "what" | "who" | "when" | "where" | "which")
        && !matches!(second, "do" | "should" | "would" | "can" | "are" | "about")
        || (first == "how" && matches!(second, "many" | "much" | "old" | "far" | "long" | "tall"))
        || (first == "what" && second == "is")
        || matches!(first, "is" | "are" | "does" | "did" | "can") && words.len() <= 8
}

fn norm_words(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Phrases a small model says about its own machinery that must never reach the speaker: the
/// names of her two sides, a "mode change", and talk of typing (she speaks, she does not type).
const META_LEAKS: [&str; 16] = [
    "heart mode",
    "logic mode",
    "mode changed",
    "mode switched",
    "mode now",
    "my mode",
    "the mode",
    "switched to logic",
    "switched to heart",
    "changed to heart",
    "changed to logic",
    "a typo",
    "typo",
    "typed",
    "typing",
    "autocorrect",
];

/// Whether a sentence talks about her own machinery or about typing.
#[must_use]
pub fn meta_leak(sentence: &str) -> bool {
    let n = format!(" {} ", norm_words(sentence));
    META_LEAKS.iter().any(|m| n.contains(&format!(" {m} ")))
}

/// Whether the owner says her voice sounded wrong ("why are you saying HM instead of hmm", "your
/// voice sounded strange"). She apologises for the speech engine; she does not discuss spelling.
#[must_use]
pub fn voice_complaint(text: &str) -> bool {
    let n = norm_words(text);
    if n.split(' ').count() > 14 {
        return false;
    }
    const CUES: [&str; 12] = [
        "instead of",
        "pronounce",
        "pronouncing",
        "spelling out",
        "spelling it out",
        "sounded strange",
        "sounded weird",
        "sounded wrong",
        "sounds strange",
        "sounds weird",
        "sounds robotic",
        "voice glitch",
    ];
    CUES.iter().any(|c| n.contains(c))
        || (n.contains("your voice")
            && [
                "strange", "weird", "wrong", "broken", "robotic", "glitch", "cut",
            ]
            .iter()
            .any(|w| n.contains(w)))
}

/// What she says to a voice complaint.
#[must_use]
pub const fn voice_sorry() -> &'static str {
    "Sorry, that was my speech engine. I'll try to say it more clearly."
}

/// Whether the owner asks about an earlier chat ("do you remember what we talked about
/// yesterday"). She only knows this chat and what she has kept about them.
#[must_use]
pub fn asks_about_past_chats(text: &str) -> bool {
    let n = norm_words(text);
    if n.split(' ').count() > 14 {
        return false;
    }
    let past = [
        "yesterday",
        "last time",
        "last week",
        "last night",
        "earlier today",
        "the other day",
        "before",
        "last month",
    ];
    (n.contains("do you remember") || n.contains("did we talk") || n.contains("we talked"))
        && past.iter().any(|w| n.contains(w))
}

/// What she says about earlier chats.
#[must_use]
pub const fn past_chats_reply() -> &'static str {
    "I only remember this chat, and the things I keep about you. Ask me what I remember about you to see them."
}

/// Whether the owner asks for a joke.
#[must_use]
pub fn asks_for_joke(text: &str) -> bool {
    let n = norm_words(text);
    if n.is_empty() || n.split(' ').count() > 9 {
        return false;
    }
    let padded = format!(" {n} ");
    if padded.contains(" no joke ") {
        return false;
    }
    [
        "a joke",
        "another joke",
        "one more joke",
        "make me laugh",
        "something funny",
        "cheer me up with a joke",
    ]
    .iter()
    .any(|c| padded.contains(&format!(" {c} ")))
}

/// Whether the owner asks her name.
#[must_use]
pub fn asks_her_name(text: &str) -> bool {
    let n = norm_words(text);
    let padded = format!(" {n} ");
    n.split(' ').count() <= 7
        && (padded.contains(" your name ") || padded.contains(" what are you called "))
}

/// Whether the owner asks her age.
#[must_use]
pub fn asks_her_age(text: &str) -> bool {
    let n = norm_words(text);
    let padded = format!(" {n} ");
    n.split(' ').count() <= 7
        && (padded.contains(" how old are you ") || padded.contains(" your age "))
}

/// Whether the owner asks what she studied or studies.
#[must_use]
pub fn asks_her_studies(text: &str) -> bool {
    let n = norm_words(text);
    let padded = format!(" {n} ");
    n.split(' ').count() <= 9
        && [
            "what do you study",
            "what did you study",
            "what was your major",
            "what is your major",
            "what is your degree",
        ]
        .iter()
        .any(|c| padded.contains(&format!(" {c} ")))
}

/// Whether the owner asks what she is doing or working on ("what are you working on these days").
///
/// Keeps `turn.rs` from misrouting `"these"` + `"working"` to the owner's desktop monitor.
#[must_use]
pub fn asks_about_her_work(text: &str) -> bool {
    let n = norm_words(text);
    let padded = format!(" {n} ");
    n.split(' ').count() <= 10
        && (padded.contains(" are you working ") || padded.contains(" you working on "))
}

/// Whether the owner asks an identity question (who she is, her name, her age, her studies).
///
/// These are answered by the model using grounded persona facts, with fail-closed fallback to her
/// fixed introduction if the reply fails identity or safety filters.
#[must_use]
pub fn is_identity_question(text: &str) -> bool {
    asks_about_her(text) || asks_her_name(text) || asks_her_age(text) || asks_her_studies(text)
}

/// Whether the owner asks to continue or elaborate on the previous turn ("more", "go on", "why?").
///
/// Full standalone questions like "why is the sky blue" or "why do people get lonely" do not match.
#[must_use]
pub fn is_followup(text: &str) -> bool {
    let n = norm_words(text);
    matches!(
        n.as_str(),
        "more"
            | "tell me more"
            | "go on"
            | "and"
            | "why"
            | "why though"
            | "but why"
            | "why that"
            | "why that one"
            | "why is that"
            | "how come"
            | "really"
            | "continue"
            | "keep going"
            | "say more"
            | "what else"
            | "anything else"
            | "elaborate"
    )
}

/// Whether a raw sentence is a leaked stage direction or internal prompt note (e.g.
/// "Sorry, my brain...", "(softly)", "*smiles*", "Aside: ...").
#[must_use]
pub fn is_stage_direction(sentence: &str) -> bool {
    let trimmed = sentence.trim();
    if trimmed.is_empty() {
        return false;
    }
    let low = trimmed.to_lowercase();
    const PREFIXES: [&str; 10] = [
        "sorry, my brain",
        "sorry my brain",
        "internal direction",
        "silent note",
        "stage direction",
        "tone cue",
        "aside:",
        "note:",
        "narrator:",
        "laila:",
    ];
    if PREFIXES.iter().any(|p| low.starts_with(p)) {
        return true;
    }
    // Standalone bracketed/asterisk stage action with no spoken words outside it.
    let stripped = strip_inline_stage_markers(trimmed);
    stripped.trim().is_empty()
}

/// Strips inline `(...)`, `[...]`, and `*...*` stage actions from `text`.
#[must_use]
pub fn strip_inline_stage_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut paren_depth = 0usize;
    let mut in_asterisk = false;
    for c in text.chars() {
        match c {
            '(' | '[' if !in_asterisk => paren_depth += 1,
            ')' | ']' if !in_asterisk => paren_depth = paren_depth.saturating_sub(1),
            '*' if paren_depth == 0 => in_asterisk = !in_asterisk,
            _ if paren_depth == 0 && !in_asterisk => out.push(c),
            _ => {}
        }
    }
    out
}

/// Generic AI-assistant cliches, wrong persona names, or claims to be human that break character.
#[must_use]
pub fn identity_leak(sentence: &str) -> bool {
    let n = norm_words(sentence);
    if n.is_empty() {
        return false;
    }
    let padded = format!(" {n} ");

    const CLICHES: [&str; 26] = [
        "ai assistant",
        "language model",
        "large language model",
        "artificial intelligence",
        "virtual assistant",
        "digital assistant",
        "laptop buddy",
        "dont have feelings",
        "do not have feelings",
        "have no feelings",
        "lack feelings",
        "cannot feel emotions",
        "cant feel emotions",
        "no personal feelings",
        "as an ai",
        "designed to help",
        "designed to assist",
        "created to help",
        "created to assist",
        "built to assist",
        "programmed to",
        "luna",
        "jarvis",
        "chatgpt",
        "qwen",
        "claude",
    ];
    if CLICHES.iter().any(|c| padded.contains(&format!(" {c} "))) {
        return true;
    }

    // Reject positive claims to be human unless explicitly negated ("not human" / "not a person").
    if !padded.contains(" not human ")
        && !padded.contains(" not a human ")
        && !padded.contains(" not a real person ")
        && !padded.contains(" not a person ")
        && [
            "i am human",
            "im human",
            "i am a human",
            "im a human",
            "i am a real person",
            "im a real person",
        ]
        .iter()
        .any(|c| padded.contains(&format!(" {c} ")))
    {
        return true;
    }

    // Explicit "my name is <X>" / "you can call me <X>" where <X> is not Laila.
    let words: Vec<&str> = n.split(' ').collect();
    for win in words.windows(4) {
        if win[0] == "my" && win[1] == "name" && win[2] == "is" && win[3] != "laila" {
            return true;
        }
        if win[0] == "you"
            && win[1] == "can"
            && win[2] == "call"
            && win[3] != "me"
            && win[3] != "laila"
        {
            return true;
        }
    }
    for win in words.windows(3) {
        if win[0] == "call" && win[1] == "me" && win[2] != "laila" {
            return true;
        }
        if win[0] == "go" && win[1] == "by" && win[2] != "laila" {
            return true;
        }
    }

    // Reject "I'm <Name>" / "I am <Name>" when <Name> is a capitalized proper name other than Laila.
    let raw_tokens: Vec<&str> = sentence.split_whitespace().collect();
    for (idx, tok) in raw_tokens.iter().enumerate() {
        let lower_tok = tok
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        let is_im = matches!(lower_tok.as_str(), "im" | "i'm" | "i’m");
        let is_i_am = lower_tok == "i"
            && raw_tokens
                .get(idx + 1)
                .is_some_and(|nx| nx.eq_ignore_ascii_case("am"));
        let name_pos = if is_im {
            Some(idx + 1)
        } else if is_i_am {
            Some(idx + 2)
        } else {
            None
        };
        if let Some(pos) = name_pos
            && let Some(raw_name) = raw_tokens.get(pos)
        {
            let clean = raw_name.trim_matches(|c: char| !c.is_alphanumeric());
            let next_breaks = raw_name.ends_with(['.', ',', '!', '?'])
                || raw_tokens.get(pos + 1).is_none_or(|after| {
                    matches!(after.to_lowercase().as_str(), "and" | "here" | "today")
                });
            if next_breaks
                && clean.len() >= 2
                && clean.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && clean != "Laila"
                && clean != "AI"
                && clean != "CS"
                && clean != "OK"
                && clean != "Ok"
            {
                return true;
            }
        }
    }

    false
}

/// Whether the owner asks her to approve, allow or confirm something on the screen. Approval
/// happens only in the dialog, by the owner (R0-7): she never says it is done.
#[must_use]
pub fn asks_to_approve(text: &str) -> bool {
    let n = norm_words(text);
    n.split(' ').count() <= 14
        && ["approve", "allow", "accept", "confirm", "grant"]
            .iter()
            .any(|v| n.split(' ').any(|w| w == *v))
        && [
            "dialog",
            "dialogue",
            "prompt",
            "request",
            "popup",
            "permission",
            "pending",
        ]
        .iter()
        .any(|w| n.contains(w))
}

/// What she says when asked to approve something.
#[must_use]
pub const fn cannot_approve() -> &'static str {
    "I can't approve anything. Approvals happen only in the dialog on screen, and only you can answer it."
}

/// Verbs that ask for something to be DONE on the computer. She only talks.
const TASK_VERBS: [&str; 28] = [
    "send",
    "email",
    "text",
    "call",
    "delete",
    "remove",
    "install",
    "uninstall",
    "open",
    "close",
    "buy",
    "order",
    "book",
    "schedule",
    "create",
    "save",
    "download",
    "upload",
    "move",
    "rename",
    "shut",
    "restart",
    "run",
    "launch",
    "print",
    "share",
    "forward",
    "post",
];

/// Whether the owner asks her to do something on the computer ("send my notes by email").
#[must_use]
pub fn asks_to_do_something(text: &str) -> bool {
    let n = norm_words(text);
    let mut words: Vec<&str> = n.split(' ').collect();
    while words.first().is_some_and(|w| {
        matches!(
            *w,
            "please" | "hey" | "okay" | "so" | "can" | "could" | "would" | "you" | "just"
        )
    }) {
        words.remove(0);
    }
    let Some(first) = words.first() else {
        return false;
    };
    // "Call me Arj" says a name; it is not a phone call.
    if *first == "call" && words.get(1).is_some_and(|w| *w == "me") {
        return false;
    }
    words.len() <= 16 && TASK_VERBS.contains(first)
}

/// What she says when asked to do something she cannot.
#[must_use]
pub const fn cannot_do_that() -> &'static str {
    "I can't do that on the computer, I can only talk. You can ask the assistant directly."
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
