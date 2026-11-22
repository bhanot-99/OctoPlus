//! Turn router for Semantic-NeuroOS (C5).
//!
//! Implements Architecture.md §11, FR-KNO-03, and KPI-13.
//! Pure, deterministic classification of user turns into one of four routes:
//! 1. [`RouteKind::NudgeReply`]
//! 2. [`RouteKind::Action`]
//! 3. [`RouteKind::Desktop`]
//! 4. [`RouteKind::General`]
//!
//! The model never chooses a route (AB-13).

/// Route kinds evaluated deterministically by the turn router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    /// Short reply to an active nudge prompt.
    NudgeReply,
    /// Imperative skill intent to be executed.
    Action,
    /// Query grounded in the user's screen activity, history, or retrieved context.
    Desktop,
    /// General conversational query answered from model knowledge or general facts.
    General,
}

impl RouteKind {
    /// Returns the static string representation of this route kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NudgeReply => "nudge_reply",
            Self::Action => "action",
            Self::Desktop => "desktop",
            Self::General => "general",
        }
    }
}

/// Skill intent hint accompanying an [`RouteKind::Action`] decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionHint {
    Pause,
    Forget,
    Remember,
    ForgetFact,
    Cleanup,
    Search,
    Fetch,
    AppendNote,
    NotesSearch,
    CalendarRead,
    DiskReport,
}

/// Input parameters for turn routing.
#[derive(Debug, Clone)]
pub struct RouteInput<'a> {
    pub text: &'a str,
    pub nudge_window_open: bool,
    pub retrieval_best_score: Option<f64>,
    pub cutoff: f64,
}

/// Routing decision produced by [`route`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    pub route: RouteKind,
    pub deictic: bool,
    pub time_ref: bool,
    pub action_hint: Option<ActionHint>,
}

/// Grouped pattern data for deterministic turn routing.
#[derive(Debug)]
pub struct RouterPatterns {
    pub nudge_replies: &'static [&'static str],
    pub request_prefixes: &'static [&'static str],
    pub deictic_questions: &'static [&'static str],
    pub deictic_words: &'static [&'static str],
    pub activity_cues: &'static [&'static str],
    pub time_refs: &'static [&'static str],
    pub time_app_words: &'static [&'static str],
}

/// Master constant table containing all routing word and phrase patterns.
pub const PATTERNS: RouterPatterns = RouterPatterns {
    nudge_replies: &[
        "mute that one",
        "mute this one",
        "mute it",
        "silence it",
        "give me a minute",
        "give me a moment",
        "give me a second",
        "can t right now",
        "can t now",
        "yes",
        "yeah",
        "yep",
        "yup",
        "sure",
        "ok",
        "okay",
        "no",
        "nope",
        "nah",
        "not now",
        "stop",
        "stop it",
        "stop telling me",
        "stop telling me about this",
        "show me",
        "okay show me",
        "ok show me",
        "later",
        "maybe later",
        "snooze",
        "dismiss",
        "go away",
        "cancel",
        "never mind",
        "nevermind",
        "remind me later",
        "tell me more",
        "thanks",
        "thank you",
        "got it",
        "do it",
        "sounds good",
        "please do",
        "yes please",
        "no thanks",
        "no thank you",
        "sure thing",
        "dont show this again",
        "don t show this again",
        "dont show again",
        "don t show again",
        "please stop",
        "ok thanks",
        "okay thanks",
        "please show me",
    ],
    request_prefixes: &[
        "i was wondering if you could please",
        "i was wondering if you could",
        "i wonder if you could",
        "i was hoping you could",
        "would it be possible to",
        "is it possible to",
        "go ahead and",
        "would you mind",
        "do you mind",
        "please to",
        "you can",
        "you may",
        "i d like to",
        "i would like to",
        "i want to",
        "i need to",
        "hey",
        "ok so",
        "okay so",
        "yeah",
        "so",
        "now",
        "also",
        "can you please tell me",
        "could you please tell me",
        "can you tell me",
        "could you tell me",
        "can you please",
        "could you please",
        "would you please",
        "will you please",
        "i would like you to",
        "i d like you to",
        "id like you to",
        "i want you to",
        "i need you to",
        "can you",
        "could you",
        "would you",
        "will you",
        "please tell me",
        "tell me",
        "please",
        "just",
    ],
    deictic_questions: &[
        "what is on my screen",
        "what s on my screen",
        "what is on screen",
        "what s on screen",
        "what is this",
        "what was this",
        "what s this",
        "what is that",
        "what was that",
        "what s that",
        "what are these",
        "what were these",
        "what are those",
        "what were those",
        "what is it",
        "what was it",
        "what s it",
    ],
    deictic_words: &[
        "this",
        "that",
        "it",
        "these",
        "those",
        "here",
        "what i was doing",
        "what was i doing",
        "what i was working on",
        "what was i working on",
        "what i was looking at",
        "what was i looking at",
        "what i was reading",
        "what was i reading",
        "what am i looking at",
        "what am i doing",
        "what am i working on",
        "what am i reading",
        "what i just looked at",
        "what did i just",
        "what i just read",
        "what i just opened",
        "what i just did",
        "what i just closed",
        "what did i just read",
        "what did i just do",
        "what did i just open",
        "what did i just look at",
    ],
    activity_cues: &[
        "doing",
        "working",
        "looking",
        "reading",
        "open",
        "window",
        "app",
        "screen",
        "tab",
        "file",
        "writing",
        "watching",
        "browsing",
        "running",
        "editing",
        "using",
        "spent",
        "time",
        "error",
        "terminal",
        "browser",
        "editor",
        "document",
        "page",
        "video",
        "message",
        "build",
        "code",
        "email",
        "mail",
        "playing",
        "listening",
        "happened",
    ],
    time_refs: &[
        "today",
        "yesterday",
        "earlier",
        "last hour",
        "this morning",
        "this afternoon",
        "this evening",
        "a minute ago",
        "just now",
        "last night",
        "this week",
        "last week",
        "an hour ago",
        "minutes ago",
        "hours ago",
        "past hour",
        "recently",
        "a while ago",
        "a moment ago",
        "half an hour ago",
        "ago",
        "last time",
    ],
    time_app_words: &[
        "today",
        "yesterday",
        "earlier",
        "hour",
        "hours",
        "minute",
        "minutes",
        "morning",
        "afternoon",
        "night",
        "week",
        "weeks",
        "month",
        "months",
        "session",
        "day",
        "days",
        "history",
        "activity",
        "ago",
        "chrome",
        "firefox",
        "browser",
        "terminal",
        "slack",
        "code",
        "app",
        "window",
        "tab",
        "screen",
    ],
};

