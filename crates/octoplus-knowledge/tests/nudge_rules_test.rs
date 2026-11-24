#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;

use octoplus_knowledge::nudge::rules::{
    MAX_FILE_BYTES, MAX_RULES, OFFERABLE_SKILLS, RuleError, TEMPLATES, defaults, load,
    parse_duration,
};
use octoplus_knowledge::nudge::types::{ArgSource, Delivery, Direction, Priority, Rule, Signal};

#[test]
fn test_default_rules_field_by_field() {
    let rules = defaults().expect("defaults must load");
    assert_eq!(rules.len(), 8);

    // Also assert load(None) produces identical rules
    let rules_none = load(None).expect("load(None) must succeed");
    assert_eq!(rules, rules_none);

    // 1. disk_nearly_full: disk.used_percent at_least 90, rearm 85, hold_for 2m, cooldown 24h,
    //    important, template disk.nearly_full, offer system.disk_report
    let r0 = &rules[0];
    assert_eq!(r0.id, "disk_nearly_full");
    assert_eq!(r0.signal, Signal::DiskUsedPercent);
    assert_eq!(r0.direction, Direction::AtLeast);
    assert_eq!(r0.fire_at, 90.0);
    assert_eq!(r0.rearm, 85.0);
    assert_eq!(r0.hold_for_s, 120);
    assert_eq!(r0.cooldown_s, 86400);
    assert_eq!(r0.priority, Priority::Important);
    assert_eq!(r0.deliver, None);
    assert_eq!(r0.template, "disk.nearly_full");
    assert_eq!(r0.args, vec![("percent".to_string(), ArgSource::Value)]);
    assert_eq!(r0.offer_skill.as_deref(), Some("system.disk_report"));
    assert!(r0.enabled);

    // 2. disk_critical: disk.used_percent at_least 95, rearm 92, hold_for 1m, cooldown 24h,
    //    urgent, template disk.critical (free_gib = extra), offer system.disk_report
    let r1 = &rules[1];
    assert_eq!(r1.id, "disk_critical");
    assert_eq!(r1.signal, Signal::DiskUsedPercent);
    assert_eq!(r1.direction, Direction::AtLeast);
    assert_eq!(r1.fire_at, 95.0);
    assert_eq!(r1.rearm, 92.0);
    assert_eq!(r1.hold_for_s, 60);
    assert_eq!(r1.cooldown_s, 86400);
    assert_eq!(r1.priority, Priority::Urgent);
    assert_eq!(r1.deliver, None);
    assert_eq!(r1.template, "disk.critical");
    assert_eq!(r1.args, vec![("free_gib".to_string(), ArgSource::Extra)]);
    assert_eq!(r1.offer_skill.as_deref(), Some("system.disk_report"));
    assert!(r1.enabled);

    // 3. disk_filling_fast: disk.fill_hours at_most 24, rearm 36, hold_for 5m, cooldown 24h,
    //    important, template disk.filling_fast (hours = value), offer system.disk_report
    let r2 = &rules[2];
    assert_eq!(r2.id, "disk_filling_fast");
    assert_eq!(r2.signal, Signal::DiskFillHours);
    assert_eq!(r2.direction, Direction::AtMost);
    assert_eq!(r2.fire_at, 24.0);
    assert_eq!(r2.rearm, 36.0);
    assert_eq!(r2.hold_for_s, 300);
    assert_eq!(r2.cooldown_s, 86400);
    assert_eq!(r2.priority, Priority::Important);
    assert_eq!(r2.deliver, None);
    assert_eq!(r2.template, "disk.filling_fast");
    assert_eq!(r2.args, vec![("hours".to_string(), ArgSource::Value)]);
    assert_eq!(r2.offer_skill.as_deref(), Some("system.disk_report"));
    assert!(r2.enabled);

    // 4. battery_low: battery.percent at_most 15, rearm 20, hold_for 0s, cooldown 2h,
    //    important, template battery.low
    let r3 = &rules[3];
    assert_eq!(r3.id, "battery_low");
    assert_eq!(r3.signal, Signal::BatteryPercent);
    assert_eq!(r3.direction, Direction::AtMost);
    assert_eq!(r3.fire_at, 15.0);
    assert_eq!(r3.rearm, 20.0);
    assert_eq!(r3.hold_for_s, 0);
    assert_eq!(r3.cooldown_s, 7200);
    assert_eq!(r3.priority, Priority::Important);
    assert_eq!(r3.deliver, None);
    assert_eq!(r3.template, "battery.low");
    assert_eq!(r3.args, vec![("percent".to_string(), ArgSource::Value)]);
    assert_eq!(r3.offer_skill, None);
    assert!(r3.enabled);

    // 5. running_hot: temperature.c at_least 90, rearm 80, hold_for 2m, cooldown 24h,
    //    important, template thermal.hot, offer system.read_metadata
    let r4 = &rules[4];
    assert_eq!(r4.id, "running_hot");
    assert_eq!(r4.signal, Signal::TemperatureC);
    assert_eq!(r4.direction, Direction::AtLeast);
    assert_eq!(r4.fire_at, 90.0);
    assert_eq!(r4.rearm, 80.0);
    assert_eq!(r4.hold_for_s, 120);
    assert_eq!(r4.cooldown_s, 86400);
    assert_eq!(r4.priority, Priority::Important);
    assert_eq!(r4.deliver, None);
    assert_eq!(r4.template, "thermal.hot");
    assert!(r4.args.is_empty());
    assert_eq!(r4.offer_skill.as_deref(), Some("system.read_metadata"));
    assert!(r4.enabled);

    // 6. memory_pressure: memory.available_percent at_most 10, rearm 15, hold_for 2m, cooldown 24h,
    //    info, template memory.pressure, offer system.read_metadata
    let r5 = &rules[5];
    assert_eq!(r5.id, "memory_pressure");
    assert_eq!(r5.signal, Signal::MemoryAvailablePercent);
    assert_eq!(r5.direction, Direction::AtMost);
    assert_eq!(r5.fire_at, 10.0);
    assert_eq!(r5.rearm, 15.0);
    assert_eq!(r5.hold_for_s, 120);
    assert_eq!(r5.cooldown_s, 86400);
    assert_eq!(r5.priority, Priority::Info);
    assert_eq!(r5.deliver, None);
    assert_eq!(r5.template, "memory.pressure");
    assert!(r5.args.is_empty());
    assert_eq!(r5.offer_skill.as_deref(), Some("system.read_metadata"));
    assert!(r5.enabled);

    // 7. long_session: session.active_minutes at_least 90, rearm 5, hold_for 0s, cooldown 3h,
    //    info, deliver notify, template session.long (minutes = value)
    let r6 = &rules[6];
    assert_eq!(r6.id, "long_session");
    assert_eq!(r6.signal, Signal::ActiveMinutes);
    assert_eq!(r6.direction, Direction::AtLeast);
    assert_eq!(r6.fire_at, 90.0);
    assert_eq!(r6.rearm, 5.0);
    assert_eq!(r6.hold_for_s, 0);
    assert_eq!(r6.cooldown_s, 10800);
    assert_eq!(r6.priority, Priority::Info);
    assert_eq!(r6.deliver, Some(Delivery::Notify));
    assert_eq!(r6.template, "session.long");
    assert_eq!(r6.args, vec![("minutes".to_string(), ArgSource::Value)]);
    assert_eq!(r6.offer_skill, None);
    assert!(r6.enabled);

    // 8. part_degraded: parts.degraded at_least 1, rearm 0.5, hold_for 2m, cooldown 24h,
    //    info, template part.degraded
    let r7 = &rules[7];
    assert_eq!(r7.id, "part_degraded");
    assert_eq!(r7.signal, Signal::PartsDegraded);
    assert_eq!(r7.direction, Direction::AtLeast);
    assert_eq!(r7.fire_at, 1.0);
    assert_eq!(r7.rearm, 0.5);
    assert_eq!(r7.hold_for_s, 120);
    assert_eq!(r7.cooldown_s, 86400);
    assert_eq!(r7.priority, Priority::Info);
    assert_eq!(r7.deliver, None);
    assert_eq!(r7.template, "part.degraded");
    assert!(r7.args.is_empty());
    assert_eq!(r7.offer_skill, None);
    assert!(r7.enabled);
}

