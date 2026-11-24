#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;

use octoplus_knowledge::nudge::eval::{EvalConfig, Evaluator};
use octoplus_knowledge::nudge::rules::defaults;
use octoplus_knowledge::nudge::types::{
    ArgSource, Candidate, Direction, Priority, Reading, Rule, Signal,
};

#[allow(clippy::too_many_arguments)]
fn make_rule(
    id: &str,
    signal: Signal,
    direction: Direction,
    fire_at: f64,
    rearm: f64,
    hold_for_s: u64,
    cooldown_s: u64,
    priority: Priority,
) -> Rule {
    Rule {
        id: id.to_string(),
        signal,
        direction,
        fire_at,
        rearm,
        hold_for_s,
        cooldown_s,
        priority,
        deliver: None,
        template: "test.template".to_string(),
        args: vec![("val".to_string(), ArgSource::Value)],
        offer_skill: None,
        enabled: true,
    }
}

#[test]
fn test_defaults_evaluator() {
    let rules = defaults().expect("defaults load");
    let cfg = EvalConfig { max_gap_s: 30 };
    let mut eval = Evaluator::new(rules, cfg);
    assert_eq!(eval.rules().len(), 8);

    // Initial tick with normal readings
    let candidates = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 50.0,
            extra: 0.0,
        }],
    );
    assert!(candidates.is_empty());

    let status = eval.status();
    assert_eq!(status.len(), 8);
    for s in status {
        assert!(s.enabled);
        assert_eq!(s.snoozed_until_ns, 0);
        assert_eq!(s.last_fired_ns, 0);
    }
}

#[test]
fn test_exactly_at_fire_at_fires() {
    let rule = make_rule(
        "r1",
        Signal::BatteryPercent,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Exactly 90.0 at_least should fire
    let cands = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 90.0,
            extra: 0.0,
        }],
    );
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].rule_id, "r1");
    assert_eq!(cands[0].at_ns, 1_000_000_000);
}

#[test]
fn test_hold_for_boundary() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        5,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Tick 1: t = 1.0s, holds (timer starts at 1.0s)
    let c1 = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 92.0,
            extra: 0.0,
        }],
    );
    assert!(c1.is_empty());

    // Tick 2: t = 5.999999999s (1 ns before 5s elapsed) -> does not fire
    let c2 = eval.tick(
        5_999_999_999,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 92.0,
            extra: 0.0,
        }],
    );
    assert!(c2.is_empty());

    // Tick 3: t = 6.000000000s (exactly 5s elapsed) -> FIRES
    let c3 = eval.tick(
        6_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 92.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c3.len(), 1);
    assert_eq!(c3[0].rule_id, "r1");
}

#[test]
fn test_cooldown_boundary() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 100 });

    // Tick 1: fires at t = 10s
    let c1 = eval.tick(
        10_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c1.len(), 1);

    // Tick 2: t = 20s, drops below rearm (75 < 80) -> re-arms
    let c2 = eval.tick(
        20_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 75.0,
            extra: 0.0,
        }],
    );
    assert!(c2.is_empty());

    // Tick 3: t = 69.999999999s (1 ns short of 60s cooldown from 10s) -> holds, but cooldown blocks
    let c3 = eval.tick(
        69_999_999_999,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c3.is_empty());

    // Tick 4: t = 70.0s (exactly 60s cooldown elapsed) -> FIRES
    let c4 = eval.tick(
        70_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c4.len(), 1);
}

#[test]
fn test_rearm_strictly_past() {
    let rule = make_rule(
        "r1",
        Signal::DiskUsedPercent,
        Direction::AtLeast,
        90.0,
        85.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 200 });

    // Tick 1: fires at 90.0
    eval.tick(
        10_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 90.0,
            extra: 0.0,
        }],
    );

    // Tick 2: value exactly at rearm 85.0 -> does NOT rearm
    eval.tick(
        80_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 85.0,
            extra: 0.0,
        }],
    );

    // Tick 3: value rises back to 91.0 -> should NOT fire (still unarmed)
    let c3 = eval.tick(
        90_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 91.0,
            extra: 0.0,
        }],
    );
    assert!(c3.is_empty());

    // Tick 4: strictly past rearm (84.99) -> REARMS
    eval.tick(
        100_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 84.99,
            extra: 0.0,
        }],
    );

    // Tick 5: reaches 92.0 -> FIRES
    let c5 = eval.tick(
        165_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 92.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c5.len(), 1);
}

