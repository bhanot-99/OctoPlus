//! Bounded, symlink-safe size walk shared by `system.disk_report` and `system.cleanup`.
//!
//! Never follows symlinks, never crosses into another filesystem, and stops
//! after [`MAX_VISITS`] entries so a huge tree cannot exhaust the time limit
//! (rules.md §9.7 "bound everything"). Sizes are allocated bytes (`st_blocks × 512`),
//! which is what deleting actually frees.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Most directory entries one skill run inspects.
pub const MAX_VISITS: u64 = 500_000;

/// Running totals for one walk.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Entries inspected so far.
    pub visits: u64,
    /// True once the walk stopped early.
    pub truncated: bool,
}

/// Allocated size of `path` and everything under it.
/// Unreadable entries count as zero (the skill reports sizes, it does not fail on them).
#[must_use]
pub fn size(path: &Path, budget: &mut Budget) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    let dev = meta.dev();
    let mut total = 0_u64;
    let mut stack = vec![(path.to_path_buf(), meta)];
    while let Some((p, m)) = stack.pop() {
        if budget.visits >= MAX_VISITS {
            budget.truncated = true;
            break;
        }
        budget.visits += 1;
        total = total.saturating_add(m.blocks().saturating_mul(512));
        if !m.file_type().is_dir() || m.dev() != dev {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in entries.flatten() {
            if let Ok(child) = e.metadata() {
                // `DirEntry::metadata` does not follow symlinks.
                stack.push((e.path(), child));
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_files_not_symlink_targets() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/f"), vec![1u8; 64 * 1024]).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("big"), vec![1u8; 1 << 20]).unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("a/link")).unwrap();
        let mut b = Budget::default();
        let s = size(d.path(), &mut b);
        assert!(s >= 64 * 1024, "{s}");
        assert!(s < 1 << 20, "followed a symlink: {s}");
        assert!(!b.truncated);
        assert_eq!(size(&d.path().join("missing"), &mut b), 0);
    }

    #[test]
    fn stops_at_the_budget() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(d.path().join(i.to_string()), b"x").unwrap();
        }
        let mut b = Budget {
            visits: MAX_VISITS - 3,
            truncated: false,
        };
        let _ = size(d.path(), &mut b);
        assert!(b.truncated);
    }
}