#[test]
fn test_parse_duration() {
    let valid = [
        ("0s", 0),
        ("90s", 90),
        ("2m", 120),
        ("5m", 300),
        ("1h", 3600),
        ("24h", 86400),
        ("7d", 604800),
        ("30d", 2592000),
    ];
    for (input, expected) in valid {
        assert_eq!(
            parse_duration(input),
            Some(expected),
            "parse_duration({input:?}) failed"
        );
    }

    let rejects = [
        "", "5", "5x", " 5m", "-5m", "1.5h", "+5m", "5m ", "s", "m", "h", "d", "10 s", "five_m",
        "1.0s", "-1s",
    ];
    for input in rejects {
        assert_eq!(
            parse_duration(input),
            None,
            "expected parse_duration({input:?}) to be None"
        );
    }

    // Huge duration must not panic; it saturates or returns None
    let huge = parse_duration("99999999999999999999h");
    assert!(huge == Some(u64::MAX) || huge.is_none());
}

#[test]
fn test_merge_override_one_threshold_keeps_rest() {
    let user_toml = r#"[rule.disk_nearly_full]
fire_at = 88
"#;
    let rules = load(Some(user_toml)).expect("merge override should succeed");
    assert_eq!(rules.len(), 8);

    let dnf = rules
        .iter()
        .find(|r| r.id == "disk_nearly_full")
        .expect("rule found");
    assert_eq!(dnf.fire_at, 88.0);
    assert_eq!(dnf.signal, Signal::DiskUsedPercent);
    assert_eq!(dnf.direction, Direction::AtLeast);
    assert_eq!(dnf.rearm, 85.0);
    assert_eq!(dnf.hold_for_s, 120);
    assert_eq!(dnf.cooldown_s, 86400);
    assert_eq!(dnf.priority, Priority::Important);
    assert_eq!(dnf.deliver, None);
    assert_eq!(dnf.template, "disk.nearly_full");
    assert_eq!(dnf.args, vec![("percent".to_string(), ArgSource::Value)]);
    assert_eq!(dnf.offer_skill.as_deref(), Some("system.disk_report"));
    assert!(dnf.enabled);
}

