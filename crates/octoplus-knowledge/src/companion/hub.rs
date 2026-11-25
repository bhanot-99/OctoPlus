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
    Pulse, PulseCommand, PulseConfig, PulseCounters, PulseDecision, PulseInput, Response, Topic,
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

/// One talk she started, for "today's talks" in OctoPit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkRecord {
    /// When the line was spoken.
    pub at_ns: u64,
    /// What kind of talk it was.
    pub topic: Topic,
    /// What the owner did with it (`None` while waiting).
    pub response: Option<Response>,
}

/// Most talks kept for the list.
const TALK_LOG: usize = 100;

/// Everything OctoPit shows about her rhythm, taken at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct HubStatus {
    /// The global pause switch.
    pub paused: bool,
    /// Minutes the owner's quiet request still holds.
    pub quiet_minutes_left: u32,
    /// Talks started today.
    pub talks_today: u32,
    /// Talks started in the last hour.
    pub talks_last_hour: u32,
    /// The schedule.
    pub config: PulseConfig,
    /// The owner's taste in jokes.
    pub humour: HumourLevel,
    /// "Chat more / less" as a percentage of the normal rhythm.
    pub chattiness_percent: u32,
    /// Recent talks, newest first.
    pub talks: Vec<TalkRecord>,
    /// What was kept by chance.
    pub memory: MemoryCounters,
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
    log: std::collections::VecDeque<TalkRecord>,
    last_topic: Option<Topic>,
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
                log: std::collections::VecDeque::new(),
                last_topic: None,
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
            mark_last(&mut g.log, Response::Ignored);
        }
        let d = g.pulse.tick(input);
        if let PulseDecision::Talk(topic) = d {
            g.last_topic = Some(topic);
        }
        d
    }

    /// The opening line of a talk was spoken: remember it so the conversation that follows
    /// knows what she just said, and start waiting for the answer.
    pub fn talk_started(&self, now_ns: u64, line: &str) {
        let mut g = self.lock();
        g.pending = Some(Pending { started_ns: now_ns });
        g.opener = Some((now_ns, line.to_string()));
        let topic = g.last_topic.unwrap_or(Topic::CheckIn);
        g.log.push_front(TalkRecord {
            at_ns: now_ns,
            topic,
            response: None,
        });
        g.log.truncate(TALK_LOG);
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
                    if live {
                        mark_last(&mut g.log, Response::NotNow);
                    }
                }
                other => apply_command(&mut g.pulse, &other, now_ns, minute_of_day),
            }
            return OwnerTurn::Command(cmd);
        }
        if live {
            g.pending = None;
            g.pulse
                .record_response(now_ns, minute_of_day, Response::Accepted);
            mark_last(&mut g.log, Response::Accepted);
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

    /// Everything OctoPit shows about her rhythm right now.
    pub fn status(&self, now_ns: u64) -> HubStatus {
        let g = self.lock();
        let today_start = now_ns.saturating_sub(24 * 3600 * NS_PER_S);
        HubStatus {
            paused: g.pulse.paused(),
            quiet_minutes_left: g.pulse.snooze_minutes_left(now_ns),
            talks_today: g.pulse.talks_today(),
            talks_last_hour: u32::try_from(g.pulse.talks_in_last_hour()).unwrap_or(u32::MAX),
            config: g.pulse.config(),
            humour: g.humour.level(),
            chattiness_percent: (g.pulse.rate_scale() * 100.0).round() as u32,
            talks: g
                .log
                .iter()
                .filter(|t| t.at_ns >= today_start)
                .copied()
                .collect(),
            memory: g.memory.counters(),
        }
    }

    /// An owner control from OctoPit (the same effects as the spoken ones).
    pub fn control(&self, now_ns: u64, minute_of_day: u16, c: &PulseCommand) {
        let mut g = self.lock();
        apply_command(&mut g.pulse, c, now_ns, minute_of_day);
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

/// Fills in what the owner did with the newest talk that is still waiting.
fn mark_last(log: &mut std::collections::VecDeque<TalkRecord>, r: Response) {
    if let Some(t) = log.iter_mut().find(|t| t.response.is_none()) {
        t.response = Some(r);
    }
}