/// Normalises input text: lower-case, punctuation folded to single spaces, trimmed.
#[must_use]
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            for lc in c.to_lowercase() {
                out.push(lc);
            }
        } else {
            pending_space = true;
        }
    }
    out
}

/// Checks if `text` contains `phrase` with word boundaries.
///
/// Assumes `text` is already normalised.
#[must_use]
pub fn contains_word(text: &str, phrase: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(phrase) {
        let idx = start + pos;
        let before_ok = idx == 0 || text.as_bytes().get(idx - 1) == Some(&b' ');
        let after_idx = idx + phrase.len();
        let after_ok = after_idx == text.len() || text.as_bytes().get(after_idx) == Some(&b' ');
        if before_ok && after_ok {
            return true;
        }
        start = idx + 1;
    }
    false
}

/// Checks if `text` starts with `phrase` at a word boundary.
///
/// Returns the remainder of `text` after `phrase` (trimmed), if matched.
#[must_use]
pub fn strip_prefix_word<'a>(text: &'a str, phrase: &str) -> Option<&'a str> {
    if text == phrase {
        Some("")
    } else if text.starts_with(phrase) && text.as_bytes().get(phrase.len()) == Some(&b' ') {
        Some(&text[phrase.len() + 1..])
    } else {
        None
    }
}

/// First words that start a short answer to a nudge.
const REPLY_STARTERS: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "no",
    "nope",
    "nah",
    "later",
    "stop",
    "dismiss",
    "snooze",
    "cancel",
    "never",
    "not now",
    "not right now",
    "show me",
    "go ahead",
    "remind me",
    "don t",
    "dont",
    "do not",
    "thanks",
    "thank you",
    "got it",
    "do it",
    "please",
    "maybe",
    "fine",
    "alright",
    "not at the moment",
    "not today",
    "not yet",
    "skip",
    "hold off",
    "i ll",
    "i will",
    "leave it",
    "leave that",
    "not interested",
    "absolutely",
    "of course",
    "that s fine",
    "i m fine",
    "no need",
    "ignore",
    "sounds good",
    "sounds great",
    "ask me",
    "i don t care",
    "i do not care",
    "that would be",
    "that s helpful",
    "yes please",
];

/// Phrases that mean "stop telling me", wherever they sit in a reply.
const STOP_PHRASES: &[&str] = &[
    "don t show",
    "dont show",
    "stop showing",
    "stop telling",
    "stop reminding",
    "stop notifying",
    "disable this",
    "turn this off",
    "switch this off",
    "mute this",
    "never show",
    "never tell me",
    "ask me again",
    "remind me again",
    "turn off this",
    "switch off this",
    "no more of this",
    "i do not want to see",
    "i don t want to see",
    "never showed me",
    "don t want to receive",
    "do not want to receive",
    "do this later",
    "do it later",
    "some other time",
    "another time",
    "never show me",
    "not show me",
    "no longer show",
    "no more alerts",
    "do not want this",
    "don t want this",
];

/// Longest reply, in words, that is still taken as an answer to a nudge and not a new question.
const MAX_REPLY_WORDS: usize = 6;

/// Longest reply that opens with a reply word ("yes please that would be really helpful").
const MAX_STARTER_REPLY_WORDS: usize = 24;

/// Words that, right after a reply starter, begin a new question ("no, what is rust").
const QUESTION_STARTERS: &[&str] = &[
    "what", "how", "why", "who", "when", "where", "which", "can", "could", "explain", "is", "are",
    "does", "do you",
];

