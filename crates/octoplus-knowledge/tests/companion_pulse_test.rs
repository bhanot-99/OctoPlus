#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Pulse scheduler (story C-S03): a fake-clock simulation of 200 days plus the spoken controls.

use octoplus_knowledge::companion::guards;
use octoplus_knowledge::companion::persona::Persona;
use octoplus_knowledge::companion::pulse::{
    Pulse, PulseCommand, PulseConfig, PulseDecision, PulseInput, Response, Topic, WaitReason,
    ack_line, apply_command, parse_pulse_command,
};
use octoplus_knowledge::companion::rng::Rng;
use octoplus_knowledge::nudge::types::UserContext;

const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;
const TICK: u64 = 30;

fn cfg() -> PulseConfig {
    PulseConfig::from_persona(&Persona::default_packaged().unwrap())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    /// Answers 60 %, ignores 30 %, says not now 10 %.
    Normal,
    /// Never answers.
    Ignores,
}

#[derive(Default)]
struct SimResult {
    talks: Vec<(u64, u64, Topic)>, // (day, second of day, topic)
    free_hours: f64,
    max_free_gap_min: u64,
    violations: Vec<String>,
    counters_talks: u64,
}

/// What the scripted owner is doing at a minute of the day.
fn script(minute: u32) -> (bool, bool, bool, bool, bool) {
    // (asleep_or_idle, away, call, fullscreen, quiet_by_clock)
    let asleep = !(7 * 60 + 30..24 * 60).contains(&minute);
    let away = (12 * 60 + 30..13 * 60).contains(&minute);
    let call = (15 * 60..15 * 60 + 45).contains(&minute);
    let fullscreen = (20 * 60..21 * 60 + 30).contains(&minute);
    let quiet = !(8 * 60..23 * 60).contains(&minute);
    (asleep, away, call, fullscreen, quiet)
}

fn run_sim(days: u64, seed: u64, owner: Owner, tweak: impl Fn(&mut Pulse)) -> SimResult {
    let mut pulse = Pulse::new(cfg(), seed);
    tweak(&mut pulse);
    let mut world = Rng::new(seed ^ 0xABCD);
    let mut res = SimResult::default();
    let mut session_left_ticks = 0u32;
    let mut free_min_since_talk = 0u64;
    let mut last_talk_ns: Option<u64> = None;
    let mut prev_away = false;
    let mut free_ticks = 0u64;

    for day in 0..days {
        for tick in 0..(86_400 / TICK) {
            let sec = tick * TICK;
            let now_ns = (day * 86_400 + sec) * NS_PER_S + 1_000_000_000_000_000;
            let minute = u32::try_from(sec / 60).unwrap();
            let (asleep, away, call, fullscreen, quiet) = script(minute);
            let in_session = session_left_ticks > 0;
            session_left_ticks = session_left_ticks.saturating_sub(1);
            let input = PulseInput {
                now_ns,
                minute_of_day: u16::try_from(minute).unwrap(),
                day_index: day,
                ctx: UserContext {
                    paused: false,
                    quiet_hours: false,
                    in_turn: false,
                    mic_busy: call,
                    media_playing: false,
                    fullscreen,
                    away: away || asleep,
                    voice_available: true,
                },
                in_session,
                guest: false,
                battery_saver: false,
                idle_ms: if asleep || away { 3_600_000 } else { 5_000 },
                returned_from_idle: prev_away && !away && !asleep,
                task_finished: world.chance(0.002),
                natural_pause: world.chance(0.02),
            };
            prev_away = away || asleep;
            let is_free = !(asleep || away || call || fullscreen || quiet || in_session);
            if is_free {
                free_ticks += 1;
                if tick % 2 == 0 {
                    free_min_since_talk += 1;
                }
                res.max_free_gap_min = res.max_free_gap_min.max(free_min_since_talk);
            }
            match pulse.tick(&input) {
                PulseDecision::Talk(topic) => {
                    // Hard rules, checked at the moment of the talk.
                    if quiet {
                        res.violations
                            .push(format!("talk in quiet hours d{day} m{minute}"));
                    }
                    if call {
                        res.violations
                            .push(format!("talk during a call d{day} m{minute}"));
                    }
                    if fullscreen {
                        res.violations
                            .push(format!("talk in fullscreen d{day} m{minute}"));
                    }
                    if away || asleep {
                        res.violations
                            .push(format!("talk while away d{day} m{minute}"));
                    }
                    if in_session {
                        res.violations
                            .push(format!("talk in a session d{day} m{minute}"));
                    }
                    if let Some(l) = last_talk_ns
                        && now_ns - l < 10 * NS_PER_MIN
                    {
                        res.violations
                            .push(format!("gap under 10 min d{day} m{minute}"));
                    }
                    last_talk_ns = Some(now_ns);
                    free_min_since_talk = 0;
                    res.talks.push((day, sec, topic));
                    let r = match owner {
                        Owner::Ignores => Response::Ignored,
                        Owner::Normal => {
                            let x = world.next_f64();
                            if x < 0.6 {
                                Response::Accepted
                            } else if x < 0.9 {
                                Response::Ignored
                            } else {
                                Response::NotNow
                            }
                        }
                    };
                    if r == Response::Accepted {
                        session_left_ticks = 6; // a three minute chat
                    }
                    pulse.record_response(now_ns, u16::try_from(minute).unwrap(), r);
                }
                PulseDecision::Wait(_) => {}
            }
        }
    }
    res.free_hours = free_ticks as f64 * TICK as f64 / 3600.0;
    res.counters_talks = pulse.counters().talks;
    res
}