#[test]
fn test_merge_add_new_complete_rule() {
    let user_toml = r#"[rule.custom_rule]
signal = "session.active_minutes"
direction = "at_least"
fire_at = 45
rearm = 10
hold_for = "5m"
cooldown = "1h"
priority = "info"
deliver = "voice"
template = "session.long"
offer_skill = "system.read_metadata"
enabled = true
"#;
    let rules = load(Some(user_toml)).expect("add new rule should succeed");
    assert_eq!(rules.len(), 9);

    // Verify defaults are first 8 in default order
    assert_eq!(rules[0].id, "disk_nearly_full");
    assert_eq!(rules[7].id, "part_degraded");

    // 9th is the new rule
    let cr = &rules[8];
    assert_eq!(cr.id, "custom_rule");
    assert_eq!(cr.signal, Signal::ActiveMinutes);
    assert_eq!(cr.direction, Direction::AtLeast);
    assert_eq!(cr.fire_at, 45.0);
    assert_eq!(cr.rearm, 10.0);
    assert_eq!(cr.hold_for_s, 300);
    assert_eq!(cr.cooldown_s, 3600);
    assert_eq!(cr.priority, Priority::Info);
    assert_eq!(cr.deliver, Some(Delivery::Voice));
    assert_eq!(cr.template, "session.long");
    assert_eq!(cr.args, vec![("minutes".to_string(), ArgSource::Value)]);
    assert_eq!(cr.offer_skill.as_deref(), Some("system.read_metadata"));
    assert!(cr.enabled);
}

#[test]
fn test_merge_incomplete_new_rule_is_missing() {
    // Missing signal
    let toml = r#"[rule.incomplete]
fire_at = 50
rearm = 40
priority = "info"
template = "battery.low"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Missing {
            rule: "incomplete".to_string(),
            field: "signal",
        })
    );

    // Missing fire_at
    let toml = r#"[rule.incomplete]
signal = "battery.percent"
rearm = 40
priority = "info"
template = "battery.low"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Missing {
            rule: "incomplete".to_string(),
            field: "fire_at",
        })
    );

    // Missing rearm
    let toml = r#"[rule.incomplete]
signal = "battery.percent"
fire_at = 50
priority = "info"
template = "battery.low"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Missing {
            rule: "incomplete".to_string(),
            field: "rearm",
        })
    );

    // Missing priority
    let toml = r#"[rule.incomplete]
signal = "battery.percent"
fire_at = 50
rearm = 40
template = "battery.low"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Missing {
            rule: "incomplete".to_string(),
            field: "priority",
        })
    );

    // Missing template
    let toml = r#"[rule.incomplete]
signal = "battery.percent"
fire_at = 50
rearm = 40
priority = "info"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Missing {
            rule: "incomplete".to_string(),
            field: "template",
        })
    );
}

