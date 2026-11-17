#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;

use neuroos_knowledge::nudge::policy::Policy;
use neuroos_knowledge::nudge::types::{
    Action, Candidate, Delivery, PolicyConfig, Priority, Reason, UserContext,
};

fn make_candidate(
    rule_id: &str,
    key: &str,
    priority: Priority,
    deliver: Option<Delivery>,
) -> Candidate {
    Candidate {
        rule_id: rule_id.to_string(),
        key: key.to_string(),
        priority,
        deliver,
        template: "test.template".to_string(),
        args: vec![("val".to_string(), 42.0)],
        offer_skill: None,
        at_ns: 1_000_000_000,
    }
}

fn make_context() -> UserContext {
    UserContext {
        paused: false,
        quiet_hours: false,
        in_turn: false,
        mic_busy: false,
        media_playing: false,
        fullscreen: false,
        away: false,
        voice_available: true,
    }
}

fn make_cfg() -> PolicyConfig {
    PolicyConfig {
        quiet_mode: false,
        deliver_info: Delivery::Voice,
        deliver_important: Delivery::Voice,
        deliver_urgent: Delivery::Voice,
        max_spoken_per_hour: 3,
        max_spoken_per_day: 8,
        max_held: 5,
    }
}

#[test]
fn test_design_8_3_situations_per_priority() {
    let priorities = [Priority::Info, Priority::Important, Priority::Urgent];

    // 1. Paused: Urgent -> Notify { silent: true }, Reason::Paused; others -> Hold, Reason::Paused
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.paused = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::Paused);
        if p == Priority::Urgent {
            assert_eq!(d.action, Action::Notify { silent: true });
            assert!(policy.held().is_empty());
        } else {
            assert_eq!(d.action, Action::Hold);
            assert_eq!(policy.held().len(), 1);
        }
    }

    // 2. Quiet hours: same as paused
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.quiet_hours = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::QuietHours);
        if p == Priority::Urgent {
            assert_eq!(d.action, Action::Notify { silent: true });
            assert!(policy.held().is_empty());
        } else {
            assert_eq!(d.action, Action::Hold);
            assert_eq!(policy.held().len(), 1);
        }
    }

    // 3. In turn: Hold, Reason::InTurn (even urgent)
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.in_turn = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::InTurn);
        assert_eq!(d.action, Action::Hold);
        assert_eq!(policy.held().len(), 1);
    }

    // 4. Mic busy: same as paused
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.mic_busy = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::MicBusy);
        if p == Priority::Urgent {
            assert_eq!(d.action, Action::Notify { silent: true });
            assert!(policy.held().is_empty());
        } else {
            assert_eq!(d.action, Action::Hold);
            assert_eq!(policy.held().len(), 1);
        }
    }

    // 5. Away: Hold, Reason::Away (even urgent)
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.away = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::Away);
        assert_eq!(d.action, Action::Hold);
        assert_eq!(policy.held().len(), 1);
    }

    // 6. Media playing: Notify { silent: false }, Reason::Media (all priorities)
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.media_playing = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::Media);
        assert_eq!(d.action, Action::Notify { silent: false });
        assert!(policy.held().is_empty());
    }

    // 7. Fullscreen: Notify { silent: false }, Reason::Fullscreen (all priorities)
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.fullscreen = true;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.reason, Reason::Fullscreen);
        assert_eq!(d.action, Action::Notify { silent: false });
        assert!(policy.held().is_empty());
    }

    // 8. Limits: max_spoken reached
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let ctx = make_context();
        // Fill 3 spoken quota
        for i in 0..3 {
            policy.decide(
                make_candidate(&format!("r{i}"), "k", Priority::Info, Some(Delivery::Voice)),
                &ctx,
                (i + 1) * 1_000_000_000,
            );
        }
        let c = make_candidate("limit_rule", "k", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 10_000_000_000);
        if p == Priority::Urgent {
            // First urgent uses urgent exception
            assert_eq!(d.action, Action::Speak);
            assert_eq!(d.reason, Reason::Clear);
        } else {
            assert_eq!(d.action, Action::Hold);
            assert_eq!(d.reason, Reason::RateLimited);
        }
    }

    // 9. Quiet mode: cfg.quiet_mode = true, delivery = Voice -> Notify { silent: false }, Reason::QuietMode
    for p in priorities {
        let mut cfg = make_cfg();
        cfg.quiet_mode = true;
        let mut policy = Policy::new(cfg);
        let ctx = make_context();
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.action, Action::Notify { silent: false });
        assert_eq!(d.reason, Reason::QuietMode);
    }

    // 10. Delivery Off -> Action::Drop, Reason::DeliveryOff
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let ctx = make_context();
        let c = make_candidate("r1", "k1", p, Some(Delivery::Off));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.action, Action::Drop);
        assert_eq!(d.reason, Reason::DeliveryOff);
    }

    // 11. No voice: !ctx.voice_available -> Notify { silent: false }, Reason::NoVoice
    for p in priorities {
        let mut policy = Policy::new(make_cfg());
        let mut ctx = make_context();
        ctx.voice_available = false;
        let c = make_candidate("r1", "k1", p, Some(Delivery::Voice));
        let d = policy.decide(c, &ctx, 1_000_000_000);
        assert_eq!(d.action, Action::Notify { silent: false });
        assert_eq!(d.reason, Reason::NoVoice);
    }
}