#[test]
fn test_flapping_around_fire_at_never_fires_twice_in_episode() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        1,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Tick 1: 90.0 -> fires
    assert_eq!(
        eval.tick(
            1_000_000_000,
            &[Reading {
                signal: Signal::TemperatureC,
                key: "".to_string(),
                value: 90.0,
                extra: 0.0,
            }]
        )
        .len(),
        1
    );

    // Flapping above rearm (80.0): 88.0, 92.0, 85.0, 95.0
    for (i, val) in [88.0, 92.0, 85.0, 95.0].iter().enumerate() {
        let t = (2 + i as u64) * 1_000_000_000;
        let c = eval.tick(
            t,
            &[Reading {
                signal: Signal::TemperatureC,
                key: "".to_string(),
                value: *val,
                extra: 0.0,
            }],
        );
        assert!(
            c.is_empty(),
            "flapping must not fire again without re-arming"
        );
    }
}

#[test]
fn test_absent_reading_rearms() {
    let rule = make_rule(
        "r1",
        Signal::BatteryPercent,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        10,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Tick 1: fires
    assert_eq!(
        eval.tick(
            1_000_000_000,
            &[Reading {
                signal: Signal::BatteryPercent,
                key: "".to_string(),
                value: 90.0,
                extra: 0.0,
            }]
        )
        .len(),
        1
    );

    // Tick 2: reading is absent (no BatteryPercent reading) -> hold timer clears, re-arms!
    assert!(eval.tick(2_000_000_000, &[]).is_empty());

    // Tick 3: after cooldown (10s), reading holds again -> FIRES!
    let c3 = eval.tick(
        15_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 90.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c3.len(), 1);
}

#[test]
fn test_at_most_direction_mirrors_at_least() {
    let rule = make_rule(
        "r1",
        Signal::BatteryPercent,
        Direction::AtMost,
        15.0,
        20.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 100 });

    // Exactly at fire_at (15.0) fires
    let c1 = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 15.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c1.len(), 1);

    // Exactly at rearm (20.0) does NOT rearm
    eval.tick(
        10_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 20.0,
            extra: 0.0,
        }],
    );

    // Dropping back to 14.0 does not fire (still unarmed)
    let c3 = eval.tick(
        70_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 14.0,
            extra: 0.0,
        }],
    );
    assert!(c3.is_empty());

    // Strictly past rearm (20.1) rearms!
    eval.tick(
        80_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 20.1,
            extra: 0.0,
        }],
    );

    // Now drops to 14.0 -> FIRES!
    let c5 = eval.tick(
        150_000_000_000,
        &[Reading {
            signal: Signal::BatteryPercent,
            key: "".to_string(),
            value: 14.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c5.len(), 1);
}

#[test]
fn test_disabled_and_snoozed_rules() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        10,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 50 });

    // Disable rule
    assert!(eval.set_enabled("r1", false));
    let c1 = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c1.is_empty());

    // Re-enable rule while it still holds -> fires on first tick allowed
    assert!(eval.set_enabled("r1", true));
    let c2 = eval.tick(
        2_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c2.len(), 1);

    // Rearm
    eval.tick(
        3_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 70.0,
            extra: 0.0,
        }],
    );

    // Snooze until t = 30s
    assert!(eval.snooze("r1", 30_000_000_000));

    // At t = 20s (cooldown elapsed, but snoozed) -> holds, does not fire
    let c4 = eval.tick(
        20_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c4.is_empty());

    // At t = 30s (snooze expired) while still holding -> FIRES
    let c5 = eval.tick(
        30_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c5.len(), 1);
}

#[test]
fn test_restore_blocks_fire_inside_cooldown() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 100 });

    // Restore rule as having fired at t = 10s
    assert!(eval.restore("r1", 10_000_000_000, 0, None));

    // Tick at t = 30s (20s into 60s cooldown) -> blocked by restored cooldown
    let c1 = eval.tick(
        30_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c1.is_empty());

    // Tick at t = 70s (60s elapsed from restored 10s) -> FIRES
    let c2 = eval.tick(
        70_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c2.len(), 1);
}

