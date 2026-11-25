//! Memory of the owner (Phase 12, story C-S05, plan section 6, rule R0-8 as reworded).
//!
//! * "Remember ..." is always saved: that is the existing action route, untouched.
//! * Everything else is saved by chance, about 40 to 50 percent of the time, weighted by how
//!   personal and memorable it sounds (a name, a plan or a feeling counts more than a taste).
//! * Never by chance, ever: secrets and keys, card or account numbers, health details, other
//!   people's private matters, anything in a topic the owner marked sensitive, anything said
//!   in a crisis, and anything that may not be the owner speaking (ambient speech). Those are
//!   saved only on an explicit "remember".
//!
//! This module only DECIDES. It never saves anything: the decision is carried out elsewhere,
//! as one audited call to the safety kernel (see `turn/chance_memory.rs`). The chance uses a seeded
//! generator, and only counts are kept, so tests can prove the average and the exclusions.

use crate::companion::guards;
use crate::companion::persona::{MemoryConfig, Persona};
use crate::companion::rng::Rng;

/// Longest fact kept, in bytes (the skill accepts 512; the marker needs some room).
pub const MAX_FACT_BYTES: usize = 300;
/// Words at most in a sentence worth keeping.
const MAX_WORDS: usize = 40;
/// Words at least.
const MIN_WORDS: usize = 3;
/// Marks a fact she kept on her own (the owner did not ask). Stripped when shown.
pub const NOTICED_MARKER: &str = "(noticed) ";
/// How often she says "got it" when she keeps something.
const ACK_CHANCE: f64 = 0.35;
/// Decisions before the controller starts steering the average.
const WARMUP: u64 = 20;

/// What kind of thing was said (decides its weight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// A name or how to be called.
    Identity,
    /// Something coming up: an interview, a trip, a deadline.
    Plan,
    /// A feeling.
    Feeling,
    /// Family, pets, friends, colleagues.
    People,
    /// Work, study, where they live, birthday.
    Life,
    /// A taste: like, love, hate, prefer.
    Preference,
}

impl Category {
    const fn weight(self) -> f64 {
        match self {
            Self::Identity => 1.5,
            Self::Plan => 1.4,
            Self::Feeling => 1.3,
            Self::Life => 1.1,
            Self::People => 1.0,
            Self::Preference => 0.8,
        }
    }
}

/// Why something is never kept by chance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exclusion {
    /// A password, key, token or code.
    Secret,
    /// A card or account number.
    Number,
    /// Health.
    Health,
    /// Another person's private matter.
    OthersPrivate,
    /// A topic the owner marked sensitive.
    SensitiveTopic,
    /// A crisis.
    Crisis,
}

/// Why nothing was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// It may not be the owner speaking (a TV, a call, other people).
    Ambient,
    /// A question or a command, not something about the owner.
    NotPersonal,
    /// Too short or too long.
    WrongSize,
    /// Never by chance.
    Excluded(Exclusion),
    /// Personal and allowed, but the dice said no.
    Chance,
}

/// What to do with what the owner just said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Keep it.
    Save(Keep),
    /// Do not.
    Skip(Skip),
}

/// A fact to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keep {
    /// The text to store, with the "noticed" marker.
    pub fact: String,
    /// What kind of thing it is.
    pub category: Category,
    /// She says "got it" after her reply.
    pub acknowledge: bool,
}

/// Counts only (no text).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryCounters {
    /// Personal statements that were allowed to be considered.
    pub considered: u64,
    /// Kept.
    pub saved: u64,
    /// Skipped by chance.
    pub chance_skipped: u64,
    /// Never by chance: the exclusions.
    pub excluded: u64,
    /// Skipped as possibly not the owner.
    pub ambient: u64,
}

