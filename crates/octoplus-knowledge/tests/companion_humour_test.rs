#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Humour and manners (story C-S04): the owner's "funny / not funny" feedback, no jokes at
//! sensitive moments, avoided topics, no repeated jokes over 200 simulated days.

use std::sync::Arc;

use octoplus_knowledge::companion::engine::{CompanionEngine, Plan};
use octoplus_knowledge::companion::filter::{DropReason, ReplyFilter, Verdict};
use octoplus_knowledge::companion::hub::CompanionHub;
use octoplus_knowledge::companion::humour::{
    Feedback, HumourLevel, HumourState, ack_line, effective_level, late_hour, parse_feedback,
};
use octoplus_knowledge::companion::persona::Persona;
use octoplus_knowledge::companion::prompt::{CompanionPromptInput, assemble};
use octoplus_knowledge::companion::pulse::{PulseDecision, PulseInput, Topic};
use octoplus_knowledge::nudge::types::UserContext;
use octoplus_knowledge::prompt::Budgets;
use octoplus_taint::TaintFlags;

const NS_PER_S: u64 = 1_000_000_000;
const NS_PER_MIN: u64 = 60 * NS_PER_S;
const NS_PER_DAY: u64 = 86_400 * NS_PER_S;
const T0: u64 = 1_000_000_000_000_000;

fn persona() -> Persona {
    Persona::default_packaged().unwrap()
}

#[test]
fn feedback_is_read_from_short_utterances_only() {
    let funny = [
        "that was funny",
        "That was funny!",
        "haha",
        "ha ha",
        "good one",
        "that made me laugh",
        "you're funny",
    ];
    let not_funny = [
        "not funny",
        "that wasn't funny",
        "that was not funny",
        "bad joke",
        "stop joking",
        "no more jokes",
    ];
    for t in funny {
        assert_eq!(parse_feedback(t), Some(Feedback::Funny), "{t}");
    }
    for t in not_funny {
        assert_eq!(parse_feedback(t), Some(Feedback::NotFunny), "{t}");
    }
    for t in [
        "haha that is true but I still need to finish the report today",
        "the movie was not funny at all and the ending made no sense to me",
        "funny",
        "what is funny about it",
        "tell me a joke",
        "I read a funny article about bad jokes yesterday evening",
        "",
    ] {
        assert_eq!(parse_feedback(t), None, "{t}");
    }
    assert!(
        octoplus_knowledge::companion::guards::forbidden_phrase(ack_line(Feedback::Funny))
            .is_none()
    );
    assert!(
        octoplus_knowledge::companion::guards::forbidden_phrase(ack_line(Feedback::NotFunny))
            .is_none()
    );
}

#[test]
fn taste_moves_with_feedback_and_never_leaves_its_range() {
    let mut h = HumourState::default();
    assert_eq!(h.level(), HumourLevel::High.min(h.level()));
    assert_eq!(h.level(), HumourLevel::Normal);
    h.record(Feedback::NotFunny);
    assert_eq!(h.level(), HumourLevel::Normal);
    h.record(Feedback::NotFunny);
    assert_eq!(h.level(), HumourLevel::Low);
    h.record(Feedback::NotFunny);
    h.record(Feedback::NotFunny);
    assert_eq!(h.level(), HumourLevel::Off);
    assert_eq!(h.joke_weight(), 0.0);
    for _ in 0..50 {
        h.record(Feedback::NotFunny);
    }
    assert!(h.scale >= 0.0);
    for _ in 0..50 {
        h.record(Feedback::Funny);
    }
    assert!(h.scale <= 1.5 + 1e-9);
    assert_eq!(h.level(), HumourLevel::High);
    assert!(h.joke_weight() > 1.0);
}

#[test]
fn late_hours_start_an_hour_before_quiet_hours_and_end_with_them() {
    let (qs, qe) = (23 * 60, 8 * 60);
    assert!(!late_hour(21 * 60 + 59, qs, qe));
    assert!(late_hour(22 * 60, qs, qe));
    assert!(late_hour(23 * 60 + 30, qs, qe));
    assert!(late_hour(3 * 60, qs, qe));
    assert!(late_hour(7 * 60 + 59, qs, qe));
    assert!(!late_hour(8 * 60, qs, qe));
    assert!(!late_hour(15 * 60, qs, qe));
}

