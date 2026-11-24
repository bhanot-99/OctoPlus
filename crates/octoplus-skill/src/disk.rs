//! `system.disk_report` (SAFE): what uses space in the allowlisted folders
//! (caches, configured build folders, the trash) and how full the disk is.
//! Read-only; it only looks inside the roots C6 passed on the command line.

use std::path::Path;

use serde_json::{Value, json};

use crate::walk::{self, Budget};
use crate::{Invocation, SkillError};

/// Largest entries listed per root.
pub const TOP_PER_ROOT: usize = 10;

fn top_entries(root: &Path, budget: &mut Budget) -> (u64, Vec<(String, u64)>) {
    let mut entries: Vec<(String, u64)> = Vec::new();
    let mut total = 0_u64;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let bytes = walk::size(&e.path(), budget);
            total = total.saturating_add(bytes);
            entries.push((e.file_name().to_string_lossy().into_owned(), bytes));
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.truncate(TOP_PER_ROOT);
    (total, entries)
}

fn filesystem(path: &Path) -> Option<Value> {
    let st = rustix::fs::statvfs(path).ok()?;
    let total = st.f_blocks.saturating_mul(st.f_frsize);
    let avail = st.f_bavail.saturating_mul(st.f_frsize);
    Some(json!({"total_bytes": total, "available_bytes": avail}))
}

/// Runs the skill. Takes no arguments.
///
/// # Errors
/// [`SkillError::BadArgs`] for any argument.
pub fn run(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    if args.as_object().is_none_or(|m| !m.is_empty()) {
        return Err(SkillError::BadArgs);
    }
    let mut budget = Budget::default();
    let mut roots = Vec::new();
    for root in &inv.roots {
        let (bytes, top) = top_entries(root, &mut budget);
        let top: Vec<Value> = top
            .into_iter()
            .map(|(name, b)| json!({"name": name, "bytes": b}))
            .collect();
        roots.push(json!({"path": root, "bytes": bytes, "top": top}));
    }
    let trash = inv.trash.as_ref().map(|t| {
        let bytes = walk::size(t, &mut budget);
        json!({"path": t, "bytes": bytes})
    });
    let fs = inv
        .roots
        .first()
        .or(inv.trash.as_ref())
        .and_then(|p| filesystem(p));
    Ok(json!({
        "roots": roots,
        "trash": trash,
        "filesystem": fs,
        "truncated": budget.truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use octoplus_sandbox::child::{ChildLimits, SyscallProfile};

    fn inv(roots: Vec<std::path::PathBuf>, trash: Option<std::path::PathBuf>) -> Invocation {
        Invocation {
            capability: "system.disk_report".into(),
            mode: crate::Mode::Run,
            read_only: vec![],
            read_write: vec![],
            roots,
            trash,
            limits: ChildLimits {
                address_space_bytes: 1 << 30,
                cpu_seconds: 5,
                open_files: 64,
                file_size_bytes: 0,
            },
            profile: SyscallProfile::ReadOnly,
        }
    }

    #[test]
    fn reports_largest_entries_first() {
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("cache");
        std::fs::create_dir_all(cache.join("big")).unwrap();
        std::fs::write(cache.join("big/f"), vec![0u8; 256 * 1024]).unwrap();
        std::fs::write(cache.join("small"), vec![0u8; 4096]).unwrap();
        let trash = d.path().join("Trash");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::write(trash.join("files/old"), vec![0u8; 8192]).unwrap();
        let v = run(&inv(vec![cache], Some(trash)), &json!({})).unwrap();
        let top = v["roots"][0]["top"].as_array().unwrap();
        assert_eq!(top[0]["name"], "big");
        assert_eq!(top[1]["name"], "small");
        assert!(v["roots"][0]["bytes"].as_u64().unwrap() >= 256 * 1024);
        assert!(v["trash"]["bytes"].as_u64().unwrap() >= 8192);
        assert!(v["filesystem"]["total_bytes"].as_u64().unwrap() > 0);
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn refuses_arguments() {
        assert_eq!(
            run(&inv(vec![], None), &json!({"x": 1})),
            Err(SkillError::BadArgs)
        );
    }
}
