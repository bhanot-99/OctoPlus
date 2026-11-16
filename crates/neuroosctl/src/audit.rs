//! Implementation of `neuroosctl audit` (FR-KER-06, FR-KER-07, design.md §7).

use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;
use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{AuditRecord, AuditTailRequest, AuditVerifyRequest};
use serde::{Deserialize, Serialize};

use crate::AuditCommands;
use crate::style::{Styler, middle_truncate};

/// JSON envelope for `neuroosctl audit tail --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTailJson {
    pub schema: u32,
    pub records: Vec<AuditRecordJson>,
}

/// JSON record for `neuroosctl audit tail --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecordJson {
    pub seq: u64,
    pub at_ns: u64,
    pub kind: String,
    pub skill: String,
    pub status: String,
    pub record_hash: String,
    pub tier: String,
    pub taint: u32,
    pub origin: String,
}

/// JSON output for `neuroosctl audit verify --json`.
#[derive(Debug, Serialize)]
pub struct AuditVerifyJson {
    pub schema: u32,
    pub chain_ok: bool,
    pub records_checked: u64,
    pub first_bad_seq: Option<u64>,
}

/// Formats nanoseconds since UNIX epoch into human-readable local timestamp `YYYY-MM-DD HH:MM:SS`.
#[must_use]
pub fn format_timestamp(at_ns: u64) -> String {
    if let Ok(ts) = Timestamp::from_nanosecond(i128::from(at_ns)) {
        let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
        return zoned.strftime("%Y-%m-%d %H:%M:%S").to_string();
    }
    format_timestamp_utc(at_ns)
}

/// Fallback UTC timestamp formatting if nanoseconds are out of range.
#[must_use]
pub fn format_timestamp_utc(at_ns: u64) -> String {
    let secs = at_ns / 1_000_000_000;
    let days = (secs / 86400) as i64;
    let rem_secs = (secs % 86400) as u32;
    let hours = rem_secs / 3600;
    let mins = (rem_secs % 3600) / 60;
    let s = rem_secs % 60;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {hours:02}:{mins:02}:{s:02}")
}

/// Returns the formatted design.md §7 tier label with optional taint tags (e.g. `[REVIEW] [WEB]`).
#[must_use]
pub fn format_tier(tier: &str, taint: u32) -> String {
    let base = match tier {
        "SAFE" => "[SAFE]",
        "REVIEW" => "[REVIEW]",
        "DANGEROUS" => "[DANGEROUS]",
        "" | "NONE" => "—",
        other => other,
    };
    if (taint & 1) != 0 {
        if base == "—" {
            "[WEB]".to_string()
        } else {
            format!("{base} [WEB]")
        }
    } else {
        base.to_string()
    }
}

/// Returns the formatted design.md §7 tier label for a registered skill.
#[must_use]
pub fn skill_tier_label(skill: &str) -> &'static str {
    match skill {
        "system.read_metadata"
        | "system.disk_report"
        | "calendar.read"
        | "notes.search"
        | "privacy.pause" => "[SAFE]",
        "profile.remember" | "profile.forget" | "memory.forget" | "notes.append"
        | "system.cleanup" | "web.fetch" | "web.search" => "[REVIEW]",
        "shell.exec" => "[DANGEROUS]",
        _ => "—",
    }
}

/// Formats a single audit record row for text display.
#[must_use]
pub fn format_audit_row(record: &AuditRecord, styler: &Styler) -> String {
    let time_str = format_timestamp(record.at_ns);
    let tier_label = format_tier(&record.tier, record.taint);
    let status_label = if record.status.is_empty() {
        "—"
    } else {
        &record.status
    };
    let hash_str = styler.dim(&middle_truncate(&record.record_hash, 13));

    format!(
        "{:<6}{:<21}{:<10}{:<25}{:<18}{:<16}{}",
        record.seq, time_str, record.kind, record.skill, tier_label, status_label, hash_str,
    )
}

/// Executes audit subcommands (`audit tail`, `audit verify`).
pub async fn run_audit(runtime_root: &Path, command: AuditCommands, styler: Styler) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Kernel, "kernel");
    let client = match Client::connect(&sock, 1024 * 1024).await {
        Ok(c) => c,
        Err(_) => {
            eprintln!("{}", styler.error("could not connect to neuroos-kernel"));
            eprintln!(
                "{}",
                styler.hint("start the service with 'systemctl --user start neuroos-kernel'")
            );
            return 1;
        }
    };

    match command {
        AuditCommands::Tail { limit, json, quiet } => {
            run_audit_tail(&client, limit, json, quiet, styler).await
        }
        AuditCommands::Verify { json, quiet } => {
            run_audit_verify(&client, json, quiet, styler).await
        }
    }
}

