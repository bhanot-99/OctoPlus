//! `notes.search` (SAFE) and `notes.append` (REVIEW) on the notes folders C6 passes in
//! `inv.roots` (FR-KER-10, FR-KER-05). The child's Landlock rules already confine it to those
//! folders; the checks here are a second layer, so a bug cannot widen what the sandbox allows.
//!
//! Rules shared by both skills: symbolic links are never followed, only regular `.md` and
//! `.txt` files count, and every walk and read is bounded (rules §9.7). Output never contains
//! control characters.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{Invocation, Mode, SkillError};

/// Longest search query (matches the manifest).
pub const MAX_QUERY_CHARS: usize = 256;
/// Largest note file `notes.search` reads (FR-KER-05 bounds).
pub const MAX_READ_FILE_BYTES: u64 = 256 * 1024;
/// Most files one search looks at.
pub const MAX_FILES: usize = 2000;
/// Most bytes one search reads in total.
pub const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
/// Deepest folder level searched below a notes folder.
pub const MAX_DEPTH: usize = 4;
/// Most matches returned.
pub const MAX_MATCHES: usize = 10;
/// Longest snippet, in characters.
pub const MAX_SNIPPET_CHARS: usize = 160;
/// Longest note name (matches the manifest's intent).
pub const MAX_NOTE_NAME: usize = 128;
/// Longest text one append adds (matches the manifest).
pub const MAX_APPEND_BYTES: usize = 4096;
/// Largest a note may grow to.
pub const MAX_NOTE_FILE_BYTES: u64 = 1024 * 1024;

fn has_note_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".txt")
}

fn clean_snippet(line: &str) -> String {
    line.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_SNIPPET_CHARS)
        .collect()
}

// ---- notes.search -----------------------------------------------------------------------

#[derive(Default)]
struct SearchState {
    files: usize,
    bytes: u64,
    matches: Vec<(String, String)>,
    truncated: bool,
}

fn search_dir(root: &Path, dir: &Path, depth: usize, needle: &str, st: &mut SearchState) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if st.matches.len() >= MAX_MATCHES {
            st.truncated = true;
            return;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        // `symlink_metadata`: a link is skipped, never followed.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_dir() {
            if depth < MAX_DEPTH {
                search_dir(root, &path, depth + 1, needle, st);
            }
        } else if meta.file_type().is_file() && has_note_extension(name) {
            search_file(root, &path, meta.len(), needle, st);
        }
    }
}

fn search_file(root: &Path, path: &Path, len: u64, needle: &str, st: &mut SearchState) {
    if len > MAX_READ_FILE_BYTES || st.files >= MAX_FILES || st.bytes + len > MAX_TOTAL_BYTES {
        st.truncated = st.truncated || st.files >= MAX_FILES || st.bytes + len > MAX_TOTAL_BYTES;
        return;
    }
    st.files += 1;
    st.bytes += len;
    let Ok(mut f) = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
    else {
        return;
    };
    let mut raw = Vec::new();
    if Read::by_ref(&mut f)
        .take(MAX_READ_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .is_err()
    {
        return;
    }
    let Ok(text) = String::from_utf8(raw) else {
        return;
    };
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    for line in text.lines() {
        if line.to_lowercase().contains(needle) {
            if st.matches.len() >= MAX_MATCHES {
                st.truncated = true;
                return;
            }
            st.matches.push((rel.clone(), clean_snippet(line)));
        }
    }
}

/// `notes.search {"query"}`.
///
/// # Errors
/// [`SkillError::BadArgs`] for a bad query, [`SkillError::PathOutsideAllowlist`] with no folder.
pub fn search(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    let obj = args.as_object().ok_or(SkillError::BadArgs)?;
    if obj.len() != 1 {
        return Err(SkillError::BadArgs);
    }
    let query = obj
        .get("query")
        .and_then(Value::as_str)
        .ok_or(SkillError::BadArgs)?;
    let n = query.chars().count();
    if n == 0 || n > MAX_QUERY_CHARS || query.chars().any(char::is_control) {
        return Err(SkillError::BadArgs);
    }
    if inv.roots.is_empty() {
        return Err(SkillError::PathOutsideAllowlist);
    }
    let needle = query.to_lowercase();
    let mut st = SearchState::default();
    for root in &inv.roots {
        search_dir(root, root, 0, &needle, &mut st);
    }
    let matches: Vec<Value> = st
        .matches
        .iter()
        .map(|(f, s)| json!({"file": f, "snippet": s}))
        .collect();
    Ok(json!({"matches": matches, "truncated": st.truncated}))
}

// ---- notes.append -----------------------------------------------------------------------

/// The final file name for a requested note name, or `None` when it is not acceptable.
fn note_file_name(raw: &str) -> Option<String> {
    let ok_chars = raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'));
    if raw.is_empty()
        || raw.len() > MAX_NOTE_NAME
        || !ok_chars
        || raw.starts_with('.')
        || raw.contains("..")
        || raw.trim().is_empty()
    {
        return None;
    }
    if has_note_extension(raw) {
        Some(raw.to_string())
    } else {
        Some(format!("{raw}.md"))
    }
}

fn valid_append_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_APPEND_BYTES
        && text
            .chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t')
}

