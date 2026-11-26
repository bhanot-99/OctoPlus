#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Laila starts a chat herself (story C-S03): the hub rules, the mapping from the moment to the
//! scheduler's input, and the runner end to end against fake C3, C4, C6 and C2 servers.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use octoplus_ipc::server::{ServerConfig, ensure_private_dir};
use octoplus_ipc::{Handler, HandlerError, ReconnectingClient, Request, Server, StreamSink};
use octoplus_knowledge::companion::guards;
use octoplus_knowledge::companion::hub::{ANSWER_WINDOW_S, CompanionHub, OwnerTurn};
use octoplus_knowledge::companion::persona::Persona;
use octoplus_knowledge::companion::pulse::{
    PulseCommand, PulseDecision, PulseInput, Topic, WaitReason,
};
use octoplus_knowledge::companion::runner::{
    CompanionRunner, MomentFlags, RunnerSettings, VoiceMoment, build_input, local_day,
    opening_request, topic_instruction,
};
use octoplus_knowledge::nudge::engine::NudgePeers;
use octoplus_knowledge::nudge::types::UserContext;
use octoplus_knowledge::prompt::Budgets;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    Ack, EvaluateResult, GenerateDone, HealthSignals, MonitorEvent, StopReason, Tier, TokenChunk,
    VoiceStatus,
};

const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;
const BASE: u64 = 1_000_000_000_000_000;

fn persona() -> Persona {
    Persona::default_packaged().unwrap()
}

fn ten_am() -> PulseInput {
    PulseInput {
        now_ns: BASE,
        minute_of_day: 10 * 60,
        day_index: 7,
        ctx: UserContext {
            voice_available: true,
            ..UserContext::default()
        },
        idle_ms: 0,
        ..PulseInput::default()
    }
}

// ---- the hub ---------------------------------------------------------------------------------

#[test]
fn an_answer_inside_the_window_is_accepted_once() {
    let hub = CompanionHub::new(&persona(), 1);
    assert!(matches!(
        hub.tick(&ten_am()),
        PulseDecision::Talk(Topic::Morning)
    ));
    hub.talk_started(BASE, "Good morning. What does the day look like?");
    let t = BASE + 20 * NS_PER_S;
    assert_eq!(
        hub.on_owner_turn(t, 600, "busy day, meetings all afternoon"),
        OwnerTurn::Plain {
            answered_talk: true
        }
    );
    // A second remark is just conversation.
    assert_eq!(
        hub.on_owner_turn(t + NS_PER_S, 600, "and you?"),
        OwnerTurn::Plain {
            answered_talk: false
        }
    );
    let c = hub.counters();
    assert_eq!((c.accepted, c.ignored, c.not_now), (1, 0, 0));
}

#[test]
fn a_talk_nobody_answers_is_counted_ignored_and_cools_down() {
    let hub = CompanionHub::new(&persona(), 1);
    assert!(matches!(hub.tick(&ten_am()), PulseDecision::Talk(_)));
    hub.talk_started(BASE, "Hey, how is it going?");
    let later = PulseInput {
        now_ns: BASE + (ANSWER_WINDOW_S + 5) * NS_PER_S,
        ..ten_am()
    };
    // The late tick records the ignored talk, then the cool-down holds her back.
    assert!(matches!(hub.tick(&later), PulseDecision::Wait(_)));
    let c = hub.counters();
    assert_eq!(c.ignored, 1);
    let much_later = PulseInput {
        now_ns: BASE + 12 * NS_PER_MIN,
        ..ten_am()
    };
    assert_eq!(
        hub.tick(&much_later),
        PulseDecision::Wait(WaitReason::Cooldown)
    );
    // And an answer that comes far too late is not an answer to that talk.
    assert_eq!(
        hub.on_owner_turn(BASE + 10 * NS_PER_MIN, 600, "oh sorry, I was away"),
        OwnerTurn::Plain {
            answered_talk: false
        }
    );
}