/// Openings of a new request, not an answer to a nudge.
const NEW_REQUEST_STARTERS: &[&str] = &[
    "tell me a",
    "tell me about",
    "give me",
    "write",
    "list",
    "translate",
    "calculate",
    "define",
    "recommend",
    "suggest",
    "play",
    "explain",
    "describe",
    "summarise",
    "summarize",
    "compare",
    "find",
    "search",
    "look",
    "open",
    "add",
    "remember",
    "forget",
    "delete",
    "pause",
];

/// "yeah sure go ahead because ... show me what is using space": a clear yes first.
fn starts_with_strong_yes(norm: &str) -> bool {
    [
        "yes",
        "yeah",
        "yep",
        "sure",
        "ok",
        "okay",
        "go ahead",
        "please do",
        "no",
        "nope",
        "nah",
        "don t worry",
    ]
    .iter()
    .any(|w| strip_prefix_word(norm, w).is_some())
}

/// Checks if `norm` is a short answer to a nudge: a known phrase, or a short text that starts
/// with a reply word and does not turn into a new question or a skill request.
#[must_use]
pub fn is_nudge_reply(norm: &str) -> bool {
    if PATTERNS.nudge_replies.contains(&norm) {
        return true;
    }
    // "can you snooze this until tonight": a polite wrapper around a short reply.
    for pre in [
        "can you",
        "could you",
        "please",
        "would you",
        "would you mind",
    ] {
        if let Some(rest) = strip_prefix_word(norm, pre)
            && [
                "snooze",
                "dismiss",
                "mute",
                "stop",
                "ignore",
                "skip",
                "remind me",
                "reminding me",
            ]
            .iter()
            .any(|v| strip_prefix_word(rest, v).is_some())
        {
            return true;
        }
    }
    let words = norm.split(' ').count();
    if words <= MAX_STARTER_REPLY_WORDS
        && detect_action(norm).is_none()
        && STOP_PHRASES.iter().any(|p| contains_word(norm, p))
    {
        return true;
    }
    if words > MAX_STARTER_REPLY_WORDS || detect_action(norm).is_some() {
        return false;
    }
    if words > MAX_REPLY_WORDS {
        // Longer than a short statement: only a text that opens with a reply word and does not
        // turn into a question or a new request is still an answer.
        return REPLY_STARTERS
            .iter()
            .find_map(|st| strip_prefix_word(norm, st))
            .is_some_and(|rest| {
                rest.starts_with("why not")
                    || !QUESTION_STARTERS
                        .iter()
                        .chain(NEW_REQUEST_STARTERS)
                        .any(|q| strip_prefix_word(rest, q).is_some())
                        && (!detect_deictic(norm) || starts_with_strong_yes(norm))
            });
    }
    // Inside a nudge window a short statement that is not a question, a request for something
    // new or a screen question is most likely an answer to the nudge, whatever its wording.
    let starts_a_new_turn = QUESTION_STARTERS
        .iter()
        .chain(NEW_REQUEST_STARTERS)
        .any(|q| strip_prefix_word(norm, q).is_some());
    if !starts_a_new_turn && norm.split(' ').count() < MAX_REPLY_WORDS && !detect_deictic(norm) {
        return true;
    }
    REPLY_STARTERS
        .iter()
        .find_map(|st| strip_prefix_word(norm, st))
        .is_some_and(|rest| {
            // "show me what is using it" asks to be shown; only "no, what is rust" is new.
            norm.starts_with("show me")
                || rest.starts_with("why not")
                || !QUESTION_STARTERS
                    .iter()
                    .any(|q| strip_prefix_word(rest, q).is_some())
        })
}

/// Verbs that remove stored things, strong ones may mean a single fact ("forget that").
const FORGET_STRONG: &[&str] = &["forget", "delete", "erase", "wipe"];
/// Weaker verbs: only a forget request when they name history, a time or a fact.
const FORGET_WEAK: &[&str] = &["clear", "remove", "get rid of"];
/// Words that mean "activity over time", so the request is to forget a stretch of history.
const HISTORY_WORDS: &[&str] = &[
    "history",
    "activity",
    "everything",
    "what i did",
    "what i was doing",
    "browsing",
    "session",
    "the last",
    "from",
];

/// "forget that", "wipe my history from today", "remove the fact that I live in Pune".
fn forget_intent(text: &str) -> Option<ActionHint> {
    let (rest, strong) = FORGET_STRONG
        .iter()
        .find_map(|v| strip_prefix_word(text, v))
        .map(|r| (r, true))
        .or_else(|| {
            FORGET_WEAK
                .iter()
                .find_map(|v| strip_prefix_word(text, v))
                .map(|r| (r, false))
        })?;
    let timed = PATTERNS
        .time_app_words
        .iter()
        .any(|w| contains_word(rest, w))
        || HISTORY_WORDS.iter().any(|w| contains_word(rest, w));
    if contains_word(rest, "fact") {
        return Some(ActionHint::ForgetFact);
    }
    if timed {
        return Some(ActionHint::Forget);
    }
    // No time or history word: only the strong verbs, and only when they point at a thing
    // of the user's ("forget that", "forget my favourite colour", "forget about my dog").
    let points_at_a_thing = ["that", "my", "about", "the", "what"]
        .iter()
        .any(|w| strip_prefix_word(rest, w).is_some());
    (strong && points_at_a_thing).then_some(ActionHint::ForgetFact)
}

