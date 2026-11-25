//! The loop that lets Laila start a chat herself (Phase 12, story C-S03, plan section 4.4).
//!
//! Every 30 s it reads the moment (C3's health signals and C2's status), asks the scheduler
//! ([`super::pulse`]) whether this is a good time, and if so writes one short opening line with
//! the language model, checks it like any other reply, and asks C2 to speak it. C2 then keeps a
//! quiet session open for the answer. Words only: nothing here can act on the computer.

use std::sync::Arc;
use std::time::Duration;

use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    EvaluateRequest, GenerateRequest, HealthSignals, QueryHealthSignals, SpeakCompanion, Taint,
    VoiceStatus, VoiceStatusRequest,
};
use octoplus_taint::TaintFlags;
use tracing::{debug, warn};

use crate::companion::hub::CompanionHub;
use crate::companion::mode::Mode;
use crate::companion::persona::Persona;
use crate::companion::prompt::{self, CompanionPromptInput};
use crate::companion::pulse::{PulseDecision, PulseInput, TICK_S, Topic};
use crate::nudge::engine::NudgePeers;
use crate::nudge::signals::{self, ContextInputs};
use crate::prompt::Budgets;
use crate::speech::SentenceStream;

const C3_DEADLINE: Duration = Duration::from_millis(500);
const VOICE_DEADLINE: Duration = Duration::from_secs(2);
const KERNEL_DEADLINE: Duration = Duration::from_millis(500);
const GENERATION_DEADLINE: Duration = Duration::from_secs(30);
/// Longest opening line, in characters (C2 accepts 400 bytes).
const MAX_LINE_CHARS: usize = 220;
/// Most sentences of an opening line.
const MAX_LINE_SENTENCES: usize = 2;
/// Tokens asked of the model for an opening line.
const LINE_TOKENS: u32 = 60;
/// How far back C3's samples are read for the owner's state.
const SAMPLE_LOOKBACK_S: u64 = 120;
const NS_PER_S: u64 = 1_000_000_000;

/// What the runner needs from the machine around it.
#[derive(Debug, Clone, Copy)]
pub struct RunnerSettings {
    /// Seconds C2 listens for the owner's answer after the opening line.
    pub listen_seconds: u32,
    /// Prompt budgets.
    pub budgets: Budgets,
    /// A monitor sample is stale after this many seconds.
    pub stale_after_s: u64,
}

/// What C2 said about the moment, reduced to what the scheduler needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiceMoment {
    /// C2 is reachable and capturing.
    pub available: bool,
    /// Another application holds the microphone.
    pub mic_busy: bool,
    /// A spoken turn is being heard or spoken.
    pub busy: bool,
    /// An open-microphone session is running.
    pub session_active: bool,
    /// Guest mode is on.
    pub guest: bool,
}

impl VoiceMoment {
    /// Reads C2's status reply.
    #[must_use]
    pub fn from_status(s: &VoiceStatus) -> Self {
        Self {
            available: s.capture_running,
            mic_busy: s.mic_in_use_by_other,
            busy: s.turn_in_progress || s.speaking,
            session_active: s.session_active,
            guest: s.guest_seconds_left > 0,
        }
    }
}

/// What is known about the moment besides C3's samples and C2's status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MomentFlags {
    /// The owner was idle at the previous tick.
    pub was_idle: bool,
    /// The monitor's samples are stale (it is paused or gone).
    pub monitor_stale: bool,
}

/// Builds the scheduler's input for one tick. Pure: the clock, the time zone and the previous
/// idle state come in as arguments. Returns the input and whether the owner is idle now.
#[must_use]
pub fn build_input(
    sig: &HealthSignals,
    voice: VoiceMoment,
    now_ns: u64,
    tz: &jiff::tz::TimeZone,
    prev: MomentFlags,
) -> (PulseInput, bool) {
    let minute = signals::local_minute(now_ns, tz);
    let idle_now = sig.idle || (sig.locked_known && sig.locked);
    let ctx = signals::build_context(ContextInputs {
        idle: idle_now,
        fullscreen: sig.fullscreen,
        media_playing: sig.media_playing,
        locked: sig.locked,
        locked_known: sig.locked_known,
        monitor_stale: prev.monitor_stale,
        own_pause_active: false,
        in_turn: voice.busy,
        mic_busy: voice.mic_busy,
        quiet_hours: false,
        voice_available: voice.available,
    });
    // The scheduler only needs "away or not": while the owner is active the idle time is zero.
    let idle_ms = if idle_now { 3_600_000 } else { 0 };
    let input = PulseInput {
        now_ns,
        minute_of_day: u16::try_from(minute).unwrap_or(0),
        day_index: local_day(now_ns, tz),
        ctx,
        in_session: voice.session_active,
        guest: voice.guest,
        battery_saver: false,
        idle_ms,
        returned_from_idle: prev.was_idle && !idle_now,
        task_finished: false,
        natural_pause: false,
    };
    (input, idle_now)
}

