//! Spool Writer for C7 Fetcher (FR-FET-08, FR-STO-10, Architecture §7.3).
//!
//! Writes extracted documents into `/var/spool/octoplus-fetcher/` conforming strictly
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
    /// Nothing of the document fits the spool size limit (or it has no text).
    #[error("document does not fit the spool limit")]
    TooLarge,
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

/// One document to spool.
#[derive(Debug, Clone, Copy)]
pub struct SpoolEntry<'a> {
    /// The page's final URL.
    pub url: &'a str,
    /// Extracted title.
    pub title: &'a str,
    /// When it was fetched, epoch nanoseconds (AB-8).
    pub fetched_at_ns: u64,
    /// Extracted text.
    pub text: &'a str,
}

/// Writes an extracted document to `spool_dir` atomically under `filename`.
///
/// C3 refuses a spool file larger than `storage.spool_max_bytes` (FR-STO-10), so the text is
/// cut at a character boundary until the whole file fits `max_bytes`; otherwise a long page
/// would be fetched and then silently dropped (audit A-06).
///
/// # Errors
/// Returns [`SpoolWriteError`] on invalid filenames, I/O errors, or when even an empty
/// text would not fit.
pub fn write_spool(
    spool_dir: &Path,
    filename: &str,
    entry: &SpoolEntry<'_>,
    max_bytes: u64,
) -> Result<PathBuf, SpoolWriteError> {
    if !valid_spool_name(filename) {
        return Err(SpoolWriteError::InvalidFilename(filename.to_string()));
    }
    let json_bytes = fitted_json(entry, max_bytes)?;

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

/// Serialises `entry`, shortening its text until the result fits `max_bytes`.
fn fitted_json(entry: &SpoolEntry<'_>, max_bytes: u64) -> Result<Vec<u8>, SpoolWriteError> {
    let mut text = entry.text.trim();
    // Each pass removes at least the excess, so a few passes always settle.
    for _ in 0..16 {
        if text.is_empty() {
            break;
        }
        let payload = SpoolPayload {
            version: 1,
            url: entry.url,
            title: entry.title,
            fetched_at_ns: entry.fetched_at_ns,
            text,
        };
        let bytes = serde_json::to_vec(&payload)?;
        let len = bytes.len() as u64;
        if len <= max_bytes {
            return Ok(bytes);
        }
        // Shrink in proportion (escaping can grow text up to six times), with a 5 % margin;
        // always by at least one byte so the loop ends.
        let keep_scaled = u128::from(max_bytes) * 95 * text.len() as u128 / (u128::from(len) * 100);
        let mut keep = usize::try_from(keep_scaled)
            .unwrap_or(usize::MAX)
            .min(text.len().saturating_sub(1));
        while keep > 0 && !text.is_char_boundary(keep) {
            keep -= 1;
        }
        text = text[..keep].trim_end();
    }
    Err(SpoolWriteError::TooLarge)
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
        let entry = SpoolEntry {
            url: "https://example.com/test",
            title: "Example Title",
            fetched_at_ns: 1_700_000_000,
            text: "Clean content from web.",
        };
        let target = write_spool(dir.path(), "fetch_doc_1.json", &entry, 256 * 1024).unwrap();

        assert!(target.exists());
        let content = std::fs::read_to_string(target).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["url"], "https://example.com/test");
        assert_eq!(v["title"], "Example Title");
        assert_eq!(v["text"], "Clean content from web.");
        assert_eq!(v["fetched_at_ns"], 1_700_000_000);
    }

    /// Regression (audit A-06): a long page is cut to fit C3's limit instead of being
    /// written whole and refused by C3.
    #[test]
    fn long_text_is_cut_to_fit_the_limit() {
        let dir = tempdir().unwrap();
        let long = "é\"quoted\" text. ".repeat(40_000);
        let entry = SpoolEntry {
            url: "https://example.com/long",
            title: "Long",
            fetched_at_ns: 1,
            text: &long,
        };
        let max = 64 * 1024;
        let path = write_spool(dir.path(), "fetch_long.json", &entry, max).unwrap();
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() as u64 <= max, "{} > {max}", bytes.len());
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let text = v["text"].as_str().unwrap();
        assert!(text.len() > 30_000, "kept only {} bytes", text.len());
        assert!(long.starts_with(text));
    }

    #[test]
    fn empty_text_or_impossible_limit_is_refused() {
        let dir = tempdir().unwrap();
        let mut entry = SpoolEntry {
            url: "https://example.com/x",
            title: "X",
            fetched_at_ns: 1,
            text: "   ",
        };
        assert!(matches!(
            write_spool(dir.path(), "fetch_e.json", &entry, 4096),
            Err(SpoolWriteError::TooLarge)
        ));
        entry.text = "some text";
        assert!(matches!(
            write_spool(dir.path(), "fetch_f.json", &entry, 10),
            Err(SpoolWriteError::TooLarge)
        ));
    }
}