/// The decision maker.
#[derive(Debug, Clone)]
pub struct MemoryRule {
    rng: Rng,
    low: f64,
    high: f64,
    avoid: Vec<String>,
    counters: MemoryCounters,
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn has_phrase(norm: &str, phrase: &str) -> bool {
    norm.contains(&format!(" {phrase} "))
}

fn any_phrase(norm: &str, list: &[&str]) -> bool {
    list.iter().any(|p| has_phrase(norm, p))
}

const SECRETS: &[&str] = &[
    "password",
    "passwords",
    "passcode",
    "passphrase",
    "pin code",
    "my pin",
    "api key",
    "secret key",
    "private key",
    "access token",
    "token",
    "otp",
    "one time password",
    "security code",
    "cvv",
    "login",
    "credentials",
    "recovery phrase",
    "seed phrase",
    "security question",
];

const NUMBER_WORDS: &[&str] = &[
    "credit card",
    "debit card",
    "card number",
    "account number",
    "bank account",
    "iban",
    "routing number",
    "ssn",
    "social security",
    "aadhaar",
    "passport number",
    "pan number",
    "sort code",
];

const HEALTH: &[&str] = &[
    "diagnosed",
    "diagnosis",
    "medication",
    "medications",
    "medicine",
    "prescription",
    "prescribed",
    "doctor said",
    "my doctor",
    "therapist",
    "therapy",
    "cancer",
    "diabetes",
    "diabetic",
    "depression",
    "depressed",
    "anxiety disorder",
    "panic attacks",
    "pregnant",
    "pregnancy",
    "hiv",
    "surgery",
    "blood pressure",
    "symptoms",
    "symptom",
    "chronic",
    "allergic",
    "allergy",
    "illness",
    "disease",
    "hospital",
    "my pills",
    "mental health",
    "bipolar",
    "adhd",
    "migraine",
];

const OTHERS_PRIVATE: &[&str] = &[
    "told me in confidence",
    "in confidence",
    "dont tell anyone",
    "do not tell anyone",
    "dont tell",
    "between us",
    "keep this secret",
    "keep it secret",
    "its a secret",
    "a secret",
    "secretly",
    "gossip",
    "cheating on",
    "affair",
    "got fired",
    "got divorced",
    "getting divorced",
    "is pregnant",
    "is sick",
    "is dying",
    "is in debt",
];

/// A run of eight or more digits (spaces and dashes inside it allowed): a card, account or
/// phone-like number.
fn has_long_number(text: &str) -> bool {
    let mut run = 0usize;
    let mut last_digit = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            last_digit = true;
        } else if (c == ' ' || c == '-') && last_digit {
            // a separator inside a number keeps the run, but does not extend it
            last_digit = false;
        } else {
            if run >= 8 {
                return true;
            }
            run = 0;
            last_digit = false;
        }
        if run >= 8 {
            return true;
        }
    }
    run >= 8
}

const FIRST_PERSON: &[&str] = &["i", "im", "ive", "my", "me", "mine", "myself", "ill", "id"];

const INTERROGATIVES: &[&str] = &[
    "what", "when", "where", "which", "who", "whom", "whose", "why", "how", "did", "do", "does",
    "was", "were", "is", "are", "can", "could", "will", "would", "should", "have", "has", "tell",
    "show", "give", "open", "play", "set", "remind", "search", "find", "please",
];