/// The local day number (changes at local midnight).
#[must_use]
pub fn local_day(now_ns: u64, tz: &jiff::tz::TimeZone) -> u64 {
    let secs = i64::try_from(now_ns / NS_PER_S).unwrap_or(0);
    let offset = jiff::Timestamp::from_second(secs)
        .map(|t| tz.to_offset(t).seconds())
        .unwrap_or(0);
    u64::try_from(secs.saturating_add(i64::from(offset)).div_euclid(86_400)).unwrap_or(0)
}

/// What to ask the model for, per topic. Our own text, never the owner's.
#[must_use]
pub const fn topic_instruction(t: Topic) -> &'static str {
    match t {
        Topic::CheckIn => {
            "Start a short chat. Ask how the owner's day is going, in your own words."
        }
        Topic::Joke => {
            "Start a short chat with one short, gentle joke that is not about the owner."
        }
        Topic::FollowUp => {
            "Start a short chat by asking how something the owner told you about went."
        }
        Topic::Observation => {
            "Start a short chat with a small, harmless remark about the time of day or the weather in general."
        }
        Topic::FunFact => {
            "Start a short chat with one small fun fact, then ask if the owner knew it."
        }
        Topic::BreakReminder => "Gently suggest a short break, once, without nagging.",
        Topic::Morning => "Say good morning in your own way and ask what the day looks like.",
        Topic::Lunch => "Say it is around lunch time and ask if the owner has eaten.",
        Topic::Evening => "Say the evening is here and ask how the day went.",
    }
}

/// The user block of the opening-line prompt.
#[must_use]
pub fn opening_request(t: Topic) -> String {
    format!(
        "{} Say one or two short sentences, at most 25 words in total, and end with a question the owner can answer in a few words. Do not mention any past conversation or memory.",
        topic_instruction(t)
    )
}

/// The runner.
#[derive(Debug, Clone)]
pub struct CompanionRunner {
    peers: NudgePeers,
    hub: Arc<CompanionHub>,
    persona: Arc<Persona>,
    settings: RunnerSettings,
}

impl CompanionRunner {
    /// Creates the runner.
    #[must_use]
    pub fn new(
        peers: NudgePeers,
        hub: Arc<CompanionHub>,
        persona: Arc<Persona>,
        settings: RunnerSettings,
    ) -> Self {
        Self {
            peers,
            hub,
            persona,
            settings,
        }
    }

    /// The loop: one [`Self::tick`] every [`TICK_S`] seconds until `stop` fires.
    pub async fn run(&self, tz: jiff::tz::TimeZone, mut stop: tokio::sync::watch::Receiver<bool>) {
        let mut every = tokio::time::interval(Duration::from_secs(TICK_S));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut was_idle = false;
        loop {
            tokio::select! {
                res = stop.changed() => {
                    if res.is_err() || *stop.borrow() {
                        break;
                    }
                }
                _ = every.tick() => {
                    let now = octoplus_common::time::now_ns().unwrap_or(0);
                    was_idle = self.tick(now, &tz, was_idle).await;
                }
            }
        }
    }

    /// One evaluation. Returns whether the owner was idle (for "returned from idle").
    pub async fn tick(&self, now_ns: u64, tz: &jiff::tz::TimeZone, was_idle: bool) -> bool {
        let since = now_ns.saturating_sub(SAMPLE_LOOKBACK_S * NS_PER_S);
        let got = self
            .peers
            .storage
            .call(
                Body::QueryHealthSignals(QueryHealthSignals { since_ns: since }),
                C3_DEADLINE,
            )
            .await;
        let Ok(Body::HealthSignals(sig)) = got else {
            // C3 unreachable: say nothing (never guess).
            return was_idle;
        };
        let newest = sig
            .samples
            .iter()
            .map(|e| e.observed_at_ns)
            .max()
            .unwrap_or(0);
        let stale =
            newest == 0 || now_ns.saturating_sub(newest) > self.settings.stale_after_s * NS_PER_S;
        let voice = self.voice_moment().await;
        let (input, idle_now) = build_input(
            &sig,
            voice,
            now_ns,
            tz,
            MomentFlags {
                was_idle,
                monitor_stale: stale,
            },
        );
        match self.hub.tick(&input) {
            PulseDecision::Wait(_) => {}
            PulseDecision::Talk(topic) => {
                debug!("companion talk: {topic:?}");
                self.start_talk(topic, now_ns).await;
            }
        }
        idle_now
    }

