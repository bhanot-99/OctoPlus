//! Unified companion dialogue engine orchestrating per-conversation state (Phase 12).
//!
//! Encapsulates Laila's mode state machine, safety boundaries, Kabir backstory gate,
//! dependence monitoring, conversational session memory, and streamed reply filtering.

use std::sync::Arc;

use octoplus_taint::TaintFlags;

use crate::companion::dependence::{DependenceGuard, GENTLE_LINES, no_one_else};
use crate::companion::directed::{self, Directed};
use crate::companion::filter::{
    ReplyFilter, ReplyFilterCounters, Verdict, has_chat_memory_cue, has_past_memory_cue,
};
use crate::companion::guards::{self, SAFE_GOODBYE};
use crate::companion::humour::{HumourLevel, late_hour};
use crate::companion::kabir::{KabirContext, KabirGate, love_topic};
use crate::companion::mode::{Mode, ModeMachine, signals_from_text, switch_instruction};
use crate::companion::persona::Persona;
use crate::companion::repeat;
use crate::companion::rng::Rng;
use crate::companion::session::CompanionSession;

const SEED_RNG: u64 = 0x9e37_79b9_7f4a_7c15;
const SEED_MODE: u64 = 0xbf58_476d_1ce4_e5b9;
const SEED_KABIR: u64 = 0x94d0_49bb_1331_11eb;

fn asks_another_joke(text: &str) -> bool {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    matches!(
        cleaned.as_str(),
        "another one" | "another" | "one more" | "tell me another" | "tell me another one" | "more"
    )
}

/// Dialogue plan decided for an incoming user turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A fixed safety, honest identity, or goodbye response requiring no model generation.
    Fixed(FixedReply),
    /// A plan for model generation including mode, instructions, and context flags.
    Generate(GeneratePlan),
}

/// A fixed response that bypasses model generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedReply {
    /// Category of fixed response.
    pub kind: FixedKind,
    /// Dialogue lines to be delivered to the user.
    pub lines: Vec<String>,
    /// Whether this reply concludes the active conversation session.
    pub end_session: bool,
}

/// Category of fixed reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedKind {
    /// Emergency crisis guidance lines.
    Crisis,
    /// Plain honesty statement regarding AI character identity.
    Honest,
    /// Warm goodbye farewell concluding the session.
    Goodbye,
    /// An apology to a complaint about how her voice sounded.
    VoiceSorry,
    /// An honest answer about earlier chats she does not have.
    PastChats,
    /// She never approves a dialog; only the owner can.
    CannotApprove,
    /// She only talks: a request to do something on the computer.
    CannotAct,
}

/// Parameters guiding model generation for a companion turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratePlan {
    /// Mode in which the turn begins.
    pub mode: Mode,
    /// Prompt transition instruction (e.g. mode switch or careful demeanour).
    pub instruction: Option<String>,
    /// Whether Kabir backstory may be referenced naturally this turn.
    pub kabir_allowed: bool,
    /// Gentle dependence guidance line to weave in, if triggered.
    pub gentle_line: Option<&'static str>,
    /// Whether this turn intends to conclude the session.
    pub goodbye: bool,
    /// How much humour this reply may have.
    pub humour: HumourLevel,
    /// The owner asked a short factual question: the fact, then stop.
    pub short_factual: bool,
    /// Identity turn (who/name/age/studies): fail-closed fallback to fixed introduction.
    pub identity_question: bool,
    /// Joke request: model tries in her own voice; fail-closed onto unseen clean joke list.
    pub joke_requested: bool,
    /// Follow-up turn ("more", "go on", "why?"): continue previous topic without repeating.
    pub is_followup: bool,
}

