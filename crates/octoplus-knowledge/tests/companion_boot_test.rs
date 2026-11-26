//! Tests for companion persona preloading and fallback (Phase 12, ADR-0019).

#![allow(clippy::unwrap_used)]

use tempfile::tempdir;

use octoplus_knowledge::companion::boot::{
    MAX_PERSONA_FILE_BYTES, format_persona_error, last_error_log, load_from_dir, persona,
};
use octoplus_knowledge::companion::persona::{Persona, PersonaError};

#[test]
fn owner_file_loaded() {
    let dir = tempdir().unwrap();
    let default_toml = include_str!("../../../deploy/persona/laila.toml");
    let custom_toml = default_toml.replace("name = \"Laila\"", "name = \"Laila Test Modified\"");
    std::fs::write(dir.path().join("persona.toml"), custom_toml).unwrap();

    let loaded = load_from_dir(dir.path()).expect("valid owner persona must load");
    assert_eq!(loaded.name, "Laila Test Modified");
}

#[test]
fn invalid_file_fallback() {
    let dir = tempdir().unwrap();
    // Missing required fields / broken TOML
    std::fs::write(dir.path().join("persona.toml"), b"bogus_key = true\n").unwrap();

    let loaded = load_from_dir(dir.path());
    assert!(loaded.is_none(), "invalid file must yield None");

    // persona() must cleanly fall back to default packaged persona
    let p = persona().expect("persona() must fall back to packaged default");
    assert_eq!(p.name, "Laila");
}

#[test]
fn oversize_file_ignored() {
    let dir = tempdir().unwrap();
    let default_toml = include_str!("../../../deploy/persona/laila.toml");
    // Create a file > 64 KiB
    let mut big = default_toml.to_string();
    let padding = "\n# padding line with comment\n".repeat(3000);
    big.push_str(&padding);
    assert!(big.len() > MAX_PERSONA_FILE_BYTES);

    std::fs::write(dir.path().join("persona.toml"), big).unwrap();

    let loaded = load_from_dir(dir.path());
    assert!(
        loaded.is_none(),
        "oversize persona file (> 64 KiB) must be ignored"
    );
}

#[test]
fn error_log_contains_no_persona_text() {
    let dir = tempdir().unwrap();
    let canary_secret = "CANARY_SECRET_NEVER_LEAK_IN_LOGS_987654";
    let canary_key = "forbidden_custom_key";
    let default_toml = include_str!("../../../deploy/persona/laila.toml");
    let invalid_toml = format!("{default_toml}\n{canary_key} = \"{canary_secret}\"\n");

    std::fs::write(dir.path().join("persona.toml"), invalid_toml).unwrap();

    let loaded = load_from_dir(dir.path());
    assert!(loaded.is_none());

    let logged = last_error_log().expect("a warning log must have been recorded");

    // Must name the key
    assert!(
        logged.contains(canary_key),
        "log must name the offending key: {logged}"
    );

    // Must NOT contain the secret value
    assert!(
        !logged.contains(canary_secret),
        "log must not contain file text: {logged}"
    );

    // Also test format_persona_error directly across variants
    let val_err = PersonaError::Validation {
        key: "modes.logic.speed",
        reason: "speed must be positive",
    };
    let formatted_val = format_persona_error(&val_err);
    assert!(formatted_val.contains("modes.logic.speed"));
    assert!(formatted_val.contains("speed must be positive"));

    let miss_err = PersonaError::MissingFixedLine { key: "boundaries" };
    let formatted_miss = format_persona_error(&miss_err);
    assert!(formatted_miss.contains("boundaries"));

    // Verify TOML unknown key formatting
    let bad_parse = Persona::from_toml("invalid_top_key = \"secret_value_xyz\"\n").unwrap_err();
    let formatted_toml = format_persona_error(&bad_parse);
    assert!(formatted_toml.contains("invalid_top_key"));
    assert!(!formatted_toml.contains("secret_value_xyz"));
}