#[test]
fn not_now_to_a_talk_is_one_reaction_and_quiets_her_for_an_hour() {
    let hub = CompanionHub::new(&persona(), 1);
    assert!(matches!(hub.tick(&ten_am()), PulseDecision::Talk(_)));
    hub.talk_started(BASE, "Hey, how is it going?");
    assert_eq!(
        hub.on_owner_turn(BASE + 8 * NS_PER_S, 600, "not now"),
        OwnerTurn::Command(PulseCommand::NotNow)
    );
    let c = hub.counters();
    assert_eq!((c.accepted, c.ignored, c.not_now), (0, 0, 1));
    // Nothing is counted as ignored later, and she stays quiet for an hour.
    let t = BASE + 30 * NS_PER_MIN;
    assert_eq!(
        hub.tick(&PulseInput {
            now_ns: t,
            ..ten_am()
        }),
        PulseDecision::Wait(WaitReason::Snoozed)
    );
    assert_eq!(hub.counters().ignored, 0);
}

#[test]
fn spoken_controls_change_the_rhythm_even_with_no_talk_pending() {
    let hub = CompanionHub::new(&persona(), 1);
    assert_eq!(
        hub.on_owner_turn(BASE, 600, "be quiet for an hour"),
        OwnerTurn::Command(PulseCommand::QuietFor { minutes: 60 })
    );
    assert_eq!(
        hub.tick(&PulseInput {
            now_ns: BASE + 10 * NS_PER_MIN,
            ..ten_am()
        }),
        PulseDecision::Wait(WaitReason::Snoozed)
    );
    assert_eq!(
        hub.on_owner_turn(BASE + NS_PER_MIN, 600, "pause the companion"),
        OwnerTurn::Command(PulseCommand::Pause)
    );
    assert!(hub.paused());
    hub.on_owner_turn(BASE + 2 * NS_PER_MIN, 600, "resume the companion");
    assert!(!hub.paused());
}

#[test]
fn the_opening_line_is_remembered_once_and_forgotten_when_unanswered() {
    let hub = CompanionHub::new(&persona(), 1);
    hub.talk_started(BASE, "Hey, how is it going?");
    assert_eq!(
        hub.take_opener(BASE + 5 * NS_PER_S).as_deref(),
        Some("Hey, how is it going?")
    );
    assert_eq!(hub.take_opener(BASE + 6 * NS_PER_S), None, "once only");
    hub.talk_started(BASE, "Another line?");
    assert_eq!(
        hub.take_opener(BASE + (ANSWER_WINDOW_S + 1) * NS_PER_S),
        None,
        "an unanswered line never leaks into a later chat"
    );
}

#[test]
fn opening_lines_get_the_same_checks_as_any_reply() {
    let hub = CompanionHub::new(&persona(), 1);
    assert!(hub.check_line("Hey, how is your day going?").is_some());
    assert!(
        hub.check_line("Hey, how is your day going?").is_none(),
        "no repeats"
    );
    assert!(hub.check_line("Don't go, I'll miss you.").is_none());
    assert!(
        hub.check_line("I was thinking about that coffee shop you mentioned.")
            .is_none()
    );
    assert!(hub.check_line("   ").is_none());
}

// ---- the moment ------------------------------------------------------------------------------

fn tz() -> jiff::tz::TimeZone {
    jiff::tz::TimeZone::UTC
}

#[test]
fn the_moment_is_mapped_to_the_schedulers_input() {
    let sig = HealthSignals {
        fullscreen: true,
        media_playing: true,
        ..HealthSignals::default()
    };
    let voice = VoiceMoment {
        available: true,
        mic_busy: true,
        busy: false,
        session_active: true,
        guest: true,
    };
    let now = 20 * 86_400 * NS_PER_S + 10 * 3600 * NS_PER_S + 30 * NS_PER_MIN;
    let (i, idle) = build_input(&sig, voice, now, &tz(), MomentFlags::default());
    assert!(!idle);
    assert_eq!(i.minute_of_day, 10 * 60 + 30);
    assert_eq!(i.day_index, 20);
    assert!(i.ctx.fullscreen && i.ctx.media_playing && i.ctx.mic_busy && i.ctx.voice_available);
    assert!(i.in_session && i.guest);
    assert_eq!(i.idle_ms, 0);

    let sig = HealthSignals {
        idle: true,
        ..HealthSignals::default()
    };
    let (i, idle) = build_input(
        &sig,
        VoiceMoment::default(),
        now,
        &tz(),
        MomentFlags::default(),
    );
    assert!(idle && i.ctx.away && i.idle_ms >= 600_000);
    assert!(!i.ctx.voice_available);
    // Coming back from idle is noticed once.
    let (i, idle) = build_input(
        &HealthSignals::default(),
        VoiceMoment::default(),
        now,
        &tz(),
        MomentFlags {
            was_idle: true,
            monitor_stale: false,
        },
    );
    assert!(!idle && i.returned_from_idle);
    // A stale monitor pauses her (the nudge engine's rule).
    let (i, _) = build_input(
        &HealthSignals::default(),
        VoiceMoment::default(),
        now,
        &tz(),
        MomentFlags {
            was_idle: false,
            monitor_stale: true,
        },
    );
    assert!(i.ctx.paused);
    assert_eq!(local_day(now, &tz()), 20);
    assert_eq!(local_day(now + 14 * 3600 * NS_PER_S, &tz()), 21);
}