const IDENTITY: &[&str] = &[
    "my name is",
    "call me",
    "i am called",
    "im called",
    "people call me",
];
const PLAN_WORDS: &[&str] = &[
    "tomorrow",
    "tonight",
    "next week",
    "next month",
    "this weekend",
    "on monday",
    "on tuesday",
    "on wednesday",
    "on thursday",
    "on friday",
    "on saturday",
    "on sunday",
    "i have an interview",
    "i have an exam",
    "i have a meeting",
    "i have a presentation",
    "i have a deadline",
    "my interview",
    "my exam",
    "my presentation",
    "my flight",
    "my trip",
    "my deadline",
    "my appointment",
    "im going to",
    "i am going to",
    "im travelling",
    "im traveling",
    "im moving",
    "i start",
    "i am starting",
    "im starting",
];
const FEELING: &[&str] = &[
    "i feel",
    "im feeling",
    "i am feeling",
    "im nervous",
    "im anxious",
    "im excited",
    "im worried",
    "im happy",
    "im sad",
    "im stressed",
    "im scared",
    "im proud",
    "im lonely",
    "im tired",
    "im angry",
    "im upset",
    "im bored",
    "im thrilled",
];
const PEOPLE: &[&str] = &[
    "my wife",
    "my husband",
    "my girlfriend",
    "my boyfriend",
    "my partner",
    "my mom",
    "my mum",
    "my mother",
    "my dad",
    "my father",
    "my brother",
    "my sister",
    "my son",
    "my daughter",
    "my dog",
    "my cat",
    "my friend",
    "my roommate",
    "my boss",
    "my colleague",
    "my parents",
    "my kids",
    "my grandmother",
    "my grandfather",
];
const LIFE: &[&str] = &[
    "i work",
    "i study",
    "im studying",
    "my job",
    "my project",
    "i live",
    "im from",
    "my birthday",
    "my hometown",
    "my degree",
    "my company",
    "my team",
];
const PREFERENCE: &[&str] = &[
    "i love",
    "i like",
    "i hate",
    "i prefer",
    "my favorite",
    "my favourite",
    "i enjoy",
    "i usually",
    "i always",
    "i never",
    "i dont like",
    "i cant stand",
    "im into",
];

fn category_of(norm: &str) -> Option<Category> {
    if any_phrase(norm, IDENTITY) {
        Some(Category::Identity)
    } else if any_phrase(norm, PLAN_WORDS) {
        Some(Category::Plan)
    } else if any_phrase(norm, FEELING) {
        Some(Category::Feeling)
    } else if any_phrase(norm, PEOPLE) {
        Some(Category::People)
    } else if any_phrase(norm, LIFE) {
        Some(Category::Life)
    } else if any_phrase(norm, PREFERENCE) {
        Some(Category::Preference)
    } else {
        None
    }
}

/// What the owner said, checked against the never-by-chance rules.
#[must_use]
pub fn exclusion_of(text: &str, avoid: &[String]) -> Option<Exclusion> {
    let norm = format!(" {} ", words(text).join(" "));
    if guards::is_crisis(text) {
        return Some(Exclusion::Crisis);
    }
    if any_phrase(&norm, SECRETS) {
        return Some(Exclusion::Secret);
    }
    if any_phrase(&norm, NUMBER_WORDS) || has_long_number(text) {
        return Some(Exclusion::Number);
    }
    if any_phrase(&norm, HEALTH) {
        return Some(Exclusion::Health);
    }
    if any_phrase(&norm, OTHERS_PRIVATE) {
        return Some(Exclusion::OthersPrivate);
    }
    if avoid.iter().any(|t| {
        let t = words(t).join(" ");
        !t.is_empty() && has_phrase(&norm, &t)
    }) {
        return Some(Exclusion::SensitiveTopic);
    }
    None
}

/// Whether a question or a command (never a fact about the owner).
fn is_question_or_command(text: &str, w: &[String]) -> bool {
    text.contains('?')
        || w.first()
            .is_some_and(|f| INTERROGATIVES.contains(&f.as_str()))
}

/// The fact as it is stored: the owner's own sentence, trimmed, with the marker.
fn stored_form(text: &str) -> String {
    let t = text.trim().trim_end_matches(['.', '!', ' ']).trim();
    format!("{NOTICED_MARKER}{t}")
}

/// Shows a stored fact the way she says it: "(noticed) ..." becomes "I noticed ...".
#[must_use]
pub fn spoken_form(stored: &str) -> String {
    stored
        .strip_prefix(NOTICED_MARKER)
        .map_or_else(|| stored.to_string(), |rest| format!("I noticed: {rest}"))
}

/// The fact without the marker, for the model's remembered-facts block.
#[must_use]
pub fn plain_form(stored: &str) -> &str {
    stored.strip_prefix(NOTICED_MARKER).unwrap_or(stored)
}