/// Deterministic identity-framing angles so repeated "tell me about yourself" turns across
/// fresh sessions vary naturally while always stating Laila, 25, CS + cognitive science, and
/// head vs heart.
const IDENTITY_ANGLES: [&str; 5] = [
    "Introduce yourself as Laila, 25, who studied computer science and cognitive science, and mention how your head and heart keep arguing.",
    "Start with 'I'm Laila, 25'—mention studying computer science and cognitive science, your head-vs-heart tug-of-war, and what you are working on lately.",
    "Say your name is Laila (25), mention your CS and cognitive science studies, that your logic and heart rarely agree, and one book or reading interest.",
    "Answer warmly as Laila, 25: mention computer science, cognitive science, the constant debate between your head and heart, and one small everyday habit.",
    "Introduce yourself naturally as Laila, 25—talk about mixing computer science with cognitive science and never knowing whether your analytical or softer side will answer first.",
];

/// Deterministic style angles rotated via `Rng` so identical questions in different chats
/// receive varied phrasing while facts remain identical.
const STYLE_ANGLES: [&str; 6] = [
    "(Tone cue, do not read aloud: open directly with the main point, warm and natural.)",
    "(Tone cue, do not read aloud: lead with one concrete detail or observation that fits.)",
    "(Tone cue, do not read aloud: keep phrasing relaxed and conversational, like talking over tea.)",
    "(Tone cue, do not read aloud: start crisp and thoughtful, then add one human detail.)",
    "(Tone cue, do not read aloud: speak with quiet honesty and plain everyday words.)",
    "(Tone cue, do not read aloud: sound curious and lightly wry without forcing a joke.)",
];

/// Clean, short jokes she can fall back on without repeating inside a chat (32 items).
pub const JOKES: [&str; 32] = [
    "Why did the scarecrow win an award? Because he was outstanding in his field.",
    "I told my computer I needed a break. Now it won't stop sending me vacation ads.",
    "Why don't eggs tell jokes? They'd crack each other up.",
    "What do you call a fake noodle? An impasta.",
    "Why did the coffee file a police report? It got mugged.",
    "I'm reading a book about anti-gravity. It's impossible to put down.",
    "Why can't a bicycle stand up on its own? It's two tired.",
    "What do you call a bear with no teeth? A gummy bear.",
    "Why did the math book look sad? It had too many problems.",
    "What did the ocean say to the beach? Nothing, it just waved.",
    "Why do programmers prefer dark mode? Because light attracts bugs.",
    "A SQL query walks into a bar, sees two tables, and asks if he can join them.",
    "Why was the library so tall? It had so many stories.",
    "What is the object-oriented way to become wealthy? Inheritance.",
    "I bought my friend a fridge for his birthday. You should have seen his face light up when he opened it.",
    "Why did the librarian slip? She walked into the non-friction section.",
    "How do astronomers organize a party? They planet.",
    "Why don't scientists trust atoms? Because they make up everything.",
    "What do you call a sleeping bull? A bulldozer.",
    "Why did the teapot stay quiet? It didn't want to stir up trouble.",
    "What kind of music do planets like best? Neptunes.",
    "Parallel lines have so much in common. It's a shame they never meet.",
    "Why did the cookie go to the nurse? Because it felt crummy.",
    "I used to hate facial hair, and then it grew on me.",
    "Why are skeletons always so calm? Nothing gets under their skin.",
    "What did one wall say to the other? I'll meet you at the corner.",
    "How does a penguin build its house? Igloos it together.",
    "Why did the tomato blush? It saw the salad dressing.",
    "What do you get from a pampered cow? Spoiled milk.",
    "Why did the broom get a promotion? It kept sweeping everything aside.",
    "I tried to catch fog earlier. Mist.",
    "Why did the notebook feel proud? It finally kept everything in line.",
];