#[test]
fn test_merge_disable_default_with_enabled_false() {
    let toml = r#"[rule.battery_low]
enabled = false
"#;
    let rules = load(Some(toml)).expect("disable default should succeed");
    assert_eq!(rules.len(), 8);
    let bat = rules.iter().find(|r| r.id == "battery_low").unwrap();
    assert!(!bat.enabled);
}

#[test]
fn test_unknown_key_at_each_of_the_four_levels() {
    // Level 1: top level
    let toml1 = "unknown_top = 123\n";
    assert_eq!(
        load(Some(toml1)),
        Err(RuleError::UnknownKey {
            rule: "".to_string(),
            key: "unknown_top".to_string(),
        })
    );

    // Level 2: under rule
    let toml2 = "[rule]\nunknown_under_rule = 456\n";
    assert_eq!(
        load(Some(toml2)),
        Err(RuleError::UnknownKey {
            rule: "rule".to_string(),
            key: "unknown_under_rule".to_string(),
        })
    );

    // Level 3: inside a rule
    let toml3 = "[rule.battery_low]\nunknown_in_rule = 789\n";
    assert_eq!(
        load(Some(toml3)),
        Err(RuleError::UnknownKey {
            rule: "battery_low".to_string(),
            key: "unknown_in_rule".to_string(),
        })
    );

    // Level 4: inside args
    let toml4 = "[rule.battery_low.args]\nunknown_in_args = \"value\"\n";
    assert_eq!(
        load(Some(toml4)),
        Err(RuleError::UnknownKey {
            rule: "battery_low".to_string(),
            key: "unknown_in_args".to_string(),
        })
    );
}

#[test]
fn test_bad_values_each_validation_row() {
    // 1. fire_at non-finite
    let toml_nan_fire = "[rule.battery_low]\nfire_at = nan\n";
    assert_eq!(
        load(Some(toml_nan_fire)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "fire_at",
            reason: "must be finite",
        })
    );

    // 2. rearm non-finite
    let toml_inf_rearm = "[rule.battery_low]\nrearm = inf\n";
    assert_eq!(
        load(Some(toml_inf_rearm)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "rearm",
            reason: "must be finite",
        })
    );

    // 3. direction at_least requires rearm <= fire_at
    let toml_at_least = "[rule.running_hot]\nfire_at = 90\nrearm = 95\n";
    assert_eq!(
        load(Some(toml_at_least)),
        Err(RuleError::Invalid {
            rule: "running_hot".to_string(),
            field: "rearm",
            reason: "at_least requires rearm <= fire_at",
        })
    );

    // 4. direction at_most requires rearm >= fire_at
    let toml_at_most = "[rule.battery_low]\nfire_at = 20\nrearm = 15\n";
    assert_eq!(
        load(Some(toml_at_most)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "rearm",
            reason: "at_most requires rearm >= fire_at",
        })
    );

    // 5. percent signal fire_at out of 0..=100
    let toml_pct_fire = "[rule.disk_nearly_full]\nfire_at = 105\n";
    assert_eq!(
        load(Some(toml_pct_fire)),
        Err(RuleError::Invalid {
            rule: "disk_nearly_full".to_string(),
            field: "fire_at",
            reason: "percent signals must be between 0 and 100",
        })
    );

    // 6. percent signal rearm out of 0..=100
    let toml_pct_rearm = "[rule.disk_nearly_full]\nrearm = -1\n";
    assert_eq!(
        load(Some(toml_pct_rearm)),
        Err(RuleError::Invalid {
            rule: "disk_nearly_full".to_string(),
            field: "rearm",
            reason: "percent signals must be between 0 and 100",
        })
    );

    // 7. temperature.c fire_at out of 0..=150
    let toml_temp = "[rule.running_hot]\nfire_at = 155\n";
    assert_eq!(
        load(Some(toml_temp)),
        Err(RuleError::Invalid {
            rule: "running_hot".to_string(),
            field: "fire_at",
            reason: "temperature must be between 0 and 150",
        })
    );

    // 8. disk.fill_hours fire_at out of 0..=10000
    let toml_fill = "[rule.disk_filling_fast]\nfire_at = 10001\nrearm = 10001\n";
    assert_eq!(
        load(Some(toml_fill)),
        Err(RuleError::Invalid {
            rule: "disk_filling_fast".to_string(),
            field: "fire_at",
            reason: "disk.fill_hours must be between 0 and 10000",
        })
    );

    // 9. session.active_minutes fire_at out of 0..=100000
    let toml_sess = "[rule.long_session]\nfire_at = 100001\n";
    assert_eq!(
        load(Some(toml_sess)),
        Err(RuleError::Invalid {
            rule: "long_session".to_string(),
            field: "fire_at",
            reason: "session.active_minutes must be between 0 and 100000",
        })
    );

    // 10. parts.degraded fire_at out of 0..=100
    let toml_parts = "[rule.part_degraded]\nfire_at = 101\n";
    assert_eq!(
        load(Some(toml_parts)),
        Err(RuleError::Invalid {
            rule: "part_degraded".to_string(),
            field: "fire_at",
            reason: "parts.degraded must be between 0 and 100",
        })
    );

    // 11. hold_for > 24h
    let toml_hold = "[rule.running_hot]\nhold_for = \"25h\"\n";
    assert_eq!(
        load(Some(toml_hold)),
        Err(RuleError::Invalid {
            rule: "running_hot".to_string(),
            field: "hold_for",
            reason: "hold_for must be at most 24h",
        })
    );

    // 12. cooldown > 30d
    let toml_cool = "[rule.running_hot]\ncooldown = \"31d\"\n";
    assert_eq!(
        load(Some(toml_cool)),
        Err(RuleError::Invalid {
            rule: "running_hot".to_string(),
            field: "cooldown",
            reason: "cooldown must be at most 30d",
        })
    );
}