#[test]
fn every_topic_has_a_plain_instruction_that_bans_invented_memories() {
    for t in [
        Topic::CheckIn,
        Topic::Joke,
        Topic::FollowUp,
        Topic::Observation,
        Topic::FunFact,
        Topic::BreakReminder,
        Topic::Morning,
        Topic::Lunch,
        Topic::Evening,
    ] {
        let line = topic_instruction(t);
        assert!(line.len() > 20);
        assert!(guards::forbidden_phrase(line).is_none());
        let req = opening_request(t);
        assert!(req.contains("Do not mention any past conversation or memory"));
        assert!(req.contains("at most 25 words"));
    }
}

// ---- the runner against fake peers -------------------------------------------------------------

#[derive(Default)]
struct Seen {
    spoken: Vec<(String, u32)>,
    prompts: Vec<String>,
    kernel_taints: Vec<u32>,
}

type Shared = Arc<Mutex<Seen>>;

struct FakeStorage {
    signals: HealthSignals,
}

impl Handler for FakeStorage {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::QueryHealthSignals(_) => Ok(Body::HealthSignals(self.signals.clone())),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

struct FakeKernel {
    seen: Shared,
    allow: bool,
}

impl Handler for FakeKernel {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::EvaluateRequest(e) => {
                self.seen
                    .lock()
                    .unwrap()
                    .kernel_taints
                    .push(e.context_taint.map_or(u32::MAX, |t| t.flags));
                Ok(Body::EvaluateResult(EvaluateResult {
                    tier: Tier::Safe as i32,
                    allowed_without_dialog: self.allow,
                }))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

struct FakeInference {
    seen: Shared,
    reply: Vec<String>,
}

impl Handler for FakeInference {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::GenerateRequest(g) = req.body else {
            return Err(HandlerError::unsupported());
        };
        self.seen.lock().unwrap().prompts.push(g.prompt);
        for text in &self.reply {
            sink.send(Body::TokenChunk(TokenChunk {
                text: text.clone(),
                tokens: 1,
            }))
            .await
            .map_err(HandlerError::from)?;
        }
        Ok(Body::GenerateDone(GenerateDone {
            reason: StopReason::EndOfText as i32,
            ..GenerateDone::default()
        }))
    }
}

struct FakeVoice {
    seen: Shared,
    status: VoiceStatus,
    refuse: bool,
}

impl Handler for FakeVoice {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::VoiceStatusRequest(_) => Ok(Body::VoiceStatus(self.status)),
            Body::SpeakCompanion(c) => {
                if self.refuse {
                    return Err(HandlerError::new(
                        octoplus_proto::v1::ErrorCode::Unavailable,
                        "busy",
                        true,
                    ));
                }
                self.seen
                    .lock()
                    .unwrap()
                    .spoken
                    .push((c.text, c.listen_seconds));
                Ok(Body::Ack(Ack {}))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    runner: CompanionRunner,
    hub: Arc<CompanionHub>,
    seen: Shared,
    stop: tokio::sync::watch::Sender<bool>,
}

struct Opts {
    signals: HealthSignals,
    voice: VoiceStatus,
    reply: Vec<&'static str>,
    allow: bool,
    refuse: bool,
}

impl Default for Opts {
    fn default() -> Self {
        let now = octoplus_common::time::now_ns().unwrap();
        Self {
            signals: HealthSignals {
                samples: vec![MonitorEvent {
                    observed_at_ns: now,
                    ..MonitorEvent::default()
                }],
                ..HealthSignals::default()
            },
            voice: VoiceStatus {
                capture_running: true,
                ..VoiceStatus::default()
            },
            reply: vec!["Hey, ", "how is your day going?"],
            allow: true,
            refuse: false,
        }
    }
}

async fn serve<H: Handler>(sock: &Path, handler: H, stop: &tokio::sync::watch::Sender<bool>) {
    let server = Server::bind(sock, ServerConfig::new(1 << 20, Duration::from_secs(5)))
        .await
        .unwrap();
    let mut rx = stop.subscribe();
    tokio::spawn(server.serve(Arc::new(handler), async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
}

async fn rig(o: Opts) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let socks = dir.path().join("s");
    ensure_private_dir(&socks).unwrap();
    let (stop, _) = tokio::sync::watch::channel(false);
    let seen: Shared = Arc::default();
    let (c3, c4, c6, c2) = (
        socks.join("c3.sock"),
        socks.join("c4.sock"),
        socks.join("c6.sock"),
        socks.join("c2.sock"),
    );
    serve(&c3, FakeStorage { signals: o.signals }, &stop).await;
    serve(
        &c6,
        FakeKernel {
            seen: seen.clone(),
            allow: o.allow,
        },
        &stop,
    )
    .await;
    serve(
        &c4,
        FakeInference {
            seen: seen.clone(),
            reply: o.reply.iter().map(ToString::to_string).collect(),
        },
        &stop,
    )
    .await;
    serve(
        &c2,
        FakeVoice {
            seen: seen.clone(),
            status: o.voice,
            refuse: o.refuse,
        },
        &stop,
    )
    .await;
    let c = |p: &Path| Arc::new(ReconnectingClient::new(p, 1 << 20));
    let hub = CompanionHub::new(&persona(), 3);
    let runner = CompanionRunner::new(
        NudgePeers {
            storage: c(&c3),
            kernel: c(&c6),
            inference: c(&c4),
            voice: c(&c2),
        },
        hub.clone(),
        Arc::new(persona()),
        RunnerSettings {
            listen_seconds: 30,
            budgets: Budgets {
                facts: 48,
                focus: 0,
                history: 0,
                evidence: 0,
                bytes_per_token: 3,
            },
            stale_after_s: 60,
        },
    );
    Rig {
        _dir: dir,
        runner,
        hub,
        seen,
        stop,
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

#[tokio::test]
async fn the_runner_asks_c2_to_speak_the_checked_line_and_the_chat_can_follow() {
    let r = rig(Opts::default()).await;
    // 10:00 UTC today, with the monitor sample stamped at the same instant.
    let now_real = octoplus_common::time::now_ns().unwrap();
    let day_start = now_real - now_real % (86_400 * NS_PER_S);
    let t = day_start + 10 * 3600 * NS_PER_S;
    let sig = HealthSignals {
        samples: vec![MonitorEvent {
            observed_at_ns: t,
            ..MonitorEvent::default()
        }],
        ..HealthSignals::default()
    };
    drop(r);
    let r = rig(Opts {
        signals: sig,
        ..Opts::default()
    })
    .await;
    r.runner.tick(t, &tz(), false).await;
    let seen = r.seen.lock().unwrap();
    assert_eq!(seen.spoken.len(), 1, "one line spoken");
    assert_eq!(seen.spoken[0].0, "Hey, how is your day going?");
    assert_eq!(seen.spoken[0].1, 30, "listen window from the settings");
    assert_eq!(
        seen.kernel_taints.len(),
        1,
        "the kernel checked the model call"
    );
    assert!(
        seen.prompts[0].contains("good morning"),
        "first talk of the day is the morning one"
    );
    assert!(seen.prompts[0].contains("Do not mention any past conversation"));
    drop(seen);
    // Her line is remembered for the chat that follows, once.
    assert_eq!(
        r.hub.take_opener(t + 3 * NS_PER_S).as_deref(),
        Some("Hey, how is your day going?")
    );
}

#[tokio::test]
async fn nothing_is_spoken_when_the_moment_or_the_line_is_wrong() {
    let t0 = octoplus_common::time::now_ns().unwrap();
    let day_start = t0 - t0 % (86_400 * NS_PER_S);
    let t = day_start + 10 * 3600 * NS_PER_S;
    let fresh = HealthSignals {
        samples: vec![MonitorEvent {
            observed_at_ns: t,
            ..MonitorEvent::default()
        }],
        ..HealthSignals::default()
    };
    let cases: Vec<(&str, Opts)> = vec![
        (
            "mic busy (a call)",
            Opts {
                signals: fresh.clone(),
                voice: VoiceStatus {
                    capture_running: true,
                    mic_in_use_by_other: true,
                    ..VoiceStatus::default()
                },
                ..Opts::default()
            },
        ),
        (
            "fullscreen",
            Opts {
                signals: HealthSignals {
                    fullscreen: true,
                    ..fresh.clone()
                },
                ..Opts::default()
            },
        ),
        (
            "owner away",
            Opts {
                signals: HealthSignals {
                    idle: true,
                    ..fresh.clone()
                },
                ..Opts::default()
            },
        ),
        (
            "session running",
            Opts {
                signals: fresh.clone(),
                voice: VoiceStatus {
                    capture_running: true,
                    session_active: true,
                    ..VoiceStatus::default()
                },
                ..Opts::default()
            },
        ),
        (
            "guest mode",
            Opts {
                signals: fresh.clone(),
                voice: VoiceStatus {
                    capture_running: true,
                    guest_seconds_left: 600,
                    ..VoiceStatus::default()
                },
                ..Opts::default()
            },
        ),
        (
            "voice not capturing",
            Opts {
                signals: fresh.clone(),
                voice: VoiceStatus::default(),
                ..Opts::default()
            },
        ),
        (
            "stale monitor",
            Opts {
                signals: HealthSignals::default(),
                ..Opts::default()
            },
        ),
        (
            "kernel says no",
            Opts {
                signals: fresh.clone(),
                allow: false,
                ..Opts::default()
            },
        ),
        (
            "the model writes a guilt line",
            Opts {
                signals: fresh.clone(),
                reply: vec!["Don't go, ", "I'll miss you."],
                ..Opts::default()
            },
        ),
        (
            "the model invents a memory",
            Opts {
                signals: fresh.clone(),
                reply: vec!["I was thinking about that coffee shop you mentioned."],
                ..Opts::default()
            },
        ),
        (
            "C2 refuses the line",
            Opts {
                signals: fresh.clone(),
                refuse: true,
                ..Opts::default()
            },
        ),
    ];
    for (name, opts) in cases {
        let r = rig(opts).await;
        r.runner.tick(t, &tz(), false).await;
        assert!(
            r.seen.lock().unwrap().spoken.is_empty(),
            "{name}: nothing may be spoken"
        );
        assert_eq!(
            r.hub.take_opener(t),
            None,
            "{name}: no talk may be remembered"
        );
    }
}

#[tokio::test]
async fn a_follow_up_talk_is_built_from_the_plan_the_owner_mentioned() {
    let t0 = octoplus_common::time::now_ns().unwrap();
    let r = rig(Opts {
        reply: vec!["How did the interview go?"],
        ..Opts::default()
    })
    .await;
    // The owner mentioned a plan and it was kept (try until the dice allow it).
    let mut kept = false;
    for i in 0..60u64 {
        if let octoplus_knowledge::companion::memory::Decision::Save(_) = r.hub.consider_memory(
            "I have an interview on Thursday at ten",
            true,
            t0 + i * NS_PER_S,
        ) {
            kept = true;
            break;
        }
    }
    assert!(kept);
    let line = r
        .runner
        .write_line(Topic::FollowUp, t0 + 100 * NS_PER_S)
        .await;
    assert_eq!(line.as_deref(), Some("How did the interview go?"));
    {
        let seen = r.seen.lock().unwrap();
        assert!(seen.prompts[0].contains("I have an interview on Thursday at ten"));
        assert!(!seen.prompts[0].contains("(noticed)"));
    }
    // It is asked about once only.
    assert_eq!(r.hub.take_followup(t0 + 101 * NS_PER_S), None);
    // And a follow-up with nothing to follow up on becomes an ordinary check-in.
    let line = r
        .runner
        .write_line(Topic::FollowUp, t0 + 200 * NS_PER_S)
        .await;
    // (The fake model says the same sentence again, which the repeat guard rightly drops.)
    assert_eq!(line, None);
    let seen = r.seen.lock().unwrap();
    assert!(seen.prompts[1].contains("how the owner's day is going"));
}