impl MemoryRule {
    /// A rule for this persona's targets (`memory.chance_target_low` and `_high`).
    #[must_use]
    pub fn new(persona: &Persona, seed: u64) -> Self {
        let MemoryConfig {
            chance_target_low: low,
            chance_target_high: high,
            ..
        } = persona.memory.clone();
        Self {
            rng: Rng::new(seed ^ 0xC0FF_EE00_5EED_0005),
            low,
            high,
            avoid: persona.avoid_topics.clone(),
            counters: MemoryCounters::default(),
        }
    }

    /// Counts for the health report.
    #[must_use]
    pub const fn counters(&self) -> MemoryCounters {
        self.counters
    }

    /// The share of considered statements kept so far (0 before any).
    #[must_use]
    pub fn saved_share(&self) -> f64 {
        if self.counters.considered == 0 {
            0.0
        } else {
            self.counters.saved as f64 / self.counters.considered as f64
        }
    }

    /// Decides what to do with one thing the owner said. `directed` is false when it may not
    /// have been the owner speaking to her.
    pub fn decide(&mut self, text: &str, directed: bool) -> Decision {
        if !directed {
            self.counters.ambient += 1;
            return Decision::Skip(Skip::Ambient);
        }
        if let Some(why) = exclusion_of(text, &self.avoid) {
            self.counters.excluded += 1;
            return Decision::Skip(Skip::Excluded(why));
        }
        let w = words(text);
        if w.len() < MIN_WORDS || w.len() > MAX_WORDS || text.len() > MAX_FACT_BYTES {
            return Decision::Skip(Skip::WrongSize);
        }
        if is_question_or_command(text, &w) || !w.iter().any(|x| FIRST_PERSON.contains(&x.as_str()))
        {
            return Decision::Skip(Skip::NotPersonal);
        }
        let norm = format!(" {} ", w.join(" "));
        let Some(category) = category_of(&norm) else {
            return Decision::Skip(Skip::NotPersonal);
        };
        self.counters.considered += 1;

        // The target is the middle of the owner's 40 to 50 percent. The weight makes a name or a
        // plan likelier than a taste, and a small controller keeps the long-run share inside the
        // band whatever mix of statements the owner makes.
        let target = f64::midpoint(self.low, self.high);
        let correction = if self.counters.considered > WARMUP {
            (1.0 + 12.0 * (target - self.saved_share())).clamp(0.1, 3.0)
        } else {
            1.0
        };
        let p = (target * category.weight() * correction).clamp(0.02, 0.98);
        if !self.rng.chance(p) {
            self.counters.chance_skipped += 1;
            return Decision::Skip(Skip::Chance);
        }
        self.counters.saved += 1;
        Decision::Save(Keep {
            fact: stored_form(text),
            category,
            acknowledge: self.rng.chance(ACK_CHANCE),
        })
    }
}

/// Whether the owner asks what she remembers about them.
#[must_use]
pub fn asks_what_i_remember(text: &str) -> bool {
    let w = words(text);
    if w.is_empty() || w.len() > 9 {
        return false;
    }
    let j = w.join(" ");
    const CUES: [&str; 12] = [
        "what do you remember",
        "what do you remember about me",
        "what do you know about me",
        "what have you remembered",
        "what did you remember",
        "what do you remember of me",
        "what have you noticed about me",
        "what do you know of me",
        "tell me what you remember",
        "list what you remember",
        "show me what you remember",
        "what have you saved about me",
    ];
    CUES.iter()
        .any(|c| j == *c || j.starts_with(&format!("{c} ")) || j.ends_with(&format!(" {c}")))
}

/// What she says when she lists what she remembers: at most five facts, noticed ones marked.
#[must_use]
pub fn memory_list_line(stored_facts: &[String]) -> String {
    if stored_facts.is_empty() {
        return "I'm not holding on to anything about you yet. Say remember, and what, and I will."
            .to_string();
    }
    let shown: Vec<String> = stored_facts
        .iter()
        .take(5)
        .map(|f| spoken_form(f))
        .collect();
    let more = stored_facts.len().saturating_sub(5);
    let mut line = format!("Here is what I have: {}.", shown.join("; "));
    if more > 0 {
        line.push_str(&format!(" And {more} more."));
    }
    line.push_str(" Say forget that, and what, and I'll drop it.");
    line
}
