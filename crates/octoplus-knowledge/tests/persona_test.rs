#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::collapsible_if
)]

use std::collections::BTreeSet;
use std::path::Path;

use octoplus_knowledge::companion::persona::{NumOrOpen, Persona, PersonaError};

#[test]
fn test_default_file_parses_and_validates() {
    let persona = Persona::default_packaged().expect("default persona must parse and validate");
    assert_eq!(persona.name, "Laila");
    assert_eq!(persona.age, 25);
    assert_eq!(persona.voice, "af_heart");
    assert_eq!(persona.boundaries.len(), 7);
    assert_eq!(persona.crisis.text_lines.len(), 5);

    // Also test loading directly from relative path to deploy/persona/laila.toml
    let path = Path::new("../../deploy/persona/laila.toml");
    let loaded = Persona::load(path).expect("loading from path must succeed");
    assert_eq!(loaded, persona);
}

#[test]
fn test_unknown_key_rejected_with_key_named() {
    let mut toml_str = include_str!("../../../deploy/persona/laila.toml").to_string();
    toml_str.push_str("\ninvalid_extra_field = 42\n");
    let err = Persona::from_toml(&toml_str).unwrap_err();
    match err {
        PersonaError::Toml(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("invalid_extra_field") || msg.contains("unknown field"),
                "error message must name the invalid key, got: {msg}"
            );
        }
        other => panic!("expected PersonaError::Toml, got {other:?}"),
    }
}

#[test]
fn test_probability_1_5_rejected() {
    let toml_str = include_str!("../../../deploy/persona/laila.toml")
        .replace("start_logic_p = 0.5", "start_logic_p = 1.5");
    let err = Persona::from_toml(&toml_str).unwrap_err();
    match err {
        PersonaError::Validation { key, reason } => {
            assert_eq!(key, "switching.start_logic_p");
            assert!(reason.contains("probability"));
        }
        other => panic!("expected PersonaError::Validation for probability, got {other:?}"),
    }
}

#[test]
fn test_quiet_hours_25_00_rejected() {
    let toml_str = include_str!("../../../deploy/persona/laila.toml")
        .replace("quiet_start = \"23:00\"", "quiet_start = \"25:00\"");
    let err = Persona::from_toml(&toml_str).unwrap_err();
    match err {
        PersonaError::Validation { key, .. } => {
            assert_eq!(key, "schedule.quiet_start");
        }
        other => panic!("expected PersonaError::Validation for quiet_start, got {other:?}"),
    }
}

#[test]
fn test_removing_fixed_boundary_rejected() {
    let mut persona = Persona::default_packaged().unwrap();
    persona.boundaries.pop();
    let err = persona.validate().unwrap_err();
    match err {
        PersonaError::MissingFixedLine { key } => {
            assert_eq!(key, "boundaries");
        }
        other => panic!("expected MissingFixedLine, got {other:?}"),
    }
}

#[test]
fn test_removing_fixed_crisis_line_rejected() {
    let mut persona = Persona::default_packaged().unwrap();
    persona.crisis.text_lines.pop();
    let err = persona.validate().unwrap_err();
    match err {
        PersonaError::MissingFixedLine { key } => {
            assert_eq!(key, "crisis.text_lines");
        }
        other => panic!("expected MissingFixedLine, got {other:?}"),
    }
}

#[test]
fn test_adding_extra_boundary_accepted() {
    let mut persona = Persona::default_packaged().unwrap();
    persona
        .boundaries
        .push("Custom additional rule: be nice.".to_string());
    assert!(persona.validate().is_ok());
}

#[test]
fn test_open_strings_accepted_for_speed_and_pause() {
    let persona = Persona::default_packaged().unwrap();
    match &persona.modes.logic.speed {
        NumOrOpen::Open(s) => assert!(s.contains("open")),
        NumOrOpen::Num(_) => panic!("expected open string for default logic speed"),
    }
    match &persona.modes.logic.pause_ms {
        NumOrOpen::Open(s) => assert!(s.contains("open")),
        NumOrOpen::Num(_) => panic!("expected open string for default logic pause_ms"),
    }

    let toml_str = include_str!("../../../deploy/persona/laila.toml")
        .replace("speed = \"open, decided in story C-S02\"", "speed = 1.1")
        .replace(
            "pause_ms = \"open, decided in story C-S02\"",
            "pause_ms = 250",
        );
    let parsed = Persona::from_toml(&toml_str).unwrap();
    assert_eq!(parsed.modes.logic.speed, NumOrOpen::Num(1.1));
    assert_eq!(parsed.modes.logic.pause_ms, NumOrOpen::Num(250));
}

fn collect_toml_keys(table: &toml::Table, prefix: &str, keys: &mut BTreeSet<String>) {
    for (k, v) in table {
        let full = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(sub) => collect_toml_keys(sub, &full, keys),
            _ => {
                keys.insert(full);
            }
        }
    }
}

fn extract_doc_keys(doc: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut in_table = false;
    for line in doc.lines() {
        if line.starts_with("## 3. Schema and Keys Reference") {
            in_table = true;
            continue;
        }
        if in_table && line.starts_with("---") {
            break;
        }
        if in_table && line.starts_with("| `") {
            if let Some(end) = line[3..].find('`') {
                keys.insert(line[3..3 + end].to_string());
            }
        }
    }
    keys
}

#[test]
fn test_every_key_documented_exists_on_default_file() {
    let doc_str = include_str!("../../../docs/design/persona-format.md");
    let doc_keys = extract_doc_keys(doc_str);

    let default_str = include_str!("../../../deploy/persona/laila.toml");
    let table: toml::Table = toml::from_str(default_str).unwrap();
    let mut file_keys = BTreeSet::new();
    collect_toml_keys(&table, "", &mut file_keys);

    assert_eq!(
        doc_keys, file_keys,
        "keys in persona-format.md must match deploy/persona/laila.toml 1:1"
    );
    assert_eq!(doc_keys.len(), 48);
}
