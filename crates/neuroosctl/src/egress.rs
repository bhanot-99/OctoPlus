//! Implementation of `neuroosctl egress` (FR-KER-07, design.md §7).

use std::path::Path;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::EgressListRequest;
use neuroos_proto::v1::envelope::Body;
use serde::Serialize;

use crate::EgressCommands;
use crate::audit::format_timestamp;
use crate::style::Styler;

/// JSON envelope for `neuroosctl egress list --json`.
#[derive(Debug, Serialize)]
pub struct EgressListJson {
    pub schema: u32,
    pub records: Vec<EgressRecordJson>,
}

/// JSON record for `neuroosctl egress list --json`.
#[derive(Debug, Serialize)]
pub struct EgressRecordJson {
    pub at_ns: u64,
    pub host: String,
    pub sent_text: String,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

/// Truncates string with trailing ellipsis `…` if length exceeds `max_len`.
#[must_use]
pub fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let keep = max_len.saturating_sub(1);
        let prefix: String = s.chars().take(keep).collect();
        format!("{prefix}…")
    }
}

/// Formats byte count into human-readable representation (e.g. `412 B`, `38 KiB`).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        let kib = bytes / 1024;
        let rem = (bytes % 1024) * 10 / 1024;
        if rem == 0 {
            format!("{kib} KiB")
        } else {
            format!("{kib}.{rem} KiB")
        }
    } else if bytes < 1024 * 1024 * 1024 {
        let mib = bytes / (1024 * 1024);
        let rem = (bytes % (1024 * 1024)) * 10 / (1024 * 1024);
        if rem == 0 {
            format!("{mib} MiB")
        } else {
            format!("{mib}.{rem} MiB")
        }
    } else {
        let gib = bytes / (1024 * 1024 * 1024);
        let rem = (bytes % (1024 * 1024 * 1024)) * 10 / (1024 * 1024 * 1024);
        if rem == 0 {
            format!("{gib} GiB")
        } else {
            format!("{gib}.{rem} GiB")
        }
    }
}

/// Executes egress subcommands (`egress list`).
pub async fn run_egress(runtime_root: &Path, command: EgressCommands, styler: Styler) -> i32 {
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
        EgressCommands::List { limit, json, quiet } => {
            run_egress_list(&client, limit, json, quiet, styler).await
        }
    }
}

async fn run_egress_list(
    client: &Client,
    limit: u32,
    json: bool,
    quiet: bool,
    styler: Styler,
) -> i32 {
    let req = EgressListRequest { limit };
    let res = client
        .call(Body::EgressListRequest(req), Duration::from_secs(5))
        .await;

    let records = match res {
        Ok(Body::EgressRecords(records)) => records.records,
        Ok(_) => {
            eprintln!(
                "{}",
                styler.error("unexpected response from neuroos-kernel for egress list")
            );
            return 1;
        }
        Err(e) => {
            eprintln!(
                "{}",
                styler.error(&format!("failed to fetch egress records: {e}"))
            );
            return 1;
        }
    };

    if json {
        let json_records = records
            .into_iter()
            .map(|r| EgressRecordJson {
                at_ns: r.at_ns,
                host: r.host,
                sent_text: r.sent_text,
                bytes_out: r.bytes_out,
                bytes_in: r.bytes_in,
            })
            .collect();
        let payload = EgressListJson {
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
            "{}{}{}{}{}",
            styler.bold(&format!("{:<6}", "SEQ")),
            styler.bold(&format!("{:<21}", "TIME")),
            styler.bold(&format!("{:<20}", "HOST")),
            styler.bold(&format!("{:<42}", "SENT")),
            styler.bold("BYTES (out/in)"),
        );

        for (idx, record) in records.iter().enumerate() {
            let seq = idx + 1;
            let time_str = format_timestamp(record.at_ns);
            let sent_display = format!("[SENT] \"{}\"", truncate_str(&record.sent_text, 32));
            let bytes_display = format!(
                "{} / {}",
                format_bytes(record.bytes_out),
                format_bytes(record.bytes_in)
            );

            println!(
                "{:<6}{:<21}{:<20}{:<42}{}",
                seq, time_str, record.host, sent_display, bytes_display,
            );
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(412), "412 B");
        assert_eq!(format_bytes(38 * 1024), "38 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1 MiB");
        assert_eq!(format_bytes(1536 * 1024), "1.5 MiB");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 6), "hello…");
    }
}