/// Detects an imperative skill intent in normalised text.
#[must_use]
pub fn detect_action(norm: &str) -> Option<ActionHint> {
    // Strip up to two request prefixes (e.g. "please can you ...")
    let mut text = norm;
    for prefix in PATTERNS.request_prefixes {
        if let Some(rest) = strip_prefix_word(text, prefix) {
            text = rest;
            break;
        }
    }
    for prefix in PATTERNS.request_prefixes {
        if let Some(rest) = strip_prefix_word(text, prefix) {
            text = rest;
            break;
        }
    }

    // 1. Pause
    if text == "pause"
        || strip_prefix_word(text, "pause").is_some()
        || strip_prefix_word(text, "stop listening").is_some()
        || strip_prefix_word(text, "mute").is_some()
        || strip_prefix_word(text, "unpause").is_some()
        || strip_prefix_word(text, "unmute").is_some()
    {
        return Some(ActionHint::Pause);
    }

    // 2. Forget: a forgetting verb first, then what follows decides activity or one fact.
    if let Some(hint) = forget_intent(text) {
        return Some(hint);
    }

    // 3. Remember
    if strip_prefix_word(text, "remember that").is_some()
        || strip_prefix_word(text, "remember my").is_some()
        || strip_prefix_word(text, "remember i").is_some()
        || strip_prefix_word(text, "keep in mind").is_some()
        || strip_prefix_word(text, "bear in mind").is_some()
        || strip_prefix_word(text, "take note that").is_some()
        || strip_prefix_word(text, "save a fact that").is_some()
        || strip_prefix_word(text, "store a fact that").is_some()
        || strip_prefix_word(text, "note a fact that").is_some()
        || strip_prefix_word(text, "take a note that").is_some()
        || strip_prefix_word(text, "make a note that").is_some()
        || strip_prefix_word(text, "note that").is_some()
        || strip_prefix_word(text, "for the record").is_some()
    {
        return Some(ActionHint::Remember);
    }

    // 4. The remaining intents: a leading verb phrase plus an object word, so "free up some
    //    space on my laptop" and "find the word budget in my notes" are found without a
    //    phrase list for every wording. Order matters: notes before the web, the web before fetch.
    for intent in INTENTS {
        if let Some(rest) = intent.verbs.iter().find_map(|v| strip_prefix_word(text, v))
            && (intent.objects.is_empty() || intent.objects.iter().any(|o| contains_word(rest, o)))
        {
            return Some(intent.hint);
        }
    }

    // 5. A bare "my schedule", and questions that name a calendar or the disk.
    if ["my calendar", "my schedule", "my agenda"].contains(&text)
        || ["my calendar", "my schedule", "my agenda"].iter().any(|c| {
            strip_prefix_word(text, c).is_some_and(|r| detect_time_ref(r) || r.contains("tomorrow"))
        })
    {
        return Some(ActionHint::CalendarRead);
    }
    // A bare word about the disk: "disk?", "disk space", "storage".
    if [
        "disk",
        "disk space",
        "my disk",
        "storage",
        "free space",
        "my storage",
    ]
    .contains(&text)
    {
        return Some(ActionHint::DiskReport);
    }
    let asks = [
        "what", "show", "check", "read", "view", "open", "list", "do", "how", "which", "when",
        "give", "run", "generate", "tell", "get", "pull up", "bring up", "any",
    ];
    // "how do I free up disk space on Linux" asks how, not for the report.
    let how_to = [
        "how do i",
        "how can i",
        "how to",
        "how should i",
        "how would i",
    ]
    .iter()
    .any(|w| strip_prefix_word(text, w).is_some());
    let leads_with_ask = !how_to && asks.iter().any(|w| strip_prefix_word(text, w).is_some());
    if leads_with_ask {
        if [
            "calendar",
            "schedule",
            "agenda",
            "appointments",
            "meetings",
            "meeting",
            "events",
            "event",
            "shift",
            "class",
            "lecture",
            "exam",
            "flight",
            "scheduled",
            "appointment",
            "reminders",
        ]
        .iter()
        .any(|w| contains_word(text, w))
            && (contains_word(text, "my")
                || contains_word(text, "any")
                || contains_word(text, "next")
                || contains_word(text, "upcoming"))
            || text.contains("what do i have on")
            || text.contains("do i have scheduled")
            || text.contains("do i have planned")
        {
            return Some(ActionHint::CalendarRead);
        }
        let space = contains_word(text, "disk")
            || contains_word(text, "storage")
            || contains_word(text, "hard drive")
            || (contains_word(text, "space")
                && ["laptop", "drive", "storage", "computer", "machine"]
                    .iter()
                    .any(|w| contains_word(text, w)));
        let about_usage = [
            "using", "taking", "eating", "filling", "left", "free", "usage", "report", "much",
            "full",
        ]
        .iter()
        .any(|w| contains_word(text, w));
        if space && about_usage {
            return Some(ActionHint::DiskReport);
        }
    }

    None
}