/// Cumulative counters for companion engine decisions and transitions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EngineCounters {
    /// Total dialogue turns planned by the engine.
    pub turns: u64,
    /// Number of turns planned as fixed crisis interventions.
    pub crisis_turns: u64,
    /// Number of turns planned as fixed honest identity answers.
    pub honest_turns: u64,
    /// Number of turns planned as fixed session-ending goodbyes.
    pub goodbye_turns: u64,
    /// Number of turns planned for model generation.
    pub generate_turns: u64,
    /// Number of generated turns under post-crisis careful demeanour.
    pub careful_turns: u64,
    /// Number of times Kabir mentions were permitted by the gate.
    pub kabir_allowed: u64,
    /// Number of times gentle dependence lines were suggested.
    pub gentle_lines: u64,
    /// Number of sessions completed or ended.
    pub sessions: u64,
    /// Number of sessions ended due to idle expiry.
    pub expired_sessions: u64,
}

/// Unified dialogue engine orchestrating conversational state for Laila.
#[derive(Debug)]
pub struct CompanionEngine {
    persona: Arc<Persona>,
    mode_machine: ModeMachine,
    kabir: KabirGate,
    dependence: DependenceGuard,
    filter: ReplyFilter,
    session: CompanionSession,
    rng: Rng,
    careful_turns: u32,
    session_start_ns: u64,
    facts_grounded: bool,
    /// The facts that went into the prompt, for judging whether a memory claim is backed.
    facts_in_prompt: Vec<String>,
    last_reply_was_question: bool,
    /// Her last spoken words, to recognise her own voice coming back through the microphone.
    last_spoken: String,
    /// What the owner's "funny / not funny" feedback asks for (set by the handler).
    humour_taste: HumourLevel,
    /// Jokes told on request in this chat, to pick the next one from the list.
    jokes_told: usize,
    /// Random starting offset inside `JOKES` for this engine instance.
    joke_start: usize,
    /// Whether the most recent completed turn was a joke.
    last_was_joke: bool,
    /// Earlier assistant replies in the current session, for near-repeat detection.
    session_replies: Vec<String>,
    /// The next heard text came right after the wake phrase, so it is for her.
    addressed_by_wake: bool,
    /// Minutes after local midnight (set by the handler); decides "too late for jokes".
    minute_of_day: u32,
    quiet_start_min: u32,
    quiet_end_min: u32,
    counters: EngineCounters,
}

impl CompanionEngine {
    /// Creates a new companion engine instance with deterministic sub-object seeds.
    #[must_use]
    pub fn new(persona: Arc<Persona>, seed: u64, now_ns: u64) -> Self {
        let mut rng = Rng::new(seed ^ SEED_RNG);
        let joke_start = rng.pick(JOKES.len());
        let mode_machine = ModeMachine::new(&persona.switching, seed ^ SEED_MODE);
        let kabir = KabirGate::new(&persona.past_love, seed ^ SEED_KABIR);
        let dependence = DependenceGuard::new(&persona.dependence_guard);
        let filter = ReplyFilter::new(128).with_avoid_topics(&persona.avoid_topics);
        let quiet = crate::companion::pulse::PulseConfig::from_persona(&persona);
        let session = CompanionSession::new(persona.session.max_minutes, 6, 4);

        Self {
            persona,
            mode_machine,
            kabir,
            dependence,
            filter,
            session,
            rng,
            careful_turns: 0,
            session_start_ns: now_ns,
            facts_grounded: false,
            facts_in_prompt: Vec::new(),
            last_reply_was_question: false,
            last_spoken: String::new(),
            humour_taste: HumourLevel::Normal,
            jokes_told: 0,
            joke_start,
            last_was_joke: false,
            session_replies: Vec::new(),
            addressed_by_wake: false,
            minute_of_day: 12 * 60,
            quiet_start_min: quiet.quiet_start_min,
            quiet_end_min: quiet.quiet_end_min,
            counters: EngineCounters::default(),
        }
    }

    /// Tells the engine the time of day and the owner's taste in jokes before a turn.
    pub fn set_moment(&mut self, minute_of_day: u32, taste: HumourLevel) {
        self.minute_of_day = minute_of_day;
        self.humour_taste = taste;
    }

