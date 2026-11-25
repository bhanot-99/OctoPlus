//! Preloading and fallback resolution for the companion persona (Phase 12, ADR-0019).
//!
//! At daemon startup before dropping privileges or entering the sandbox, [`preload`]
//! inspects `<config_dir>/persona.toml`. If present and valid (and <= 64 KiB), it is
//! stored for daemon operations. If invalid or missing, it logs a warning naming the key
//! (never the text of the file) and cleanly falls back to the packaged default persona.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::persona::{Persona, PersonaError};

/// Maximum allowed file size for `<config_dir>/persona.toml` (64 KiB).
pub const MAX_PERSONA_FILE_BYTES: usize = 64 * 1024;

static PERSONA: OnceLock<Option<Arc<Persona>>> = OnceLock::new();

/// Preloads `<config_dir>/persona.toml` into a static cache.
///
/// If the file is missing, stores `None`. If present but larger than [`MAX_PERSONA_FILE_BYTES`],
/// or if parsing/validation fails, logs a warning and stores `None` to enable clean fallback.
pub fn preload(config_dir: &Path) {
    let loaded = load_from_dir(config_dir);
    let _ = PERSONA.set(loaded);
}

/// Reads and validates `<config_dir>/persona.toml` directly without caching in [`OnceLock`].
///
/// Returns `Some(Arc<Persona>)` if the file exists, is a regular file <= 64 KiB, and parses
/// validly. Logs ONE warning on parse/validation error naming the key and returns `None`.
pub fn load_from_dir(config_dir: &Path) -> Option<Arc<Persona>> {
    let path = config_dir.join("persona.toml");
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return None,
    };
    if !meta.is_file() || meta.len() > MAX_PERSONA_FILE_BYTES as u64 {
        return None;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            tracing::warn!("failed to read persona.toml");
            return None;
        }
    };
    match Persona::from_toml(&text) {
        Ok(p) => Some(Arc::new(p)),
        Err(err) => {
            log_persona_error(&err);
            None
        }
    }
}

/// Formats one warning naming the problem key without leaking file text.
pub fn format_persona_error(err: &PersonaError) -> String {
    match err {
        PersonaError::Validation { key, reason } => {
            format!("persona.toml validation error in key '{key}': {reason}")
        }
        PersonaError::MissingFixedLine { key } => {
            format!("persona.toml missing mandatory line in key '{key}'")
        }
        PersonaError::Toml(e) => {
            let msg = e.message();
            if let Some(start) = msg.find("unknown field `") {
                let rest = &msg[start + "unknown field `".len()..];
                if let Some(end) = rest.find('`') {
                    let key = &rest[..end];
                    return format!("unknown key in persona.toml: '{key}'");
                }
            }
            "syntax or format error in persona.toml".to_string()
        }
        PersonaError::Io(_) => "failed to read persona.toml".to_string(),
    }
}

static LAST_ERROR_LOG: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Emits one warning naming the problem key without leaking file text.
fn log_persona_error(err: &PersonaError) {
    let msg = format_persona_error(err);
    tracing::warn!("{msg}");
    if let Ok(mut lock) = LAST_ERROR_LOG.lock() {
        *lock = Some(msg);
    }
}

/// Retrieves the most recent error message logged by [`log_persona_error`].
#[doc(hidden)]
pub fn last_error_log() -> Option<String> {
    LAST_ERROR_LOG.lock().ok().and_then(|l| l.clone())
}

/// Returns the owner's configured persona if loaded, else the packaged default persona.
///
/// # Errors
/// Returns [`PersonaError`] only if the packaged default persona fails to parse.
pub fn persona() -> Result<Arc<Persona>, PersonaError> {
    if let Some(Some(p)) = PERSONA.get() {
        return Ok(Arc::clone(p));
    }
    Persona::default_packaged().map(Arc::new)
}