#[test]
fn test_unknown_ids_return_false() {
    let mut eval = Evaluator::new(Vec::new(), EvalConfig { max_gap_s: 10 });
    assert!(!eval.set_enabled("unknown", true));
    assert!(!eval.snooze("unknown", 100));
    assert!(!eval.restore("unknown", 100, 100, None));
}

#[test]
fn test_two_mount_points_are_independent() {
    let rule = make_rule(
        "disk_rule",
        Signal::DiskUsedPercent,
        Direction::AtLeast,
        90.0,
        85.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Tick: "/" is 92%, "/home" is 50%
    let c1 = eval.tick(
        1_000_000_000,
        &[
            Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 92.0,
                extra: 10.0,
            },
            Reading {
                signal: Signal::DiskUsedPercent,
                key: "/home".to_string(),
                value: 50.0,
                extra: 50.0,
            },
        ],
    );
    assert_eq!(c1.len(), 1);
    assert_eq!(c1[0].key, "/");

    // Next tick: "/home" rises to 95% -> "/home" fires independently
    let c2 = eval.tick(
        2_000_000_000,
        &[
            Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 92.0,
                extra: 10.0,
            },
            Reading {
                signal: Signal::DiskUsedPercent,
                key: "/home".to_string(),
                value: 95.0,
                extra: 5.0,
            },
        ],
    );
    assert_eq!(c2.len(), 1);
    assert_eq!(c2[0].key, "/home");
}

#[test]
fn test_args_resolution_value_vs_extra() {
    let mut rule = make_rule(
        "r1",
        Signal::DiskUsedPercent,
        Direction::AtLeast,
        90.0,
        85.0,
        0,
        60,
        Priority::Important,
    );
    rule.args = vec![
        ("used".to_string(), ArgSource::Value),
        ("free".to_string(), ArgSource::Extra),
    ];
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    let c = eval.tick(
        1_000_000_000,
        &[Reading {
            signal: Signal::DiskUsedPercent,
            key: "/".to_string(),
            value: 92.5,
            extra: 7.5,
        }],
    );
    assert_eq!(c.len(), 1);
    assert_eq!(
        c[0].args,
        vec![("used".to_string(), 92.5), ("free".to_string(), 7.5)]
    );
}

#[test]
fn test_priority_ordering() {
    let r_info = make_rule(
        "r_info",
        Signal::MemoryAvailablePercent,
        Direction::AtMost,
        10.0,
        15.0,
        0,
        0,
        Priority::Info,
    );
    let r_urgent = make_rule(
        "r_urgent",
        Signal::DiskUsedPercent,
        Direction::AtLeast,
        95.0,
        92.0,
        0,
        0,
        Priority::Urgent,
    );
    let r_important = make_rule(
        "r_important",
        Signal::BatteryPercent,
        Direction::AtMost,
        15.0,
        20.0,
        0,
        0,
        Priority::Important,
    );
    let mut eval = Evaluator::new(
        vec![r_info, r_urgent, r_important],
        EvalConfig { max_gap_s: 30 },
    );

    let c = eval.tick(
        1_000_000_000,
        &[
            Reading {
                signal: Signal::MemoryAvailablePercent,
                key: "".to_string(),
                value: 5.0,
                extra: 0.0,
            },
            Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 96.0,
                extra: 2.0,
            },
            Reading {
                signal: Signal::BatteryPercent,
                key: "".to_string(),
                value: 10.0,
                extra: 0.0,
            },
        ],
    );
    assert_eq!(c.len(), 3);
    assert_eq!(c[0].priority, Priority::Urgent);
    assert_eq!(c[1].priority, Priority::Important);
    assert_eq!(c[2].priority, Priority::Info);
}

#[test]
fn test_non_finite_readings_ignored() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    for val in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let c = eval.tick(
            1_000_000_000,
            &[Reading {
                signal: Signal::TemperatureC,
                key: "".to_string(),
                value: val,
                extra: 0.0,
            }],
        );
        assert!(c.is_empty(), "non-finite value {val} must be ignored");
    }
}