/// One skill intent recognised by a leading verb phrase and, when given, an object word
/// anywhere after it (`router_test` fixtures cover each).
struct Intent {
    hint: ActionHint,
    verbs: &'static [&'static str],
    objects: &'static [&'static str],
}

const INTENTS: &[Intent] = &[
    Intent {
        hint: ActionHint::Pause,
        verbs: &[
            "resume",
            "resuming",
            "pausing",
            "continue",
            "restart",
            "start",
            "stop",
            "halt",
            "suspend",
            "turn on",
            "switch on",
        ],
        objects: &[
            "watching",
            "tracking",
            "capture",
            "capturing",
            "monitoring",
            "sensing",
            "recording",
            "observing",
            "logging",
        ],
    },
    Intent {
        hint: ActionHint::AppendNote,
        verbs: &["append", "add", "put", "tack", "attach"],
        objects: &["list", "journal", "todo", "log", "diary"],
    },
    Intent {
        hint: ActionHint::Pause,
        verbs: &[
            "go silent",
            "go quiet",
            "be quiet",
            "be silent",
            "stay quiet",
            "stay silent",
        ],
        objects: &[],
    },
    Intent {
        hint: ActionHint::Fetch,
        verbs: &["go to"],
        objects: &["http", "https", "url", "link", "website"],
    },
    Intent {
        hint: ActionHint::Pause,
        verbs: &[
            "switch off",
            "turn off",
            "disable",
            "silence",
            "stop recording",
            "stop sensing",
            "stop",
        ],
        objects: &[
            "listening",
            "microphone",
            "mic",
            "sensing",
            "monitoring",
            "recording",
            "tracking",
        ],
    },
    Intent {
        hint: ActionHint::Cleanup,
        verbs: &[
            "clean up",
            "cleanup",
            "clean",
            "free up",
            "free",
            "empty",
            "tidy up",
            "tidy",
            "clear",
            "get rid of",
            "throw away",
            "scrub",
            "purge",
        ],
        objects: &[
            "disk",
            "space",
            "trash",
            "cache",
            "caches",
            "storage",
            "laptop",
            "computer",
            "bin",
            "downloads",
            "up",
        ],
    },
    Intent {
        hint: ActionHint::NotesSearch,
        verbs: &[
            "search", "find", "look", "look up", "locate", "show", "show me", "get",
        ],
        objects: &["note", "notes"],
    },
    Intent {
        hint: ActionHint::AppendNote,
        verbs: &[
            "add",
            "write",
            "append",
            "put",
            "jot",
            "save",
            "note down",
            "take a note",
            "make a note",
            "write this down",
            "write down",
            "jot down",
        ],
        objects: &["note", "notes"],
    },
    Intent {
        hint: ActionHint::AppendNote,
        verbs: &[
            "take a note",
            "make a note",
            "write this down in",
            "note down",
        ],
        objects: &[],
    },
    Intent {
        hint: ActionHint::Search,
        verbs: &[
            "search", "look up", "lookup", "google", "find", "check", "look",
        ],
        objects: &["online", "web", "internet", "google"],
    },
    Intent {
        hint: ActionHint::Fetch,
        verbs: &["open", "fetch", "get", "read", "load", "visit"],
        objects: &["link", "url", "website", "webpage", "http", "https"],
    },
];

/// Words that point at something on the screen.
const SCREEN_POINTERS: &[&str] = &["this", "that", "these", "those"];

/// Things that live on a screen: "this window", "that popup".
const SCREEN_NOUNS: &[&str] = &[
    "window",
    "windows",
    "app",
    "application",
    "program",
    "tab",
    "tabs",
    "popup",
    "dialog",
    "screen",
    "page",
    "file",
    "document",
    "error",
    "folder",
    "button",
    "icon",
    "menu",
    "process",
    "site",
    "website",
    "notification",
    "terminal",
    "extension",
    "pop up",
    "spreadsheet",
    "script",
    "commit",
    "sheet",
    "video",
    "article",
    "email",
    "pdf",
    "ticket",
    "thread",
    "chat",
    "application",
    "monitor",
    "badge",
    "taskbar",
    "panel",
    "clock",
    "banner",
    "toolbar",
    "cursor",
];

/// Phrases that put the question on the user's own screen, wherever they sit.
const SCREEN_PHRASES: &[&str] = &[
    "on my screen",
    "on the screen",
    "on screen",
    "on my monitor",
    "on my second monitor",
    "on my other monitor",
    "on my display",
    "on my desktop",
    "in front of me",
    "visible on",
];

/// Words that say a window, tab or app is in a state right now ("which window is active").
const WINDOW_STATE: &[&str] = &[
    "active",
    "focused",
    "currently",
    "current",
    "open",
    "running",
    "showing",
    "frontmost",
    "foremost",
    "visible",
    "opened",
    "focus",
];

