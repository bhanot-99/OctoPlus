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
    pub action_trigger_words: &'static [&'static str],
}

/// Master constant table containing all routing word and phrase patterns.
pub const PATTERNS: RouterPatterns = RouterPatterns {
    nudge_replies: &[
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
        "what i just looked at",
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
        "doing", "working", "looking", "reading", "open", "window", "app", "screen", "tab", "file",
        "writing", "watching", "browsing", "running", "editing", "using", "spent", "time",
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
    action_trigger_words: &[
        "pause", "stop", "mute", "forget", "delete", "remember", "clean", "cleanup", "free",
        "empty", "trash", "search", "look", "open", "fetch", "add", "write", "append", "note",
        "notes", "calendar", "schedule", "disk", "find",
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

/// Checks if `norm` matches any known short nudge reply phrase.
#[must_use]
pub fn is_nudge_reply(norm: &str) -> bool {
    PATTERNS.nudge_replies.contains(&norm)
}

/// Detects an imperative skill intent in normalised text.
#[must_use]
pub fn detect_action(norm: &str) -> Option<ActionHint> {
    // Fast-path: must contain at least one action trigger word with word boundaries
    let has_trigger = PATTERNS
        .action_trigger_words
        .iter()
        .any(|w| contains_word(norm, w));
    if !has_trigger {
        return None;
    }

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
    {
        return Some(ActionHint::Pause);
    }

    // 2. Forget vs ForgetFact
    // "forget that" with no time/app word = ForgetFact
    if let Some(rest) =
        strip_prefix_word(text, "forget that").or_else(|| strip_prefix_word(text, "delete that"))
    {
        let has_time_app = PATTERNS
            .time_app_words
            .iter()
            .any(|w| contains_word(rest, w));
        return if has_time_app {
            Some(ActionHint::Forget)
        } else {
            Some(ActionHint::ForgetFact)
        };
    }

    // Explicit activity/history forget patterns
    if strip_prefix_word(text, "forget what i did").is_some()
        || strip_prefix_word(text, "delete what i did").is_some()
        || strip_prefix_word(text, "forget everything").is_some()
        || strip_prefix_word(text, "delete everything").is_some()
        || strip_prefix_word(text, "forget my history").is_some()
        || strip_prefix_word(text, "delete my history").is_some()
        || strip_prefix_word(text, "forget the last").is_some()
        || strip_prefix_word(text, "delete the last").is_some()
        || strip_prefix_word(text, "forget my activity").is_some()
        || strip_prefix_word(text, "delete my activity").is_some()
        || strip_prefix_word(text, "forget from").is_some()
        || strip_prefix_word(text, "delete from").is_some()
    {
        return Some(ActionHint::Forget);
    }

    // Explicit fact forget patterns
    if strip_prefix_word(text, "forget fact").is_some()
        || strip_prefix_word(text, "forget the fact").is_some()
        || strip_prefix_word(text, "delete fact").is_some()
        || strip_prefix_word(text, "delete the fact").is_some()
    {
        return Some(ActionHint::ForgetFact);
    }

    // "forget my ..." / "forget about my ..." -> ForgetFact unless time/app word present
    if let Some(rest) = strip_prefix_word(text, "forget my")
        .or_else(|| strip_prefix_word(text, "delete my"))
        .or_else(|| strip_prefix_word(text, "forget about my"))
    {
        let has_time_app = PATTERNS
            .time_app_words
            .iter()
            .any(|w| contains_word(rest, w));
        return if has_time_app {
            Some(ActionHint::Forget)
        } else {
            Some(ActionHint::ForgetFact)
        };
    }

    // 3. Remember
    if strip_prefix_word(text, "remember that").is_some()
        || strip_prefix_word(text, "remember my").is_some()
        || strip_prefix_word(text, "remember i").is_some()
    {
        return Some(ActionHint::Remember);
    }

    // 4. Cleanup
    if strip_prefix_word(text, "clean up").is_some()
        || strip_prefix_word(text, "cleanup").is_some()
        || strip_prefix_word(text, "free up disk").is_some()
        || strip_prefix_word(text, "free up space").is_some()
        || strip_prefix_word(text, "empty trash").is_some()
        || strip_prefix_word(text, "empty the trash").is_some()
        || strip_prefix_word(text, "empty recycle bin").is_some()
        || strip_prefix_word(text, "empty the recycle bin").is_some()
        || text == "clean up"
        || text == "empty trash"
    {
        return Some(ActionHint::Cleanup);
    }

    // 5. Search
    if strip_prefix_word(text, "search the web for").is_some()
        || strip_prefix_word(text, "search the web").is_some()
        || strip_prefix_word(text, "look up online").is_some()
        || strip_prefix_word(text, "look this up online").is_some()
        || strip_prefix_word(text, "look that up online").is_some()
        || strip_prefix_word(text, "look it up online").is_some()
        || strip_prefix_word(text, "search online for").is_some()
        || strip_prefix_word(text, "search online").is_some()
        || strip_prefix_word(text, "look online for").is_some()
        || strip_prefix_word(text, "look online").is_some()
        || strip_prefix_word(text, "search google for").is_some()
        || strip_prefix_word(text, "search the internet for").is_some()
    {
        return Some(ActionHint::Search);
    }

    // 6. Fetch
    if strip_prefix_word(text, "open this link").is_some()
        || strip_prefix_word(text, "open the link").is_some()
        || strip_prefix_word(text, "open link").is_some()
        || strip_prefix_word(text, "open this url").is_some()
        || strip_prefix_word(text, "open the url").is_some()
        || strip_prefix_word(text, "open url").is_some()
        || strip_prefix_word(text, "fetch this link").is_some()
        || strip_prefix_word(text, "fetch the link").is_some()
        || strip_prefix_word(text, "fetch link").is_some()
        || strip_prefix_word(text, "fetch this url").is_some()
        || strip_prefix_word(text, "fetch the url").is_some()
        || strip_prefix_word(text, "fetch url").is_some()
        || strip_prefix_word(text, "fetch https").is_some()
        || strip_prefix_word(text, "fetch http").is_some()
        || strip_prefix_word(text, "open https").is_some()
        || strip_prefix_word(text, "open http").is_some()
    {
        return Some(ActionHint::Fetch);
    }

    // 7. AppendNote
    if strip_prefix_word(text, "add to my note").is_some()
        || strip_prefix_word(text, "add to my notes").is_some()
        || strip_prefix_word(text, "add to note").is_some()
        || strip_prefix_word(text, "add to notes").is_some()
        || strip_prefix_word(text, "add this to my note").is_some()
        || strip_prefix_word(text, "add this to my notes").is_some()
        || strip_prefix_word(text, "write in my note").is_some()
        || strip_prefix_word(text, "write in my notes").is_some()
        || strip_prefix_word(text, "write this in my note").is_some()
        || strip_prefix_word(text, "write this in my notes").is_some()
        || strip_prefix_word(text, "write to my note").is_some()
        || strip_prefix_word(text, "write to my notes").is_some()
        || strip_prefix_word(text, "append to my note").is_some()
        || strip_prefix_word(text, "append to my notes").is_some()
        || strip_prefix_word(text, "append to note").is_some()
        || strip_prefix_word(text, "append to notes").is_some()
        || strip_prefix_word(text, "append to").is_some()
        || strip_prefix_word(text, "take a note").is_some()
        || strip_prefix_word(text, "make a note").is_some()
    {
        return Some(ActionHint::AppendNote);
    }

    // 8. NotesSearch
    if strip_prefix_word(text, "search my notes").is_some()
        || strip_prefix_word(text, "find in my notes").is_some()
        || strip_prefix_word(text, "search in my notes").is_some()
        || strip_prefix_word(text, "find in notes").is_some()
        || strip_prefix_word(text, "look in my notes").is_some()
    {
        return Some(ActionHint::NotesSearch);
    }

    // 9. CalendarRead
    if strip_prefix_word(text, "what is on my calendar").is_some()
        || strip_prefix_word(text, "what s on my calendar").is_some()
        || strip_prefix_word(text, "check my calendar").is_some()
        || strip_prefix_word(text, "show my calendar").is_some()
        || strip_prefix_word(text, "read my calendar").is_some()
        || strip_prefix_word(text, "what is my schedule").is_some()
        || strip_prefix_word(text, "what s my schedule").is_some()
        || strip_prefix_word(text, "my schedule").is_some()
        || strip_prefix_word(text, "show my schedule").is_some()
        || strip_prefix_word(text, "view my schedule").is_some()
        || strip_prefix_word(text, "check my schedule").is_some()
        || strip_prefix_word(text, "what do i have scheduled").is_some()
        || text == "what is on my calendar"
        || text == "what s on my calendar"
        || text == "my schedule"
    {
        return Some(ActionHint::CalendarRead);
    }

    // 10. DiskReport
    if strip_prefix_word(text, "what is using disk space").is_some()
        || strip_prefix_word(text, "what s using disk space").is_some()
        || strip_prefix_word(text, "what is taking up disk space").is_some()
        || strip_prefix_word(text, "what s taking up disk space").is_some()
        || strip_prefix_word(text, "what is taking up space on my disk").is_some()
        || strip_prefix_word(text, "what is using my disk space").is_some()
        || strip_prefix_word(text, "how much disk space is left").is_some()
        || strip_prefix_word(text, "how much disk space is free").is_some()
        || strip_prefix_word(text, "how much disk space do i have").is_some()
        || strip_prefix_word(text, "disk space report").is_some()
        || strip_prefix_word(text, "disk usage report").is_some()
        || strip_prefix_word(text, "check disk space").is_some()
        || strip_prefix_word(text, "show disk space").is_some()
        || strip_prefix_word(text, "disk usage").is_some()
        || text == "what is using disk space"
        || text == "what s using disk space"
        || text == "disk space report"
        || text == "disk usage"
    {
        return Some(ActionHint::DiskReport);
    }

    None
}

/// Detects deictic reference to the user's screen or activity.
#[must_use]
pub fn detect_deictic(norm: &str) -> bool {
    // Check if the whole text is just a deictic question
    if PATTERNS.deictic_questions.contains(&norm) {
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

/// Detects a time reference in normalised text.
#[must_use]
pub fn detect_time_ref(norm: &str) -> bool {
    PATTERNS.time_refs.iter().any(|w| contains_word(norm, w))
}

/// Deterministically routes a turn based on input text, nudge window state,
/// and optional retrieval score.
#[must_use]
pub fn route(input: &RouteInput<'_>) -> RouteDecision {
    let norm = normalise(input.text);
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

    if deictic || time_ref || score_above_cutoff {
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
