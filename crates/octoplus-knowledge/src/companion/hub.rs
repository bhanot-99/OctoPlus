//! The state the Pulse scheduler shares with the chat handler (Phase 12, story C-S03).
//!
//! The runner ([`super::runner`]) ticks the scheduler and starts talks. The chat handler hears
//! what the owner says back. Both need the same [`Pulse`]: an answer to a talk is "accepted", a
//! spoken "not now" or "quiet for an hour" changes the rhythm, and a talk nobody answers within
//! the reply window is "ignored". This module holds that state behind one lock and nothing else:
//! it never touches a socket or a clock.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::companion::filter::{ReplyFilter, Verdict};
use crate::companion::humour::{Feedback, HumourLevel, HumourState, effective_level};
use crate::companion::memory::{Category, Decision, Keep, MemoryCounters, MemoryRule, plain_form};
use crate::companion::persona::Persona;
use crate::companion::pulse::{
    Pulse, PulseCommand, PulseConfig, PulseCounters, PulseDecision, PulseInput, Response,
    apply_command, parse_pulse_command,
};

const NS_PER_S: u64 = 1_000_000_000;

/// How long after a talk was spoken the owner may still answer it. This covers the time the line
/// takes to speak plus the listening window C2 keeps open.
pub const ANSWER_WINDOW_S: u64 = 60;

/// What the owner's words were, as far as the scheduler cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerTurn {
    /// A spoken control for the scheduler ("not now", "quiet for an hour"): answer it with the
    /// fixed line and do not run it through the model.
    Command(PulseCommand),
    /// Anything else. `answered_talk` is true when this was the owner's answer to a talk the
    /// companion had just started.
    Plain {
        /// The owner answered a talk that just started.
        answered_talk: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    started_ns: u64,
}

#[derive(Debug)]
struct Inner {
    pulse: Pulse,
    pending: Option<Pending>,
    opener: Option<(u64, String)>,
    filter: ReplyFilter,
    humour: HumourState,
    memory: MemoryRule,
    followup: Option<(u64, String)>,
}

/// Shared scheduler state.
#[derive(Debug)]
pub struct CompanionHub {
    inner: Mutex<Inner>,
    avoid: Vec<String>,
}