/// First-person words: a time word only points at the user's own activity next to one of these.
const SELF_WORDS: &[&str] = &["i", "my", "me", "mine", "i ve", "i d", "i m", "we", "our"];

/// Task verbs that act on text the user supplies ("translate ... today"): never about the screen.
const TEXT_TASK_VERBS: &[&str] = &[
    "rewrite",
    "translate",
    "proofread",
    "paraphrase",
    "rephrase",
    "shorten",
    "correct",
    "fix the grammar",
    "make this sound",
];

/// True when the text names the user (I, my, me).
/// `norm` without a leading "tell me", "can you show me", "please give me", "let me know".
fn without_request_opening(norm: &str) -> &str {
    const OPENINGS: &[&str] = &[
        "can you tell me",
        "could you tell me",
        "can you show me",
        "could you show me",
        "can you give me",
        "please tell me",
        "please show me",
        "please give me",
        "tell me",
        "show me",
        "give me",
        "let me know",
    ];
    OPENINGS
        .iter()
        .find_map(|o| strip_prefix_word(norm, o))
        .unwrap_or(norm)
}

fn mentions_self(norm: &str) -> bool {
    SELF_WORDS.iter().any(|w| contains_word(norm, w))
}

/// "this" followed by a time word is a time reference, not a pointer at the screen.
const THIS_TIME: &[&str] = &[
    "this week",
    "this morning",
    "this afternoon",
    "this evening",
    "this month",
    "this year",
    "this weekend",
    "this time",
];

/// Detects deictic reference to the user's screen or activity.
#[must_use]
pub fn detect_deictic(norm: &str) -> bool {
    // Check if the whole text is just a deictic question
    if PATTERNS.deictic_questions.contains(&norm) || detect_self_activity(norm) {
        return true;
    }

    // A whole "what I was/just ..." phrase already says it is about the user's activity.
    if PATTERNS
        .deictic_words
        .iter()
        .any(|w| w.contains(' ') && contains_word(norm, w))
    {
        return true;
    }

    // A task on text the user gives ("rewrite this email to ...") is not about the screen.
    if TEXT_TASK_VERBS
        .iter()
        .any(|v| strip_prefix_word(norm, v).is_some())
    {
        return false;
    }
    // "summarize what is visible on my screen", "what article was open on my second monitor".
    if SCREEN_PHRASES.iter().any(|p| contains_word(norm, p)) {
        return true;
    }
    // "what is on screen", "which document is open in front of me", "which app is open".
    if (norm.starts_with("what ") || norm.starts_with("which "))
        && (["on screen", "on my screen", "in front of me"]
            .iter()
            .any(|p| contains_word(norm, p))
            || (contains_word(norm, "open") && SCREEN_NOUNS.iter().any(|n| contains_word(norm, n))))
        && !THIS_TIME.iter().any(|p| contains_word(norm, p))
    {
        return true;
    }

    // "what program is this", "which window is that": a question word, a screen noun and a pointer.
    if (norm.starts_with("what ") || norm.starts_with("which "))
        && SCREEN_NOUNS.iter().any(|n| contains_word(norm, n))
        && SCREEN_POINTERS.iter().any(|p| contains_word(norm, p))
        && !THIS_TIME.iter().any(|p| contains_word(norm, p))
    {
        return true;
    }

    // "what is that red badge near the clock": a pointer question that names where it is.
    if ["what is", "what s", "what are", "what was"]
        .iter()
        .any(|q| {
            strip_prefix_word(norm, q).is_some_and(|r| {
                SCREEN_POINTERS
                    .iter()
                    .any(|p| strip_prefix_word(r, p).is_some())
                    && SCREEN_NOUNS.iter().any(|n| contains_word(r, n))
            })
        })
    {
        return true;
    }

    // "what was this ten minutes ago": a pointer question that only adds a time.
    if ["what was this", "what was that", "what was it"]
        .iter()
        .any(|q| strip_prefix_word(norm, q).is_some_and(|r| contains_word(r, "ago")))
    {
        return true;
    }

    // A pointer word right before a screen noun: "what is this program", "that popup".
    if SCREEN_POINTERS.iter().any(|p| {
        SCREEN_NOUNS
            .iter()
            .any(|n| contains_word(norm, &format!("{p} {n}")))
    }) && !THIS_TIME.iter().any(|p| contains_word(norm, p))
    {
        return true;
    }

    // "this week" and "this morning" are time words, not pointers at the screen.
    let stripped;
    let norm = if THIS_TIME.iter().any(|p| contains_word(norm, p)) {
        stripped = THIS_TIME
            .iter()
            .fold(norm.to_string(), |acc, p| acc.replace(p, " "));
        stripped.as_str()
    } else {
        norm
    };

    // Conservative deictic check: deictic word together with an activity cue word
    let has_deictic = PATTERNS
        .deictic_words
        .iter()
        .any(|w| contains_word(norm, w));
    let has_activity = PATTERNS
        .activity_cues
        .iter()
        .any(|w| contains_word(norm, w));

    has_deictic && has_activity
}

