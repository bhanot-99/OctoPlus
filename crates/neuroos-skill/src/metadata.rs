//! `system.read_metadata` (SAFE): machine facts only — uptime, kernel and OS
//! versions, NeuroOS version. Reads `/proc/uptime`, `/proc/sys/kernel/osrelease`
//! and `/etc/os-release`; nothing personal.

use serde_json::{Value, json};

use crate::SkillError;

fn os_pretty_name(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        l.strip_prefix("PRETTY_NAME=").map(|v| {
            v.trim_matches('"')
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect()
        })
    })
}

fn uptime_seconds(text: &str) -> Option<u64> {
    let first = text.split_whitespace().next()?;
    let secs: f64 = first.parse().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        // Truncation is intended: whole seconds.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(secs as u64)
    } else {
        None
    }
}

/// Runs the skill. Takes no arguments.
///
/// # Errors
/// [`SkillError::BadArgs`] for any argument; [`SkillError::Io`] if `/proc` is unreadable.
pub fn run(args: &Value) -> Result<Value, SkillError> {
    if args.as_object().is_none_or(|m| !m.is_empty()) {
        return Err(SkillError::BadArgs);
    }
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| uptime_seconds(&t))
        .ok_or(SkillError::Io)?;
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().chars().take(128).collect::<String>())
        .map_err(|_| SkillError::Io)?;
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| os_pretty_name(&t));
    Ok(json!({
        "uptime_s": uptime,
        "kernel": kernel,
        "os": os,
        "neuroos_version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(uptime_seconds("12345.67 999.0\n"), Some(12345));
        assert_eq!(uptime_seconds("nan 1"), None);
        assert_eq!(uptime_seconds(""), None);
        assert_eq!(
            os_pretty_name("NAME=x\nPRETTY_NAME=\"Pop!_OS 24.04 LTS\"\n").as_deref(),
            Some("Pop!_OS 24.04 LTS")
        );
        assert_eq!(os_pretty_name("NAME=x"), None);
    }

    #[test]
    fn runs_on_this_machine_and_refuses_args() {
        let v = run(&json!({})).unwrap();
        assert!(v["uptime_s"].as_u64().unwrap() > 0);
        assert!(!v["kernel"].as_str().unwrap().is_empty());
        assert_eq!(run(&json!({"x": 1})), Err(SkillError::BadArgs));
        assert_eq!(run(&json!([])), Err(SkillError::BadArgs));
    }
}
