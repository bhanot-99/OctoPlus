//! General-chat rules for Semantic-NeuroOS (C5).
//!
//! Implements FR-CHT-03 and FR-CHT-05.
//! Pure helpers deciding when to append lookup offers and professional advice caveats.
//! The model's text is never trusted to make these decisions.

use crate::router;

/// Sensitive topics requiring professional advisory routing or disclaimers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Health,
    Legal,
    Money,
}

/// Spoken when C6 refuses a confirmation because the user said no to the same request a few
/// minutes ago, or asked for too many in a minute (NFR-SEC-09).
pub const APPROVAL_TOO_SOON: &str =
    "You said no to this a moment ago, so I won't ask again for a few minutes.";

/// Spoken when another confirmation window is already open.
pub const APPROVAL_BUSY: &str =
    "Another approval window is already open. Please answer that one first.";

/// The fixed offer sentence appended to a general answer whose question depends on fresh or specific facts (FR-CHT-03).
/// Plain speech, no markup, < 100 bytes, ends with a question mark.
pub const LOOKUP_OFFER: &str = "Would you like me to look that up for you?";

/// The fixed question appended after an answer when the user stated a personal fact.
/// Plain speech, ends with '?'.
pub const REMEMBER_OFFER: &str = "Want me to remember that?";

/// The reply to a command to change the computer (create or delete a file, install, send, shut
/// down). Fixed words: the model is never asked, so it cannot claim to have done it.
pub const CANNOT_CHANGE_THE_COMPUTER: &str = "I can't do that. I can't create, change or delete files, run or install programs, or send messages for you yet. I can search the web, look back over your recent activity, keep notes, and check your disk space.";

/// How an answer from looked-up pages begins (FR-CHT-06: "From the pages I looked up…").
pub const WEB_ANSWER_LEAD: &str = "From the pages I looked up: ";

/// Said when a search stored pages but they could not be read back in time.
pub const LOOKUP_UNREAD: &str =
    "I looked it up, but I couldn't read the pages in time. Please ask again in a moment.";

/// Words a search never needs (FR-CHT-04): question words, helpers and the user's own
/// pronouns, so "which shampoo should I buy? I have a dry scalp" sends
/// "shampoo buy dry scalp". The user can still edit the text in the dialog.
const QUERY_DROP: &[&str] = &[
    "a", "am", "an", "and", "are", "be", "been", "can", "could", "did", "do", "does", "give",
    "had", "has", "have", "hey", "how", "i", "i'd", "i'm", "i've", "is", "it", "me", "mine", "my",
    "myself", "our", "please", "should", "show", "tell", "that", "the", "this", "to", "us", "was",
    "we", "were", "what", "which", "who", "will", "would", "you", "your",
];

/// Longest search query drafted from a question, in words.
const QUERY_MAX_WORDS: usize = 10;