/// First-person phrases that talk about what the user was doing.
const SELF_ACTIVITY_PHRASES: &[&str] = &[
    "was i",
    "am i",
    "were i",
    "have i been",
    "had i been",
    "i was",
    "i have been",
    "i ve been",
    "i just",
    "i had open",
    "i have open",
    "i opened",
    "i last",
    "i closed",
    "i edited",
    "i used",
];

/// Activity words that make a first-person phrase a question about the user's own activity.
const SELF_ACTIVITY_CUES: &[&str] = &[
    "doing",
    "working",
    "looking",
    "reading",
    "watching",
    "browsing",
    "running",
    "editing",
    "using",
    "writing",
    "playing",
    "listening",
    "opened",
    "closed",
    "edited",
    "wrote",
    "watched",
    "used",
    "visited",
    "saved",
    "installed",
    "downloaded",
    "clicked",
    "typed",
    "open",
    "tab",
    "window",
    "app",
    "file",
    "document",
    "page",
    "video",
    "terminal",
    "browser",
    "editor",
    "build",
    "project",
    "spent",
    "folder",
    "type",
    "read",
    "launch",
    "launched",
    "program",
    "site",
    "application",
    "launch",
];

/// Words after "was I / am I" that are not about activity on the screen.
const NOT_ACTIVITY_ING: &[&str] = &[
    "going",
    "being",
    "getting",
    "having",
    "feeling",
    "thinking",
    "wondering",
    "trying",
    "supposed",
    "meant",
    "talking",
    "saying",
];

/// "what was I typing": a present participle right after "was I / am I" is the user's activity.
fn participle_after_self(norm: &str) -> bool {
    ["was i", "am i", "were i", "have i been", "had i been"]
        .iter()
        .any(|p| {
            norm.find(p).is_some_and(|at| {
                norm[at + p.len()..]
                    .split(' ')
                    .find(|w| !w.is_empty())
                    .is_some_and(|w| {
                        w.len() > 4 && w.ends_with("ing") && !NOT_ACTIVITY_ING.contains(&w)
                    })
            })
        })
}

/// "What was I using before lunch", "what was the last thing I opened": the user asks about
/// their own recent activity even though no "this" or "that" is spoken.
#[must_use]
pub fn detect_self_activity(norm: &str) -> bool {
    let asks = norm.starts_with("what ")
        || norm.starts_with("which ")
        || norm.starts_with("how ")
        || norm.starts_with("where ")
        || norm.starts_with("when ")
        || norm.starts_with("show ");
    if asks && participle_after_self(norm) {
        return true;
    }
    // "was I editing main.rs or lib.rs when the power cut out": a question that opens on "was I".
    if ["was i", "am i", "were i", "did i", "have i been"]
        .iter()
        .any(|p| strip_prefix_word(norm, p).is_some())
        && (participle_after_self(norm)
            || SELF_ACTIVITY_CUES.iter().any(|c| contains_word(norm, c)))
    {
        return true;
    }
    SELF_ACTIVITY_PHRASES.iter().any(|p| contains_word(norm, p))
        && SELF_ACTIVITY_CUES.iter().any(|c| contains_word(norm, c))
        && (norm.starts_with("what ")
            || norm.starts_with("which ")
            || norm.starts_with("how ")
            || norm.starts_with("where ")
            || norm.starts_with("when ")
            || norm.starts_with("show "))
}

/// Questions about the user's own windows and documents that have no "this" in them: "what
/// window is currently active", "what document did I save before lunch". Kept apart from
/// [`detect_deictic`] because they do not point at one thing on the screen at one moment.
#[must_use]
pub fn asks_about_own_screen(norm: &str) -> bool {
    // "active window?", "open windows", "focused app": a bare fragment about the screen.
    let fragment: Vec<&str> = norm.split(' ').collect();
    if fragment.len() <= 3
        && fragment
            .iter()
            .any(|w| ["window", "windows", "tab", "tabs", "app", "application"].contains(w))
        && fragment.iter().any(|w| WINDOW_STATE.contains(w))
    {
        return true;
    }
    // "what was the paper I read before lunch", "did I already send that review": a question
    // about something the user did on the machine.
    let asks = ["what ", "which ", "did ", "where ", "when ", "how "]
        .iter()
        .any(|q| norm.starts_with(q));
    if asks
        && [
            "i read",
            "i looked at",
            "i saw",
            "i visited",
            "i watched",
            "i wrote",
            "i sent",
            "i typed",
            "i saved",
            "i downloaded",
            "i installed",
            "i clicked",
            "i launched",
            "i searched",
            "i opened",
            "i edited",
            "was i in",
            "was i on",
            "i was in",
            "i was on",
        ]
        .iter()
        .any(|p| contains_word(norm, p))
    {
        return true;
    }
    if norm.starts_with("did i ") && !norm.contains("forget") {
        return true;
    }
    if !(norm.starts_with("what ") || norm.starts_with("which ")) {
        return false;
    }
    let head: Vec<&str> = norm.split(' ').take(4).collect();
    let window_noun = head
        .iter()
        .any(|w| ["window", "windows", "tab", "tabs", "terminal", "workspace"].contains(w));
    let screen_noun = head.iter().any(|w| SCREEN_NOUNS.contains(w));
    let app_noun = head
        .iter()
        .any(|w| ["application", "app", "program"].contains(w));
    if app_noun
        && ["focus", "focused", "frontmost", "foremost", "currently"]
            .iter()
            .any(|w| contains_word(norm, w))
    {
        return true;
    }
    if window_noun
        && (WINDOW_STATE.iter().any(|w| contains_word(norm, w))
            || contains_word(norm, "has")
            || contains_word(norm, "had")
            || contains_word(norm, "was")
            || mentions_self(norm))
    {
        return true;
    }
    if screen_noun
        && mentions_self(norm)
        && ["did", "was", "have", "had", "am"]
            .iter()
            .any(|w| contains_word(norm, w))
    {
        return true;
    }
    false
}

