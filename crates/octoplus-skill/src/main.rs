//! `octoplus-skill` binary. Order matters (FR-KER-05, NFR-SEC-04):
//!
//! 1. parse the trusted command line;
//! 2. resource limits;
//! 3. Landlock on exactly the declared folders (TCP denied);
//! 4. seccomp allowlist;
//! 5. only then read the untrusted arguments from stdin;
//! 6. run and print one output line.
//!
//! Exit codes: 0 = an output line was written (it may report a skill error);
//! 3 = the sandbox could not be applied (nothing was read or done);
//! 2 = bad command line or unreadable input.

use std::io::{Read, Write};
use std::process::ExitCode;

use octoplus_skill::{Invocation, MAX_ARGS_BYTES, lock_down, output, run};

#[allow(clippy::print_stderr)] // a child without a logger: stderr is collected by C6, never user content
fn main() -> ExitCode {
    let inv = match Invocation::parse(std::env::args_os().skip(1)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("octoplus-skill: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = lock_down(&inv) {
        eprintln!("octoplus-skill: sandbox not applied: {e}");
        return ExitCode::from(3);
    }
    let mut buf = Vec::new();
    let limit = u64::try_from(MAX_ARGS_BYTES + 1).unwrap_or(u64::MAX);
    if std::io::stdin().take(limit).read_to_end(&mut buf).is_err() || buf.len() > MAX_ARGS_BYTES {
        eprintln!("octoplus-skill: arguments unreadable or too large");
        return ExitCode::from(2);
    }
    let result = serde_json::from_slice(&buf)
        .map_err(|_| octoplus_skill::SkillError::BadArgs)
        .and_then(|args| run(&inv, &args));
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", output(&result))
        .and_then(|()| out.flush())
        .is_err()
    {
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}