#[test]
fn two_hundred_days_obey_every_hard_rule() {
    let r = run_sim(200, 7, Owner::Normal, |_| {});
    assert!(
        r.violations.is_empty(),
        "violations: {:?}",
        &r.violations[..r.violations.len().min(5)]
    );
    assert_eq!(r.counters_talks as usize, r.talks.len());

    // Hourly cap (4) in every rolling hour and the daily cap (40).
    let times: Vec<u64> = r.talks.iter().map(|(d, s, _)| d * 86_400 + s).collect();
    for (i, t) in times.iter().enumerate() {
        let in_hour = times[..=i].iter().filter(|x| t - **x < 3600).count();
        assert!(
            in_hour <= 4,
            "more than 4 talks in one hour (day {})",
            t / 86_400
        );
    }
    for day in 0..200u64 {
        let n = r.talks.iter().filter(|(d, _, _)| *d == day).count();
        assert!(n <= 40, "day {day} had {n} talks");
    }
    // Never silent for hours while the owner is free (floor 90 min, stretching to 270 after unwelcome talks).
    assert!(
        r.max_free_gap_min <= 280,
        "longest free gap {} min",
        r.max_free_gap_min
    );
    // Topics never repeat back to back.
    for w in r.talks.windows(2) {
        if w[0].0 == w[1].0 {
            assert_ne!(w[0].2, w[1].2, "same topic twice in a row");
        }
    }
    let per_hour = r.talks.len() as f64 / r.free_hours;
    println!(
        "pulse: {} talks over {:.0} free hours = {:.2} per free hour, longest free gap {} min",
        r.talks.len(),
        r.free_hours,
        per_hour,
        r.max_free_gap_min
    );
    assert!((1.5..=4.0).contains(&per_hour), "rate {per_hour}");
}

#[test]
fn an_owner_who_ignores_everything_gets_quieter_but_never_silent() {
    let normal = run_sim(14, 3, Owner::Normal, |_| {});
    let ignored = run_sim(14, 3, Owner::Ignores, |_| {});
    // Judge the last four days, after she has had time to learn.
    let late = |r: &SimResult| r.talks.iter().filter(|(d, _, _)| *d >= 10).count() as f64 / 4.0;
    let (per_day_normal, per_day_ignored) = (late(&normal), late(&ignored));
    println!("per day (days 11-14): normal {per_day_normal:.1}, ignored {per_day_ignored:.1}");
    assert!(
        per_day_ignored >= 2.0,
        "must not go silent: {per_day_ignored}"
    );
    assert!(
        per_day_ignored <= 9.0,
        "floor-driven rate only: {per_day_ignored}"
    );
    assert!(per_day_ignored < per_day_normal);

    // The learned weight of the hours she tried falls.
    let mut p = Pulse::new(cfg(), 1);
    for d in 0..14u64 {
        p.record_response(d * 86_400 * NS_PER_S, 10 * 60, Response::Ignored);
    }
    assert!(p.hour_weight(10) <= 0.6, "weight {}", p.hour_weight(10));
    assert!((p.hour_weight(3) - 1.0).abs() < 1e-9);
}

