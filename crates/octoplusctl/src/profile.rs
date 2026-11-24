//! `octoplusctl profile list | add | remove` (FR-PRO-04, P5-S07).
//!
//! Reading goes straight to C3. Writing never does: `add` and `remove` go through the safety
//! kernel as `profile.remember` and `profile.forget`, so every change is audited and `remove`
//! shows the confirmation dialog (R0-5, R0-8). The CLI cannot save a fact on its own.

use std::path::Path;
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::Client;
use octoplus_proto::v1::ProfileListRequest;
use octoplus_proto::v1::envelope::Body;
use serde_json::json;

use crate::ProfileCommands;
use crate::run::run_skill;
use crate::style::Styler;

/// Longest fact accepted on the command line (the skill's manifest enforces the same bound).
pub const MAX_FACT_BYTES: usize = 512;

/// The `profile.remember` arguments for typed text; `None` for an empty, over-long or
/// control-character fact (nothing is sent in that case).
#[must_use]
pub fn remember_args(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.len() > MAX_FACT_BYTES || t.chars().any(char::is_control) {
        return None;
    }
    Some(json!({ "fact": t }).to_string())
}

/// The `profile.forget` arguments for a fact id.
#[must_use]
pub fn forget_args(id: u64) -> Option<String> {
    (id >= 1).then(|| json!({ "fact_id": id }).to_string())
}

/// Runs `octoplusctl profile …` and returns the exit code.
pub async fn run_profile(runtime_root: &Path, command: ProfileCommands, styler: Styler) -> i32 {
    match command {
        ProfileCommands::List { relevant_to, json } => {
            list(runtime_root, relevant_to, json, styler).await
        }
        ProfileCommands::Add { text } => {
            let Some(args) = remember_args(&text) else {
                eprintln!(
                    "{}",
                    styler.error("a fact must be 1 to 512 bytes of plain text")
                );
                return 2;
            };
            run_skill(runtime_root, "profile.remember", Some(&args), false, styler).await
        }
        ProfileCommands::Remove { id } => {
            let Some(args) = forget_args(id) else {
                eprintln!("{}", styler.error("fact ids start at 1"));
                return 2;
            };
            run_skill(runtime_root, "profile.forget", Some(&args), false, styler).await
        }
    }
}

async fn list(
    runtime_root: &Path,
    relevant_to: Option<String>,
    as_json: bool,
    styler: Styler,
) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Storage, "storage");
    let Ok(client) = Client::connect(&sock, 1024 * 1024).await else {
        eprintln!("{}", styler.error("could not connect to octoplus-storage"));
        return 1;
    };
    let req = ProfileListRequest {
        relevant_to: relevant_to.unwrap_or_default(),
    };
    match client
        .call(Body::ProfileListRequest(req), Duration::from_secs(5))
        .await
    {
        Ok(Body::ProfileFacts(f)) => {
            for fact in &f.facts {
                if as_json {
                    println!(
                        "{}",
                        json!({
                            "id": fact.id,
                            "text": fact.text,
                            "saved_at_ns": fact.saved_at_ns,
                            "taint": fact.taint.as_ref().map_or(0, |t| t.flags),
                        })
                    );
                } else {
                    println!("{}  {}", fact.id, fact.text);
                }
            }
            0
        }
        Ok(_) | Err(_) => {
            eprintln!("{}", styler.error("could not list the saved facts"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_args_trim_and_refuse_bad_text() {
        assert_eq!(
            remember_args("  my dog is Rex ").as_deref(),
            Some(r#"{"fact":"my dog is Rex"}"#)
        );
        // JSON-significant characters stay data.
        assert_eq!(
            remember_args(r#"say "hi" \ ok"#).as_deref(),
            Some(r#"{"fact":"say \"hi\" \\ ok"}"#)
        );
        for bad in ["", "   ", "a\nb", "a\u{0}b", &"x".repeat(513)] {
            assert_eq!(remember_args(bad), None, "{bad:?}");
        }
        assert!(remember_args(&"x".repeat(512)).is_some());
    }

    #[test]
    fn forget_args_need_an_id() {
        assert_eq!(forget_args(0), None);
        assert_eq!(forget_args(9).as_deref(), Some(r#"{"fact_id":9}"#));
    }
}
