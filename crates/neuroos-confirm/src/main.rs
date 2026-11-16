//! `neuroos-confirm` binary: C6 starts it as its own child, writes one request
//! to stdin, and reads one decision line from stdout (Architecture §8.4).
//!
//! Exit codes: 0 = a decision was written; 1 = bad request, I/O error or a
//! failed self-test (C6 treats anything but a valid decision line as "not approved").
//!
//! `--self-test <scenario>` runs scripted checks against the real window
//! (design.md §6.4 rules; security.md §8 "Dialog safety"). Scenarios:
//! `enter`, `allow-early`, `escape`, `expire`, `edit:<text>`, `danger`,
//! `screenshot:<png path>`.

use std::io::{Read, Write};
use std::process::ExitCode;

use neuroos_confirm::protocol::{
    DialogDecision, DialogRequest, MAX_REQUEST_BYTES, PROTOCOL_VERSION,
};
use neuroos_confirm::ui::{self, SelfTest};

fn parse_self_test(args: &[String]) -> Result<Option<SelfTest>, String> {
    match args {
        [] => Ok(None),
        [flag, scenario] if flag == "--self-test" => {
            let t = match scenario.as_str() {
                "enter" => SelfTest::Enter,
                "allow-early" => SelfTest::AllowEarlyThenLate,
                "escape" => SelfTest::Escape,
                "expire" => SelfTest::Expire,
                "danger" => SelfTest::DangerPhrase,
                s if s.starts_with("edit:") => SelfTest::EditQuery(s["edit:".len()..].to_owned()),
                s if s.starts_with("screenshot:") => {
                    SelfTest::Screenshot(s["screenshot:".len()..].to_owned())
                }
                _ => return Err("unknown self-test scenario".into()),
            };
            Ok(Some(t))
        }
        _ => Err("usage: neuroos-confirm [--self-test <scenario>]".into()),
    }
}

fn read_request() -> Result<DialogRequest, String> {
    let mut buf = Vec::new();
    let limit = u64::try_from(MAX_REQUEST_BYTES + 1).unwrap_or(u64::MAX);
    std::io::stdin()
        .take(limit)
        .read_to_end(&mut buf)
        .map_err(|e| format!("stdin: {e}"))?;
    DialogRequest::parse(&buf).map_err(|e| e.to_string())
}

#[allow(clippy::print_stderr)] // a child without a logger: stderr goes to C6's journal
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let self_test = match parse_self_test(&args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("neuroos-confirm: {e}");
            return ExitCode::from(2);
        }
    };
    let req = match read_request() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("neuroos-confirm: rejected request: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (outcome, failures) = ui::run(&req, self_test);
    let line = DialogDecision {
        version: PROTOCOL_VERSION,
        action_id: req.action_id.clone(),
        decision: outcome.decision,
        query: outcome.query,
    }
    .to_line();
    let Ok(line) = line else {
        eprintln!("neuroos-confirm: cannot encode decision");
        return ExitCode::FAILURE;
    };
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        return ExitCode::FAILURE;
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        for f in failures {
            eprintln!("neuroos-confirm: self-test failed: {f}");
        }
        ExitCode::FAILURE
    }
}