/// Drafts the search text for `question` (FR-CHT-04): lower case, punctuation removed, the
/// words in [`QUERY_DROP`] left out, at most [`QUERY_MAX_WORDS`] words. A question made only
/// of such words is sent as the user wrote it, trimmed.
#[must_use]
pub fn search_query(question: &str) -> String {
    let words: Vec<String> = question
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .map(|w| {
            w.trim_matches(|c: char| c == '\'' || c == '-')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty() && !QUERY_DROP.contains(&w.as_str()))
        .take(QUERY_MAX_WORDS)
        .collect();
    if words.is_empty() {
        question.trim().to_string()
    } else {
        words.join(" ")
    }
}

/// Keywords and phrases indicating a question depends on fresh or specific real-world facts.
const FRESHNESS_PATTERNS: &[&str] = &[
    "latest",
    "newest",
    "recent",
    "recently",
    "current",
    "currently",
    "news",
    "headline",
    "headlines",
    "trending",
    "happening",
    "today",
    "todays",
    "tonight",
    "this week",
    "this month",
    "this year",
    "right now",
    "at the moment",
    "weather",
    "forecast",
    "temperature",
    "temp",
    "rain",
    "raining",
    "snow",
    "snowing",
    "price",
    "prices",
    "cost",
    "costs",
    "costing",
    "cheap",
    "cheapest",
    "expensive",
    "how much is",
    "how much does",
    "how much do",
    "how much will",
    "where to buy",
    "deal",
    "deals",
    "discount",
    "discounts",
    "sale",
    "ticket",
    "tickets",
    "flight",
    "flights",
    "hotel",
    "hotels",
    "version",
    "versions",
    "release",
    "releases",
    "released",
    "release date",
    "launch date",
    "update",
    "updates",
    "changelog",
    "patch notes",
    "score",
    "scores",
    "standing",
    "standings",
    "who won",
    "who is winning",
    "winner",
    "winners",
    "match",
    "game",
    "tournament",
    "championship",
    "playoffs",
    "olympics",
    "medal",
    "medals",
    "best",
    "top",
    "fastest",
    "hottest",
    "most popular",
    "highest rated",
    "stock price",
    "exchange rate",
    "traffic",
    "server status",
    "outage",
];

/// Health keywords and stems.
const HEALTH_TERMS: &[&str] = &[
    "health",
    "symptom",
    "symptoms",
    "medicine",
    "medication",
    "medications",
    "dose",
    "dosage",
    "diagnosis",
    "diagnose",
    "diagnosed",
    "pain",
    "doctor",
    "physician",
    "hospital",
    "clinic",
    "illness",
    "disease",
    "infection",
    "fever",
    "cough",
    "headache",
    "prescription",
    "prescribe",
    "prescribed",
    "drug",
    "drugs",
    "treatment",
    "therapy",
    "surgery",
    "cancer",
    "diabetes",
    "blood pressure",
    "allergic",
    "allergy",
    "allergies",
    "vaccine",
    "vaccination",
    "pregnant",
    "pregnancy",
    "ibuprofen",
    "paracetamol",
    "acetaminophen",
    "aspirin",
    "antibiotic",
    "antibiotics",
    "medicines",
    "pills",
    "tablet",
    "tablets",
    "painkiller",
    "painkillers",
    "insulin",
    "mg",
    "overdose",
    "side effects",
];

/// Legal keywords and stems.
const LEGAL_TERMS: &[&str] = &[
    "legal",
    "lawsuit",
    "law",
    "lawyer",
    "attorney",
    "contract",
    "contracts",
    "sue",
    "suing",
    "sued",
    "visa",
    "visas",
    "tenant",
    "tenants",
    "landlord",
    "custody",
    "court",
    "judge",
    "settlement",
    "litigation",
    "copyright",
    "patent",
    "trademark",
    "divorce",
    "nda",
    "statute",
    "criminal",
    "crime",
    "prosecutor",
    "liability",
    "eviction",
    "evict",
    "citizenship",
    "asylum",
];

/// Money keywords and stems.
const MONEY_TERMS: &[&str] = &[
    "money",
    "invest",
    "investing",
    "investment",
    "investments",
    "stock",
    "stocks",
    "loan",
    "loans",
    "tax",
    "taxes",
    "taxation",
    "mortgage",
    "mortgages",
    "crypto",
    "cryptocurrency",
    "bitcoin",
    "ethereum",
    "debt",
    "debts",
    "finance",
    "financial",
    "bank",
    "banking",
    "bankruptcy",
    "interest rate",
    "dividend",
    "dividends",
    "portfolio",
    "retirement",
    "401k",
    "ira",
    "fund",
    "funds",
    "credit card",
    "credit score",
];

/// Plain positive replies.
const YES_PHRASES: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "yes please",
    "yes do it",
    "yes go ahead",
    "please do",
    "go ahead",
    "do it",
    "sure thing",
    "why not",
    "of course",
    "definitely",
    "absolutely",
    "yes please do",
    "sure go ahead",
    "sure please do",
    "yeah please do",
    "yeah do it",
    "yeah go ahead",
];

/// Plain negative replies.
const NO_PHRASES: &[&str] = &[
    "no",
    "nope",
    "nah",
    "cancel",
    "dont",
    "no thanks",
    "no thank you",
    "not now",
    "no not now",
    "never mind",
    "nevermind",
    "no way",
    "dont do it",
    "dont do that",
    "please dont",
    "please do not",
    "no never mind",
    "no cancel",
];

/// Phrases indicating acceptance of a lookup offer.
const LOOKUP_ACCEPT_PHRASES: &[&str] = &[
    "yes",
    "ye",
    "ya",
    "yea",
    "yeah",
    "go for it",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "yes please",
    "please do",
    "go ahead",
    "do it",
    "sure thing",
    "of course",
    "look it up",
    "look that up",
    "please look it up",
    "sure look it up",
    "yes look it up",
    "yeah look it up",
    "yes please look it up",
    "look it up please",
    "look that up please",
    "look it up for me",
    "look that up for me",
    "search for it",
    "search it",
    "search online",
    "search for it please",
    "yes search for it",
    "sure search for it",
    "yeah search for it",
    "find out",
    "please find out",
    "yes find out",
];