/// `notes.append {"note","text"}`; in plan mode it validates and writes nothing.
///
/// # Errors
/// [`SkillError::BadArgs`], [`SkillError::PathOutsideAllowlist`] (no folder, or the target is
/// not a plain file), [`SkillError::Io`].
pub fn append(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    let obj = args.as_object().ok_or(SkillError::BadArgs)?;
    if obj.len() != 2 {
        return Err(SkillError::BadArgs);
    }
    let note = obj
        .get("note")
        .and_then(Value::as_str)
        .ok_or(SkillError::BadArgs)?;
    let text = obj
        .get("text")
        .and_then(Value::as_str)
        .ok_or(SkillError::BadArgs)?;
    let name = note_file_name(note).ok_or(SkillError::BadArgs)?;
    if !valid_append_text(text) {
        return Err(SkillError::BadArgs);
    }
    let folder = inv.roots.first().ok_or(SkillError::PathOutsideAllowlist)?;
    let target = folder.join(&name);

    // The target may already exist: it must be a plain file, never a link.
    let existing = match std::fs::symlink_metadata(&target) {
        Ok(m) if m.file_type().is_file() => Some(m.len()),
        Ok(_) => return Err(SkillError::PathOutsideAllowlist),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(SkillError::Io),
    };
    let current = existing.unwrap_or(0);
    let lead_newline = existing.is_some_and(|len| len > 0 && !ends_with_newline(&target));
    let added = u64::try_from(text.len() + 1 + usize::from(lead_newline)).unwrap_or(u64::MAX);
    if current.saturating_add(added) > MAX_NOTE_FILE_BYTES {
        return Err(SkillError::BadArgs);
    }
    if inv.mode == Mode::Plan {
        return Ok(json!({"file": name, "appended_bytes": added, "planned": true}));
    }

    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(&target)
        .map_err(|_| SkillError::Io)?;
    // Check the file we actually opened, not the path again (no race between check and write).
    let meta = f.metadata().map_err(|_| SkillError::Io)?;
    if !meta.file_type().is_file() {
        return Err(SkillError::PathOutsideAllowlist);
    }
    let mut out = String::with_capacity(text.len() + 2);
    if lead_newline {
        out.push('\n');
    }
    out.push_str(text);
    out.push('\n');
    f.write_all(out.as_bytes()).map_err(|_| SkillError::Io)?;
    Ok(json!({"file": name, "appended_bytes": added}))
}

fn ends_with_newline(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return true;
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return true;
    };
    if len == 0 {
        return true;
    }
    use std::io::{Seek, SeekFrom};
    let mut last = [0_u8; 1];
    f.seek(SeekFrom::Start(len - 1))
        .and_then(|_| f.read_exact(&mut last))
        .map_or(true, |()| last[0] == b'\n')
}
