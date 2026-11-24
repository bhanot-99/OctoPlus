#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Nudge isolation (FR-NUD-10, R0-6, phases.md 9.3 SC): the nudge engine has no path to evidence
//! text, to the model, or to `Dispatch`. A source check, with a negative control.

use std::path::Path;

/// Things the nudge tree must never name.
const FORBIDDEN: &[&str] = &[
    "DispatchRequest",
    "DispatchResult",
    "GenerateRequest",
    "TokenChunk",
    "QueryHybrid",
    "HybridResult",
    "Evidence",
    "ProfileFact",
    "ProfileAdd",
    "ForgetRequest",
    "StoreInteraction",
    "crate::action",
    "crate::prompt",
    "crate::chat",
    "crate::facts",
    "crate::evidence",
    "crate::turn",
    "crate::session",
    "crate::sanitize",
    "crate::deictic",
];

/// Other parts of this crate the nudge tree may use (the router's text helpers for replies).
const ALLOWED_CRATE: &[&str] = &["crate::router"];

fn violations(name: &str, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for f in FORBIDDEN {
            if code.contains(f) {
                out.push(format!("{name}:{}: names `{f}`", i + 1));
            }
        }
        let mut rest = code;
        while let Some(at) = rest.find("crate::") {
            let tail = &rest[at..];
            let end = tail[7..].find("::").map_or(tail.len(), |e| e + 7);
            let path = &tail[..end];
            if !ALLOWED_CRATE.contains(&path) {
                out.push(format!("{name}:{}: uses `{path}`", i + 1));
            }
            rest = &tail[7..];
        }
    }
    out
}

fn nudge_sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nudge");
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            v.push((
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
    v.sort();
    v
}

#[test]
fn the_nudge_tree_cannot_reach_evidence_the_model_or_dispatch() {
    let sources = nudge_sources();
    assert!(sources.len() >= 10, "found {} files", sources.len());
    let bad: Vec<String> = sources.iter().flat_map(|(n, s)| violations(n, s)).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_notification_carries_only_a_template_id_and_numbers() {
    // The engine builds `NotifyRequest { template_id, args: TemplateArg { name, value: f64 }, silent }`
    // and nothing else: no `format!` can put text into it.
    let engine = nudge_sources()
        .into_iter()
        .find(|(n, _)| n == "engine.rs")
        .unwrap()
        .1;
    assert!(engine.contains("NotifyRequest {"));
    assert!(
        !engine.contains("format!("),
        "the engine must not build text"
    );
}

#[test]
fn negative_control_the_checker_catches_what_it_forbids() {
    let bad = "use crate::action::parse_call;\nlet r = DispatchRequest { skill: s };\nuse crate::router::normalise;";
    let v = violations("x.rs", bad);
    assert!(v.iter().any(|l| l.contains("crate::action")), "{v:?}");
    assert!(v.iter().any(|l| l.contains("DispatchRequest")), "{v:?}");
    assert!(!v.iter().any(|l| l.contains("crate::router")), "{v:?}");
}
