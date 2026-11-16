//! `system.cleanup` (REVIEW, FR-KER-10, OQ-07, ADR-0010 decision 8).
//!
//! `plan` (read-only) turns the requested paths into the exact list the dialog
//! shows: each entry with its size. `run` (after approval) re-checks every path
//! and permanently deletes it; the trash root means "empty the trash".
//!
//! **Allowlist check (security.md §4.4 "path or command trick"):** a path is
//! accepted only if, after resolving every symlink in its *parent*, it lies
//! strictly inside one of the cleanup roots C6 passed (never a root itself),
//! or it is exactly the trash root. The final component is never followed: a
//! symlink is deleted as a link, its target untouched. Overlapping entries are
//! refused. The child's Landlock policy holds only these roots, so a path that
//! slipped past this check would still fail with `EACCES`.

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::walk::{self, Budget};
use crate::{Invocation, Mode, SkillError};

/// Sub-folders emptied for "empty the trash" (freedesktop trash spec).
const TRASH_PARTS: [&str; 3] = ["files", "info", "expunged"];

/// One validated target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An entry strictly inside a cleanup root (resolved parent + final name).
    Entry(PathBuf),
    /// Empty the trash.
    EmptyTrash(PathBuf),
}

impl Target {
    fn path(&self) -> &Path {
        match self {
            Self::Entry(p) | Self::EmptyTrash(p) => p,
        }
    }
}

fn canonical(p: &Path) -> Result<PathBuf, SkillError> {
    std::fs::canonicalize(p).map_err(|_| SkillError::PathOutsideAllowlist)
}

/// Validates one requested path against the roots and the trash.
///
/// # Errors
/// [`SkillError::PathOutsideAllowlist`] for anything not allowed or missing.
pub fn validate(inv: &Invocation, requested: &Path) -> Result<Target, SkillError> {
    if !requested.is_absolute()
        || requested
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(SkillError::PathOutsideAllowlist);
    }
    // Must exist (without following a final symlink).
    std::fs::symlink_metadata(requested).map_err(|_| SkillError::PathOutsideAllowlist)?;
    if let Some(trash) = &inv.trash
        && canonical(trash)? == canonical(requested)?
        && !std::fs::symlink_metadata(requested)
            .map_err(|_| SkillError::PathOutsideAllowlist)?
            .file_type()
            .is_symlink()
    {
        return Ok(Target::EmptyTrash(canonical(trash)?));
    }
    let name = requested
        .file_name()
        .ok_or(SkillError::PathOutsideAllowlist)?;
    let parent = requested.parent().ok_or(SkillError::PathOutsideAllowlist)?;
    let real = canonical(parent)?.join(name);
    for root in &inv.roots {
        let Ok(root) = canonical(root) else { continue };
        if real != root && real.starts_with(&root) {
            return Ok(Target::Entry(real));
        }
    }
    Err(SkillError::PathOutsideAllowlist)
}

fn targets(inv: &Invocation, args: &Value) -> Result<Vec<Target>, SkillError> {
    let obj = args.as_object().ok_or(SkillError::BadArgs)?;
    if obj.len() != 1 {
        return Err(SkillError::BadArgs);
    }
    let list = obj
        .get("paths")
        .and_then(Value::as_array)
        .ok_or(SkillError::BadArgs)?;
    if list.is_empty() {
        return Err(SkillError::BadArgs);
    }
    let mut out: Vec<Target> = Vec::with_capacity(list.len());
    for item in list {
        let s = item.as_str().ok_or(SkillError::BadArgs)?;
        let t = validate(inv, Path::new(s))?;
        // Overlap (same entry twice, or one inside another) would double-count and confuse the list.
        if out
            .iter()
            .any(|o| o.path().starts_with(t.path()) || t.path().starts_with(o.path()))
        {
            return Err(SkillError::BadArgs);
        }
        out.push(t);
    }
    Ok(out)
}

fn trash_size(trash: &Path, budget: &mut Budget) -> u64 {
    TRASH_PARTS
        .iter()
        .map(|p| walk::size(&trash.join(p), budget))
        .sum()
}