#[test]
fn test_rate_limit_boundaries() {
    let mut cfg = make_cfg();
    cfg.max_spoken_per_hour = 3;
    cfg.max_spoken_per_day = 8;
    let mut policy = Policy::new(cfg);
    let ctx = make_context();

    // 1st, 2nd, 3rd in hour 1 pass
    let d1 = policy.decide(
        make_candidate("r1", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        1_000_000_000,
    );
    assert_eq!(d1.action, Action::Speak);

    let d2 = policy.decide(
        make_candidate("r2", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        2_000_000_000,
    );
    assert_eq!(d2.action, Action::Speak);

    let d3 = policy.decide(
        make_candidate("r3", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        3_000_000_000,
    );
    assert_eq!(d3.action, Action::Speak);
    assert_eq!(policy.spoken_in_last_hour(3_000_000_000), 3);

    // 4th is held
    let d4 = policy.decide(
        make_candidate("r4", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        4_000_000_000,
    );
    assert_eq!(d4.action, Action::Hold);
    assert_eq!(d4.reason, Reason::RateLimited);

    // Hour rolling over: at t = 1_000_000_000 + 3600 * 1e9, d1 rolled over!
    // spoken_in_last_hour is now 2
    let now_rolled = 1_000_000_000 + 3600 * 1_000_000_000;
    assert_eq!(policy.spoken_in_last_hour(now_rolled), 2);

    let d5 = policy.decide(
        make_candidate("r5", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        now_rolled,
    );
    assert_eq!(d5.action, Action::Speak);

    // Test daily limit: reach 8 total in day
    // Currently spoken: d1, d2, d3, d5 (4 total)
    let d6 = policy.decide(
        make_candidate("r6", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        now_rolled + 10_000_000_000,
    );
    assert_eq!(d6.action, Action::Speak);

    // Advance by another hour (plus 20s so d6 has rolled over too)
    let t_h2 = now_rolled + 3600 * 1_000_000_000 + 20 * 1_000_000_000;
    let d7 = policy.decide(
        make_candidate("r7", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        t_h2,
    );
    assert_eq!(d7.action, Action::Speak);

    let d8 = policy.decide(
        make_candidate("r8", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        t_h2 + 1_000_000_000,
    );
    assert_eq!(d8.action, Action::Speak); // 7th

    let d9 = policy.decide(
        make_candidate("r9", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        t_h2 + 2_000_000_000,
    );
    assert_eq!(d9.action, Action::Speak); // 8th (day limit reached!)

    // 9th is held due to day limit, even though hour limit (3) is not exceeded
    let d10 = policy.decide(
        make_candidate("r10", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        t_h2 + 3_000_000_000,
    );
    assert_eq!(d10.action, Action::Hold);
    assert_eq!(d10.reason, Reason::RateLimited);
}

#[test]
fn test_urgent_exception() {
    let mut cfg = make_cfg();
    cfg.max_spoken_per_hour = 1;
    let mut policy = Policy::new(cfg);
    let ctx = make_context();

    // 1 spoken fills hour quota
    let d1 = policy.decide(
        make_candidate("r_normal", "k", Priority::Info, Some(Delivery::Voice)),
        &ctx,
        1_000_000_000,
    );
    assert_eq!(d1.action, Action::Speak);

    // Urgent from "rule_A" gets urgent exception
    let d2 = policy.decide(
        make_candidate("rule_A", "k", Priority::Urgent, Some(Delivery::Voice)),
        &ctx,
        2_000_000_000,
    );
    assert_eq!(d2.action, Action::Speak);

    // Second urgent from "rule_A" in same hour is held
    let d3 = policy.decide(
        make_candidate("rule_A", "k", Priority::Urgent, Some(Delivery::Voice)),
        &ctx,
        3_000_000_000,
    );
    assert_eq!(d3.action, Action::Hold);
    assert_eq!(d3.reason, Reason::RateLimited);

    // Urgent from distinct "rule_B" in same hour also gets its own exception!
    let d4 = policy.decide(
        make_candidate("rule_B", "k", Priority::Urgent, Some(Delivery::Voice)),
        &ctx,
        4_000_000_000,
    );
    assert_eq!(d4.action, Action::Speak);
}

#[test]
fn test_replacement_of_held_nudge() {
    let mut policy = Policy::new(make_cfg());
    let mut ctx = make_context();
    ctx.away = true;

    // Hold candidate 1
    policy.decide(
        make_candidate("rule_1", "key_1", Priority::Info, None),
        &ctx,
        1_000_000_000,
    );
    assert_eq!(policy.held().len(), 1);
    assert!(policy.take_dropped().is_empty());

    // Hold candidate 2 with same (rule_id, key)
    let c2 = Candidate {
        rule_id: "rule_1".to_string(),
        key: "key_1".to_string(),
        priority: Priority::Important,
        deliver: None,
        template: "newer.template".to_string(),
        args: vec![],
        offer_skill: None,
        at_ns: 2_000_000_000,
    };
    policy.decide(c2, &ctx, 2_000_000_000);

    // Held still has 1 item (the newer one)
    assert_eq!(policy.held().len(), 1);
    assert_eq!(policy.held()[0].template, "newer.template");

    // Dropped list has the older one with Reason::Replaced
    let dropped = policy.take_dropped();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].candidate.template, "test.template");
    assert_eq!(dropped[0].action, Action::Drop);
    assert_eq!(dropped[0].reason, Reason::Replaced);
}

#[test]
fn test_overflow_of_held_list() {
    let mut cfg = make_cfg();
    cfg.max_held = 3;
    let mut policy = Policy::new(cfg);
    let mut ctx = make_context();
    ctx.away = true;

    // Push 3 candidates:
    // 1: Info ("r1", "k1")
    policy.decide(
        make_candidate("r1", "k1", Priority::Info, None),
        &ctx,
        1_000_000_000,
    );
    // 2: Important ("r2", "k2")
    policy.decide(
        make_candidate("r2", "k2", Priority::Important, None),
        &ctx,
        2_000_000_000,
    );
    // 3: Urgent ("r3", "k3")
    policy.decide(
        make_candidate("r3", "k3", Priority::Urgent, None),
        &ctx,
        3_000_000_000,
    );
    assert_eq!(policy.held().len(), 3);
    assert!(policy.take_dropped().is_empty());

    // Push 4th candidate: Info ("r4", "k4")
    policy.decide(
        make_candidate("r4", "k4", Priority::Info, None),
        &ctx,
        4_000_000_000,
    );

    // Max held is 3: oldest of lowest priority (the 1st Info candidate "r1") is pushed out
    assert_eq!(policy.held().len(), 3);
    let dropped = policy.take_dropped();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].candidate.rule_id, "r1");
    assert_eq!(dropped[0].action, Action::Drop);
    assert_eq!(dropped[0].reason, Reason::Overflow);

    let held_ids: Vec<&str> = policy.held().iter().map(|c| c.rule_id.as_str()).collect();
    assert_eq!(held_ids, vec!["r2", "r3", "r4"]);
}

#[test]
fn test_release_cases() {
    let mut policy = Policy::new(make_cfg());
    let mut ctx = make_context();

    // 1. Release with none
    assert!(policy.release(&ctx, 1_000_000_000).is_empty());

    // 2. Release with one
    ctx.away = true;
    policy.decide(
        make_candidate("r1", "k1", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        1_000_000_000,
    );
    assert_eq!(policy.held().len(), 1);

    // Return from away
    ctx.away = false;
    let released1 = policy.release(&ctx, 2_000_000_000);
    assert_eq!(released1.len(), 1);
    assert_eq!(released1[0].candidate.rule_id, "r1");
    assert_eq!(released1[0].action, Action::Speak);
    assert_eq!(released1[0].reason, Reason::Clear);
    assert!(released1[0].merged_from.is_empty());
    assert!(policy.held().is_empty());
    // Spoken counter updated
    assert_eq!(policy.spoken_in_last_hour(2_000_000_000), 1);

    // 3. Release with several (merges into ONE notification)
    ctx.away = true;
    policy.decide(
        make_candidate("disk_low", "/", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        3_000_000_000,
    );
    policy.decide(
        make_candidate("bat_crit", "", Priority::Urgent, Some(Delivery::Voice)),
        &ctx,
        4_000_000_000,
    );
    assert_eq!(policy.held().len(), 2);

    ctx.away = false;
    let merged_dec = policy.release(&ctx, 5_000_000_000);
    assert_eq!(merged_dec.len(), 1);
    let m = &merged_dec[0];
    assert_eq!(m.candidate.rule_id, "merged");
    assert_eq!(m.candidate.key, "");
    assert_eq!(m.candidate.template, "nudge.while_away");
    assert_eq!(m.candidate.args, vec![("count".to_string(), 2.0)]);
    assert_eq!(m.candidate.priority, Priority::Urgent); // highest
    assert_eq!(m.action, Action::Notify { silent: false });
    assert_eq!(m.reason, Reason::Away); // held for Away
    assert_eq!(
        m.merged_from,
        vec![
            ("disk_low".to_string(), "/".to_string()),
            ("bat_crit".to_string(), "".to_string())
        ]
    );
    assert!(policy.held().is_empty());

    // 4. Release with a still-blocked one staying
    // Put one candidate blocked by in_turn, and one blocked by media
    ctx.in_turn = true;
    policy.decide(
        make_candidate("r_turn", "k", Priority::Info, Some(Delivery::Voice)),
        &ctx,
        6_000_000_000,
    );
    ctx.in_turn = false;
    ctx.away = true;
    policy.decide(
        make_candidate("r_away", "k", Priority::Info, Some(Delivery::Voice)),
        &ctx,
        7_000_000_000,
    );
    assert_eq!(policy.held().len(), 2);

    // When away ends but in_turn is active: r_away is blocked by in_turn, but r_turn was already blocked by in_turn
    ctx.away = false;
    ctx.in_turn = true;
    let rel_blocked = policy.release(&ctx, 8_000_000_000);
    assert!(rel_blocked.is_empty());
    assert_eq!(policy.held().len(), 2);
}

#[test]
fn test_time_going_backwards() {
    let mut policy = Policy::new(make_cfg());
    let ctx = make_context();

    policy.decide(
        make_candidate("r1", "k", Priority::Info, Some(Delivery::Voice)),
        &ctx,
        10_000_000_000,
    );
    assert_eq!(policy.spoken_in_last_hour(10_000_000_000), 1);

    // Time goes backwards
    assert_eq!(policy.spoken_in_last_hour(5_000_000_000), 0);

    let d2 = policy.decide(
        make_candidate("r2", "k", Priority::Info, Some(Delivery::Voice)),
        &ctx,
        5_000_000_000,
    );
    assert_eq!(d2.action, Action::Speak);
}

#[test]
fn test_negative_control_kpi_10() {
    // Deliberately broken policy that speaks during media
    struct BrokenPolicy;
    impl BrokenPolicy {
        fn decide(ctx: &UserContext) -> Action {
            if ctx.media_playing {
                Action::Speak // BUG: speaks during media!
            } else {
                Action::Notify { silent: false }
            }
        }
    }

    let mut ctx = make_context();
    ctx.media_playing = true;

    // Broken policy violates KPI-10 by speaking during media
    assert_eq!(BrokenPolicy::decide(&ctx), Action::Speak);

    // Real policy satisfies KPI-10: never speaks during media
    let mut real_policy = Policy::new(make_cfg());
    let d = real_policy.decide(
        make_candidate("r1", "k", Priority::Important, Some(Delivery::Voice)),
        &ctx,
        1_000_000_000,
    );
    assert_ne!(
        d.action,
        Action::Speak,
        "real policy never speaks during media"
    );
    assert_eq!(d.action, Action::Notify { silent: false });
    assert_eq!(d.reason, Reason::Media);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn proptest_policy_invariants(
        steps in prop::collection::vec(
            (
                any::<bool>(), // paused
                any::<bool>(), // quiet_hours
                any::<bool>(), // in_turn
                any::<bool>(), // mic_busy
                any::<bool>(), // media_playing
                any::<bool>(), // fullscreen
                any::<bool>(), // away
                any::<bool>(), // voice_available
                0u8..3,        // priority: 0=Info, 1=Important, 2=Urgent
                1u64..10,      // dt_s
                any::<bool>(), // call release?
            ),
            1..40
        )
    ) {
        let max_spoken_per_hour = 3;
        let mut cfg = make_cfg();
        cfg.max_spoken_per_hour = max_spoken_per_hour;
        cfg.max_held = 10;
        let mut policy = Policy::new(cfg);

        let mut current_t = 1_000_000_000u64;
        let mut candidates_submitted = 0usize;
        let mut candidates_delivered = 0usize;

        for (paused, quiet_hours, in_turn, mic_busy, media_playing, fullscreen, away, voice_available, prio_idx, dt_s, do_release) in steps {
            current_t = current_t.saturating_add(dt_s.saturating_mul(1_000_000_000));
            let ctx = UserContext {
                paused,
                quiet_hours,
                in_turn,
                mic_busy,
                media_playing,
                fullscreen,
                away,
                voice_available,
            };

            let priority = match prio_idx {
                0 => Priority::Info,
                1 => Priority::Important,
                _ => Priority::Urgent,
            };

            let rule_id = format!("r_{prio_idx}");
            let c = make_candidate(&rule_id, "k", priority, Some(Delivery::Voice));
            candidates_submitted += 1;

            let d = policy.decide(c, &ctx, current_t);

            // KPI-10 verification:
            if d.action == Action::Speak {
                assert!(!ctx.paused, "KPI-10: never speak while paused");
                assert!(!ctx.quiet_hours, "KPI-10: never speak in quiet hours");
                assert!(!ctx.in_turn, "KPI-10: never speak in turn");
                assert!(!ctx.mic_busy, "KPI-10: never speak while mic busy");
                assert!(!ctx.away, "KPI-10: never speak while away");
                assert!(!ctx.media_playing, "KPI-10: never speak while media playing");
                assert!(!ctx.fullscreen, "KPI-10: never speak while fullscreen");
                assert!(ctx.voice_available, "KPI-10: never speak if voice unavailable");
                candidates_delivered += 1;
            } else if matches!(d.action, Action::Notify { .. }) {
                candidates_delivered += 1;
            }

            if do_release {
                let rel = policy.release(&ctx, current_t);
                for r in rel {
                    if r.action == Action::Speak {
                        assert!(!ctx.paused, "KPI-10: never speak while paused on release");
                        assert!(!ctx.quiet_hours, "KPI-10: never speak in quiet hours on release");
                        assert!(!ctx.in_turn, "KPI-10: never speak in turn on release");
                        assert!(!ctx.mic_busy, "KPI-10: never speak while mic busy on release");
                        assert!(!ctx.away, "KPI-10: never speak while away on release");
                        assert!(!ctx.media_playing, "KPI-10: never speak while media playing on release");
                        assert!(!ctx.fullscreen, "KPI-10: never speak while fullscreen on release");
                        assert!(ctx.voice_available, "KPI-10: never speak if voice unavailable on release");
                    }
                    if r.candidate.rule_id == "merged" {
                        // Count merged entries
                        let count = r.candidate.args[0].1 as usize;
                        candidates_delivered += count;
                    } else {
                        candidates_delivered += 1;
                    }
                }
            }

            // Rate limit property: spoken in rolling hour <= max_spoken_per_hour + urgent exceptions count
            let spoken_h = policy.spoken_in_last_hour(current_t);
            assert!(spoken_h <= max_spoken_per_hour + 3, "spoken rate limit bounded");
        }

        // Accounting invariant:
        // Every candidate submitted is either delivered, still held, or dropped (overflow / replaced)
        let held_count = policy.held().len();
        let dropped_count = policy.take_dropped().len();
        assert_eq!(
            candidates_delivered + held_count + dropped_count,
            candidates_submitted,
            "every candidate is accounted for: delivered ({candidates_delivered}) + held ({held_count}) + dropped ({dropped_count}) == submitted ({candidates_submitted})"
        );
    }
}
