//! `octoplusctl nudges list | enable | disable | test` (FR-NUD-02, FR-NUD-09, P6-S05).
//!
//! All four go to C5's knowledge socket. None of them can make a nudge act: `test` only shows
//! the rule's own notification line now, and switching a rule on or off is logged in C3.

use std::path::Path;
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::{Client, IpcError};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{ErrorCode, NudgeListRequest, NudgeRuleState, NudgeSetEnabled, NudgeTest};
use serde_json::json;

use crate::NudgesCommands;
use crate::style::Styler;

const DEADLINE: Duration = Duration::from_secs(5);

/// One line of `nudges list`.
#[must_use]
pub fn format_rule(r: &NudgeRuleState, now_ns: u64) -> String {
    let state = if !r.enabled {
        "off".to_string()
    } else if r.snoozed_until_ns > now_ns {
        let mins = (r.snoozed_until_ns - now_ns) / 60_000_000_000;
        format!("snoozed {mins} min")
    } else {
        "on".to_string()
    };
    let last = if r.last_fired_ns == 0 {
        "never".to_string()
    } else {
        let ago = now_ns.saturating_sub(r.last_fired_ns) / 60_000_000_000;
        format!("{ago} min ago")
    };
    format!("{:<20} {:<14} last: {}", r.rule_id, state, last)
}

fn fail(styler: Styler, e: &IpcError) -> i32 {
    let wire = e.to_wire();
    eprintln!("{}", styler.error(&wire.message));
    match ErrorCode::try_from(wire.code) {
        Ok(ErrorCode::NotFound | ErrorCode::InvalidRequest) => 2,
        _ => 1,
    }
}

/// Runs `octoplusctl nudges …` and returns the exit code.
pub async fn run_nudges(runtime_root: &Path, command: NudgesCommands, styler: Styler) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Knowledge, "knowledge");
    let Ok(client) = Client::connect(&sock, 1024 * 1024).await else {
        eprintln!(
            "{}",
            styler.error("could not connect to octoplus-knowledge")
        );
        return 1;
    };
    match command {
        NudgesCommands::List { json } => {
            match client
                .call(Body::NudgeListRequest(NudgeListRequest {}), DEADLINE)
                .await
            {
                Ok(Body::NudgeList(l)) => {
                    let now = octoplus_common::time::now_ns().unwrap_or(0);
                    for r in &l.rules {
                        if json {
                            println!(
                                "{}",
                                json!({
                                    "rule": r.rule_id,
                                    "enabled": r.enabled,
                                    "snoozed_until_ns": r.snoozed_until_ns,
                                    "last_fired_ns": r.last_fired_ns,
                                })
                            );
                        } else {
                            println!("{}", format_rule(r, now));
                        }
                    }
                    0
                }
                Ok(_) => {
                    eprintln!(
                        "{}",
                        styler.error("unexpected reply from octoplus-knowledge")
                    );
                    1
                }
                Err(e) => fail(styler, &e),
            }
        }
        NudgesCommands::Enable { rule } => set(&client, rule, true, styler).await,
        NudgesCommands::Disable { rule } => set(&client, rule, false, styler).await,
        NudgesCommands::Test { rule, voice } => {
            match client
                .call(
                    Body::NudgeTest(NudgeTest {
                        rule_id: rule,
                        speak: voice,
                    }),
                    DEADLINE,
                )
                .await
            {
                Ok(_) => {
                    println!("test notification sent");
                    0
                }
                Err(e) => fail(styler, &e),
            }
        }
    }
}

async fn set(client: &Client, rule: String, enabled: bool, styler: Styler) -> i32 {
    match client
        .call(
            Body::NudgeSetEnabled(NudgeSetEnabled {
                rule_id: rule.clone(),
                enabled,
            }),
            DEADLINE,
        )
        .await
    {
        Ok(_) => {
            println!("{rule}: {}", if enabled { "on" } else { "off" });
            0
        }
        Err(e) => fail(styler, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(enabled: bool, snoozed: u64, fired: u64) -> NudgeRuleState {
        NudgeRuleState {
            rule_id: "disk_nearly_full".into(),
            enabled,
            snoozed_until_ns: snoozed,
            last_fired_ns: fired,
        }
    }

    #[test]
    fn rule_lines() {
        let now = 10_000 * 1_000_000_000;
        assert!(format_rule(&rule(true, 0, 0), now).ends_with("on             last: never"));
        assert!(format_rule(&rule(false, 0, 0), now).contains(" off "));
        let snoozed = format_rule(&rule(true, now + 120 * 60_000_000_000, 0), now);
        assert!(snoozed.contains("snoozed 120 min"), "{snoozed}");
        let fired = format_rule(&rule(true, 0, now - 90 * 60_000_000_000), now);
        assert!(fired.ends_with("last: 90 min ago"), "{fired}");
        // Disabled wins over a stale snooze.
        assert!(format_rule(&rule(false, now + 1, 0), now).contains(" off "));
    }
}
