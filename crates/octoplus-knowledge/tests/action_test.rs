#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Contract test (AB-1 allows a test that reads another part's *files*, not its crate): the
//! action route's closed skill table must agree with the kernel's skill manifests.

use std::path::Path;

use octoplus_knowledge::action::{Param, Request, SPECS, parse_call};
use serde_json::Value;

fn manifest(skill: &str) -> toml_like::Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../octoplus-kernel/skills")
        .join(format!("{skill}.toml"));
    toml_like::parse(&std::fs::read_to_string(path).unwrap())
}

/// A tiny reader for the manifest keys this test needs (no TOML dependency in C5).
mod toml_like {
    use std::collections::BTreeMap;

    #[derive(Debug, Default)]
    pub struct Manifest {
        pub tier: String,
        pub disabled: bool,
        /// arg name -> (type, min, max, max_len, required)
        pub args: BTreeMap<String, BTreeMap<String, String>>,
    }

    pub fn parse(text: &str) -> Manifest {
        let mut m = Manifest::default();
        let mut section = String::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(h) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = h.to_string();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            if let Some(arg) = section.strip_prefix("args.") {
                m.args
                    .entry(arg.to_string())
                    .or_default()
                    .insert(k.into(), v.into());
            } else if section.is_empty() {
                match k {
                    "tier" => m.tier = v.into(),
                    "disabled" => m.disabled = v == "true",
                    _ => {}
                }
            }
        }
        m
    }
}

#[test]
fn every_callable_skill_exists_enabled_and_not_dangerous() {
    for spec in SPECS {
        let m = manifest(spec.skill);
        assert!(!m.disabled, "{} is disabled", spec.skill);
        assert_ne!(m.tier, "DANGEROUS", "{}", spec.skill);
    }
}

#[test]
fn model_parameters_match_manifest_arguments() {
    for spec in SPECS {
        let m = manifest(spec.skill);
        for p in spec.params {
            match (spec.skill, p) {
                // The model's `range` becomes since_ns/until_ns (see action.rs).
                ("memory.forget", Param::Range { .. }) => {
                    assert!(m.args.contains_key("since_ns") && m.args.contains_key("until_ns"));
                }
                (_, Param::Int { name, min, max, .. }) => {
                    let a = &m.args[*name];
                    assert_eq!(a["type"], "int", "{} {name}", spec.skill);
                    assert!(a["min"].parse::<u64>().unwrap() <= *min);
                    if let Some(manifest_max) = a.get("max") {
                        assert!(
                            *max <= manifest_max.parse::<u64>().unwrap(),
                            "{} {name}",
                            spec.skill
                        );
                    }
                }
                (
                    _,
                    Param::Text {
                        name,
                        max,
                        required,
                    },
                ) => {
                    let a = &m.args[*name];
                    assert_eq!(a["type"], "string", "{} {name}", spec.skill);
                    assert!(
                        *max <= a["max_len"].parse::<usize>().unwrap(),
                        "{} {name}",
                        spec.skill
                    );
                    assert_eq!(a["required"] == "true", *required, "{} {name}", spec.skill);
                }
                (s, Param::Range { .. }) => panic!("{s}: range only belongs to memory.forget"),
            }
        }
        // Every required manifest arg is reachable from the model schema.
        for (name, a) in &m.args {
            if a["required"] == "true" {
                assert!(
                    spec.params.iter().any(|p| match p {
                        Param::Int { name: n, .. } | Param::Text { name: n, .. } => n == name,
                        Param::Range { .. } => false,
                    }),
                    "{} requires {name} which the model schema lacks",
                    spec.skill
                );
            }
        }
    }
}

#[test]
fn a_parsed_call_only_carries_manifest_keys() {
    let c = parse_call(
        r#"{"skill":"memory.forget","range":"last_day","app":"org.x"}"#,
        Request {
            allowed: &["memory.forget"],
            user_text: "forget org.x from the last day",
            now_ns: 2_000_000_000_000_000_000,
        },
    )
    .unwrap();
    let args: Value = serde_json::from_str(&c.args_json).unwrap();
    let m = manifest("memory.forget");
    for key in args.as_object().unwrap().keys() {
        assert!(m.args.contains_key(key), "{key}");
    }
}