    async fn voice_moment(&self) -> VoiceMoment {
        match self
            .peers
            .voice
            .call(
                Body::VoiceStatusRequest(VoiceStatusRequest {}),
                VOICE_DEADLINE,
            )
            .await
        {
            Ok(Body::VoiceStatus(s)) => VoiceMoment::from_status(&s),
            _ => VoiceMoment::default(),
        }
    }

    /// Writes the opening line, checks it, and asks C2 to speak it. Any failure means no talk.
    async fn start_talk(&self, topic: Topic, now_ns: u64) -> bool {
        let Some(line) = self.write_line(topic, now_ns).await else {
            debug!("companion talk dropped: no usable line");
            return false;
        };
        let body = Body::SpeakCompanion(SpeakCompanion {
            text: line.clone(),
            listen_seconds: self.settings.listen_seconds,
        });
        match self.peers.voice.call(body, VOICE_DEADLINE).await {
            Ok(_) => {
                self.hub.talk_started(now_ns, &line);
                true
            }
            Err(e) => {
                debug!("C2 could not take the companion line: {e}");
                false
            }
        }
    }

    /// One opening line from the model, or `None`.
    pub async fn write_line(&self, topic: Topic, now_ns: u64) -> Option<String> {
        let mode = if (now_ns / NS_PER_S).is_multiple_of(2) {
            Mode::Logic
        } else {
            Mode::Heart
        };
        // A follow-up is built from the plan the owner mentioned (and she may say "you told
        // me"); without one it is an ordinary check-in.
        let followup = if topic == Topic::FollowUp {
            self.hub.take_followup(now_ns)
        } else {
            None
        };
        let topic = if topic == Topic::FollowUp && followup.is_none() {
            Topic::CheckIn
        } else {
            topic
        };
        let facts: Vec<String> = followup.iter().cloned().collect();
        let request = opening_request(topic);
        let built = prompt::assemble(
            &CompanionPromptInput {
                persona: &self.persona,
                mode,
                switch_instruction: None,
                kabir_allowed: false,
                gentle_line: None,
                humour: self.hub.humour_level(false),
                facts: &facts,
                facts_taint: if facts.is_empty() {
                    TaintFlags::empty()
                } else {
                    TaintFlags::USER_PROVIDED
                },
                history: "",
                history_taint: TaintFlags::empty(),
                question: &request,
            },
            &self.settings.budgets,
        );
        // The same check every model call gets (C6, capability llm.generate).
        let allowed = self
            .peers
            .kernel
            .call(
                Body::EvaluateRequest(EvaluateRequest {
                    capability: "llm.generate".to_string(),
                    context_taint: Some(Taint {
                        flags: built.taint.bits(),
                    }),
                }),
                KERNEL_DEADLINE,
            )
            .await;
        if !matches!(allowed, Ok(Body::EvaluateResult(r)) if r.allowed_without_dialog) {
            return None;
        }
        let client = tokio::time::timeout(Duration::from_secs(2), self.peers.inference.get())
            .await
            .ok()?
            .ok()?;
        let req = Body::GenerateRequest(GenerateRequest {
            prompt: built.text,
            max_tokens: LINE_TOKENS,
            grammar: String::new(),
            interactive: false,
            stable_prefix_chars: built.stable_prefix_chars,
        });
        let mut stream = client.stream(req, GENERATION_DEADLINE).await.ok()?;
        let mut pieces = SentenceStream::new();
        let mut sentences: Vec<String> = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(Body::TokenChunk(c)) => sentences.extend(pieces.feed(&c.text)),
                Ok(Body::GenerateDone(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    warn!("companion line generation failed: {e}");
                    return None;
                }
            }
        }
        sentences.extend(pieces.flush());
        let mut kept: Vec<String> = Vec::new();
        for s in sentences {
            let checked = if followup.is_some() {
                self.hub.check_grounded_line(&s)
            } else {
                self.hub.check_line(&s)
            };
            if let Some(ok) = checked {
                kept.push(ok);
            }
            if kept.len() >= MAX_LINE_SENTENCES {
                break;
            }
        }
        let line = kept.join(" ");
        let line = line.trim();
        if line.is_empty() || line.chars().count() > MAX_LINE_CHARS {
            return None;
        }
        Some(line.to_string())
    }
}
