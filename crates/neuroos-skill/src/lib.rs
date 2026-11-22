//! `neuroos-skill` — the sandboxed child that runs one skill (FR-KER-05,
//! Architecture §8.2 "skill child", ADR-0010 decision 2).
//!
//! C6 starts one child per action. **Trusted** settings (which skill, which
//! folders, which limits) come on the command line from C6's own registry;
//! the **untrusted** arguments (from C5, already schema-checked and approved)
//! come on stdin. The child locks itself down — rlimits, then Landlock on
//! exactly its folders, then the seccomp allowlist — before it reads stdin,
//! so even arguments that lie cannot reach anything outside those folders.
//!
//! Output: one JSON object on stdout, `{"ok":true,"result":…}` or
//! `{"ok":false,"error":"<code>"}`. Error codes never contain user content.

pub mod calendar;
pub mod cleanup;
pub mod disk;
pub mod invocation;
pub mod metadata;
pub mod notes;
pub mod walk;

use serde_json::Value;

pub use invocation::{Invocation, InvocationError, Mode};

/// Largest argument document the child reads (matches the kernel's schema limit).
pub const MAX_ARGS_BYTES: usize = 64 * 1024;
/// Largest output C6 accepts from a child.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Why a skill failed. The `code` is what C6 records; it never holds user content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    /// Arguments are not what this skill accepts.
    #[error("bad_args")]
    BadArgs,
    /// A path is outside the allowlisted roots, is a root itself, or does not exist.
    #[error("path_outside_allowlist")]
    PathOutsideAllowlist,
    /// The capability is not one this binary runs.
    #[error("unknown_capability")]
    UnknownCapability,
    /// The mode is not valid for this skill (e.g. `run` without a plan).
    #[error("bad_mode")]
    BadMode,
    /// Filesystem error while working.
    #[error("io")]
    Io,
}

impl SkillError {
    /// Stable code string for the output and the audit log.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadArgs => "bad_args",
            Self::PathOutsideAllowlist => "path_outside_allowlist",
            Self::UnknownCapability => "unknown_capability",
            Self::BadMode => "bad_mode",
            Self::Io => "io",
        }
    }
}

/// Runs the skill named in `inv` on `args`. Pure dispatch; the sandbox is applied by the binary.
///
/// # Errors
/// Any [`SkillError`].
pub fn run(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    match inv.capability.as_str() {
        "system.read_metadata" => metadata::run(args),
        "system.disk_report" => disk::run(inv, args),
        "system.cleanup" => cleanup::run(inv, args),
        "notes.search" => notes::search(inv, args),
        "notes.append" => notes::append(inv, args),
        "calendar.read" => calendar::read(inv, args, unix_now()),
        _ => Err(SkillError::UnknownCapability),
    }
}

/// Seconds since the Unix epoch (0 if the clock is before it).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Applies, in order, the resource limits, Landlock on exactly the invocation's
/// folders, and the seccomp allowlist. Call before reading any input.
///
/// # Errors
/// Any [`neuroos_sandbox::SandboxError`]; the child must exit without reading input.
pub fn lock_down(inv: &Invocation) -> Result<(), neuroos_sandbox::SandboxError> {
    neuroos_sandbox::child::apply_limits(&inv.limits)?;
    neuroos_sandbox::restrict(&neuroos_sandbox::Policy {
        read_only: inv.read_only.clone(),
        read_write: inv.read_write.clone(),
        connect_tcp: Vec::new(),
    })?;
    neuroos_sandbox::child::apply_seccomp(inv.profile)
}

/// The child's single output line.
#[must_use]
pub fn output(result: &Result<Value, SkillError>) -> String {
    let v = match result {
        Ok(r) => serde_json::json!({"ok": true, "result": r}),
        Err(e) => serde_json::json!({"ok": false, "error": e.code()}),
    };
    v.to_string()
}
