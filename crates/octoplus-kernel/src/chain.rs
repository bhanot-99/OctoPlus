//! Generic hash-chained JSON-lines log file with rotation and integrity verification.
//!
//! Satisfies FR-KER-06, FR-KER-07, rules.md R0-5, and Architecture §8.5.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Maximum file size before rotation in bytes (10 MB, Architecture §8.5).
pub const ROTATE_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum number of rotated files to retain (5, Architecture §8.5).
pub const ROTATE_KEEP: usize = 5;

/// 64-character hexadecimal representation of zero hash.
const ZERO_HASH_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Result of a durable append operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendInfo {
    /// Monotonically increasing sequence number assigned to the record.
    pub seq: u64,
    /// SHA-256 digest of the raw line bytes (excluding trailing newline).
    pub line_hash: [u8; 32],
}

/// Report produced by chain integrity verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyReport {
    /// True if all hashes, sequence numbers, and head checks passed.
    pub chain_ok: bool,
    /// Total number of valid records checked across all rotated files.
    pub records_checked: u64,
    /// Sequence number of the first failing record, or 0 if all OK.
    pub first_bad_seq: u64,
}

/// Errors occurring during chained log operations.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    /// Standard I/O failure (including ENOSPC or short writes).
    #[error("I/O error on chained log: {0}")]
    Io(#[from] std::io::Error),
    /// Unparsable or corrupted log record on disk.
    #[error("corrupt chained log")]
    Corrupt,
    /// JSON serialization failure.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Format bytes as lower-case 64-character hex string.
pub fn format_hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Computes SHA-256 of the given byte slice.
fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Formats a chained line with `"seq"` as the FIRST field and `"prev_hash"` as the LAST field.
fn format_chained_line(seq: u64, inner_json: &str, prev_hash_hex: &str) -> String {
    let inner_trimmed = inner_json.trim();
    if inner_trimmed.starts_with('{') && inner_trimmed.ends_with('}') {
        let body = &inner_trimmed[1..inner_trimmed.len() - 1];
        if body.is_empty() {
            format!("{{\"seq\":{seq},\"prev_hash\":\"{prev_hash_hex}\"}}\n")
        } else {
            format!("{{\"seq\":{seq},{body},\"prev_hash\":\"{prev_hash_hex}\"}}\n")
        }
    } else {
        format!("{{\"seq\":{seq},\"prev_hash\":\"{prev_hash_hex}\"}}\n")
    }
}

/// Opens a log file in mode 0600 with append and read permissions.
fn open_log_file(path: &Path) -> Result<File, ChainError> {
    OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(ChainError::Io)
}

/// Reads the last line of a file (if non-empty) to recover sequence number and line hash.
fn read_last_line_info(path: &Path) -> Result<Option<(u64, [u8; 32])>, ChainError> {
    if !path.exists() {
        return Ok(None);
    }
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() == 0 {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)?;
    let last_line = match content.lines().rfind(|l| !l.trim().is_empty()) {
        Some(l) => l,
        None => return Ok(None),
    };

    let val: Value = serde_json::from_str(last_line).map_err(|_| ChainError::Corrupt)?;
    let seq = val
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or(ChainError::Corrupt)?;
    val.get("prev_hash")
        .and_then(Value::as_str)
        .ok_or(ChainError::Corrupt)?;

    let hash = sha256_digest(last_line.trim_end_matches('\n').as_bytes());
    Ok(Some((seq, hash)))
}

/// Truncates an unfinished trailing line (no trailing newline) back to the last newline (BUG-036).
///
/// An unacknowledged mid-line write from disk-full/short-write is truncated away; complete
/// lines that fail JSON parsing remain corrupt.
fn clean_trailing_partial_line(path: &Path) -> Result<(), ChainError> {
    if !path.exists() {
        return Ok(());
    }
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() == 0 {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    if bytes.ends_with(b"\n") {
        return Ok(());
    }
    let valid_len = match bytes.iter().rposition(|&b| b == b'\n') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(valid_len as u64)?;
    file.sync_data()?;
    Ok(())
}

/// Hash-chained JSON-lines file writer and validator.
#[derive(Debug)]
pub struct ChainedLog {
    path: PathBuf,
    file: File,
    max_bytes: u64,
    keep: usize,
    current_bytes: u64,
    next_seq: u64,
    head_hash: [u8; 32],
    needs_recovery: bool,
}

impl ChainedLog {
    /// Opens or creates a chained log with custom rotation limits.
    ///
    /// # Errors
    /// Returns [`ChainError::Corrupt`] if the tail record is malformed.
    pub fn open(path: impl AsRef<Path>, max_bytes: u64, keep: usize) -> Result<Self, ChainError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        clean_trailing_partial_line(&path)?;

        let file = open_log_file(&path)?;
        let current_bytes = file.metadata()?.len();

        let mut next_seq = 1;
        let mut head_hash = [0u8; 32];

        if let Some((last_seq, last_hash)) = read_last_line_info(&path)? {
            next_seq = last_seq + 1;
            head_hash = last_hash;
        } else {
            // If current file is empty, check if rotated .1 file has tail info
            let rot1 = path.with_extension(format!(
                "{}.1",
                path.extension().and_then(|s| s.to_str()).unwrap_or("jsonl")
            ));
            if let Some((last_seq, last_hash)) = read_last_line_info(&rot1)? {
                next_seq = last_seq + 1;
                head_hash = last_hash;
            }
        }

        Ok(Self {
            path,
            file,
            max_bytes,
            keep,
            current_bytes,
            next_seq,
            head_hash,
            needs_recovery: false,
        })
    }

    /// Opens or creates a chained log with default Architecture §8.5 rotation parameters (10 MB, 5 files).
    pub fn open_default(path: impl AsRef<Path>) -> Result<Self, ChainError> {
        Self::open(path, ROTATE_BYTES, ROTATE_KEEP)
    }

    /// Path to rotated file `path.i`.
    fn rotated_path(&self, i: usize) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("log");
        self.path.with_file_name(format!("{file_name}.{i}"))
    }

    /// Re-opens and recovers the log state after an I/O error to prevent chain forks.
    fn recover(&mut self) -> Result<(), ChainError> {
        clean_trailing_partial_line(&self.path)?;
        self.file = open_log_file(&self.path)?;
        let current_bytes = self.file.metadata()?.len();
        self.current_bytes = current_bytes;

        if let Some((last_seq, last_hash)) = read_last_line_info(&self.path)? {
            self.next_seq = last_seq + 1;
            self.head_hash = last_hash;
        } else {
            let rot1 = self.rotated_path(1);
            if let Some((last_seq, last_hash)) = read_last_line_info(&rot1)? {
                self.next_seq = last_seq + 1;
                self.head_hash = last_hash;
            } else {
                self.next_seq = 1;
                self.head_hash = [0u8; 32];
            }
        }
        self.needs_recovery = false;
        Ok(())
    }

    /// Rotates files: `name` -> `name.1`, `name.1` -> `name.2`, up to `keep`.
    fn rotate(&mut self) -> Result<(), ChainError> {
        let _ = self.file.flush();
        let _ = self.file.sync_data();

        // Evict oldest beyond keep
        for i in (1..=self.keep).rev() {
            let src = self.rotated_path(i);
            let dst = self.rotated_path(i + 1);
            if i == self.keep {
                if src.exists() {
                    std::fs::remove_file(&src).map_err(|e| {
                        self.needs_recovery = true;
                        ChainError::Io(e)
                    })?;
                }
            } else if src.exists() {
                std::fs::rename(&src, &dst).map_err(|e| {
                    self.needs_recovery = true;
                    ChainError::Io(e)
                })?;
            }
        }

        if self.path.exists() {
            std::fs::rename(&self.path, self.rotated_path(1)).map_err(|e| {
                self.needs_recovery = true;
                ChainError::Io(e)
            })?;
        }

        self.file = open_log_file(&self.path).inspect_err(|_| {
            self.needs_recovery = true;
        })?;
        self.current_bytes = 0;
        Ok(())
    }

    /// Appends a record to the chained log, syncing it to disk before returning.
    ///
    /// # Errors
    /// Returns [`ChainError::Io`] on write or sync failure. Sets recovery flag so subsequent
    /// calls re-read tail from disk.
    pub fn append(&mut self, record: &impl Serialize) -> Result<AppendInfo, ChainError> {
        if self.needs_recovery {
            self.recover()?;
        }

        let inner_json = serde_json::to_string(record)?;
        let prev_hash_hex = if self.next_seq == 1 {
            ZERO_HASH_HEX.to_string()
        } else {
            format_hex(&self.head_hash)
        };

        let line = format_chained_line(self.next_seq, &inner_json, &prev_hash_hex);
        let line_bytes = line.as_bytes();
        let line_len = line_bytes.len() as u64;

        if self.current_bytes > 0 && self.current_bytes + line_len > self.max_bytes {
            self.rotate()?;
        }

        let line_hash = sha256_digest(line.trim_end_matches('\n').as_bytes());

        if let Err(e) = self.file.write_all(line_bytes) {
            self.needs_recovery = true;
            return Err(ChainError::Io(e));
        }

        if let Err(e) = self.file.sync_data() {
            self.needs_recovery = true;
            return Err(ChainError::Io(e));
        }

        let seq = self.next_seq;
        self.head_hash = line_hash;
        self.next_seq += 1;
        self.current_bytes += line_len;

        Ok(AppendInfo { seq, line_hash })
    }

    /// Returns the current in-memory head hash.
    #[must_use]
    pub fn head_hash(&self) -> Option<[u8; 32]> {
        if self.next_seq == 1 {
            None
        } else {
            Some(self.head_hash)
        }
    }

    /// Returns the sequence number that will be assigned to the next record.
    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Returns the last `n` raw lines across rotated files, capped at 1000, in chronological order.
    ///
    /// # Errors
    /// Returns [`ChainError::Io`] if files cannot be read.
    pub fn tail(&mut self, n: usize) -> Result<Vec<String>, ChainError> {
        let limit = n.min(1000);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let _ = self.file.flush();
        let _ = self.file.sync_data();

        let mut files = Vec::new();
        if self.path.exists() {
            files.push(self.path.clone());
        }
        for i in 1..=self.keep {
            let p = self.rotated_path(i);
            if p.exists() {
                files.push(p);
            }
        }

        let mut collected = Vec::new();
        for f in files {
            if collected.len() >= limit {
                break;
            }
            if let Ok(content) = std::fs::read_to_string(&f) {
                let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
                for &line in lines.iter().rev() {
                    collected.push(line.to_string());
                    if collected.len() >= limit {
                        break;
                    }
                }
            }
        }

        collected.reverse();
        Ok(collected)
    }

    /// Verifies the cryptographic hash-chain across all existing rotated files up to current.
    #[must_use]
    pub fn verify(path: impl AsRef<Path>, expected_head: Option<[u8; 32]>) -> VerifyReport {
        let path = path.as_ref();
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };

        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => {
                return VerifyReport {
                    chain_ok: expected_head.is_none(),
                    records_checked: 0,
                    first_bad_seq: 0,
                };
            }
        };

        // Discover rotated files in parent dir
        let mut rotated: Vec<(usize, PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    let prefix = format!("{file_name}.");
                    let parsed = name
                        .strip_prefix(&prefix)
                        .and_then(|suffix| suffix.parse::<usize>().ok());
                    if let Some(num) = parsed {
                        rotated.push((num, p));
                    }
                }
            }
        }

        // Sort rotated files descending (oldest first: .5, .4, ..., .1)
        rotated.sort_by_key(|a| std::cmp::Reverse(a.0));

        let mut files_to_check: Vec<PathBuf> = rotated.into_iter().map(|(_, p)| p).collect();
        if path.exists() {
            files_to_check.push(path.to_path_buf());
        }

        if files_to_check.is_empty() {
            return VerifyReport {
                chain_ok: expected_head.is_none(),
                records_checked: 0,
                first_bad_seq: 0,
            };
        }

        let mut records_checked: u64 = 0;
        let mut expected_seq: Option<u64> = None;
        let mut prev_line_hash: Option<[u8; 32]> = None;
        let mut last_line_hash: Option<[u8; 32]> = None;
        let mut last_seq_seen: u64 = 0;

        for f in files_to_check {
            let content = match std::fs::read_to_string(&f) {
                Ok(c) => c,
                Err(_) => {
                    return VerifyReport {
                        chain_ok: false,
                        records_checked,
                        first_bad_seq: expected_seq.unwrap_or(1),
                    };
                }
            };

            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }

                let val: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => {
                        return VerifyReport {
                            chain_ok: false,
                            records_checked,
                            first_bad_seq: expected_seq.unwrap_or(1),
                        };
                    }
                };

                let seq = match val.get("seq").and_then(Value::as_u64) {
                    Some(s) => s,
                    None => {
                        return VerifyReport {
                            chain_ok: false,
                            records_checked,
                            first_bad_seq: expected_seq.unwrap_or(1),
                        };
                    }
                };

                if expected_seq.is_some_and(|exp| seq != exp) {
                    return VerifyReport {
                        chain_ok: false,
                        records_checked,
                        first_bad_seq: seq,
                    };
                }

                let prev_hash_str = match val.get("prev_hash").and_then(Value::as_str) {
                    Some(h) => h,
                    None => {
                        return VerifyReport {
                            chain_ok: false,
                            records_checked,
                            first_bad_seq: seq,
                        };
                    }
                };

                if let Some(prev_h) = prev_line_hash {
                    let exp_prev_hex = format_hex(&prev_h);
                    if prev_hash_str != exp_prev_hex {
                        return VerifyReport {
                            chain_ok: false,
                            records_checked,
                            first_bad_seq: seq,
                        };
                    }
                }

                let line_bytes = line.trim_end_matches('\n').as_bytes();
                let current_hash = sha256_digest(line_bytes);
                prev_line_hash = Some(current_hash);
                last_line_hash = Some(current_hash);
                last_seq_seen = seq;
                expected_seq = Some(seq + 1);
                records_checked += 1;
            }
        }

        if records_checked == 0 {
            return VerifyReport {
                chain_ok: expected_head.is_none(),
                records_checked: 0,
                first_bad_seq: 0,
            };
        }

        if expected_head.is_some_and(|exp_head| last_line_hash != Some(exp_head)) {
            return VerifyReport {
                chain_ok: false,
                records_checked,
                first_bad_seq: last_seq_seen,
            };
        }

        VerifyReport {
            chain_ok: true,
            records_checked,
            first_bad_seq: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};
    use std::os::unix::fs::PermissionsExt;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestItem {
        msg: String,
        num: u64,
    }

    #[test]
    fn append_and_reopen_continues_seq_and_hash() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let info1 = log
            .append(&TestItem {
                msg: "first".into(),
                num: 1,
            })
            .unwrap();
        assert_eq!(info1.seq, 1);

        let info2 = log
            .append(&TestItem {
                msg: "second".into(),
                num: 2,
            })
            .unwrap();
        assert_eq!(info2.seq, 2);

        drop(log);

        let mut log2 = ChainedLog::open_default(&path).unwrap();
        assert_eq!(log2.next_seq(), 3);
        assert_eq!(log2.head_hash(), Some(info2.line_hash));

        let info3 = log2
            .append(&TestItem {
                msg: "third".into(),
                num: 3,
            })
            .unwrap();
        assert_eq!(info3.seq, 3);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);

        let val3: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(
            val3.get("prev_hash").unwrap().as_str().unwrap(),
            format_hex(&info2.line_hash)
        );
    }

    #[test]
    fn verify_ok_on_clean_log() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clean.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let mut last_hash = [0u8; 32];
        for i in 1..=5 {
            let info = log
                .append(&TestItem {
                    msg: format!("msg {i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }

        let report = ChainedLog::verify(&path, Some(last_hash));
        assert!(report.chain_ok);
        assert_eq!(report.records_checked, 5);
        assert_eq!(report.first_bad_seq, 0);
    }

    #[test]
    fn editing_any_single_byte_of_any_line_is_detected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tamper.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let mut last_hash = [0u8; 32];
        for i in 1..=4 {
            let info = log
                .append(&TestItem {
                    msg: format!("item_{i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }
        drop(log);

        let original_content = std::fs::read_to_string(&path).unwrap();
        let original_lines: Vec<&str> = original_content.lines().collect();
        let total = original_lines.len();

        for i in 0..total {
            let mut tampered_lines: Vec<String> =
                original_lines.iter().map(|s| s.to_string()).collect();

            // Flip one character in the message string value
            tampered_lines[i] = tampered_lines[i].replace("item_", "xtem_");
            let new_content = tampered_lines.join("\n") + "\n";
            std::fs::write(&path, new_content).unwrap();

            let report = ChainedLog::verify(&path, Some(last_hash));
            assert!(!report.chain_ok, "tampering line {i} must fail verify");

            let expected_bad = if i + 1 < total {
                (i + 2) as u64
            } else {
                total as u64
            };
            assert_eq!(
                report.first_bad_seq, expected_bad,
                "first_bad_seq mismatch for line {i}"
            );
        }
    }

    #[test]
    fn deleting_middle_line_is_detected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("del_mid.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let mut last_hash = [0u8; 32];
        for i in 1..=3 {
            let info = log
                .append(&TestItem {
                    msg: format!("item {i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }
        drop(log);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // Remove line 2 (seq 2)
        let tampered = format!("{}\n{}\n", lines[0], lines[2]);
        std::fs::write(&path, tampered).unwrap();

        let report = ChainedLog::verify(&path, Some(last_hash));
        assert!(!report.chain_ok);
    }

    #[test]
    fn deleting_last_line_detected_only_with_expected_head() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("del_last.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let mut last_hash = [0u8; 32];
        for i in 1..=3 {
            let info = log
                .append(&TestItem {
                    msg: format!("item {i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }
        drop(log);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // Keep only first 2 lines
        let truncated = format!("{}\n{}\n", lines[0], lines[1]);
        std::fs::write(&path, truncated).unwrap();

        // Without expected_head: prefix remains self-consistent
        let report_no_head = ChainedLog::verify(&path, None);
        assert!(report_no_head.chain_ok);
        assert_eq!(report_no_head.records_checked, 2);

        // With expected_head: detects missing last line
        let report_with_head = ChainedLog::verify(&path, Some(last_hash));
        assert!(!report_with_head.chain_ok);
    }

    #[test]
    fn reordering_two_lines_is_detected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("reorder.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let mut last_hash = [0u8; 32];
        for i in 1..=3 {
            let info = log
                .append(&TestItem {
                    msg: format!("item {i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }
        drop(log);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // Swap line 2 and line 3
        let reordered = format!("{}\n{}\n{}\n", lines[0], lines[2], lines[1]);
        std::fs::write(&path, reordered).unwrap();

        let report = ChainedLog::verify(&path, Some(last_hash));
        assert!(!report.chain_ok);
    }

    #[test]
    fn unparsable_last_line_on_open_fails_corrupt() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("corrupt.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        let _ = log
            .append(&TestItem {
                msg: "ok".into(),
                num: 1,
            })
            .unwrap();
        drop(log);

        // Append corrupt bytes to last line
        use std::io::Write;
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"bad_json\n").unwrap();
        drop(f);

        assert!(matches!(
            ChainedLog::open_default(&path),
            Err(ChainError::Corrupt)
        ));
    }

    #[test]
    fn bug036_short_write_mid_line_truncated_on_open() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("short_write.jsonl");

        let mut log = ChainedLog::open_default(&path).unwrap();
        log.append(&TestItem {
            msg: "first".into(),
            num: 1,
        })
        .unwrap();
        log.append(&TestItem {
            msg: "second".into(),
            num: 2,
        })
        .unwrap();
        drop(log);

        // Append a partial unacknowledged line without trailing newline
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"seq\":3,\"msg\":\"parti").unwrap();
        f.flush().unwrap();
        drop(f);

        // Opening must truncate partial bytes and succeed with next_seq 3
        let reopened = ChainedLog::open_default(&path).unwrap();
        assert_eq!(reopened.next_seq(), 3);
        drop(reopened);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("parti"),
            "partial bytes must be truncated away"
        );
        assert!(content.ends_with('\n'), "file must end with newline");

        // Now append a complete garbage line WITH trailing newline -> must fail Corrupt
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"totally garbage unparsable line\n").unwrap();
        f.flush().unwrap();
        drop(f);

        assert!(matches!(
            ChainedLog::open_default(&path),
            Err(ChainError::Corrupt)
        ));

        // Test recover() directly: re-open clean log, simulate mid-write failure by setting
        // needs_recovery and writing a partial line to disk.
        let temp2 = tempfile::tempdir().unwrap();
        let path2 = temp2.path().join("recover_test.jsonl");
        let mut log2 = ChainedLog::open_default(&path2).unwrap();
        log2.append(&TestItem {
            msg: "item1".into(),
            num: 1,
        })
        .unwrap();

        // Write partial line to disk and set recovery flag
        let mut f2 = OpenOptions::new().append(true).open(&path2).unwrap();
        f2.write_all(b"{\"seq\":2,\"msg\":\"truncated").unwrap();
        f2.flush().unwrap();
        drop(f2);
        log2.needs_recovery = true;

        // Next append must run recover(), truncate partial bytes, and successfully write seq 2
        let info2 = log2
            .append(&TestItem {
                msg: "item2".into(),
                num: 2,
            })
            .unwrap();
        assert_eq!(info2.seq, 2);
    }

    #[test]
    fn rotation_with_small_max_bytes_and_keep_2() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rotate.jsonl");

        // Small max_bytes: ~140 bytes per line, rotate after 2 lines (~350 bytes)
        let mut log = ChainedLog::open(&path, 350, 2).unwrap();

        let mut last_hash = [0u8; 32];
        for i in 1..=6 {
            let info = log
                .append(&TestItem {
                    msg: format!("long_record_payload_number_{i}"),
                    num: i,
                })
                .unwrap();
            last_hash = info.line_hash;
        }

        // Verify rotated files exist
        let rot1 = path.with_file_name("rotate.jsonl.1");
        let rot2 = path.with_file_name("rotate.jsonl.2");
        let rot3 = path.with_file_name("rotate.jsonl.3");

        assert!(path.exists());
        assert!(rot1.exists());
        assert!(rot2.exists());
        assert!(!rot3.exists(), "keep=2 must drop .3 file");

        // Verify across rotated files
        let report = ChainedLog::verify(&path, Some(last_hash));
        assert!(report.chain_ok);
        assert!(report.records_checked > 0);

        // Tail across rotation boundary
        let tail_lines = log.tail(5).unwrap();
        assert_eq!(tail_lines.len(), 5);
    }

    #[test]
    fn file_mode_is_0600() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("perms.jsonl");

        let _log = ChainedLog::open_default(&path).unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn dev_full_append_returns_err() {
        // Only run on systems with /dev/full
        let dev_full = Path::new("/dev/full");
        if !dev_full.exists() {
            return;
        }

        let mut log = match ChainedLog::open_default(dev_full) {
            Ok(l) => l,
            Err(_) => return, // Permission denied if not writable
        };

        let res = log.append(&TestItem {
            msg: "full".into(),
            num: 1,
        });
        assert!(
            res.is_err(),
            "/dev/full must fail append with storage error"
        );
    }

    proptest! {
        #[test]
        fn prop_chain_verification_and_mutation(
            count in 1usize..20usize,
            mutate_idx in 0usize..20usize,
            flip_pos in 0usize..20usize,
        ) {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("prop.jsonl");

            let mut log = ChainedLog::open_default(&path).unwrap();
            let mut last_hash = [0u8; 32];
            for i in 1..=count {
                let info = log.append(&TestItem {
                    msg: format!("msg_payload_{i}"),
                    num: i as u64,
                }).unwrap();
                last_hash = info.line_hash;
            }
            drop(log);

            // Clean log verifies OK
            let report = ChainedLog::verify(&path, Some(last_hash));
            prop_assert!(report.chain_ok);
            prop_assert_eq!(report.records_checked, count as u64);

            // Mutate a random line
            let content = std::fs::read_to_string(&path).unwrap();
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let target_line = mutate_idx % lines.len();

            if let Some(pos) = lines[target_line].find("msg_payload_") {
                let mutate_pos = pos + (flip_pos % 10);
                if mutate_pos < lines[target_line].len() {
                    let mut bytes = lines[target_line].clone().into_bytes();
                    bytes[mutate_pos] ^= 0x01;
                    if let Ok(mutated) = String::from_utf8(bytes) {
                        lines[target_line] = mutated;
                        let tampered_content = lines.join("\n") + "\n";
                        std::fs::write(&path, tampered_content).unwrap();

                        let tampered_report = ChainedLog::verify(&path, Some(last_hash));
                        prop_assert!(!tampered_report.chain_ok);
                    }
                }
            }
        }
    }
}