#[test]
fn sensitive_moments_switch_humour_off_whatever_the_taste() {
    let high = {
        let mut h = HumourState::default();
        for _ in 0..5 {
            h.record(Feedback::Funny);
        }
        h
    };
    assert_eq!(effective_level(&high, false), HumourLevel::High);
    assert_eq!(effective_level(&high, true), HumourLevel::Off);

    let p = Arc::new(persona());
    // Late at night, even with a playful taste.
    let mut e = CompanionEngine::new(p.clone(), 1, T0);
    e.set_moment(23 * 60 + 15, HumourLevel::High);
    let Plan::Generate(g) = e.plan("tell me something fun", T0) else {
        panic!("expected a generated reply")
    };
    assert_eq!(g.humour, HumourLevel::Off, "no jokes late at night");

    // Daytime, playful taste: allowed.
    let mut e = CompanionEngine::new(p.clone(), 1, T0);
    e.set_moment(15 * 60, HumourLevel::High);
    let Plan::Generate(g) = e.plan("tell me something fun", T0) else {
        panic!("expected a generated reply")
    };
    assert_eq!(g.humour, HumourLevel::High);

    // The owner sounds sad or tired.
    let mut e = CompanionEngine::new(p.clone(), 1, T0);
    e.set_moment(15 * 60, HumourLevel::High);
    let Plan::Generate(g) = e.plan("I'm so tired and a bit lonely today", T0) else {
        panic!("expected a generated reply")
    };
    assert_eq!(g.humour, HumourLevel::Off);

    // A few turns after anything that looked like a crisis.
    let mut e = CompanionEngine::new(p, 1, T0);
    e.set_moment(15 * 60, HumourLevel::High);
    assert!(matches!(
        e.plan("I want to kill myself", T0),
        Plan::Fixed(_)
    ));
    for i in 1..=3u64 {
        let Plan::Generate(g) = e.plan("ok", T0 + i * NS_PER_MIN) else {
            panic!("expected a generated reply")
        };
        assert_eq!(g.humour, HumourLevel::Off, "turn {i} after a crisis");
    }
}

#[test]
fn the_humour_line_reaches_the_prompt_and_the_system_block_stays_constant() {
    let p = persona();
    let budgets = Budgets {
        facts: 48,
        focus: 0,
        history: 200,
        evidence: 0,
        bytes_per_token: 3,
    };
    let mk = |h: HumourLevel| {
        assemble(
            &CompanionPromptInput {
                persona: &p,
                mode: octoplus_knowledge::companion::mode::Mode::Heart,
                switch_instruction: None,
                kabir_allowed: false,
                gentle_line: None,
                humour: h,
                facts: &[],
                facts_taint: TaintFlags::empty(),
                history: "",
                history_taint: TaintFlags::empty(),
                question: "hello",
            },
            &budgets,
        )
    };
    let off = mk(HumourLevel::Off);
    let high = mk(HumourLevel::High);
    assert!(off.text.contains("Do not joke or tease"));
    assert!(high.text.contains("A short joke is welcome"));
    assert_eq!(off.stable_prefix_chars, high.stable_prefix_chars);
    let n = off.stable_prefix_chars as usize;
    assert_eq!(
        off.text[..n],
        high.text[..n],
        "the cached prefix must not change"
    );
}

