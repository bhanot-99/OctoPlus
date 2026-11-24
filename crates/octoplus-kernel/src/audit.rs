//! Audit action log for capability execution tracking (Architecture §8.5, FR-KER-06).
//!
//! Stores hash-chained `ATTEMPT` and `RESULT` records at `<data_dir>/audit/actions.jsonl`.
//! Arguments are NEVER stored, only their SHA-256 hash.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::chain::{AppendInfo, ChainError, ChainedLog, VerifyReport, format_hex};

/// Action record kind: initial attempt before action, or final outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActionKind {
    /// Recorded immediately before dispatch (mandatory gate, R0-5).
    Attempt,
    /// Recorded after execution completes or fails.
    Result,
}

/// Capability safety tier recorded in the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActionTier {
    Safe,
    Review,
    Dangerous,
    None,
}

impl From<crate::registry::Tier> for ActionTier {
    fn from(tier: crate::registry::Tier) -> Self {
        match tier {
            crate::registry::Tier::Safe => Self::Safe,
            crate::registry::Tier::Review => Self::Review,
            crate::registry::Tier::Dangerous => Self::Dangerous,
        }
    }
}

/// Origin source of the capability request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionOrigin {
    Voice,
    Cli,
    NudgeReply,
    Unspecified,
}

/// Final outcome status of an executed or rejected action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionStatus {
    Executed,
    Rejected,
    DeniedPolicy,
    Expired,
    Failed,
}

/// Structured record written to `actions.jsonl` (excluding `seq` and `prev_hash`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub at_ns: u64,
    pub kind: ActionKind,
    pub action_id: String,
    pub skill: String,
    pub tier: ActionTier,
    pub taint: u32,
    pub origin: ActionOrigin,
    pub args_hash: String,
    pub status: Option<ActionStatus>,
}

/// Action audit log manager storing hash-chained records.
#[derive(Debug)]
pub struct ActionLog {
    path: PathBuf,
    log: ChainedLog,
}

impl ActionLog {
    /// Opens the action audit log at `<audit_dir>/actions.jsonl` with default rotation limits.
    ///
    /// # Errors
    /// Returns [`ChainError`] on I/O or corruption errors.
    pub fn open(audit_dir: impl AsRef<Path>) -> Result<Self, ChainError> {
        let path = audit_dir.as_ref().join("actions.jsonl");
        let log = ChainedLog::open_default(&path)?;
        Ok(Self { path, log })
    }

    /// Opens the action audit log with custom rotation parameters.
    ///
    /// # Errors
    /// Returns [`ChainError`] on I/O or corruption errors.
    pub fn open_with_limits(
        audit_dir: impl AsRef<Path>,
        max_bytes: u64,
        keep: usize,
    ) -> Result<Self, ChainError> {
        let path = audit_dir.as_ref().join("actions.jsonl");
        let log = ChainedLog::open(&path, max_bytes, keep)?;
        Ok(Self { path, log })
    }

    /// Records an `ATTEMPT` entry before action execution.
    ///
    /// # Errors
    /// Returns [`ChainError`] if disk append or sync fails.
    #[allow(clippy::too_many_arguments)]
    pub fn attempt(
        &mut self,
        at_ns: u64,
        action_id: &str,
        skill: &str,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: &str,
    ) -> Result<AppendInfo, ChainError> {
        let record = ActionRecord {
            at_ns,
            kind: ActionKind::Attempt,
            action_id: action_id.to_string(),
            skill: skill.to_string(),
            tier,
            taint,
            origin,
            args_hash: args_hash.to_string(),
            status: None,
        };
        self.log.append(&record)
    }

    /// Records a `RESULT` entry after action execution completes.
    ///
    /// # Errors
    /// Returns [`ChainError`] if disk append or sync fails.
    #[allow(clippy::too_many_arguments)]
    pub fn result(
        &mut self,
        at_ns: u64,
        action_id: &str,
        skill: &str,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: &str,
        status: ActionStatus,
    ) -> Result<AppendInfo, ChainError> {
        let record = ActionRecord {
            at_ns,
            kind: ActionKind::Result,
            action_id: action_id.to_string(),
            skill: skill.to_string(),
            tier,
            taint,
            origin,
            args_hash: args_hash.to_string(),
            status: Some(status),
        };
        self.log.append(&record)
    }

    /// Returns the last `limit` parsed audit records for RPC queries.
    ///
    /// # Errors
    /// Returns [`ChainError`] on read error.
    pub fn tail(
        &mut self,
        limit: usize,
    ) -> Result<Vec<octoplus_proto::v1::AuditRecord>, ChainError> {
        let raw_lines = self.log.tail(limit)?;
        let mut proto_records = Vec::with_capacity(raw_lines.len());

        for line in raw_lines {
            let line_bytes = line.trim_end_matches('\n').as_bytes();
            let hash = sha2::Sha256::digest(line_bytes);
            let record_hash = format_hex(&hash.into());

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                let seq = val.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
                let at_ns = val.get("at_ns").and_then(|v| v.as_u64()).unwrap_or(0);
                let kind = val
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let skill = val
                    .get("skill")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = val
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tier = val
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let taint = val
                    .get("taint")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                let origin = val
                    .get("origin")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                proto_records.push(octoplus_proto::v1::AuditRecord {
                    seq,
                    at_ns,
                    kind,
                    skill,
                    status,
                    record_hash,
                    tier,
                    taint,
                    origin,
                });
            }
        }

        Ok(proto_records)
    }

    /// Verifies the cryptographic integrity of the action audit log.
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

    /// Returns the path to `actions.jsonl`.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_shape_and_no_args_stored() {
        let temp = tempfile::tempdir().unwrap();
        let mut log = ActionLog::open(temp.path()).unwrap();

        let raw_arg_text = "secret_password_12345_do_not_store";
        let args_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        let info1 = log
            .attempt(
                1_000_000,
                "018f-test-id",
                "system.cleanup",
                ActionTier::Review,
                0,
                ActionOrigin::Voice,
                args_hash,
            )
            .unwrap();
        assert_eq!(info1.seq, 1);

        let info2 = log
            .result(
                2_000_000,
                "018f-test-id",
                "system.cleanup",
                ActionTier::Review,
                0,
                ActionOrigin::Voice,
                args_hash,
                ActionStatus::Executed,
            )
            .unwrap();
        assert_eq!(info2.seq, 2);

        let content = std::fs::read_to_string(log.path()).unwrap();
        // Assert raw argument content is NEVER stored
        assert!(
            !content.contains(raw_arg_text),
            "argument content must never be stored"
        );

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        // Check attempt line shape
        let v1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v1["seq"], 1);
        assert_eq!(v1["at_ns"], 1_000_000);
        assert_eq!(v1["kind"], "ATTEMPT");
        assert_eq!(v1["action_id"], "018f-test-id");
        assert_eq!(v1["skill"], "system.cleanup");
        assert_eq!(v1["tier"], "REVIEW");
        assert_eq!(v1["taint"], 0);
        assert_eq!(v1["origin"], "VOICE");
        assert_eq!(v1["args_hash"], args_hash);
        assert!(v1["status"].is_null());
        assert!(v1["prev_hash"].is_string());

        // Check result line shape
        let v2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(v2["seq"], 2);
        assert_eq!(v2["kind"], "RESULT");
        assert_eq!(v2["status"], "EXECUTED");

        // Verify chain integrity
        let report = log.verify();
        assert!(report.chain_ok);
        assert_eq!(report.records_checked, 2);
    }
}
