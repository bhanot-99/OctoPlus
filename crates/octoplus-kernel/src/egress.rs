//! Outbound egress ledger (Architecture §8.5, FR-KER-07).
//!
//! Stores hash-chained outbound request records at `<data_dir>/audit/egress.jsonl`.
//! Contains `sent_text` which is user content (0600 mode; never logged in tracing at info+).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chain::{AppendInfo, ChainError, ChainedLog, VerifyReport};

/// Structured record written to `egress.jsonl` (excluding `seq` and `prev_hash`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressRecordData {
    pub at_ns: u64,
    pub action_id: String,
    pub host: String,
    pub sent_text: String,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

/// Outbound egress ledger manager storing hash-chained records.
#[derive(Debug)]
pub struct EgressLog {
    path: PathBuf,
    log: ChainedLog,
}

impl EgressLog {
    /// Opens the egress ledger at `<audit_dir>/egress.jsonl` with default rotation limits.
    ///
    /// # Errors
    /// Returns [`ChainError`] on I/O or corruption errors.
    pub fn open(audit_dir: impl AsRef<Path>) -> Result<Self, ChainError> {
        let path = audit_dir.as_ref().join("egress.jsonl");
        let log = ChainedLog::open_default(&path)?;
        Ok(Self { path, log })
    }

    /// Opens the egress ledger with custom rotation parameters.
    ///
    /// # Errors
    /// Returns [`ChainError`] on I/O or corruption errors.
    pub fn open_with_limits(
        audit_dir: impl AsRef<Path>,
        max_bytes: u64,
        keep: usize,
    ) -> Result<Self, ChainError> {
        let path = audit_dir.as_ref().join("egress.jsonl");
        let log = ChainedLog::open(&path, max_bytes, keep)?;
        Ok(Self { path, log })
    }

    /// Appends an outbound request record to `egress.jsonl`.
    ///
    /// # Errors
    /// Returns [`ChainError`] if disk append or sync fails.
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &mut self,
        at_ns: u64,
        action_id: &str,
        host: &str,
        sent_text: &str,
        bytes_out: u64,
        bytes_in: u64,
    ) -> Result<AppendInfo, ChainError> {
        let record = EgressRecordData {
            at_ns,
            action_id: action_id.to_string(),
            host: host.to_string(),
            sent_text: sent_text.to_string(),
            bytes_out,
            bytes_in,
        };
        self.log.append(&record)
    }

    /// Lists the most recent `limit` egress records, oldest first (newest last).
    ///
    /// # Errors
    /// Returns [`ChainError`] on read error.
    pub fn list(
        &mut self,
        limit: usize,
    ) -> Result<Vec<octoplus_proto::v1::EgressRecord>, ChainError> {
        let raw_lines = self.log.tail(limit)?;
        let mut proto_records = Vec::with_capacity(raw_lines.len());

        for line in raw_lines {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                let at_ns = val.get("at_ns").and_then(|v| v.as_u64()).unwrap_or(0);
                let host = val
                    .get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let sent_text = val
                    .get("sent_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let bytes_out = val.get("bytes_out").and_then(|v| v.as_u64()).unwrap_or(0);
                let bytes_in = val.get("bytes_in").and_then(|v| v.as_u64()).unwrap_or(0);

                proto_records.push(octoplus_proto::v1::EgressRecord {
                    at_ns,
                    host,
                    sent_text,
                    bytes_out,
                    bytes_in,
                });
            }
        }

        Ok(proto_records)
    }

    /// Verifies the cryptographic integrity of the egress ledger.
    #[must_use]
    pub fn verify(&self) -> VerifyReport {
        ChainedLog::verify(&self.path, self.log.head_hash())
    }

    /// Returns the current in-memory head hash.
    #[must_use]
    pub fn head_hash(&self) -> Option<[u8; 32]> {
        self.log.head_hash()
    }

    /// Returns the next sequence number.
    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.log.next_seq()
    }

    /// Returns the path to `egress.jsonl`.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_record_shape_and_listing() {
        let temp = tempfile::tempdir().unwrap();
        let mut log = EgressLog::open(temp.path()).unwrap();

        let info = log
            .append(
                1_700_000_000,
                "action-001",
                "api.weather.com",
                "GET /v1/forecast?city=Seattle",
                512,
                4096,
            )
            .unwrap();
        assert_eq!(info.seq, 1);

        let content = std::fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let val: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(val["seq"], 1);
        assert_eq!(val["at_ns"], 1_700_000_000);
        assert_eq!(val["action_id"], "action-001");
        assert_eq!(val["host"], "api.weather.com");
        assert_eq!(val["sent_text"], "GET /v1/forecast?city=Seattle");
        assert_eq!(val["bytes_out"], 512);
        assert_eq!(val["bytes_in"], 4096);
        assert!(val["prev_hash"].is_string());

        let records = log.list(10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].host, "api.weather.com");
        assert_eq!(records[0].bytes_out, 512);
        assert_eq!(records[0].bytes_in, 4096);

        let report = log.verify();
        assert!(report.chain_ok);
        assert_eq!(report.records_checked, 1);
    }
}