#[test]
fn test_disk_projection() {
    let rule = make_rule(
        "disk_filling",
        Signal::DiskFillHours,
        Direction::AtMost,
        19.7,
        36.0,
        0,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 120 });

    // 1. Steady 1 GiB/h fall from 20 GiB free over 20 minutes
    // Feed one reading per minute for 20 minutes (21 points)
    // Rate = 1 GiB/h. At minute 20, free GiB = 20 - (20/60) = 19.667 GiB.
    // Hours until full = 19.667 / 1.0 = ~19.67 hours (about 20 hours).
    let mut cands = Vec::new();
    let mut last_t = 0;
    for min in 0..=20 {
        let t_ns = min * 60 * 1_000_000_000;
        last_t = t_ns;
        let free_gib = 20.0 - (min as f64) * (1.0 / 60.0);
        let c = eval.tick(
            t_ns,
            &[Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 80.0,
                extra: free_gib,
            }],
        );
        if !c.is_empty() {
            cands = c;
        }
    }
    // At 20 minutes, span = 1200s >= 600s, entries = 21 >= 5.
    // Derived reading should have value ≈ 19.67 <= 19.7 fire_at -> fires!
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].rule_id, "disk_filling");
    let hours = cands[0].args[0].1;
    assert!((19.0..=20.5).contains(&hours), "expected ~20h, got {hours}");

    // 2. Under 10 minutes of history or flat/rising series gives no reading
    let mut eval_short = Evaluator::new(
        vec![make_rule(
            "r",
            Signal::DiskFillHours,
            Direction::AtMost,
            50.0,
            60.0,
            0,
            0,
            Priority::Info,
        )],
        EvalConfig { max_gap_s: 120 },
    );
    // 6 points over 5 minutes (300s < 600s)
    for min in 0..=5 {
        let t_ns = min * 60 * 1_000_000_000;
        let c = eval_short.tick(
            t_ns,
            &[Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 50.0,
                extra: 20.0 - min as f64 * 0.1,
            }],
        );
        assert!(c.is_empty(), "under 10 minutes of history must not fire");
    }

    // Flat series over 20 minutes
    let mut eval_flat = Evaluator::new(
        vec![make_rule(
            "r",
            Signal::DiskFillHours,
            Direction::AtMost,
            50.0,
            60.0,
            0,
            0,
            Priority::Info,
        )],
        EvalConfig { max_gap_s: 120 },
    );
    for min in 0..=20 {
        let t_ns = min * 60 * 1_000_000_000;
        let c = eval_flat.tick(
            t_ns,
            &[Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 50.0,
                extra: 20.0,
            }],
        );
        assert!(c.is_empty(), "flat series must not fire");
    }

    // 3. Series that falls then recovers
    // Continue from last_t on eval: free space recovers back to 25.0 over 20 minutes
    for min in 1..=20 {
        let t_ns = last_t + min * 60 * 1_000_000_000;
        let c = eval.tick(
            t_ns,
            &[Reading {
                signal: Signal::DiskUsedPercent,
                key: "/".to_string(),
                value: 70.0,
                extra: 19.667 + min as f64 * 0.3,
            }],
        );
        assert!(c.is_empty(), "recovering disk space must not fire");
    }
}