#[test]
fn not_now_is_followed_by_an_hour_of_quiet_and_snooze_is_respected() {
    let mut p = Pulse::new(cfg(), 9);
    let base = 1_000_000_000_000_000u64;
    let mk = |t: u64| PulseInput {
        now_ns: t,
        minute_of_day: 10 * 60,
        day_index: 5,
        ctx: UserContext {
            voice_available: true,
            ..UserContext::default()
        },
        idle_ms: 1000,
        ..PulseInput::default()
    };
    p.record_response(base, 600, Response::NotNow);
    for m in 0..60u64 {
        assert_eq!(
            p.tick(&mk(base + m * NS_PER_MIN)),
            PulseDecision::Wait(WaitReason::Snoozed),
            "minute {m}"
        );
    }
    p.snooze_for(base + 61 * NS_PER_MIN, 120);
    for m in 61..180u64 {
        assert!(matches!(
            p.tick(&mk(base + m * NS_PER_MIN)),
            PulseDecision::Wait(WaitReason::Snoozed)
        ));
    }
    // After the snooze the floor makes her talk at once.
    assert!(matches!(
        p.tick(&mk(base + 190 * NS_PER_MIN)),
        PulseDecision::Talk(_)
    ));
}

#[test]
fn chat_more_raises_and_chat_less_lowers_the_rate() {
    let base = run_sim(30, 11, Owner::Normal, |_| {}).talks.len();
    let more = run_sim(30, 11, Owner::Normal, |p| {
        p.chat_more();
        p.chat_more();
    })
    .talks
    .len();
    let less = run_sim(30, 11, Owner::Normal, |p| {
        p.chat_less();
        p.chat_less();
    })
    .talks
    .len();
    println!("30 days: less {less}, base {base}, more {more}");
    assert!(more >= base, "more {more} base {base}");
    assert!(less <= base, "less {less} base {base}");
    assert!(more > less);
}

#[test]
fn same_seed_same_decisions_other_seed_differs() {
    let a = run_sim(5, 42, Owner::Normal, |_| {});
    let b = run_sim(5, 42, Owner::Normal, |_| {});
    let c = run_sim(5, 43, Owner::Normal, |_| {});
    assert_eq!(a.talks, b.talks);
    assert_ne!(a.talks, c.talks);
}

