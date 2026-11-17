#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use neuroos_knowledge::deictic::{Anchor, DeicticInfo, parse};
use neuroos_knowledge::router;
use proptest::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    text: String,
    deictic: bool,
    anchor: String,
}

const FIXTURES_RAW: &str = include_str!("fixtures/deictic.jsonl");

fn parse_expected_anchor(s: &str) -> Anchor {
    if s == "now" {
        Anchor::Now
    } else if s == "none" {
        Anchor::None
    } else if let Some(rest) = s.strip_prefix("seconds_ago:") {
        Anchor::SecondsAgo(rest.parse::<u64>().expect("valid seconds_ago number"))
    } else if let Some(rest) = s.strip_prefix("range:") {
        Anchor::Range(rest.parse::<u64>().expect("valid range number"))
    } else {
        panic!("unexpected anchor in fixture: {s}");
    }
}

#[test]
fn test_deictic_fixtures() {
    let fixtures: Vec<Fixture> = FIXTURES_RAW
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| serde_json::from_str::<Fixture>(line).expect("valid fixture line"))
        .collect();

    assert!(
        fixtures.len() >= 70,
        "expected >= 70 fixtures, got {}",
        fixtures.len()
    );

    let mut matches = 0usize;
    for (idx, f) in fixtures.iter().enumerate() {
        let info = parse(&f.text);
        let expected_anchor = parse_expected_anchor(&f.anchor);

        let deictic_ok = info.deictic == f.deictic;
        let anchor_ok = info.anchor == expected_anchor;

        if deictic_ok && anchor_ok {
            matches += 1;
        } else {
            eprintln!(
                "Fixture #{} mismatch: text={:?}, got={:?}, expected=(deictic={}, anchor={:?})",
                idx + 1,
                f.text,
                info,
                f.deictic,
                expected_anchor
            );
        }
    }

    let accuracy = (matches as f64) / (fixtures.len() as f64);
    assert!(
        accuracy >= 0.95,
        "expected accuracy >= 95%, got {:.2}% ({}/{} matches)",
        accuracy * 100.0,
        matches,
        fixtures.len()
    );
    // Our own fixture set should achieve 100%
    assert_eq!(
        matches,
        fixtures.len(),
        "All fixtures in deictic.jsonl must match 100%"
    );
}

#[test]
fn test_deictic_tables() {
    let cases = [
        (
            "what is this",
            DeicticInfo {
                deictic: true,
                anchor: Anchor::Now,
            },
        ),
        (
            "what was that",
            DeicticInfo {
                deictic: true,
                anchor: Anchor::Now,
            },
        ),
        (
            "what is on this screen",
            DeicticInfo {
                deictic: true,
                anchor: Anchor::Now,
            },
        ),
        (
            "what app is open right now",
            DeicticInfo {
                deictic: true,
                anchor: Anchor::Now,
            },
        ),
        (
            "what am I doing currently",
            DeicticInfo {
                deictic: true,
                anchor: Anchor::Now,
            },
        ),
        (
            "at the moment check status",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Now,
            },
        ),
        (
            "a minute ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(60),
            },
        ),
        (
            "two minutes ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(120),
            },
        ),
        (
            "three minutes ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(180),
            },
        ),
        (
            "ten minutes ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(600),
            },
        ),
        (
            "twenty minutes ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(1200),
            },
        ),
        (
            "an hour ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(3600),
            },
        ),
        (
            "two hours ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(7200),
            },
        ),
        (
            "five hours ago",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(18000),
            },
        ),
        (
            "just now",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::SecondsAgo(30),
            },
        ),
        (
            "earlier",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(3600),
            },
        ),
        (
            "recently",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(3600),
            },
        ),
        (
            "last hour",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(3600),
            },
        ),
        (
            "past hour",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(3600),
            },
        ),
        (
            "today",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(86400),
            },
        ),
        (
            "this morning",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(86400),
            },
        ),
        (
            "this afternoon",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(86400),
            },
        ),
        (
            "this evening",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(86400),
            },
        ),
        (
            "yesterday",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(172800),
            },
        ),
        (
            "last night",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(172800),
            },
        ),
        (
            "this week",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(604800),
            },
        ),
        (
            "last week",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::Range(604800),
            },
        ),
        (
            "what is rust",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::None,
            },
        ),
        (
            "I know the way",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::None,
            },
        ),
        (
            "nowhere to hide",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::None,
            },
        ),
        (
            "wait a minute",
            DeicticInfo {
                deictic: false,
                anchor: Anchor::None,
            },
        ),
    ];

    for (text, expected) in cases {
        let got = parse(text);
        assert_eq!(
            got, expected,
            "text={:?}, got={:?}, expected={:?}",
            text, got, expected
        );
    }
}

proptest! {
    #[test]
    fn proptest_deictic_invariants(text in "\\PC*") {
        let info = parse(&text);
        let norm = router::normalise(&text);
        let expected_deictic = router::detect_deictic(&norm);

        // Invariant 1: deictic flag strictly matches router::detect_deictic
        prop_assert_eq!(info.deictic, expected_deictic);

        // Invariant 2: if deictic is true and no time words, anchor is at least Now or Range/SecondsAgo
        if info.deictic {
            prop_assert_ne!(info.anchor, Anchor::None);
        }
    }
}