fn plan(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    let mut budget = Budget::default();
    let mut items = Vec::new();
    let mut total = 0_u64;
    for t in targets(inv, args)? {
        let (bytes, kind) = match &t {
            Target::Entry(p) => (walk::size(p, &mut budget), "entry"),
            Target::EmptyTrash(p) => (trash_size(p, &mut budget), "trash"),
        };
        total = total.saturating_add(bytes);
        items.push(json!({"path": t.path(), "bytes": bytes, "kind": kind}));
    }
    Ok(json!({"items": items, "total_bytes": total, "truncated": budget.truncated}))
}

/// Deletes `p` without following a final symlink.
fn delete(p: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.file_type().is_dir() {
        // std's remove_dir_all does not follow symlinks inside the tree (CVE-2022-21658 fix).
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

fn execute(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    let mut budget = Budget::default();
    let (mut freed, mut deleted, mut failed) = (0_u64, 0_u64, 0_u64);
    for t in targets(inv, args)? {
        let victims: Vec<PathBuf> = match &t {
            Target::Entry(p) => vec![p.clone()],
            Target::EmptyTrash(p) => TRASH_PARTS
                .iter()
                .filter_map(|part| std::fs::read_dir(p.join(part)).ok())
                .flat_map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .collect(),
        };
        for v in victims {
            let bytes = walk::size(&v, &mut budget);
            match delete(&v) {
                Ok(()) => {
                    freed = freed.saturating_add(bytes);
                    deleted += 1;
                }
                Err(_) => failed += 1,
            }
        }
    }
    Ok(json!({"freed_bytes": freed, "deleted": deleted, "failed": failed}))
}

/// Runs the skill in the invocation's mode.
///
/// # Errors
/// [`SkillError::BadArgs`] or [`SkillError::PathOutsideAllowlist`]; nothing is
/// deleted if any requested path is refused.
pub fn run(inv: &Invocation, args: &Value) -> Result<Value, SkillError> {
    match inv.mode {
        Mode::Plan => plan(inv, args),
        Mode::Run => execute(inv, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neuroos_sandbox::child::{ChildLimits, SyscallProfile};

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        inv: Invocation,
    }

    fn fixture(mode: Mode) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap();
        let cache = home.join(".cache");
        let trash = home.join(".local/share/Trash");
        std::fs::create_dir_all(cache.join("thumbs")).unwrap();
        std::fs::write(cache.join("thumbs/a.png"), vec![0u8; 16384]).unwrap();
        std::fs::write(cache.join("one.bin"), vec![0u8; 4096]).unwrap();
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(trash.join("files/old.txt"), vec![0u8; 8192]).unwrap();
        std::fs::write(trash.join("info/old.txt.trashinfo"), b"[Trash Info]").unwrap();
        std::fs::write(home.join("precious.txt"), b"keep me").unwrap();
        let inv = Invocation {
            capability: "system.cleanup".into(),
            mode,
            read_only: vec![],
            read_write: vec![],
            roots: vec![cache],
            trash: Some(trash),
            limits: ChildLimits {
                address_space_bytes: 1 << 30,
                cpu_seconds: 5,
                open_files: 64,
                file_size_bytes: 0,
            },
            profile: SyscallProfile::FileMutating,
        };
        Fixture {
            _dir: dir,
            home,
            inv,
        }
    }

    fn paths(v: &[&Path]) -> Value {
        json!({"paths": v.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>()})
    }

    #[test]
    fn plan_lists_sizes_and_changes_nothing() {
        let f = fixture(Mode::Plan);
        let thumbs = f.home.join(".cache/thumbs");
        let trash = f.home.join(".local/share/Trash");
        let v = run(&f.inv, &paths(&[&thumbs, &trash])).unwrap();
        assert_eq!(v["items"][0]["kind"], "entry");
        assert!(v["items"][0]["bytes"].as_u64().unwrap() >= 16384);
        assert_eq!(v["items"][1]["kind"], "trash");
        assert!(v["total_bytes"].as_u64().unwrap() >= 16384 + 8192);
        assert!(thumbs.exists() && trash.join("files/old.txt").exists());
    }

    #[test]
    fn run_deletes_entries_and_empties_trash() {
        let f = fixture(Mode::Run);
        let thumbs = f.home.join(".cache/thumbs");
        let one = f.home.join(".cache/one.bin");
        let trash = f.home.join(".local/share/Trash");
        let v = run(&f.inv, &paths(&[&thumbs, &one, &trash])).unwrap();
        assert_eq!(v["deleted"], 4);
        assert_eq!(v["failed"], 0);
        assert!(!thumbs.exists() && !one.exists());
        assert!(trash.join("files").exists(), "trash folders stay");
        assert_eq!(std::fs::read_dir(trash.join("files")).unwrap().count(), 0);
        assert!(f.home.join("precious.txt").exists());
        assert!(f.home.join(".cache").exists(), "the root itself stays");
    }

    #[test]
    fn crafted_paths_are_refused() {
        let f = fixture(Mode::Run);
        let cache = f.home.join(".cache");
        // A symlink inside the root pointing outside: its *parent* resolves fine,
        // so the entry is the link itself — deleting it must not touch the target.
        std::os::unix::fs::symlink(f.home.join("precious.txt"), cache.join("link")).unwrap();
        // A symlinked directory inside the root pointing outside, used as a parent.
        std::os::unix::fs::symlink(&f.home, cache.join("escape")).unwrap();
        let bad: Vec<PathBuf> = vec![
            cache.clone(),                                   // the root itself
            f.home.join("precious.txt"),                     // outside
            cache.join("../precious.txt"),                   // dot-dot
            cache.join("escape/precious.txt"),               // parent symlink escapes
            PathBuf::from("relative/path"),                  // relative
            cache.join("missing"),                           // does not exist
            f.home.join(".local/share/Trash/files/old.txt"), // inside trash, not the trash
            PathBuf::from("/etc/passwd"),
        ];
        for p in &bad {
            assert_eq!(
                run(&f.inv, &paths(&[p.as_path()])),
                Err(SkillError::PathOutsideAllowlist),
                "{} must be refused",
                p.display()
            );
        }
        // One bad path refuses the whole request: nothing is deleted.
        let ok = cache.join("one.bin");
        assert!(
            run(
                &f.inv,
                &paths(&[ok.as_path(), f.home.join("precious.txt").as_path()])
            )
            .is_err()
        );
        assert!(ok.exists());
        // The link is deleted as a link; its target survives.
        run(&f.inv, &paths(&[cache.join("link").as_path()])).unwrap();
        assert!(f.home.join("precious.txt").exists());
        assert!(!cache.join("link").exists());
    }

    #[test]
    fn overlapping_and_malformed_requests_are_refused() {
        let f = fixture(Mode::Plan);
        let thumbs = f.home.join(".cache/thumbs");
        let inner = thumbs.join("a.png");
        assert_eq!(
            run(&f.inv, &paths(&[&thumbs, &inner])),
            Err(SkillError::BadArgs)
        );
        assert_eq!(
            run(&f.inv, &paths(&[&thumbs, &thumbs])),
            Err(SkillError::BadArgs)
        );
        assert_eq!(run(&f.inv, &json!({"paths": []})), Err(SkillError::BadArgs));
        assert_eq!(
            run(&f.inv, &json!({"paths": [1]})),
            Err(SkillError::BadArgs)
        );
        assert_eq!(
            run(&f.inv, &json!({"paths": ["/x"], "extra": 1})),
            Err(SkillError::BadArgs)
        );
        assert_eq!(run(&f.inv, &json!(["/x"])), Err(SkillError::BadArgs));
    }

    #[test]
    fn trash_symlink_is_not_the_trash() {
        let f = fixture(Mode::Plan);
        let link = f.home.join(".cache/trashlink");
        std::os::unix::fs::symlink(f.home.join(".local/share/Trash"), &link).unwrap();
        // A link to the trash inside a root is just an entry (the link), never "empty the trash".
        let v = run(&f.inv, &paths(&[&link])).unwrap();
        assert_eq!(v["items"][0]["kind"], "entry");
    }
}
