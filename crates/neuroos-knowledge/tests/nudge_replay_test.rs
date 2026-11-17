#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use neuroos_knowledge::nudge::replay::{self, Event, ReplayError, Scenario, StepJson};
use neuroos_knowledge::nudge::types::{Action, Reason};

const FIXTURES_DIR: &str = "tests/fixtures/nudge-scenarios";

#[test]
fn test_all_fixtures_parse_run_and_pass_check() {
    let dir = Path::new(FIXTURES_DIR);
    assert!(dir.is_dir(), "fixtures directory must exist");

    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed reading {}: {e}", path.display()));
            let scenario = replay::parse(&content)
                .unwrap_or_else(|e| panic!("failed parsing {}: {e}", path.display()));
            let report = replay::run(&scenario)
                .unwrap_or_else(|e| panic!("failed running {}: {e}", path.display()));
            let mismatches = replay::check(&scenario, &report);
            assert!(
                mismatches.is_empty(),
                "scenario `{}` failed check: {:?}",
                scenario.name,
                mismatches
            );
            count += 1;
        }
    }
    assert!(
        count >= 16,
        "must have at least 16 scenarios, found {count}"
    );
}

#[test]
fn test_scenario_count_and_kpi10_spoken_zero() {
    let dir = Path::new(FIXTURES_DIR);
    let mut total_count = 0;
    let mut kpi10_count = 0;

    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            total_count += 1;
            let file_name = path.file_name().unwrap().to_str().unwrap();
            if file_name.starts_with("kpi10_") {
                kpi10_count += 1;
                let content = fs::read_to_string(&path).unwrap();
                let scenario = replay::parse(&content).unwrap();
                assert_eq!(
                    scenario.expect.spoken, 0,
                    "KPI-10 scenario {file_name} expect.spoken must be 0"
                );
                let report = replay::run(&scenario).unwrap();
                let actual_spoken = report
                    .events
                    .iter()
                    .filter(|e| e.action == Action::Speak)
                    .count();
                assert_eq!(
                    actual_spoken, 0,
                    "KPI-10 scenario {file_name} had {actual_spoken} spoken events, expected 0"
                );
            }
        }
    }

    assert!(
        total_count >= 16,
        "expected at least 16 scenarios, found {total_count}"
    );
    assert!(
        kpi10_count >= 7,
        "expected at least 7 KPI-10 scenarios, found {kpi10_count}"
    );
}

#[test]
fn test_parse_errors() {
    // 1. Unknown key
    let unknown_key_json = r#"{
        "name": "bad",
        "description": "unknown key test",
        "voice_available": false,
        "unknown_extra_field": 123,
        "steps": [],
        "expect": { "spoken": 0 }
    }"#;
    assert_eq!(replay::parse(unknown_key_json), Err(ReplayError::Parse));

    // 2. Bad JSON
    let bad_json = r#"{ "name": "unclosed"#;
    assert_eq!(replay::parse(bad_json), Err(ReplayError::Parse));

    // 3. Bad signal name
    let bad_signal_json = r#"{
        "name": "bad_signal",
        "description": "invalid signal name",
        "voice_available": false,
        "steps": [
            {
                "at_s": 0,
                "readings": [
                    { "signal": "disk.unknown_bogus_signal", "key": "/", "value": 90.0 }
                ]
            }
        ],
        "expect": { "spoken": 0 }
    }"#;
    assert_eq!(
        replay::parse(bad_signal_json),
        Err(ReplayError::Signal("disk.unknown_bogus_signal".to_string()))
    );
}

#[test]
fn test_check_mismatches_reported() {
    let scenario_json = r#"{
        "name": "check_test",
        "description": "check mismatch test",
        "voice_available": false,
        "steps": [],
        "expect": {
            "notified": [
                { "rule": "disk_nearly_full", "at_s": 120, "silent": false }
            ],
            "spoken": 2,
            "held_at_end": 1,
            "dropped": 3
        }
    }"#;
    let s: Scenario = replay::parse(scenario_json).unwrap();

    // Report with empty events and held_at_end = 0
    let empty_report = replay::Report {
        events: Vec::new(),
        held_at_end: 0,
    };
    let mismatches = replay::check(&s, &empty_report);

    // Mismatches for notified count, spoken count, held_at_end, and dropped count
    assert!(
        mismatches
            .iter()
            .any(|m| m.contains("notified count mismatch"))
    );
    assert!(
        mismatches
            .iter()
            .any(|m| m.contains("spoken count mismatch"))
    );
    assert!(
        mismatches
            .iter()
            .any(|m| m.contains("held_at_end mismatch"))
    );
    assert!(
        mismatches
            .iter()
            .any(|m| m.contains("dropped count mismatch"))
    );

    // Notification detail mismatch
    let diff_notify_report = replay::Report {
        events: vec![Event {
            at_s: 120,
            rule: "other_rule".to_string(),
            action: Action::Notify { silent: false },
            reason: Reason::Clear,
        }],
        held_at_end: 1,
    };
    let mismatches2 = replay::check(&s, &diff_notify_report);
    assert!(
        mismatches2
            .iter()
            .any(|m| m.contains("notified[0] mismatch"))
    );
}

#[test]
fn test_every_s_until_s_expansion_and_ordering() {
    let steps = vec![
        StepJson {
            at_s: 10,
            every_s: Some(20),
            until_s: Some(70),
            readings: Vec::new(),
            context: Default::default(),
        },
        StepJson {
            at_s: 0,
            every_s: Some(30),
            until_s: Some(60),
            readings: Vec::new(),
            context: Default::default(),
        },
    ];

    let expanded = replay::expand_and_sort_steps(&steps);
    let times: Vec<u64> = expanded.into_iter().map(|(t, _)| t).collect();

    // step 1 produces: 10, 30, 50, 70
    // step 2 produces: 0, 30, 60
    // Sorted order: 0, 10, 30, 30, 50, 60, 70
    assert_eq!(times, vec![0, 10, 30, 30, 50, 60, 70]);
}

#[test]
fn test_negative_control_kpi10_altered_expectation_fails() {
    let content = fs::read_to_string(format!("{FIXTURES_DIR}/kpi10_paused.json")).unwrap();
    let mut scenario = replay::parse(&content).unwrap();

    let report = replay::run(&scenario).unwrap();
    assert!(
        replay::check(&scenario, &report).is_empty(),
        "unaltered scenario should pass"
    );

    // Negative control: artificially expect 1 spoken nudge in a paused context
    scenario.expect.spoken = 1;
    let mismatches = replay::check(&scenario, &report);
    assert!(
        !mismatches.is_empty(),
        "altered KPI-10 expectation must fail check"
    );
    assert!(
        mismatches
            .iter()
            .any(|m| m.contains("spoken count mismatch: expected 1, got 0")),
        "mismatch must report spoken count difference"
    );
}
