//! Spool Writer for C7 Fetcher (FR-FET-08, FR-STO-10, Architecture §7.3).
//!
//! Writes extracted documents into `/var/spool/neuroos-fetcher/` conforming strictly
//! to Storage Spool Format v1 (`parse_spool`). Uses atomic write semantics (temporary file + rename).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Serialized payload structure for Spool Format v1.
#[derive(Debug, Serialize)]
struct SpoolPayload<'a> {
    version: u32,
    url: &'a str,
    title: &'a str,
    fetched_at_ns: u64,
    text: &'a str,
}

/// Errors occurring during spool file creation.
#[derive(Debug, thiserror::Error)]
pub enum SpoolWriteError {
    /// Filesystem I/O error.
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    /// Filename fails safe naming criteria.
    #[error("invalid spool filename: {0}")]
    InvalidFilename(String),
    /// JSON serialization failed.
    #[error("JSON serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Checks whether a spool filename is valid per security.md §4.2 / storage spool.rs.
#[must_use]
pub fn valid_spool_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if !name.ends_with(".json") {
        return false;
    }
    if name.starts_with('.') || name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Writes an extracted document to `spool_dir` atomically under `filename`.
///
/// # Errors
/// Returns [`SpoolWriteError`] on invalid filenames or I/O errors.
#[allow(clippy::too_many_arguments)]
pub fn write_spool(
    spool_dir: &Path,
    filename: &str,
    url: &str,
    title: &str,
    fetched_at_ns: u64,
    text: &str,
) -> Result<PathBuf, SpoolWriteError> {
    if !valid_spool_name(filename) {
        return Err(SpoolWriteError::InvalidFilename(filename.to_string()));
    }

    let payload = SpoolPayload {
        version: 1,
        url,
        title,
        fetched_at_ns,
        text,
    };
    let json_bytes = serde_json::to_vec(&payload)?;

    let target_path = spool_dir.join(filename);
    let tmp_filename = format!(".tmp_{filename}");
    let tmp_path = spool_dir.join(tmp_filename);

    let mut tmp_file = File::create(&tmp_path)?;
    tmp_file.write_all(&json_bytes)?;
    tmp_file.sync_all()?;
    drop(tmp_file);

    fs::rename(&tmp_path, &target_path)?;
    Ok(target_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn valid_filename_checks() {
        assert!(valid_spool_name("doc_012345.json"));
        assert!(valid_spool_name("action-uuid-123.json"));
        assert!(!valid_spool_name(".hidden.json"));
        assert!(!valid_spool_name("doc..json"));
        assert!(!valid_spool_name("doc/traversal.json"));
        assert!(!valid_spool_name("doc.txt"));
        assert!(!valid_spool_name(""));
    }

    #[test]
    fn atomic_spool_write_succeeds() {
        let dir = tempdir().unwrap();
        let target = write_spool(
            dir.path(),
            "fetch_doc_1.json",
            "https://example.com/test",
            "Example Title",
            1_700_000_000,
            "Clean content from web.",
        )
        .unwrap();

        assert!(target.exists());
        let content = std::fs::read_to_string(target).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["url"], "https://example.com/test");
        assert_eq!(v["title"], "Example Title");
        assert_eq!(v["text"], "Clean content from web.");
        assert_eq!(v["fetched_at_ns"], 1_700_000_000);
    }
}