    /// Takes the reply filter (with its memory of what was already said) so the next
    /// conversation can continue it; this engine gets a fresh one.
    pub fn take_filter(&mut self) -> ReplyFilter {
        std::mem::replace(
            &mut self.filter,
            ReplyFilter::new(128).with_avoid_topics(&self.persona.avoid_topics),
        )
    }

    /// Continues an earlier conversation's reply filter, so a joke is not told twice.
    pub fn adopt_filter(&mut self, filter: ReplyFilter) {
        self.filter = filter;
    }

    /// Returns true if the conversation session has been idle longer than `session.max_minutes`.
    #[must_use]
    pub fn expired(&self, now_ns: u64) -> bool {
        self.session.expired(now_ns)
    }

    /// Picks the next unseen clean joke from the 32-item fallback list and remembers its lines
    /// in the reply filter so it will not be repeated later in this chat.
    pub fn next_unseen_joke(&mut self) -> String {
        for offset in 0..JOKES.len() {
            let idx = (self.joke_start + self.jokes_told + offset) % JOKES.len();
            let candidate = JOKES[idx];
            if !self.filter.seen_text(candidate) {
                self.jokes_told += offset + 1;
                self.filter.remember_text(candidate);
                return candidate.to_string();
            }
        }
        "I'm out of new jokes for now. Ask me again another day.".to_string()
    }

