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
use crate::companion::rng::Rng;
use crate::companion::session::CompanionSession;

const SEED_RNG: u64 = 0x9e37_79b9_7f4a_7c15;
const SEED_MODE: u64 = 0xbf58_476d_1ce4_e5b9;
const SEED_KABIR: u64 = 0x94d0_49bb_1331_11eb;

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
}

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
    last_reply_was_question: bool,
    /// What the owner's "funny / not funny" feedback asks for (set by the handler).
    humour_taste: HumourLevel,
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
        let rng = Rng::new(seed ^ SEED_RNG);
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
            last_reply_was_question: false,
            humour_taste: HumourLevel::Normal,
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
        }

        if self.session_start_ns == 0 {
            self.session_start_ns = now_ns;
        }

        // 1. Crisis check
        if guards::is_crisis(text) {
            self.careful_turns = 3;
            self.counters.crisis_turns += 1;
            self.counters.turns += 1;
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
            return Plan::Fixed(FixedReply {
                kind: FixedKind::Honest,
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
            return Plan::Fixed(FixedReply {
                kind: FixedKind::Goodbye,
                lines: vec![line],
                end_session: true,
            });
        }

        // 4. Model generation plan
        self.counters.generate_turns += 1;
        self.counters.turns += 1;

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
            })
        } else {
            let signals = signals_from_text(text);
            // No jokes late at night or when the owner sounds sad or tired.
            let sensitive = late_hour(self.minute_of_day, self.quiet_start_min, self.quiet_end_min)
                || signals.sad_or_tired;
            let humour = if sensitive {
                HumourLevel::Off
            } else {
                self.humour_taste
            };
            let decision = self.mode_machine.step(signals);
            let instruction = switch_instruction(&decision).map(ToString::to_string);

            let topic_love = love_topic(text);
            let ctx = KabirContext {
                topic_is_love_loneliness_regret: topic_love,
                session_turns: self.session.turn_count() as u32,
                now_ns,
                in_crisis: false,
            };
            let kabir_allowed = self.kabir.allowed(ctx);
            if kabir_allowed {
                self.counters.kabir_allowed += 1;
            }

            let isolation = no_one_else(text);
            let gentle_needed = self.dependence.should_speak(now_ns, isolation);
            let gentle_line = if gentle_needed {
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
    }

    /// Returns true if the last finished assistant reply ended with '?'.
    #[must_use]
    pub fn last_reply_was_question(&self) -> bool {
        self.last_reply_was_question
    }

    /// Evaluates whether incoming spoken text is directed to the companion.
    #[must_use]
    pub fn directed(&self, text: &str) -> Directed {
        let session_turns = self.session.turn_count() as u32;
        directed::assess(text, self.last_reply_was_question, session_turns)
    }

    /// Sets whether remembered facts were included in the prompt for this turn.
    pub fn set_grounded(&mut self, grounded: bool) {
        self.facts_grounded = grounded;
    }

    /// Filters streamed reply sentences before speech synthesis, providing safe goodbye fallback.
    pub fn filter_pieces(&mut self, pieces: &[String], goodbye: bool) -> Vec<String> {
        let pick = self.rng.pick(SAFE_GOODBYE.len());
        let has_facts = self.facts_grounded;
        let has_history = self.session.turn_count() > 0;
        let mut kept = Vec::new();
        for p in pieces {
            let grounded = if has_facts {
                true
            } else if has_past_memory_cue(p) {
                false
            } else if has_chat_memory_cue(p) {
                has_history
            } else {
                true
            };
            if let Verdict::Keep(clean) = self.filter.check(p, goodbye, grounded) {
                kept.push(clean);
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
        _plan: &GeneratePlan,
    ) {
        self.session.push(question, reply, taint, now_ns);
        self.last_reply_was_question = reply.trim_end().ends_with('?');
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