#[test]
fn every_gate_blocks_and_names_its_reason() {
    let base = 1_000_000_000_000_000u64;
    let ok = PulseInput {
        now_ns: base,
        minute_of_day: 10 * 60,
        day_index: 1,
        ctx: UserContext {
            voice_available: true,
            ..UserContext::default()
        },
        idle_ms: 1000,
        ..PulseInput::default()
    };
    let cases: Vec<(&str, PulseInput, WaitReason)> = vec![
        (
            "paused",
            PulseInput {
                ctx: UserContext {
                    paused: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::Paused,
        ),
        ("guest", PulseInput { guest: true, ..ok }, WaitReason::Guest),
        (
            "quiet",
            PulseInput {
                minute_of_day: 23 * 60 + 30,
                ..ok
            },
            WaitReason::QuietHours,
        ),
        (
            "early",
            PulseInput {
                minute_of_day: 6 * 60,
                ..ok
            },
            WaitReason::QuietHours,
        ),
        (
            "session",
            PulseInput {
                in_session: true,
                ..ok
            },
            WaitReason::InSession,
        ),
        (
            "turn",
            PulseInput {
                ctx: UserContext {
                    in_turn: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::UserBusy,
        ),
        (
            "mic",
            PulseInput {
                ctx: UserContext {
                    mic_busy: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::MicBusy,
        ),
        (
            "fullscreen",
            PulseInput {
                ctx: UserContext {
                    fullscreen: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::Fullscreen,
        ),
        (
            "media",
            PulseInput {
                ctx: UserContext {
                    media_playing: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::MediaLoud,
        ),
        (
            "away",
            PulseInput {
                ctx: UserContext {
                    away: true,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::Away,
        ),
        (
            "idle",
            PulseInput {
                idle_ms: 700_000,
                ..ok
            },
            WaitReason::Away,
        ),
        (
            "battery",
            PulseInput {
                battery_saver: true,
                ..ok
            },
            WaitReason::BatterySaver,
        ),
        (
            "novoice",
            PulseInput {
                ctx: UserContext {
                    voice_available: false,
                    ..ok.ctx
                },
                ..ok
            },
            WaitReason::NoVoice,
        ),
    ];
    for (name, input, want) in cases {
        let mut p = Pulse::new(cfg(), 1);
        assert_eq!(p.tick(&input), PulseDecision::Wait(want), "{name}");
    }
    // And the open moment talks (first talk of the day).
    let mut p = Pulse::new(cfg(), 1);
    assert!(matches!(p.tick(&ok), PulseDecision::Talk(Topic::Morning)));
    // The same moment 5 minutes later: too soon.
    let later = PulseInput {
        now_ns: base + 5 * NS_PER_MIN,
        ..ok
    };
    assert_eq!(p.tick(&later), PulseDecision::Wait(WaitReason::MinGap));
}

#[test]
fn caps_hold_even_with_every_boost() {
    let base = 1_000_000_000_000_000u64;
    let mut p = Pulse::new(cfg(), 5);
    let mut talks = 0;
    for m in 0..300u64 {
        let input = PulseInput {
            now_ns: base + m * NS_PER_MIN,
            minute_of_day: u16::try_from(8 * 60 + m).unwrap().min(1439),
            day_index: 1,
            ctx: UserContext {
                voice_available: true,
                ..UserContext::default()
            },
            idle_ms: 0,
            returned_from_idle: true,
            task_finished: true,
            natural_pause: true,
            ..PulseInput::default()
        };
        if matches!(p.tick(&input), PulseDecision::Talk(_)) {
            talks += 1;
        }
    }
    // 5 hours: gaps of 10 minutes allow at most 4 an hour.
    assert!(talks <= 20, "{talks}");
    assert!(talks >= 5);
}

#[test]
fn spoken_controls_are_read_from_short_utterances_only() {
    use PulseCommand::*;
    let yes: Vec<(&str, PulseCommand)> = vec![
        ("not now", NotNow),
        ("Not now.", NotNow),
        ("not right now", NotNow),
        ("hey, not now", NotNow),
        ("maybe later", NotNow),
        ("quiet for an hour", QuietFor { minutes: 60 }),
        ("be quiet for an hour", QuietFor { minutes: 60 }),
        ("please be quiet for an hour", QuietFor { minutes: 60 }),
        ("can you be quiet for an hour", QuietFor { minutes: 60 }),
        ("be quiet for 30 minutes", QuietFor { minutes: 30 }),
        ("quiet for twenty minutes", QuietFor { minutes: 20 }),
        ("quiet for two hours", QuietFor { minutes: 120 }),
        ("quiet for half an hour", QuietFor { minutes: 30 }),
        ("quiet for today", QuietUntilMorning),
        ("stop talking to me for today", QuietUntilMorning),
        ("chat more", ChatMore),
        ("talk more", ChatMore),
        ("talk to me more", ChatMore),
        ("chat less", ChatLess),
        ("talk less", ChatLess),
        ("be less chatty", ChatLess),
        ("pause the companion", Pause),
        ("pause companion", Pause),
        ("resume the companion", Resume),
        ("resume companion", Resume),
    ];
    for (t, want) in &yes {
        assert_eq!(parse_pulse_command(t), Some(*want), "{t}");
    }
    let no = [
        "I'm not now interested in that",
        "not now but tomorrow I will tell you the whole story about my trip",
        "what is quiet for an hour supposed to mean in the movie",
        "chat",
        "talk",
        "the more you talk the less I hear",
        "pause",
        "resume",
        "I need a quiet hour to think",
        "quiet",
        "quiet for",
        "quiet for a year",
        "tell me more",
        "less is more",
        "can you chat about music",
        "now is not the time to be quiet for sure",
        "my companion is paused in the game",
        "stop",
        "later",
        "hello",
        "how are you",
        "you talk more than my sister does on the phone",
        "talk to me about the weather",
        "I will resume work now",
        "pausing the video",
    ];
    for t in no {
        assert_eq!(parse_pulse_command(t), None, "{t}");
    }
    // Acknowledgements are plain and never use a forbidden phrase.
    for c in [
        NotNow,
        QuietFor { minutes: 5 },
        QuietUntilMorning,
        ChatMore,
        ChatLess,
        Pause,
        Resume,
    ] {
        assert!(guards::forbidden_phrase(ack_line(&c)).is_none());
    }
}

#[test]
fn commands_change_the_scheduler() {
    let base = 1_000_000_000_000_000u64;
    let mut p = Pulse::new(cfg(), 2);
    apply_command(&mut p, &PulseCommand::Pause, base, 600);
    assert!(p.paused());
    apply_command(&mut p, &PulseCommand::Resume, base, 600);
    assert!(!p.paused());
    apply_command(&mut p, &PulseCommand::ChatMore, base, 600);
    assert!(p.rate_scale() > 1.0);
    apply_command(&mut p, &PulseCommand::ChatLess, base, 600);
    apply_command(&mut p, &PulseCommand::ChatLess, base, 600);
    assert!(p.rate_scale() < 1.0);
    // Quiet until morning at 22:00 lasts until 08:00 (10 hours).
    apply_command(&mut p, &PulseCommand::QuietUntilMorning, base, 22 * 60);
    let input = PulseInput {
        now_ns: base + 9 * 60 * NS_PER_MIN,
        minute_of_day: 7 * 60,
        day_index: 1,
        ctx: UserContext {
            voice_available: true,
            ..UserContext::default()
        },
        ..PulseInput::default()
    };
    // 07:00 is inside the quiet hours anyway; check the snooze with a daytime minute value.
    let input = PulseInput {
        minute_of_day: 12 * 60,
        ..input
    };
    assert_eq!(p.tick(&input), PulseDecision::Wait(WaitReason::Snoozed));
}

#[test]
fn the_followup_topic_is_only_offered_when_available() {
    let base = 1_000_000_000_000_000u64;
    let mut seen_without = false;
    let mut seen_with = false;
    for seed in 0..300u64 {
        for (flag, seen) in [(false, &mut seen_without), (true, &mut seen_with)] {
            let mut p = Pulse::new(cfg(), seed);
            p.set_followup_available(flag);
            let input = PulseInput {
                now_ns: base,
                minute_of_day: 15 * 60,
                day_index: 1,
                ctx: UserContext {
                    voice_available: true,
                    ..UserContext::default()
                },
                idle_ms: 100,
                ..PulseInput::default()
            };
            if let PulseDecision::Talk(Topic::FollowUp) = p.tick(&input) {
                *seen = true;
            }
        }
    }
    assert!(!seen_without);
    assert!(seen_with);
}

#[test]
fn snapshot_round_trips_and_bad_data_falls_back() {
    let mut p = Pulse::new(cfg(), 1);
    p.chat_more();
    p.record_response(1, 9 * 60, Response::Accepted);
    p.record_response(2, 9 * 60, Response::Ignored);
    let snap = p.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: octoplus_knowledge::companion::pulse::PulseSnapshot =
        serde_json::from_str(&json).unwrap();
    let q = Pulse::restore(cfg(), 1, &back);
    assert!((q.rate_scale() - p.rate_scale()).abs() < 1e-9);
    assert!((q.hour_weight(9) - p.hour_weight(9)).abs() < 1e-9);

    let mut bad = back.clone();
    bad.version = 99;
    let r = Pulse::restore(cfg(), 1, &bad);
    assert!((r.rate_scale() - 1.0).abs() < 1e-9);
    let mut short = back;
    short.buckets.truncate(3);
    assert!((Pulse::restore(cfg(), 1, &short).hour_weight(9) - 1.0).abs() < 1e-9);
}
