//! The trusted command line C6 builds for a skill child, and its parser.
//! C6 builds it with [`Invocation::to_args`]; the child parses it with
//! [`Invocation::parse`]. Both sides use this one type, so they cannot drift.

use std::ffi::OsString;
use std::path::PathBuf;

use octoplus_sandbox::child::{ChildLimits, SyscallProfile};

/// Planning only (read-only) or carrying out an approved plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Compute what would happen; never changes anything.
    Plan,
    /// Do it.
    Run,
}

/// Everything the child needs to know before it reads any input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Registered capability id.
    pub capability: String,
    /// Plan or run.
    pub mode: Mode,
    /// Landlock read-only paths.
    pub read_only: Vec<PathBuf>,
    /// Landlock read-write paths.
    pub read_write: Vec<PathBuf>,
    /// Cleanup roots (caches, configured build folders): entries strictly inside may be listed or deleted.
    pub roots: Vec<PathBuf>,
    /// The trash folder, if the skill uses it.
    pub trash: Option<PathBuf>,
    /// Resource limits.
    pub limits: ChildLimits,
    /// Syscall profile.
    pub profile: SyscallProfile,
}

/// The command line is not one C6 would build.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid invocation: {0}")]
pub struct InvocationError(pub &'static str);

fn num(v: Option<OsString>, what: &'static str) -> Result<u64, InvocationError> {
    v.and_then(|s| s.into_string().ok())
        .and_then(|s| s.parse().ok())
        .ok_or(InvocationError(what))
}

fn path(v: Option<OsString>, what: &'static str) -> Result<PathBuf, InvocationError> {
    let p = PathBuf::from(v.ok_or(InvocationError(what))?);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(InvocationError(what))
    }
}

impl Invocation {
    /// The argument vector for the child (without the program name).
    #[must_use]
    pub fn to_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec![
            "--capability".into(),
            self.capability.clone().into(),
            "--mode".into(),
            match self.mode {
                Mode::Plan => "plan",
                Mode::Run => "run",
            }
            .into(),
            "--profile".into(),
            match self.profile {
                SyscallProfile::ReadOnly => "read-only",
                SyscallProfile::FileMutating => "file-mutating",
            }
            .into(),
            "--as-bytes".into(),
            self.limits.address_space_bytes.to_string().into(),
            "--cpu-s".into(),
            self.limits.cpu_seconds.to_string().into(),
            "--nofile".into(),
            self.limits.open_files.to_string().into(),
            "--fsize".into(),
            self.limits.file_size_bytes.to_string().into(),
        ];
        for (flag, list) in [
            ("--ro", &self.read_only),
            ("--rw", &self.read_write),
            ("--root", &self.roots),
        ] {
            for p in list {
                a.push(flag.into());
                a.push(p.clone().into());
            }
        }
        if let Some(t) = &self.trash {
            a.push("--trash".into());
            a.push(t.clone().into());
        }
        a
    }

    /// Parses the child's command line (without the program name).
    ///
    /// # Errors
    /// [`InvocationError`] for anything C6 would not build; the child then exits
    /// without reading input.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, InvocationError> {
        let mut it = args.into_iter();
        let mut capability = None;
        let mut mode = None;
        let mut profile = None;
        let (mut asb, mut cpu, mut nofile, mut fsize) = (None, None, None, None);
        let (mut ro, mut rw, mut roots, mut trash) = (Vec::new(), Vec::new(), Vec::new(), None);
        while let Some(flag) = it.next() {
            match flag.to_str().ok_or(InvocationError("flag"))? {
                "--capability" => {
                    capability = Some(
                        it.next()
                            .and_then(|s| s.into_string().ok())
                            .ok_or(InvocationError("capability"))?,
                    );
                }
                "--mode" => {
                    mode = Some(match it.next().as_deref().and_then(|s| s.to_str()) {
                        Some("plan") => Mode::Plan,
                        Some("run") => Mode::Run,
                        _ => return Err(InvocationError("mode")),
                    });
                }
                "--profile" => {
                    profile = Some(match it.next().as_deref().and_then(|s| s.to_str()) {
                        Some("read-only") => SyscallProfile::ReadOnly,
                        Some("file-mutating") => SyscallProfile::FileMutating,
                        _ => return Err(InvocationError("profile")),
                    });
                }
                "--as-bytes" => asb = Some(num(it.next(), "as-bytes")?),
                "--cpu-s" => cpu = Some(num(it.next(), "cpu-s")?),
                "--nofile" => nofile = Some(num(it.next(), "nofile")?),
                "--fsize" => fsize = Some(num(it.next(), "fsize")?),
                "--ro" => ro.push(path(it.next(), "ro")?),
                "--rw" => rw.push(path(it.next(), "rw")?),
                "--root" => roots.push(path(it.next(), "root")?),
                "--trash" => trash = Some(path(it.next(), "trash")?),
                _ => return Err(InvocationError("unknown flag")),
            }
        }
        let inv = Self {
            capability: capability.ok_or(InvocationError("capability"))?,
            mode: mode.ok_or(InvocationError("mode"))?,
            read_only: ro,
            read_write: rw,
            roots,
            trash,
            limits: ChildLimits {
                address_space_bytes: asb.ok_or(InvocationError("as-bytes"))?,
                cpu_seconds: cpu.ok_or(InvocationError("cpu-s"))?,
                open_files: nofile.ok_or(InvocationError("nofile"))?,
                file_size_bytes: fsize.ok_or(InvocationError("fsize"))?,
            },
            profile: profile.ok_or(InvocationError("profile"))?,
        };
        // A run that may change files must say so; a plan never may.
        if inv.mode == Mode::Plan && inv.profile != SyscallProfile::ReadOnly {
            return Err(InvocationError("plan must be read-only"));
        }
        Ok(inv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Invocation {
        Invocation {
            capability: "system.cleanup".into(),
            mode: Mode::Run,
            read_only: vec![],
            read_write: vec!["/home/u/.cache".into(), "/home/u/.local/share/Trash".into()],
            roots: vec!["/home/u/.cache".into()],
            trash: Some("/home/u/.local/share/Trash".into()),
            limits: ChildLimits {
                address_space_bytes: 1 << 30,
                cpu_seconds: 10,
                open_files: 64,
                file_size_bytes: 0,
            },
            profile: SyscallProfile::FileMutating,
        }
    }

    #[test]
    fn round_trips() {
        let inv = sample();
        assert_eq!(Invocation::parse(inv.to_args()).unwrap(), inv);
        let mut plan = sample();
        plan.mode = Mode::Plan;
        plan.profile = SyscallProfile::ReadOnly;
        assert_eq!(Invocation::parse(plan.to_args()).unwrap(), plan);
    }

    #[test]
    fn rejects_bad_command_lines() {
        let mut plan_mutating = sample();
        plan_mutating.mode = Mode::Plan;
        assert!(Invocation::parse(plan_mutating.to_args()).is_err());
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(Invocation::parse(args(&[])).is_err());
        assert!(Invocation::parse(args(&["--bogus"])).is_err());
        let mut a = sample().to_args();
        a.push("--root".into());
        a.push("relative".into());
        assert!(Invocation::parse(a).is_err());
        let mut a = sample().to_args();
        a[3] = "explode".into();
        assert!(Invocation::parse(a).is_err());
    }
}
