//! Things the nudge engine needs from the machine that the sandbox will not allow later:
//! the optional `nudges.toml` and the local time zone. `main` calls [`preload`] before the
//! sandbox is applied (the way the kernel reads its folders), and `init` reads the results.

use std::path::Path;
use std::sync::OnceLock;

use super::rules::MAX_FILE_BYTES;

static RULES_TEXT: OnceLock<Option<String>> = OnceLock::new();
static ZONE: OnceLock<jiff::tz::TimeZone> = OnceLock::new();

/// Reads `<config_dir>/nudges.toml` (if present and not larger than the limit) and the system
/// time zone. Safe to call once; later calls keep the first answer.
pub fn preload(config_dir: &Path) {
    let text = std::fs::metadata(config_dir.join("nudges.toml"))
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_FILE_BYTES as u64)
        .and_then(|_| std::fs::read_to_string(config_dir.join("nudges.toml")).ok());
    let _ = RULES_TEXT.set(text);
    let _ = ZONE.set(jiff::tz::TimeZone::system());
}

/// The user's `nudges.toml`, if there is one.
#[must_use]
pub fn user_rules() -> Option<&'static str> {
    RULES_TEXT.get().and_then(|t| t.as_deref())
}

/// The machine's time zone (UTC when [`preload`] has not run or could not tell).
#[must_use]
pub fn time_zone() -> jiff::tz::TimeZone {
    ZONE.get().cloned().unwrap_or(jiff::tz::TimeZone::UTC)
}