#[test]
fn test_wrong_args_names() {
    // Template battery.low requires ["percent"]. Providing empty args {} fails validation.
    let toml = "[rule.battery_low]\n[rule.battery_low.args]\n";
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "args",
            reason: "args keys must equal template argument names",
        })
    );
}

#[test]
fn test_extra_on_signal_without_extra() {
    // battery.percent does not support extra
    let toml = r#"[rule.battery_low.args]
percent = "extra"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "args",
            reason: "extra only allowed for disk.used_percent and disk.fill_hours",
        })
    );
}

#[test]
fn test_unknown_template() {
    let toml = r#"[rule.battery_low]
template = "unknown.template"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "template",
            reason: "unknown template",
        })
    );
}

#[test]
fn test_unknown_offer_skill() {
    let toml = r#"[rule.battery_low]
offer_skill = "system.unknown_skill"
"#;
    assert_eq!(
        load(Some(toml)),
        Err(RuleError::Invalid {
            rule: "battery_low".to_string(),
            field: "offer_skill",
            reason: "unknown offer skill",
        })
    );
}

#[test]
fn test_user_file_over_size_limit() {
    let huge = " ".repeat(MAX_FILE_BYTES + 1);
    assert_eq!(load(Some(&huge)), Err(RuleError::TooLarge));
}

#[test]
fn test_rule_limit_65_rules() {
    // 8 default rules + 57 new user rules = 65 rules (> MAX_RULES of 64)
    let mut toml = String::new();
    for i in 0..57 {
        toml.push_str(&format!(
            r#"[rule.user_rule_{i}]
signal = "session.active_minutes"
fire_at = 30
rearm = 10
priority = "info"
template = "session.long"
"#
        ));
    }
    assert_eq!(load(Some(&toml)), Err(RuleError::TooManyRules));

    // 8 defaults + 56 new user rules = 64 rules (exactly MAX_RULES, must succeed)
    let mut toml_64 = String::new();
    for i in 0..56 {
        toml_64.push_str(&format!(
            r#"[rule.user_rule_{i}]
signal = "session.active_minutes"
fire_at = 30
rearm = 10
priority = "info"
template = "session.long"
"#
        ));
    }
    let res = load(Some(&toml_64)).expect("64 rules must succeed");
    assert_eq!(res.len(), MAX_RULES);
}

