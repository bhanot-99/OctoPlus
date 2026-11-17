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

/// The fixed offer sentence appended to a general answer whose question depends on fresh or specific facts (FR-CHT-03).
/// Plain speech, no markup, < 100 bytes, ends with a question mark.
pub const LOOKUP_OFFER: &str = "Would you like me to look that up for you?";

/// The fixed question appended after an answer when the user stated a personal fact.
/// Plain speech, ends with '?'.
pub const REMEMBER_OFFER: &str = "Want me to remember that?";

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
    "yeah",
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
    if words.len() <= 7 {
        let has_lookup_verb = router::contains_word(&norm, "look it up")
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