async fn run_audit_tail(
    client: &Client,
    limit: u32,
    json: bool,
    quiet: bool,
    styler: Styler,
) -> i32 {
    let req = AuditTailRequest { limit };
    let res = client
        .call(Body::AuditTailRequest(req), Duration::from_secs(5))
        .await;

    let records = match res {
        Ok(Body::AuditRecords(records)) => records.records,
        Ok(_) => {
            eprintln!(
                "{}",
                styler.error("unexpected response from neuroos-kernel for audit tail")
            );
            return 1;
        }
        Err(e) => {
            eprintln!(
                "{}",
                styler.error(&format!("failed to fetch audit records: {e}"))
            );
            return 1;
        }
    };

    if json {
        let json_records = records
            .into_iter()
            .map(|r| AuditRecordJson {
                seq: r.seq,
                at_ns: r.at_ns,
                kind: r.kind,
                skill: r.skill,
                status: r.status,
                record_hash: r.record_hash,
                tier: r.tier,
                taint: r.taint,
                origin: r.origin,
            })
            .collect();
        let payload = AuditTailJson {
            schema: 1,
            records: json_records,
        };
        if let Ok(out) = serde_json::to_string_pretty(&payload) {
            println!("{out}");
        }
        return 0;
    }

    if !quiet {
        println!(
            "{}{}{}{}{}{}{}",
            styler.bold(&format!("{:<6}", "SEQ")),
            styler.bold(&format!("{:<21}", "WHEN (local)")),
            styler.bold(&format!("{:<10}", "ACTION")),
            styler.bold(&format!("{:<25}", "SKILL")),
            styler.bold(&format!("{:<18}", "TIER")),
            styler.bold(&format!("{:<16}", "STATUS")),
            styler.bold("HASH"),
        );

        for record in &records {
            println!("{}", format_audit_row(record, &styler));
        }
    }

    0
}

async fn run_audit_verify(client: &Client, json: bool, quiet: bool, styler: Styler) -> i32 {
    let req = AuditVerifyRequest {};
    let res = client
        .call(Body::AuditVerifyRequest(req), Duration::from_secs(10))
        .await;

    let report = match res {
        Ok(Body::AuditVerifyResult(report)) => report,
        Ok(_) => {
            eprintln!(
                "{}",
                styler.error("unexpected response from neuroos-kernel for audit verify")
            );
            return 1;
        }
        Err(e) => {
            eprintln!(
                "{}",
                styler.error(&format!("failed to verify audit log: {e}"))
            );
            return 1;
        }
    };

    let first_bad_seq = if report.chain_ok || report.first_bad_seq == 0 {
        None
    } else {
        Some(report.first_bad_seq)
    };

    if json {
        let payload = AuditVerifyJson {
            schema: 1,
            chain_ok: report.chain_ok,
            records_checked: report.records_checked,
            first_bad_seq,
        };
        if let Ok(out) = serde_json::to_string_pretty(&payload) {
            println!("{out}");
        }
    } else if !quiet {
        if report.chain_ok {
            println!(
                "{}",
                styler.ok(&format!(
                    "✔ Action audit log: {} records verified, chain intact",
                    report.records_checked
                ))
            );
        } else {
            let bad = first_bad_seq.unwrap_or(0);
            println!(
                "{}",
                styler.down(&format!("✖ Action audit log: chain broken at seq {bad}"))
            );
        }
    }

    if report.chain_ok { 0 } else { 3 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp_utc(0), "1970-01-01 00:00:00");
        let ns = 1_794_787_200 * 1_000_000_000;
        assert_eq!(format_timestamp_utc(ns), "2026-11-16 00:00:00");

        // format_timestamp must return system local time
        let expected_local = Timestamp::from_nanosecond(0)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(format_timestamp(0), expected_local);
    }

    #[test]
    fn test_skill_tier_label() {
        assert_eq!(skill_tier_label("system.read_metadata"), "[SAFE]");
        assert_eq!(skill_tier_label("system.cleanup"), "[REVIEW]");
        assert_eq!(skill_tier_label("shell.exec"), "[DANGEROUS]");
        assert_eq!(skill_tier_label("unknown"), "—");
    }
}
