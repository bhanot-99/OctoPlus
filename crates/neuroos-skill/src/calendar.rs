//! `calendar.read` (SAFE): upcoming events from local `.ics` files in the calendar folders C6
//! passes in `inv.roots` (FR-KER-10). Read-only, symlinks never followed, every read bounded.
//!
//! v1.0 limits, stated plainly: times are read as written and treated as UTC (a `TZID` is
//! ignored), and a repeating event (`RRULE`) is listed once at its own start. A skill that
//! expands recurrence and zones is a later, separate decision.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{Invocation, SkillError};

/// Largest calendar file read.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Most files one call looks at.
pub const MAX_FILES: usize = 200;
/// Deepest folder level below a calendar folder.
pub const MAX_DEPTH: usize = 2;
/// Most events returned.
pub const MAX_EVENTS: usize = 20;
/// Longest summary, in characters.
pub const MAX_SUMMARY_CHARS: usize = 120;
/// Widest look-ahead (matches the manifest).
pub const MAX_DAYS: u64 = 31;

const SECS_PER_DAY: u64 = 86_400;

/// Days from 1970-01-01 to a civil date (proleptic Gregorian); `None` for an impossible date.
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => 28 + u32::from(leap),
    };
    if d > dim {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Civil date and time of a Unix second count (for output).
fn civil_from_secs(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = if m <= 2 { y + 1 } else { y };
    let h = u32::try_from(rem / 3600).unwrap_or(0);
    let min = u32::try_from(rem % 3600 / 60).unwrap_or(0);
    (y, m, d, h, min)
}

/// `YYYYMMDD`, `YYYYMMDDTHHMMSS` or `...Z` to Unix seconds; `None` when malformed or impossible.
fn parse_start(value: &str) -> Option<i64> {
    let v = value.trim().trim_end_matches('Z');
    let (date, time) = match v.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (v, None),
    };
    if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = date[..4].parse().ok()?;
    let m: u32 = date[4..6].parse().ok()?;
    let d: u32 = date[6..8].parse().ok()?;
    let days = days_from_civil(y, m, d)?;
    let (h, mi, s) = match time {
        None => (0_i64, 0_i64, 0_i64),
        Some(t) => {
            if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let (h, mi, s): (i64, i64, i64) = (
                t[..2].parse().ok()?,
                t[2..4].parse().ok()?,
                t[4..6].parse().ok()?,
            );
            if h > 23 || mi > 59 || s > 60 {
                return None;
            }
            (h, mi, s)
        }
    };
    Some(days * 86_400 + h * 3600 + mi * 60 + s)
}

/// Joins continuation lines (a line starting with a space or tab continues the previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

/// The value of a content line whose property name is `name` (parameters ignored).
fn property<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (head, value) = line.split_once(':')?;
    let prop = head.split(';').next()?;
    prop.eq_ignore_ascii_case(name).then_some(value)
}

fn clean_summary(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_SUMMARY_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Events of one calendar text as `(start_secs, summary)`.
fn events_of(text: &str) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    let (mut inside, mut start, mut summary) = (false, None, String::new());
    for line in unfold(text) {
        let upper = line.trim().to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            (inside, start, summary) = (true, None, String::new());
        } else if upper == "END:VEVENT" {
            if inside && let Some(s) = start {
                out.push((s, clean_summary(&summary)));
            }
            inside = false;
        } else if inside {
            if let Some(v) = property(&line, "DTSTART") {
                start = parse_start(v);
            } else if let Some(v) = property(&line, "SUMMARY") {
                summary = v.to_string();
            }
        }
    }
    out
}

fn collect_files(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if files.len() >= MAX_FILES {
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let is_ics = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ics"));
        if meta.file_type().is_dir() {
            if depth < MAX_DEPTH {
                collect_files(&path, depth + 1, files);
            }
        } else if meta.file_type().is_file() && is_ics && meta.len() <= MAX_FILE_BYTES {
            files.push(path);
        }
    }
}

fn read_text(path: &Path) -> Option<String> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
        .ok()?;
    let mut raw = Vec::new();
    f.by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(raw).ok()
}

/// `calendar.read {"days"}` with the clock passed in.
///
/// # Errors
/// [`SkillError::BadArgs`] for a bad `days`, [`SkillError::PathOutsideAllowlist`] with no folder.
pub fn read(inv: &Invocation, args: &Value, now_unix_s: u64) -> Result<Value, SkillError> {
    let obj = args.as_object().ok_or(SkillError::BadArgs)?;
    if obj.len() != 1 {
        return Err(SkillError::BadArgs);
    }
    let days = obj
        .get("days")
        .and_then(Value::as_u64)
        .filter(|d| (1..=MAX_DAYS).contains(d))
        .ok_or(SkillError::BadArgs)?;
    if inv.roots.is_empty() {
        return Err(SkillError::PathOutsideAllowlist);
    }
    let from = i64::try_from(now_unix_s).map_err(|_| SkillError::BadArgs)?;
    let to = from.saturating_add(i64::try_from(days * SECS_PER_DAY).unwrap_or(i64::MAX));

    let mut files = Vec::new();
    for root in &inv.roots {
        collect_files(root, 0, &mut files);
    }
    let mut events: Vec<(i64, String)> = files
        .iter()
        .filter_map(|p| read_text(p))
        .flat_map(|t| events_of(&t))
        .filter(|(s, _)| (from..to).contains(s))
        .collect();
    events.sort();
    let truncated = events.len() > MAX_EVENTS;
    events.truncate(MAX_EVENTS);
    let list: Vec<Value> = events
        .iter()
        .map(|(s, summary)| {
            let (y, mo, d, h, mi) = civil_from_secs(*s);
            json!({"start": format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}"), "summary": summary})
        })
        .collect();
    Ok(json!({"events": list, "truncated": truncated}))
}
