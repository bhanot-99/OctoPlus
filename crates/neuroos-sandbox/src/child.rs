//! Extra locks for skill child processes (FR-KER-05, Architecture §8.2 "skill
//! runner", ADR-0010 decision 2): resource limits and a syscall allowlist.
//!
//! A skill child calls, in order and before it reads any input:
//! [`apply_limits`] → [`crate::restrict`] (its own, narrower Landlock policy) →
//! [`apply_seccomp`]. Anything not on the allowlist fails with `EPERM`; in
//! particular there is no `socket`, `connect`, `execve`, `fork`/`clone`,
//! `ptrace` or `kill`, so a skill cannot reach the network, start programs,
//! spawn helpers or touch other processes even if its own code is subverted.

use rustix::process::{Resource, Rlimit, setrlimit};
use seccompiler::{BpfMap, TargetArch};

use crate::SandboxError;

/// Which syscall set a skill gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallProfile {
    /// Read files and directories, write results to stdout.
    ReadOnly,
    /// Also rename, create directories and unlink (cleanup into the trash).
    FileMutating,
}

/// Hard resource limits for one child (soft = hard, so the child cannot raise them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLimits {
    /// `RLIMIT_AS`: address space in bytes.
    pub address_space_bytes: u64,
    /// `RLIMIT_CPU`: CPU seconds.
    pub cpu_seconds: u64,
    /// `RLIMIT_NOFILE`: open file descriptors.
    pub open_files: u64,
    /// `RLIMIT_FSIZE`: largest file the child may write.
    pub file_size_bytes: u64,
}

/// Syscalls a Rust program needs to start, read files and write to stdout.
const BASE: &[&str] = &[
    "read",
    "write",
    "readv",
    "writev",
    "pread64",
    "openat",
    "close",
    "fstat",
    "newfstatat",
    "statx",
    "lseek",
    "mmap",
    "munmap",
    "mprotect",
    "mremap",
    "brk",
    "madvise",
    "rt_sigaction",
    "rt_sigprocmask",
    "rt_sigreturn",
    "sigaltstack",
    "getdents64",
    "exit",
    "exit_group",
    "futex",
    "getrandom",
    "sched_yield",
    "sched_getaffinity",
    "clock_gettime",
    "clock_nanosleep",
    "nanosleep",
    "fcntl",
    "poll",
    "ppoll",
    "getpid",
    "gettid",
    "getuid",
    "geteuid",
    "getgid",
    "getegid",
    "uname",
    "sysinfo",
    "statfs",
    "fstatfs",
    "readlink",
    "readlinkat",
    "prlimit64",
    "set_robust_list",
    "rseq",
    "restart_syscall",
];

/// Added for [`SyscallProfile::FileMutating`].
const MUTATING: &[&str] = &[
    "renameat",
    "renameat2",
    "unlinkat",
    "mkdirat",
    "fsync",
    "fdatasync",
];

/// `EPERM`: a denied syscall fails like a permission error instead of killing
/// the child, so the skill reports FAILED with a reason.
const EPERM: u32 = 1;

fn filter_json(profile: SyscallProfile) -> String {
    let mut names: Vec<&str> = BASE.to_vec();
    if profile == SyscallProfile::FileMutating {
        names.extend_from_slice(MUTATING);
    }
    let rules: Vec<String> = names
        .iter()
        .map(|n| format!(r#"{{"syscall":"{n}"}}"#))
        .collect();
    format!(
        r#"{{"skill":{{"mismatch_action":{{"errno":{EPERM}}},"match_action":"allow","filter":[{}]}}}}"#,
        rules.join(",")
    )
}

fn compile(profile: SyscallProfile) -> Result<BpfMap, SandboxError> {
    seccompiler::compile_from_json(filter_json(profile).as_bytes(), TargetArch::x86_64)
        .map_err(|e| SandboxError::Unsupported(format!("seccomp compile: {e}")))
}

/// Installs the syscall allowlist for the calling thread (the child is single-threaded).
/// Also sets `no_new_privs`.
///
/// # Errors
/// [`SandboxError::Unsupported`]; the child must exit without reading input.
pub fn apply_seccomp(profile: SyscallProfile) -> Result<(), SandboxError> {
    let map = compile(profile)?;
    let prog = map
        .get("skill")
        .ok_or_else(|| SandboxError::Unsupported("seccomp program missing".into()))?;
    seccompiler::apply_filter(prog)
        .map_err(|e| SandboxError::Unsupported(format!("seccomp apply: {e}")))
}

fn set(resource: Resource, value: u64) -> Result<(), SandboxError> {
    setrlimit(
        resource,
        Rlimit {
            current: Some(value),
            maximum: Some(value),
        },
    )
    .map_err(|e| SandboxError::Unsupported(format!("setrlimit {resource:?}: {e}")))
}

/// Applies `limits`, plus no core dumps and no new processes.
///
/// # Errors
/// [`SandboxError::Unsupported`] if any limit cannot be set.
pub fn apply_limits(limits: &ChildLimits) -> Result<(), SandboxError> {
    set(Resource::As, limits.address_space_bytes)?;
    set(Resource::Cpu, limits.cpu_seconds)?;
    set(Resource::Nofile, limits.open_files)?;
    set(Resource::Fsize, limits.file_size_bytes)?;
    set(Resource::Core, 0)?;
    // Counts every process of this UID, so 0 makes any fork fail (seccomp also denies it).
    set(Resource::Nproc, 0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_profiles_compile() {
        assert!(compile(SyscallProfile::ReadOnly).is_ok());
        assert!(compile(SyscallProfile::FileMutating).is_ok());
    }

    #[test]
    fn dangerous_syscalls_are_not_listed() {
        for p in [SyscallProfile::ReadOnly, SyscallProfile::FileMutating] {
            let json = filter_json(p);
            for banned in [
                "socket", "connect", "execve", "execveat", "fork", "vfork", "clone", "clone3",
                "ptrace", "kill", "tgkill", "bind", "sendto", "ioctl", "mount", "setuid",
            ] {
                assert!(
                    !json.contains(&format!(r#""{banned}""#)),
                    "{banned} must not be allowed"
                );
            }
        }
        assert!(!filter_json(SyscallProfile::ReadOnly).contains("unlinkat"));
        assert!(filter_json(SyscallProfile::FileMutating).contains("unlinkat"));
    }
}
