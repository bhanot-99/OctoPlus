//! Process self-restriction (NFR-SEC-04, Architecture §8.2, AP-4).
//!
//! Each part calls [`restrict`] before it reads any input. The second layer
//! behind the systemd unit: even inside its unit, a part can then touch only
//! its declared paths, cannot make or accept TCP connections, and (where the
//! kernel supports it) cannot reach abstract Unix sockets or signal processes
//! outside its own sandbox.
//!
//! **Fail closed** (rules.md §6.5, R0-3): if the kernel cannot fully enforce
//! the filesystem and TCP rules (Landlock ABI v4, PRD §4: kernel ≥ 6.7), this
//! returns an error and the part must exit. The scope rules (ABI v6) are
//! applied when available and reported, since PRD §4 does not require them.

pub mod child;

use std::path::{Path, PathBuf};

use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, Scope, path_beneath_rules,
};

/// ABI that must be fully enforced: filesystem + TCP (PRD §4).
const REQUIRED_ABI: ABI = ABI::V4;
/// ABI for the optional scope rules (abstract sockets, signals).
const SCOPE_ABI: ABI = ABI::V6;

/// Errors applying the sandbox. Every one means "do not start".
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// The kernel does not support the required Landlock features.
    #[error("landlock not fully enforceable: {0}")]
    Unsupported(String),
    /// A declared path could not be opened to build a rule.
    #[error("sandbox path missing or unreadable: {0}")]
    Path(PathBuf),
    /// The kernel accepted the rules only partly.
    #[error("landlock only partially enforced")]
    Partial,
}

/// Which paths a part may use. Everything else is denied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// Readable (and executable) trees.
    pub read_only: Vec<PathBuf>,
    /// Readable and writable trees.
    pub read_write: Vec<PathBuf>,
}

/// What was enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// Filesystem and TCP rules fully enforced (always true on success).
    pub fs_and_tcp: bool,
    /// Abstract-socket and signal scoping enforced.
    pub scoped: bool,
    /// `no_new_privs` is set.
    pub no_new_privs: bool,
}

fn check_paths(paths: &[PathBuf]) -> Result<(), SandboxError> {
    for p in paths {
        if std::fs::metadata(p).is_err() {
            return Err(SandboxError::Path(p.clone()));
        }
    }
    Ok(())
}

/// Restricts the calling process (all threads created afterwards inherit it).
/// Call it before spawning threads and before reading any input.
///
/// # Errors
/// Any [`SandboxError`]; the caller must exit non-zero.
pub fn restrict(policy: &Policy) -> Result<Report, SandboxError> {
    check_paths(&policy.read_only)?;
    check_paths(&policy.read_write)?;
    let unsupported = |e: landlock::RulesetError| SandboxError::Unsupported(e.to_string());

    let base = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(REQUIRED_ABI))
        .map_err(unsupported)?
        .handle_access(AccessNet::from_all(REQUIRED_ABI))
        .map_err(unsupported)?
        // Scopes are best effort: PRD §4 only requires ABI v4.
        .set_compatibility(CompatLevel::BestEffort)
        .scope(Scope::from_all(SCOPE_ABI))
        .map_err(unsupported)?;
    let created = base
        .set_compatibility(CompatLevel::HardRequirement)
        .create()
        .map_err(unsupported)?
        .add_rules(path_beneath_rules(
            &policy.read_only,
            AccessFs::from_read(REQUIRED_ABI),
        ))
        .map_err(unsupported)?
        .add_rules(path_beneath_rules(
            &policy.read_write,
            AccessFs::from_all(REQUIRED_ABI),
        ))
        .map_err(unsupported)?;
    // No NetPort rule is added: every TCP bind and connect is denied (R0-1).
    let status = created.restrict_self().map_err(unsupported)?;
    match status.ruleset {
        RulesetStatus::FullyEnforced => Ok(Report {
            fs_and_tcp: true,
            scoped: true,
            no_new_privs: status.no_new_privs,
        }),
        RulesetStatus::PartiallyEnforced => {
            // Hard requirements guarantee FS+TCP; only the best-effort scopes can be missing.
            Ok(Report {
                fs_and_tcp: true,
                scoped: false,
                no_new_privs: status.no_new_privs,
            })
        }
        RulesetStatus::NotEnforced => Err(SandboxError::Partial),
    }
}

/// True if `path` is inside one of `roots` (lexically; for diagnostics).
#[must_use]
pub fn covered(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// Locks the pages holding `buf` in RAM so a secret is never written to swap
/// (security.md §4.7 "keys held in locked, zeroised memory", NFR-SEC-06).
/// The caller keeps `buf` at a stable address (boxed) for its whole life.
///
/// # Errors
/// [`SandboxError::Unsupported`] when the kernel refuses (e.g. `RLIMIT_MEMLOCK`);
/// callers holding keys must treat that as fatal.
#[allow(unsafe_code)]
pub fn lock_memory(buf: &mut [u8]) -> Result<(), SandboxError> {
    if buf.is_empty() {
        return Ok(());
    }
    // SAFETY: `buf` is a live, exclusively borrowed slice, so the pointer and
    // length describe valid memory owned by this process. `mlock` only changes
    // paging behaviour; it never reads or writes the bytes.
    unsafe { rustix::mm::mlock(buf.as_mut_ptr().cast(), buf.len()) }
        .map_err(|e| SandboxError::Unsupported(format!("mlock: {e}")))
}
