#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Companion isolation (design.md §12 DR-1, ADR-0019, rules.md R0-6):
//! Companion and nudge speech produces words only. It has no path to Dispatch or to saving facts.
//! Actions go through the router and the safety kernel's tiers.

use std::path::Path;

/// Things the companion tree and companion turn logic must never name.
const FORBIDDEN: &[&str] = &[
    "Dispatch",
    "dispatch(",
    "send_dispatch",
    "action::",
    "DispatchRequest",
    "StoreFact",
    "profile.remember",
];

fn violations(name: &str, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        for f in FORBIDDEN {
            if line.contains(f) {
                out.push(format!("{name}:{}: names forbidden `{f}`", i + 1));
            }
        }
    }
    out
}

fn companion_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("src/companion");
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            v.push((
                format!("companion/{}", p.file_name().unwrap().to_string_lossy()),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
    let turn_p = root.join("src/turn/companion_turn.rs");
    v.push((
        "turn/companion_turn.rs".to_string(),
        std::fs::read_to_string(&turn_p).unwrap(),
    ));
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn the_companion_tree_has_no_path_to_dispatch_or_fact_storage() {
    let sources = companion_sources();
    assert!(sources.len() >= 10, "found {} files", sources.len());
    let bad: Vec<String> = sources.iter().flat_map(|(n, s)| violations(n, s)).collect();
    assert!(bad.is_empty(), "DR-1 violation found:\n{bad:#?}");
}

#[test]
fn negative_control_catches_forbidden_tokens() {
    for forbidden in FORBIDDEN {
        let code = format!("fn foo() {{ let _ = \"{forbidden}\"; }}");
        let v = violations("test.rs", &code);
        assert!(
            !v.is_empty(),
            "expected negative control to catch forbidden token: {forbidden}"
        );
    }
}