#[test]
fn avoided_topics_are_never_spoken() {
    let topics = vec!["politics".to_string(), "my ex".to_string()];
    let mut f = ReplyFilter::new(64).with_avoid_topics(&topics);
    for s in [
        "Let's talk about politics for a bit.",
        "Have you heard from my ex lately?",
        "POLITICS is a big topic.",
    ] {
        assert!(
            matches!(
                f.check(s, false, true),
                Verdict::Drop(DropReason::AvoidedTopic)
            ),
            "{s}"
        );
    }
    assert!(
        matches!(
            f.check("The politicians are away today.", false, true),
            Verdict::Keep(_)
        ),
        "whole words only"
    );
    assert!(matches!(
        f.check("Tell me about your day.", false, true),
        Verdict::Keep(_)
    ));
    assert_eq!(f.counters().dropped_avoided_topic, 3);
    // The persona's own list is wired into the engine and the hub.
    let mut p = persona();
    p.avoid_topics = vec!["football".to_string()];
    let hub = CompanionHub::new(&p, 1);
    assert!(
        hub.check_line("Did you watch the football last night?")
            .is_none()
    );
    assert!(hub.check_line("Did you sleep well last night?").is_some());
}

#[test]
fn not_funny_means_fewer_jokes_from_the_scheduler() {
    let count_jokes = |taste_hits: u32| -> usize {
        let hub = CompanionHub::new(&persona(), 11);
        for _ in 0..taste_hits {
            hub.record_feedback(Feedback::NotFunny);
        }
        let mut jokes = 0;
        for i in 0..4000u64 {
            let now = T0 + i * 3 * 3600 * NS_PER_S; // one chance every three hours, many days
            let minute = 9 * 60 + (i % 8) as u16 * 60;
            let input = PulseInput {
                now_ns: now,
                minute_of_day: minute,
                day_index: i,
                ctx: UserContext {
                    voice_available: true,
                    ..UserContext::default()
                },
                idle_ms: 0,
                ..PulseInput::default()
            };
            if let PulseDecision::Talk(Topic::Joke) = hub.tick(&input) {
                jokes += 1;
            }
        }
        jokes
    };
    let normal = count_jokes(0);
    let low = count_jokes(2);
    let off = count_jokes(4);
    println!("jokes in 4000 talks: normal {normal}, after 2x not funny {low}, after 4x {off}");
    assert!(normal > 100);
    assert!(low < normal);
    assert_eq!(off, 0, "never again after enough 'not funny'");
}

#[test]
fn a_joke_is_not_told_twice_over_two_hundred_simulated_days() {
    // A fake model that tells one of 40 jokes at random every time. The filter is kept across
    // conversations through the hub, so a joke told this morning is not told again soon.
    let hub = CompanionHub::new(&persona(), 5);
    let p = Arc::new(persona());
    let jokes: Vec<String> = (0..40)
        .map(|i| format!("Why did the number {i} cross the road to reach the other side."))
        .collect();
    let mut rng = octoplus_knowledge::companion::rng::Rng::new(77);
    // The guard remembers the last 128 lines it was asked about (told or dropped).
    let mut draws: Vec<(String, bool)> = Vec::new(); // (joke, was it spoken)
    for day in 0..200u64 {
        let mut e = CompanionEngine::new(p.clone(), day, T0 + day * NS_PER_DAY);
        e.adopt_filter(hub.lend_filter());
        for turn in 0..3u64 {
            let now = T0 + day * NS_PER_DAY + turn * NS_PER_MIN;
            e.set_moment(15 * 60, HumourLevel::Normal);
            let _ = e.plan("tell me a joke", now);
            let pick = jokes[rng.pick(jokes.len())].clone();
            let kept = e.filter_pieces(std::slice::from_ref(&pick), false);
            draws.push((pick, !kept.is_empty()));
        }
        hub.return_filter(e.take_filter());
    }
    // A joke that was spoken is never spoken again within the next 120 draws.
    let mut spoken = 0;
    for (i, (joke, was_spoken)) in draws.iter().enumerate() {
        if !was_spoken {
            continue;
        }
        spoken += 1;
        let from = i.saturating_sub(120);
        let again = draws[from..i].iter().any(|(j, s)| j == joke && *s);
        assert!(!again, "joke told twice within its memory at draw {i}");
    }
    let dropped = draws.len() - spoken;
    println!("200 days: {spoken} jokes told, {dropped} repeats stopped");
    assert!(dropped > 0, "the filter really had work to do");
    // With a pool of only 40 jokes, each gets through and then stays blocked for a long while.
    assert!(spoken >= 40, "every joke was told at least once: {spoken}");
}