/// True when the question depends on current or specific facts (brands, prices, news,
/// latest, best, versions, release dates, scores, weather, right now, etc.).
///
/// NOT for timeless questions ("how does a hash map work", "what is the capital of France").
/// Uses word-boundary matching over [`router::normalise`] output.
#[must_use]
pub fn freshness_cue(text: &str) -> bool {
    let norm = router::normalise(text);
    FRESHNESS_PATTERNS
        .iter()
        .any(|&p| router::contains_word(&norm, p))
        || has_version_number(&norm)
}

/// "rust 1.80", "python 3.12": two numbers in a row (the dot was folded to a space).
fn has_version_number(norm: &str) -> bool {
    let words: Vec<&str> = norm.split(' ').collect();
    words.windows(2).any(|w| {
        w.iter()
            .all(|x| !x.is_empty() && x.len() <= 4 && x.bytes().all(|b| b.is_ascii_digit()))
    }) && words.iter().any(|w| !w.bytes().all(|b| b.is_ascii_digit()))
}

/// Identifies sensitive professional advice topics: Health, Legal, Money.
///
/// First match wins in that order.
#[must_use]
pub fn professional_topic(text: &str) -> Option<Topic> {
    let norm = router::normalise(text);
    if HEALTH_TERMS
        .iter()
        .any(|&t| router::contains_word(&norm, t))
    {
        return Some(Topic::Health);
    }
    if LEGAL_TERMS.iter().any(|&t| router::contains_word(&norm, t)) {
        return Some(Topic::Legal);
    }
    if MONEY_TERMS.iter().any(|&t| router::contains_word(&norm, t)) {
        return Some(Topic::Money);
    }
    None
}

/// Words that make a reply a refusal, whatever else it says ("don't" normalises to "don t").
const REFUSAL_WORDS: &[&str] = &[
    "no", "not", "don", "dont", "never", "nope", "nah", "stop", "cancel",
];

/// Verbs and targets of a loose acceptance: "check it", "search online", "google that".
const LOOKUP_VERBS: &[&str] = &["check", "search", "google", "look", "find"];
const LOOKUP_TARGETS: &[&str] = &["it", "that", "online", "internet", "web"];

/// The search text of a direct request ("search the web for delhi weather", "check the
/// weather in delhi online"): the request words are taken out, then [`search_query`] drafts
/// the rest. "up" goes only after "look", "for" only after a request word, and "on" only
/// before "the web" or "internet", so a topic like "news on rust" keeps its words.
#[must_use]
pub fn search_request_query(text: &str) -> String {
    const REQUEST: &[&str] = &["search", "google", "lookup", "look", "check", "find"];
    const PLACES: &[&str] = &["online", "web", "internet"];
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .map(str::to_lowercase)
        .filter(|w| !w.is_empty())
        .collect();
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| words[j].as_str());
        let next = words.get(i + 1).map(String::as_str);
        let next2 = words.get(i + 2).map(String::as_str);
        let drop = REQUEST.contains(&w.as_str())
            || PLACES.contains(&w.as_str())
            || (w == "up" && prev == Some("look"))
            || (w == "for"
                && prev.is_some_and(|p| REQUEST.contains(&p) || PLACES.contains(&p) || p == "up"))
            || (w == "on"
                && (next.is_some_and(|n| PLACES.contains(&n))
                    || (next == Some("the") && next2.is_some_and(|n| PLACES.contains(&n)))));
        if !drop {
            kept.push(w);
        }
    }
    search_query(&kept.join(" "))
}

/// True when the user's turn accepts a lookup offer.
///
/// Handles short affirmations and explicit lookup acceptance phrases.
/// Rejects affirmations that are part of a longer new question (e.g. "yes what is rust").
#[must_use]
pub fn accepts_lookup(text: &str) -> bool {
    let norm = router::normalise(text);
    if norm.is_empty() {
        return false;
    }

    if LOOKUP_ACCEPT_PHRASES.contains(&norm.as_str()) {
        return true;
    }

    // Check if it starts with an affirmative or request and contains a lookup phrase without introducing a new question
    let words: Vec<&str> = norm.split_whitespace().collect();
    // "no, don't check it" names the lookup too: any refusal word wins.
    if words.iter().any(|w| REFUSAL_WORDS.contains(w)) {
        return false;
    }
    if words.len() <= 7 {
        // "you check it on internet", "search online": a lookup verb aimed at the offer.
        let loose = words.iter().any(|w| LOOKUP_VERBS.contains(w))
            && words.iter().any(|w| LOOKUP_TARGETS.contains(w));
        let has_lookup_verb = loose
            || router::contains_word(&norm, "look it up")
            || router::contains_word(&norm, "look that up")
            || router::contains_word(&norm, "search for it")
            || router::contains_word(&norm, "search it")
            || router::contains_word(&norm, "find out");

        let has_new_question = words.iter().any(|&w| {
            matches!(
                w,
                "what" | "who" | "when" | "where" | "why" | "how" | "which"
            )
        });

        if has_lookup_verb && !has_new_question {
            return true;
        }
    }

    false
}