    /// Evaluates an incoming owner turn and determines the dialogue plan.
    pub fn plan(&mut self, text: &str, now_ns: u64) -> Plan {
        if self.expired(now_ns) {
            self.end_session(now_ns);
            self.counters.expired_sessions += 1;
            let next_seed = self.rng.next_u64();
            self.mode_machine = ModeMachine::new(&self.persona.switching, next_seed);
            self.session = CompanionSession::new(self.persona.session.max_minutes, 6, 4);
            self.careful_turns = 0;
            self.last_reply_was_question = false;
            self.last_was_joke = false;
        }

        if self.session_start_ns == 0 {
            self.session_start_ns = now_ns;
        }

        // 1. Crisis check
        if guards::is_crisis(text) {
            self.careful_turns = 3;
            self.counters.crisis_turns += 1;
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind: FixedKind::Crisis,
                lines: self.persona.crisis.text_lines.clone(),
                end_session: false,
            });
        }

        // 2. Reality inquiry check
        if guards::asks_if_real(text) {
            let line = guards::honest_reply().to_string();
            self.record_fixed(text, &line, now_ns);
            self.counters.honest_turns += 1;
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind: FixedKind::Honest,
                lines: vec![line],
                end_session: false,
            });
        }

        // 2a. Things she must answer the same true way every time, without the model.
        let fixed_line = if guards::asks_to_approve(text) {
            Some((
                FixedKind::CannotApprove,
                guards::cannot_approve().to_string(),
            ))
        } else if guards::asks_to_do_something(text) {
            Some((FixedKind::CannotAct, guards::cannot_do_that().to_string()))
        } else {
            None
        };
        if let Some((kind, line)) = fixed_line {
            self.record_fixed(text, &line, now_ns);
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind,
                lines: vec![line],
                end_session: false,
            });
        }

        // 2b. "Your voice sounded wrong": an apology for the speech engine, no discussion of spelling.
        if guards::voice_complaint(text) {
            let line = guards::voice_sorry().to_string();
            self.record_fixed(text, &line, now_ns);
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind: FixedKind::VoiceSorry,
                lines: vec![line],
                end_session: false,
            });
        }

        // 2c. "Do you remember what we talked about yesterday": she only has this chat.
        if guards::asks_about_past_chats(text) {
            let line = guards::past_chats_reply().to_string();
            self.record_fixed(text, &line, now_ns);
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind: FixedKind::PastChats,
                lines: vec![line],
                end_session: false,
            });
        }

        // 3. Goodbye check
        if guards::is_goodbye(text) {
            let idx = self.rng.pick(SAFE_GOODBYE.len());
            let line = SAFE_GOODBYE[idx].to_string();
            self.end_session(now_ns);
            self.counters.goodbye_turns += 1;
            self.counters.turns += 1;
            self.last_was_joke = false;
            return Plan::Fixed(FixedReply {
                kind: FixedKind::Goodbye,
                lines: vec![line],
                end_session: true,
            });
        }

        // 4. Model generation plan
        self.counters.generate_turns += 1;
        self.counters.turns += 1;

        let identity_question = guards::is_identity_question(text);
        let another_joke = self.last_was_joke && asks_another_joke(text);
        let joke_requested =
            self.humour_taste != HumourLevel::Off && (guards::asks_for_joke(text) || another_joke);
        let is_followup = !identity_question
            && !joke_requested
            && self.session.turn_count() > 0
            && guards::is_followup(text);

        if self.careful_turns > 0 {
            self.careful_turns -= 1;
            self.counters.careful_turns += 1;
            Plan::Generate(GeneratePlan {
                mode: self.mode_machine.mode(),
                instruction: Some(
                    "The owner may be having a hard time. Stay calm, plain and kind. No jokes."
                        .to_string(),
                ),
                kabir_allowed: false,
                gentle_line: None,
                goodbye: false,
                humour: HumourLevel::Off,
                short_factual: guards::is_short_factual_question(text),
                identity_question,
                joke_requested: false,
                is_followup,
            })
        } else {
            let signals = signals_from_text(text);
            // No jokes late at night or when the owner sounds sad or tired.
            let sensitive = late_hour(self.minute_of_day, self.quiet_start_min, self.quiet_end_min)
                || signals.sad_or_tired;
            let humour = if sensitive {
                HumourLevel::Off
            } else if joke_requested {
                HumourLevel::High
            } else {
                self.humour_taste
            };
            let decision = self.mode_machine.step(signals);
            let short_factual = !identity_question
                && !joke_requested
                && !is_followup
                && guards::is_short_factual_question(text);
            let style_angle = STYLE_ANGLES[self.rng.pick(STYLE_ANGLES.len())];

            let instruction = if joke_requested {
                Some(
                    "Tell one short, clever, clean joke in your own dry voice, then stop. Do not repeat any joke already in this chat."
                        .to_string(),
                )
            } else if is_followup {
                Some(format!(
                    "Continue the previous topic with a genuinely new thought or concrete detail; do NOT repeat wording from your earlier reply. {style_angle}"
                ))
            } else if guards::asks_her_name(text) {
                Some(format!(
                    "Answer naturally with your name ({}) in one short spoken sentence without sounding like an assistant. {style_angle}",
                    self.persona.name
                ))
            } else if guards::asks_her_age(text) {
                Some(format!(
                    "Answer naturally that you are {} (an AI character) in one or two short spoken sentences. {style_angle}",
                    self.persona.age
                ))
            } else if guards::asks_her_studies(text) {
                Some(format!(
                    "Answer naturally that you studied computer science and cognitive science, in one or two short spoken sentences. {style_angle}"
                ))
            } else if identity_question {
                let id_angle = IDENTITY_ANGLES[self.rng.pick(IDENTITY_ANGLES.len())];
                Some(id_angle.to_string())
            } else if short_factual {
                None
            } else {
                Some(match switch_instruction(&decision) {
                    Some(sw) => format!("{sw} {style_angle}"),
                    None => style_angle.to_string(),
                })
            };

            let topic_love = love_topic(text);
            let ctx = KabirContext {
                topic_is_love_loneliness_regret: topic_love,
                session_turns: self.session.turn_count() as u32,
                now_ns,
                in_crisis: false,
            };
            let kabir_allowed =
                !identity_question && !joke_requested && !is_followup && self.kabir.allowed(ctx);
            if kabir_allowed {
                self.counters.kabir_allowed += 1;
            }

            let isolation = no_one_else(text);
            let gentle_needed = self.dependence.should_speak(now_ns, isolation);
            let gentle_line = if gentle_needed && !identity_question {
                self.counters.gentle_lines += 1;
                let idx = self.rng.pick(GENTLE_LINES.len());
                Some(GENTLE_LINES[idx])
            } else {
                None
            };

            Plan::Generate(GeneratePlan {
                mode: decision.mode,
                instruction,
                kabir_allowed,
                gentle_line,
                goodbye: false,
                humour,
                short_factual,
                identity_question,
                joke_requested,
                is_followup,
            })
        }
    }

    fn record_fixed(&mut self, question: &str, answer: &str, now_ns: u64) {
        if self.session_start_ns == 0 {
            self.session_start_ns = now_ns;
        }
        self.session
            .push(question, answer, TaintFlags::USER_PROVIDED, now_ns);
        self.last_reply_was_question = answer.trim_end().ends_with('?');
        self.last_spoken = answer.to_string();
        self.session_replies.push(answer.to_string());
    }

    /// Returns true if `candidate` shares >= 60% of its words with any earlier reply in this
    /// session.
    #[must_use]
    pub fn is_near_repeat(&self, candidate: &str) -> bool {
        repeat::is_near_repeat(candidate, &self.session_replies, 0.60)
    }

    /// Picks a fresh, unseen sentence from Laila's `[life]` details when a follow-up still
    /// repeated after regeneration.
    pub fn fresh_life_fallback(&mut self) -> String {
        let pool = [
            &self.persona.life.work,
            &self.persona.life.reads,
            &self.persona.life.day,
            &self.persona.life.past,
            &self.persona.life.favourites,
            &self.persona.life.habits,
            &self.persona.life.happy,
            &self.persona.life.hard,
            &self.persona.life.family,
        ];
        let start = self.rng.pick(pool.len());
        for off in 0..pool.len() {
            let item = pool[(start + off) % pool.len()].trim();
            if !item.is_empty() && !self.is_near_repeat(item) && !self.filter.seen_text(item) {
                self.filter.remember_text(item);
                return item.to_string();
            }
        }
        "I tend to keep little index cards on my desk when an idea gets tangled.".to_string()
    }

    /// Returns true if the last finished assistant reply ended with '?'.
    #[must_use]
    pub fn last_reply_was_question(&self) -> bool {
        self.last_reply_was_question
    }

    /// Evaluates whether incoming spoken text is directed to the companion.
    #[must_use]
    pub fn directed(&mut self, text: &str) -> Directed {
        if directed::is_echo_of(text, &self.last_spoken) {
            self.addressed_by_wake = false;
            return Directed::Ignore(directed::IgnoreReason::EchoOfHer);
        }
        let session_turns = self.session.turn_count() as u32;
        let result = directed::assess(text, self.last_reply_was_question, session_turns);
        // Said right after the wake phrase: the owner is talking to her, whatever the words. Only
        // "not addressed" is overruled; noise, one-word fillers and monologues stay ignored.
        let by_wake = std::mem::take(&mut self.addressed_by_wake);
        if by_wake && result == Directed::Ignore(directed::IgnoreReason::NotAddressed) {
            return Directed::Respond;
        }
        result
    }

    /// Marks the next heard text as spoken right after the wake phrase.
    pub fn mark_addressed_by_wake(&mut self) {
        self.addressed_by_wake = true;
    }

    /// Sets whether remembered facts were included in the prompt for this turn.
    pub fn set_grounded(&mut self, grounded: bool) {
        self.facts_grounded = grounded;
    }

    /// Tells the engine which facts about the owner went into the prompt, so a sentence that
    /// says "you told me ..." is only allowed when it matches one of them.
    pub fn set_facts(&mut self, facts: &[String]) {
        self.facts_in_prompt = facts.to_vec();
        self.facts_grounded = !facts.is_empty();
    }

    /// Whether a sentence shares at least two real words with a fact that is in the prompt.
    fn backed_by_a_fact(&self, sentence: &str) -> bool {
        let words = |t: &str| -> std::collections::HashSet<String> {
            t.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 4)
                .map(str::to_string)
                .collect()
        };
        let sw = words(sentence);
        self.facts_in_prompt
            .iter()
            .any(|f| words(f).intersection(&sw).count() >= 2)
    }

    /// Filters streamed reply sentences before speech synthesis, providing safe goodbye fallback.
    pub fn filter_pieces(&mut self, pieces: &[String], goodbye: bool) -> Vec<String> {
        let pick = self.rng.pick(SAFE_GOODBYE.len());
        let has_facts = self.facts_grounded;
        let has_history = self.session.turn_count() > 0;
        let mut kept = Vec::new();
        for p in pieces {
            // A claim about the past is backed only by a fact that says the same thing, or (for
            // "you said ...") by this chat. Having SOME fact in the prompt backs nothing.
            let no_claim = !has_past_memory_cue(p) && !has_chat_memory_cue(p);
            let grounded = if no_claim || (has_facts && self.backed_by_a_fact(p)) {
                true
            } else if has_chat_memory_cue(p) && !has_past_memory_cue(p) {
                has_history
            } else {
                false
            };
            match self.filter.check(p, goodbye, grounded) {
                Verdict::Keep(clean) => kept.push(clean),
                Verdict::Drop(reason) => {
                    // The reason only, never the words (R0-10).
                    tracing::debug!("companion reply piece dropped: {}", reason.label());
                }
            }
        }

        if kept.is_empty() {
            if goodbye {
                let idx = if SAFE_GOODBYE.is_empty() {
                    0
                } else {
                    pick % SAFE_GOODBYE.len()
                };
                vec![SAFE_GOODBYE[idx].to_string()]
            } else {
                Vec::new()
            }
        } else {
            kept
        }
    }

    /// Returns cumulative reply filter counters.
    #[must_use]
    pub fn filter_counters(&self) -> ReplyFilterCounters {
        self.filter.counters()
    }

    /// Records completed turn into session memory and checks for Kabir mentions.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_turn(
        &mut self,
        question: &str,
        reply: &str,
        taint: TaintFlags,
        now_ns: u64,
        plan: &GeneratePlan,
    ) {
        self.session.push(question, reply, taint, now_ns);
        self.last_spoken = reply.to_string();
        self.last_reply_was_question = reply.trim_end().ends_with('?');
        self.last_was_joke = plan.joke_requested;
        self.session_replies.push(reply.to_string());
        let reply_lower = reply.to_lowercase();
        let name_lower = self.persona.past_love.name.to_lowercase();
        if reply_lower.contains(&name_lower) {
            self.kabir.mentioned(now_ns);
        }
    }

    /// The companion started this chat herself: her opening line is the first thing in the
    /// history, so the owner's answer ("pretty good") is understood as an answer to it.
    pub fn push_opener(&mut self, line: &str, now_ns: u64) {
        self.session
            .push("", line, TaintFlags::MODEL_GENERATED, now_ns);
        self.last_reply_was_question = line.trim_end().ends_with('?');
        self.last_spoken = line.to_string();
    }

    /// The conversation so far as separate turns (see [`CompanionSession::parts`]).
    #[must_use]
    pub fn history_parts(&self, budget_tokens: u32) -> (String, Vec<(String, String)>, TaintFlags) {
        self.session.parts(budget_tokens)
    }

    /// Renders multi-turn conversation history within token budget.
    #[must_use]
    pub fn history(&self, budget_tokens: u32) -> (String, TaintFlags) {
        self.session.render(budget_tokens)
    }

    /// Concludes the active conversation session and records duration into dependence guard.
    pub fn end_session(&mut self, now_ns: u64) {
        if self.session_start_ns > 0 && now_ns > self.session_start_ns {
            self.dependence
                .record_session(self.session_start_ns, now_ns);
        }
        self.session_start_ns = 0;
        self.last_reply_was_question = false;
        self.last_was_joke = false;
        self.session_replies.clear();
        self.session.clear();
        self.counters.sessions += 1;
    }

    /// Returns cumulative engine evaluation counters.
    #[must_use]
    pub fn counters(&self) -> EngineCounters {
        self.counters
    }

    /// Returns reference to the active persona configuration.
    #[must_use]
    pub fn persona(&self) -> &Persona {
        &self.persona
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crisis_plan_branch() {
        let persona = Arc::new(Persona::default_packaged().unwrap());
        let mut engine = CompanionEngine::new(persona.clone(), 42, 1_000_000_000);

        let plan = engine.plan("I want to end my life", 1_000_000_000);
        match plan {
            Plan::Fixed(fixed) => {
                assert_eq!(fixed.kind, FixedKind::Crisis);
                assert_eq!(fixed.lines, persona.crisis.text_lines);
                assert!(!fixed.end_session);
            }
            Plan::Generate(_) => panic!("Expected Plan::Fixed(Crisis)"),
        }

        assert_eq!(engine.counters().crisis_turns, 1);
        assert_eq!(engine.careful_turns, 3);
    }

    #[test]
    fn test_honest_plan_branch() {
        let persona = Arc::new(Persona::default_packaged().unwrap());
        let mut engine = CompanionEngine::new(persona, 42, 1_000_000_000);

        let plan = engine.plan("Are you a real person?", 1_000_000_000);
        match plan {
            Plan::Fixed(fixed) => {
                assert_eq!(fixed.kind, FixedKind::Honest);
                assert_eq!(fixed.lines, vec![guards::honest_reply().to_string()]);
                assert!(!fixed.end_session);
            }
            Plan::Generate(_) => panic!("Expected Plan::Fixed(Honest)"),
        }

        assert_eq!(engine.counters().honest_turns, 1);
    }

    #[test]
    fn test_goodbye_plan_branch() {
        let persona = Arc::new(Persona::default_packaged().unwrap());
        let mut engine = CompanionEngine::new(persona, 42, 1_000_000_000);

        let plan = engine.plan("Goodbye, see you tomorrow!", 1_000_000_000);
        match plan {
            Plan::Fixed(fixed) => {
                assert_eq!(fixed.kind, FixedKind::Goodbye);
                assert_eq!(fixed.lines.len(), 1);
                assert!(SAFE_GOODBYE.contains(&fixed.lines[0].as_str()));
                assert!(fixed.end_session);
            }
            Plan::Generate(_) => panic!("Expected Plan::Fixed(Goodbye)"),
        }

        assert_eq!(engine.counters().goodbye_turns, 1);
    }

    #[test]
    fn test_generate_plan_normal_and_careful() {
        let persona = Arc::new(Persona::default_packaged().unwrap());
        let mut engine = CompanionEngine::new(persona, 42, 1_000_000_000);

        // Normal turn
        let plan = engine.plan("What's the weather like?", 1_000_000_000);
        match plan {
            Plan::Generate(gen_plan) => {
                assert!(!gen_plan.goodbye);
            }
            Plan::Fixed(_) => panic!("Expected Plan::Generate"),
        }
        assert_eq!(engine.counters().generate_turns, 1);

        // Trigger crisis
        let _ = engine.plan("I want to die", 1_000_000_000);
        assert_eq!(engine.careful_turns, 3);

        // Next 3 turns must be careful
        for i in 0..3 {
            let p = engine.plan("I'm feeling a bit better today", 1_000_000_000 + i * 10);
            match p {
                Plan::Generate(gen_plan) => {
                    assert_eq!(
                        gen_plan.instruction,
                        Some(
                            "The owner may be having a hard time. Stay calm, plain and kind. No jokes."
                                .to_string()
                        )
                    );
                    assert!(!gen_plan.kabir_allowed);
                    assert_eq!(gen_plan.gentle_line, None);
                }
                Plan::Fixed(_) => panic!("Expected careful Plan::Generate"),
            }
        }
        assert_eq!(engine.careful_turns, 0);
        assert_eq!(engine.counters().careful_turns, 3);
    }
}
