//! `octoplusctl run <capability> [--args JSON]` — asks C6 to run one skill
//! (FR-KER-01…06). Everything goes through the kernel: tier, dialog for REVIEW,
//! audit records, sandboxed child. The CLI cannot approve anything itself (R0-7).
//!
//! Exit codes (design.md §7): 0 executed, 4 denied (by policy or by the user),
//! 1 failure (expired, skill failed, kernel unreachable), 2 usage.

use std::path::Path;
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::Client;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{DispatchRequest, DispatchStatus, ErrorCode, Origin, Taint};

use crate::style::Styler;

/// Taint bit for text typed by the user (Architecture §7.4).
const USER_PROVIDED: u32 = 1 << 1;
/// Taint bit for web content (testing the escalation by hand).
const EXTERNAL_UNTRUSTED: u32 = 1 << 0;
/// Longer than the kernel's own dispatch deadline (PRD §6.1, 155 s).
const DISPATCH_WAIT: Duration = Duration::from_secs(160);

/// The request this command sends. Taint can only be raised from the CLI.
#[must_use]
pub fn request(capability: &str, args: Option<&str>, web_tainted: bool) -> DispatchRequest {
    let flags = USER_PROVIDED | if web_tainted { EXTERNAL_UNTRUSTED } else { 0 };
    DispatchRequest {
        skill: capability.to_owned(),
        args_json: args.unwrap_or("{}").to_owned(),
        context_taint: Some(Taint { flags }),
        origin: Origin::Cli as i32,
    }
}

/// Exit code for a dispatch status.
#[must_use]
pub fn exit_code(status: i32) -> i32 {
    match DispatchStatus::try_from(status) {
        Ok(DispatchStatus::Executed) => 0,
        Ok(DispatchStatus::Rejected) => 4,
        _ => 1,
    }
}

/// Runs the command.
pub async fn run_skill(
    runtime_root: &Path,
    capability: &str,
    args: Option<&str>,
    web_tainted: bool,
    styler: Styler,
) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Kernel, "kernel");
    let Ok(client) = Client::connect(&sock, 1024 * 1024).await else {
        eprintln!("{}", styler.error("could not connect to octoplus-kernel"));
        eprintln!(
            "{}",
            styler.hint("start it with 'systemctl --user start octoplus-kernel'")
        );
        return 1;
    };
    let req = request(capability, args, web_tainted);
    match client.call(Body::DispatchRequest(req), DISPATCH_WAIT).await {
        Ok(Body::DispatchResult(r)) => {
            let status = DispatchStatus::try_from(r.status).unwrap_or(DispatchStatus::Unspecified);
            match status {
                DispatchStatus::Executed => println!("{}", r.result_json),
                DispatchStatus::Rejected => eprintln!("{}", styler.error("you denied this action")),
                DispatchStatus::Expired => {
                    eprintln!("{}", styler.error("the confirmation expired"))
                }
                _ => eprintln!(
                    "{}",
                    styler.error("the skill failed (see `octoplusctl audit tail`)")
                ),
            }
            exit_code(r.status)
        }
        Ok(_) => {
            eprintln!("{}", styler.error("unexpected reply from octoplus-kernel"));
            1
        }
        Err(e) => {
            let wire = e.to_wire();
            eprintln!("{}", styler.error(&wire.message));
            match ErrorCode::try_from(wire.code) {
                Ok(
                    ErrorCode::PermissionDenied
                    | ErrorCode::InvalidRequest
                    | ErrorCode::ResourceExhausted,
                ) => 4,
                _ => 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taint_is_user_and_only_raised() {
        let r = request("system.read_metadata", None, false);
        assert_eq!(r.context_taint.unwrap().flags, USER_PROVIDED);
        assert_eq!(r.args_json, "{}");
        assert_eq!(r.origin, Origin::Cli as i32);
        let r = request("x", Some(r#"{"a":1}"#), true);
        assert_eq!(
            r.context_taint.unwrap().flags,
            USER_PROVIDED | EXTERNAL_UNTRUSTED
        );
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(DispatchStatus::Executed as i32), 0);
        assert_eq!(exit_code(DispatchStatus::Rejected as i32), 4);
        assert_eq!(exit_code(DispatchStatus::Expired as i32), 1);
        assert_eq!(exit_code(DispatchStatus::Failed as i32), 1);
        assert_eq!(exit_code(99), 1);
    }
}