/// Evaluates a plain yes / no reply.
///
/// Returns `Some(true)` for positive short replies, `Some(false)` for negative short replies,
/// and `None` otherwise. Only whole short replies (<= 4 words after normalise).
#[must_use]
pub fn yes_no(text: &str) -> Option<bool> {
    let norm = router::normalise(text);
    let words: Vec<&str> = norm.split_whitespace().collect();
    if words.is_empty() || words.len() > 4 {
        return None;
    }

    if YES_PHRASES.contains(&norm.as_str()) {
        Some(true)
    } else if NO_PHRASES.contains(&norm.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// True when the text states a personal fact the user might want remembered
/// (e.g. "my name is ...", "i live in ...", "i like ...").
///
/// Statements only: questions and imperatives ("remember that ...") return `false`.
/// Text longer than 300 bytes returns `false`.
#[must_use]
pub fn personal_fact_statement(text: &str) -> bool {
    if text.len() > 300 {
        return false;
    }
    if text.trim().ends_with('?') {
        return false;
    }

    let norm = router::normalise(text);
    if norm.is_empty() {
        return false;
    }

    // Reject anything that also asks something ("my dog is sick, what should I do").
    if norm.split(' ').any(|w| {
        [
            "what", "how", "why", "should", "who", "where", "when", "which",
        ]
        .contains(&w)
    }) {
        return false;
    }

    // Reject questions
    let question_prefixes = [
        "what ", "is ", "are ", "who ", "where ", "when ", "why ", "how ", "do ", "does ", "did ",
        "can ", "could ", "would ", "will ",
    ];
    if question_prefixes.iter().any(|&p| norm.starts_with(p)) {
        return false;
    }

    // Reject imperatives (these are action routes, not passive fact statements)
    let imperative_prefixes = [
        "remember ",
        "remember that ",
        "please remember ",
        "forget ",
        "please forget ",
        "dont forget ",
    ];
    if imperative_prefixes.iter().any(|&p| norm.starts_with(p)) {
        return false;
    }

    // Check personal fact statements
    let personal_prefixes = [
        "call me ",
        "i live in ",
        "i reside in ",
        "i am allergic to ",
        "im allergic to ",
        "i work as ",
        "i work at ",
        "i work for ",
        "i prefer ",
        "i like ",
        "i love ",
        "i hate ",
        "i dislike ",
        "i was born in ",
        "i was born on ",
        "my name is ",
        "my birthday is ",
    ];

    if personal_prefixes.iter().any(|&p| norm.starts_with(p)) {
        return true;
    }

    // Check "my <noun> is/are ..." e.g. "my dog is called Rex", "my favorite color is blue"
    let words: Vec<&str> = norm.split_whitespace().collect();
    if words.len() >= 4 && words[0] == "my" {
        if words[2] == "is" || words[2] == "are" {
            return true;
        }
        if words.len() >= 5 && (words[3] == "is" || words[3] == "are") {
            return true;
        }
    }

    false
}

/// Appends [`LOOKUP_OFFER`] to `answer` when `fresh` is true; otherwise returns `answer` unchanged.
///
/// Idempotent: applying multiple times does not duplicate the offer.
#[must_use]
pub fn with_offer(answer: &str, fresh: bool) -> String {
    if !fresh {
        return answer.to_string();
    }

    let trimmed = answer.trim_end();
    if trimmed.ends_with(LOOKUP_OFFER) {
        return answer.to_string();
    }

    if answer.is_empty() {
        LOOKUP_OFFER.to_string()
    } else if answer.ends_with(' ') || answer.ends_with('\n') {
        format!("{answer}{LOOKUP_OFFER}")
    } else {
        format!("{answer} {LOOKUP_OFFER}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_keeps_only_what_the_search_needs() {
        assert_eq!(
            search_query("Hey, which shampoo should I buy? I have a dry scalp."),
            "shampoo buy dry scalp"
        );
        assert_eq!(
            search_query("What's the latest Rust version?"),
            "what's latest rust version"
        );
        // Owner, 2026-11-22: "tell" reached the search provider.
        assert_eq!(
            search_query("tell me the weather of delhi today"),
            "weather of delhi today"
        );
        assert_eq!(search_query("can you show me rust news"), "rust news");
        assert_eq!(search_query("  is it?  "), "is it?");
        let long = "one two three four five six seven eight nine ten eleven twelve";
        assert_eq!(search_query(long).split(' ').count(), 10);
    }
}