#[test]
fn test_negative_control_direction_rearm() {
    let bad_content = include_str!("fixtures/nudges_user_bad_rearm.toml");

    // 1. The real loader rejects it with Invalid rearm
    let res = load(Some(bad_content));
    assert_eq!(
        res,
        Err(RuleError::Invalid {
            rule: "disk_nearly_full".to_string(),
            field: "rearm",
            reason: "at_least requires rearm <= fire_at",
        })
    );

    // 2. Negative control: a loader that skipped the direction/rearm check would accept this bad file
    fn loose_loader(text: &str) -> Result<Vec<Rule>, RuleError> {
        let parse_num = |v: &toml::Value| -> Option<f64> {
            match v {
                toml::Value::Float(f) => Some(*f),
                toml::Value::Integer(i) => Some(*i as f64),
                _ => None,
            }
        };

        // Parse user table
        let val: toml::Value = toml::from_str(text).map_err(|_| RuleError::Parse)?;
        let root = val.as_table().ok_or(RuleError::Parse)?;
        let rule_table = root
            .get("rule")
            .and_then(|v| v.as_table())
            .ok_or(RuleError::Parse)?;

        let mut rules = defaults()?;
        for (id, r_val) in rule_table {
            let t = r_val.as_table().ok_or(RuleError::Parse)?;
            if let Some(r) = rules.iter_mut().find(|r| r.id == *id) {
                if let Some(f) = t.get("fire_at").and_then(parse_num) {
                    r.fire_at = f;
                }
                if let Some(rearm) = t.get("rearm").and_then(parse_num) {
                    r.rearm = rearm;
                }
                // NOTE: Deliberately SKIPPING the direction/rearm check:
                // if r.direction == AtLeast && r.rearm > r.fire_at { error! }
            }
        }
        Ok(rules)
    }

    let loose_res = loose_loader(bad_content).expect("loose loader accepts bad file");
    let dnf = loose_res
        .iter()
        .find(|r| r.id == "disk_nearly_full")
        .unwrap();
    assert_eq!(dnf.direction, Direction::AtLeast);
    assert!(
        dnf.rearm > dnf.fire_at,
        "negative control confirms rearm (85) > fire_at (80)"
    );
}

#[test]
fn test_fixtures() {
    let ok_content = include_str!("fixtures/nudges_user_ok.toml");
    let rules = load(Some(ok_content)).expect("nudges_user_ok.toml must load");
    assert_eq!(rules.len(), 9);

    let bat = rules.iter().find(|r| r.id == "battery_low").unwrap();
    assert_eq!(bat.fire_at, 10.0);
    assert_eq!(bat.rearm, 18.0);

    let custom = rules.iter().find(|r| r.id == "custom_session").unwrap();
    assert_eq!(custom.fire_at, 45.0);
    assert_eq!(custom.rearm, 10.0);

    let bad_parse = include_str!("fixtures/nudges_user_bad_parse.toml");
    assert_eq!(load(Some(bad_parse)), Err(RuleError::Parse));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn proptest_load_never_panics_and_validates(s in ".*") {
        if let Ok(rules) = load(Some(&s)) {
            assert!(rules.len() <= MAX_RULES);
            for r in &rules {
                assert!(r.fire_at.is_finite());
                assert!(r.rearm.is_finite());
                match r.direction {
                    Direction::AtLeast => assert!(r.rearm <= r.fire_at),
                    Direction::AtMost => assert!(r.rearm >= r.fire_at),
                }
                match r.signal {
                    Signal::DiskUsedPercent
                    | Signal::BatteryPercent
                    | Signal::MemoryAvailablePercent => {
                        assert!((0.0..=100.0).contains(&r.fire_at));
                        assert!((0.0..=100.0).contains(&r.rearm));
                    }
                    Signal::TemperatureC => {
                        assert!((0.0..=150.0).contains(&r.fire_at));
                        assert!((0.0..=150.0).contains(&r.rearm));
                    }
                    Signal::DiskFillHours => {
                        assert!((0.0..=10000.0).contains(&r.fire_at));
                        assert!((0.0..=10000.0).contains(&r.rearm));
                    }
                    Signal::ActiveMinutes => {
                        assert!((0.0..=100000.0).contains(&r.fire_at));
                        assert!((0.0..=100000.0).contains(&r.rearm));
                    }
                    Signal::PartsDegraded => {
                        assert!((0.0..=100.0).contains(&r.fire_at));
                        assert!((0.0..=100.0).contains(&r.rearm));
                    }
                }
                assert!(r.hold_for_s <= 24 * 3600);
                assert!(r.cooldown_s <= 30 * 86400);

                let tmpl = TEMPLATES
                    .iter()
                    .find(|(name, _)| *name == r.template)
                    .expect("valid template");
                assert_eq!(r.args.len(), tmpl.1.len());
                for (arg_name, src) in &r.args {
                    assert!(tmpl.1.contains(&arg_name.as_str()));
                    if *src == ArgSource::Extra {
                        assert!(matches!(
                            r.signal,
                            Signal::DiskUsedPercent | Signal::DiskFillHours
                        ));
                    }
                }
                if let Some(ref skill) = r.offer_skill {
                    assert!(OFFERABLE_SKILLS.contains(&skill.as_str()));
                }
            }
        }
    }
}