#[test]
fn test_gap_and_clock_jump() {
    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        5,
        60,
        Priority::Important,
    );
    let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });

    // Tick 1: starts hold timer at t = 10s
    eval.tick(
        10_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );

    // Clock jump backwards: t = 5s (< 10s) -> clears hold timer, fires nothing
    let c_back = eval.tick(
        5_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c_back.is_empty());

    // Next normal tick after backward jump: hold timer restarts at t = 6s
    eval.tick(
        6_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );

    // Forward gap: t = 60s (gap is 54s > max_gap_s 30s) -> clears hold timer, fires nothing
    let c_gap = eval.tick(
        60_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(c_gap.is_empty());

    // Next normal tick after gap: hold timer starts at t = 61s
    eval.tick(
        61_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );

    // 5s later at t = 66s -> FIRES
    let c_fire = eval.tick(
        66_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert_eq!(c_fire.len(), 1);
}

#[test]
fn test_negative_control_broken_evaluator_ignores_cooldown() {
    // A broken evaluator that ignores cooldown
    struct BrokenEvaluator {
        rule: Rule,
        armed: bool,
    }
    impl BrokenEvaluator {
        fn new(rule: Rule) -> Self {
            Self { rule, armed: true }
        }
        fn tick(&mut self, now_ns: u64, reading: &Reading) -> Vec<Candidate> {
            if reading.value >= self.rule.fire_at && self.armed {
                // BUG: does not check cooldown, fires repeatedly every tick!
                vec![Candidate {
                    rule_id: self.rule.id.clone(),
                    key: reading.key.clone(),
                    priority: self.rule.priority,
                    deliver: self.rule.deliver,
                    template: self.rule.template.clone(),
                    args: vec![],
                    offer_skill: None,
                    at_ns: now_ns,
                }]
            } else {
                vec![]
            }
        }
    }

    let rule = make_rule(
        "r1",
        Signal::TemperatureC,
        Direction::AtLeast,
        90.0,
        80.0,
        0,
        60,
        Priority::Important,
    );
    let mut broken = BrokenEvaluator::new(rule.clone());

    // First fire
    let c1 = broken.tick(
        10_000_000_000,
        &Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        },
    );
    assert_eq!(c1.len(), 1);

    // Second fire 1 second later (inside 60s cooldown!)
    let c2 = broken.tick(
        11_000_000_000,
        &Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        },
    );
    // The broken evaluator fired inside cooldown!
    assert_eq!(c2.len(), 1);

    // In contrast, the real evaluator respects cooldown
    let mut real = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 30 });
    real.tick(
        10_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    let real_c2 = real.tick(
        11_000_000_000,
        &[Reading {
            signal: Signal::TemperatureC,
            key: "".to_string(),
            value: 95.0,
            extra: 0.0,
        }],
    );
    assert!(real_c2.is_empty(), "real evaluator respects cooldown");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn proptest_evaluator_invariants(
        steps in prop::collection::vec(
            (
                1u64..10, // dt_s
                prop::option::of(50.0f64..120.0f64), // value or absent
                prop::option::of(any::<bool>()), // toggle enabled?
                prop::option::of(0u64..30), // snooze seconds from now?
            ),
            1..60
        )
    ) {
        let cooldown_s = 60;
        let rule = make_rule(
            "r1",
            Signal::TemperatureC,
            Direction::AtLeast,
            90.0,
            80.0,
            2,
            cooldown_s,
            Priority::Important,
        );
        let mut eval = Evaluator::new(vec![rule], EvalConfig { max_gap_s: 300 });

        let mut current_t = 1_000_000_000u64;
        let mut last_fire_t = None;
        let mut armed_state = true;
        let mut enabled_state = true;
        let mut snoozed_until_state = 0u64;

        for (dt_s, val_opt, toggle_en, snooze_s_opt) in steps {
            current_t = current_t.saturating_add(dt_s.saturating_mul(1_000_000_000));

            if let Some(on) = toggle_en {
                eval.set_enabled("r1", on);
                enabled_state = on;
                if on {
                    // Switching a rule on ends an earlier snooze (review F3).
                    snoozed_until_state = 0;
                }
            }

            if let Some(snooze_s) = snooze_s_opt {
                let until = current_t.saturating_add(snooze_s.saturating_mul(1_000_000_000));
                eval.snooze("r1", until);
                snoozed_until_state = until;
            }

            let readings: Vec<Reading> = match val_opt {
                Some(v) => vec![Reading {
                    signal: Signal::TemperatureC,
                    key: "".to_string(),
                    value: v,
                    extra: 0.0,
                }],
                None => vec![],
            };

            let cands = eval.tick(current_t, &readings);

            // Update model state tracker
            match val_opt {
                Some(v) => {
                    if v < 80.0 {
                        armed_state = true;
                    }
                }
                None => {
                    // Absent reading re-arms
                    armed_state = true;
                }
            }

            for cand in cands {
                assert_eq!(cand.rule_id, "r1");
                assert!(enabled_state, "never fires while disabled");
                assert!(current_t >= snoozed_until_state, "never fires while snoozed");
                if let Some(prev) = last_fire_t {
                    assert!(
                        current_t.saturating_sub(prev) >= cooldown_s.saturating_mul(1_000_000_000),
                        "never fires an instance twice within its cooldown"
                    );
                    assert!(
                        armed_state,
                        "never fires twice without passing rearm or being absent in between"
                    );
                }
                last_fire_t = Some(current_t);
                armed_state = false;
            }
        }
    }
}