impl CompanionHub {
    /// A hub for this persona. `seed` makes the scheduler's randomness repeatable.
    #[must_use]
    pub fn new(persona: &Persona, seed: u64) -> Arc<Self> {
        Arc::new(Self {
            avoid: persona.avoid_topics.clone(),
            inner: Mutex::new(Inner {
                pulse: Pulse::new(PulseConfig::from_persona(persona), seed),
                pending: None,
                opener: None,
                filter: ReplyFilter::new(128).with_avoid_topics(&persona.avoid_topics),
                humour: HumourState::default(),
                memory: MemoryRule::new(persona, seed),
                followup: None,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One scheduler tick. A talk nobody answered in time is first counted as ignored.
    pub fn tick(&self, input: &PulseInput) -> PulseDecision {
        let mut g = self.lock();
        if let Some(p) = g.pending
            && input.now_ns.saturating_sub(p.started_ns) > ANSWER_WINDOW_S * NS_PER_S
        {
            g.pending = None;
            g.pulse
                .record_response(input.now_ns, input.minute_of_day, Response::Ignored);
        }
        g.pulse.tick(input)
    }

    /// The opening line of a talk was spoken: remember it so the conversation that follows
    /// knows what she just said, and start waiting for the answer.
    pub fn talk_started(&self, now_ns: u64, line: &str) {
        let mut g = self.lock();
        g.pending = Some(Pending { started_ns: now_ns });
        g.opener = Some((now_ns, line.to_string()));
    }

    /// The opening line she just spoke, once (the chat history starts with it). A line the owner
    /// did not answer in time is forgotten, so it never leaks into a later, unrelated chat.
    pub fn take_opener(&self, now_ns: u64) -> Option<String> {
        let (at, line) = self.lock().opener.take()?;
        (now_ns.saturating_sub(at) <= ANSWER_WINDOW_S * NS_PER_S).then_some(line)
    }

    /// The owner said something in companion chat. Applies a spoken control, or counts an answer
    /// to a talk that has just started.
    pub fn on_owner_turn(&self, now_ns: u64, minute_of_day: u16, text: &str) -> OwnerTurn {
        let mut g = self.lock();
        let live = g
            .pending
            .is_some_and(|p| now_ns.saturating_sub(p.started_ns) <= ANSWER_WINDOW_S * NS_PER_S);
        if let Some(cmd) = parse_pulse_command(text) {
            g.pending = None;
            match cmd {
                // "Not now" to a talk is the owner's answer to it: one reaction, not two.
                PulseCommand::NotNow => {
                    g.pulse
                        .record_response(now_ns, minute_of_day, Response::NotNow);
                }
                other => apply_command(&mut g.pulse, &other, now_ns, minute_of_day),
            }
            return OwnerTurn::Command(cmd);
        }
        if live {
            g.pending = None;
            g.pulse
                .record_response(now_ns, minute_of_day, Response::Accepted);
            return OwnerTurn::Plain {
                answered_talk: true,
            };
        }
        OwnerTurn::Plain {
            answered_talk: false,
        }
    }

    /// Like [`Self::check_line`], for a line built from a fact she was just given (a follow-up),
    /// where "you told me" is true.
    pub fn check_grounded_line(&self, line: &str) -> Option<String> {
        match self.lock().filter.check(line, false, true) {
            Verdict::Keep(s) => Some(s),
            Verdict::Drop(_) => None,
        }
    }

    /// Runs one opening line through the same checks as every reply (forbidden phrases, repeats,
    /// invented memories). `None` means the line must not be spoken.
    pub fn check_line(&self, line: &str) -> Option<String> {
        match self.lock().filter.check(line, false, false) {
            Verdict::Keep(s) => Some(s),
            Verdict::Drop(_) => None,
        }
    }

    /// The owner said "that was funny" or "not funny": remember it, and let the scheduler tell
    /// fewer or more jokes.
    pub fn record_feedback(&self, f: Feedback) {
        let mut g = self.lock();
        g.humour.record(f);
        let w = g.humour.joke_weight();
        g.pulse.set_joke_weight(w);
    }

    /// The humour level the owner's taste asks for; `sensitive` lowers it to none.
    pub fn humour_level(&self, sensitive: bool) -> HumourLevel {
        effective_level(&self.lock().humour, sensitive)
    }

    /// The owner's taste in jokes, as small numbers.
    pub fn humour_state(&self) -> HumourState {
        self.lock().humour
    }

    /// Lends the reply filter (and its memory of what was already said) to a conversation, so a
    /// joke told this morning is not told again this evening. Give it back with
    /// [`Self::return_filter`].
    pub fn lend_filter(&self) -> ReplyFilter {
        let mut g = self.lock();
        let fresh = ReplyFilter::new(128).with_avoid_topics(&self.avoid);
        std::mem::replace(&mut g.filter, fresh)
    }

    /// Takes the filter back when a conversation ends.
    pub fn return_filter(&self, filter: ReplyFilter) {
        self.lock().filter = filter;
    }

    /// What to do with something the owner said: keep it (by chance) or not. When a plan is
    /// kept ("I have an interview on Thursday") it also becomes something she can ask about
    /// later. `directed` is false when it may not have been the owner speaking to her.
    pub fn consider_memory(&self, text: &str, directed: bool, now_ns: u64) -> Decision {
        let mut g = self.lock();
        let d = g.memory.decide(text, directed);
        if let Decision::Save(Keep {
            fact,
            category: Category::Plan,
            ..
        }) = &d
        {
            g.followup = Some((now_ns, plain_form(fact).to_string()));
            g.pulse.set_followup_available(true);
        }
        d
    }

    /// Counts of what was kept and skipped.
    pub fn memory_counters(&self) -> MemoryCounters {
        self.lock().memory.counters()
    }

    /// Something she may ask about, once, within two days of it being said.
    pub fn take_followup(&self, now_ns: u64) -> Option<String> {
        let mut g = self.lock();
        g.pulse.set_followup_available(false);
        let (at, fact) = g.followup.take()?;
        (now_ns.saturating_sub(at) <= 48 * 3600 * NS_PER_S).then_some(fact)
    }

    /// Whether the companion is paused.
    pub fn paused(&self) -> bool {
        self.lock().pulse.paused()
    }

    /// Counts for the health report.
    pub fn counters(&self) -> PulseCounters {
        self.lock().pulse.counters().clone()
    }
}