/// Detects a time reference in normalised text.
#[must_use]
pub fn detect_time_ref(norm: &str) -> bool {
    PATTERNS.time_refs.iter().any(|w| contains_word(norm, w))
}

/// Words that address the assistant and carry no request ("hey buddy ...").
const ADDRESS_WORDS: &[&str] = &["hey", "hi", "hello", "yo", "buddy", "assistant"];

/// Markers after which the real request starts when the user first explains why
/// ("my laptop is slow so can you check the disk").
const REQUEST_MARKERS: &[&str] = &[
    " so can you ",
    " so could you ",
    " so please ",
    " and can you ",
    " can you please ",
    " could you please ",
];

/// Removes what only frames a request: a greeting or the assistant's name at the start ("hey
/// buddy"), and a leading explanation before "so can you ...". Never returns empty text.
#[must_use]
pub fn strip_framing(norm: &str) -> String {
    let mut text = norm;
    loop {
        let next = ADDRESS_WORDS
            .iter()
            .find_map(|w| strip_prefix_word(text, w))
            .or_else(|| {
                // "ok buddy" / "okay buddy": the address makes "ok" a greeting, not a reply.
                ["ok buddy", "okay buddy"]
                    .iter()
                    .find_map(|w| strip_prefix_word(text, w))
            });
        match next {
            Some(rest) if !rest.is_empty() => text = rest,
            _ => break,
        }
    }
    // A long opening that explains itself before the request: keep the request.
    if text.split(' ').count() > 8
        && let Some((at, marker)) = REQUEST_MARKERS
            .iter()
            .filter_map(|m| text.find(m).map(|at| (at, m.len())))
            .min()
    {
        let rest = text[at + marker..].trim();
        if !rest.is_empty() {
            return format!("can you {rest}");
        }
    }
    text.to_string()
}

/// Deterministically routes a turn based on input text, nudge window state,
/// and optional retrieval score.
#[must_use]
pub fn route(input: &RouteInput<'_>) -> RouteDecision {
    let norm = strip_framing(&normalise(input.text));
    let deictic = detect_deictic(&norm);
    let time_ref = detect_time_ref(&norm);

    // Rule 1: Nudge Reply
    if input.nudge_window_open && is_nudge_reply(&norm) {
        return RouteDecision {
            route: RouteKind::NudgeReply,
            deictic,
            time_ref,
            action_hint: None,
        };
    }

    // Rule 2: Action
    if let Some(hint) = detect_action(&norm) {
        return RouteDecision {
            route: RouteKind::Action,
            deictic,
            time_ref,
            action_hint: Some(hint),
        };
    }

    // Rule 3: Desktop
    let score_above_cutoff = match input.retrieval_best_score {
        Some(score) => score >= input.cutoff,
        None => false,
    };

    // A time word alone ("who won the match yesterday") is not about the user's activity; it
    // needs the user in the sentence or an activity word.
    // The "me" of a polite opening ("tell me", "can you show me") is not about the user's
    // activity: "tell me the weather today" is a general question (BUG-117).
    let about_user = mentions_self(without_request_opening(&norm))
        || PATTERNS
            .activity_cues
            .iter()
            .any(|w| contains_word(&norm, w));
    let text_task = TEXT_TASK_VERBS
        .iter()
        .any(|v| strip_prefix_word(&norm, v).is_some());
    // "ago", "earlier" and "just now" are relative to the moment of asking, so they point at the
    // user's own recent activity by themselves; a calendar day ("yesterday") does not.
    let relative_time = [
        "ago",
        "earlier",
        "just now",
        "last hour",
        "past hour",
        "recently",
        "last time",
    ]
    .iter()
    .any(|w| contains_word(&norm, w));
    if !text_task
        && (deictic
            || asks_about_own_screen(&norm)
            || (time_ref && (about_user || relative_time))
            || score_above_cutoff)
    {
        return RouteDecision {
            route: RouteKind::Desktop,
            deictic,
            time_ref,
            action_hint: None,
        };
    }

    // Rule 4: General
    RouteDecision {
        route: RouteKind::General,
        deictic,
        time_ref,
        action_hint: None,
    }
}
