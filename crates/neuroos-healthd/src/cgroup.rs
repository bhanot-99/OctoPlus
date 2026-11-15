//! cgroup v2 metrics reader for `neuroos-healthd` (FR-HLT-03).
//!
//! Locates per-unit cgroups under `/sys/fs/cgroup/user.slice/user-<uid>.slice/user@<uid>.service/app.slice/<unit>`
//! (and `/sys/fs/cgroup/system.slice/neuroos-fetcher.service` for fetcher).
//! Reads `memory.current` and `cpu.stat` (`usage_usec`). Missing files or parsing failures return 0.

use std::fs;
use std::path::PathBuf;

/// Reads cgroup v2 metrics for Semantic-NeuroOS units.
#[derive(Debug, Clone)]
pub struct CgroupReader {
    cgroup_root: PathBuf,
    uid: u32,
}

impl CgroupReader {
    /// Creates a reader with explicit cgroup root and user ID (useful for tests).
    #[must_use]
    pub fn new(cgroup_root: PathBuf, uid: u32) -> Self {
        Self { cgroup_root, uid }
    }

    /// Creates a reader using the system cgroup mount (`/sys/fs/cgroup`) and current user ID.
    #[must_use]
    pub fn from_system() -> Self {
        let uid = detect_uid();
        Self::new(PathBuf::from("/sys/fs/cgroup"), uid)
    }

    /// Returns the directory containing cgroup files for `unit`.
    #[must_use]
    pub fn unit_cgroup_dir(&self, unit: &str) -> PathBuf {
        if unit == "neuroos-fetcher.service" {
            self.cgroup_root.join("system.slice").join(unit)
        } else {
            self.cgroup_root
                .join("user.slice")
                .join(format!("user-{}.slice", self.uid))
                .join(format!("user@{}.service", self.uid))
                .join("app.slice")
                .join(unit)
        }
    }

    /// Reads memory usage in bytes from `<cgroup_dir>/memory.current`.
    ///
    /// Returns 0 if the file is missing, unreadable, or unparseable.
    #[must_use]
    pub fn read_memory_bytes(&self, unit: &str) -> u64 {
        let path = self.unit_cgroup_dir(unit).join("memory.current");
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return 0,
        };
        content.trim().parse::<u64>().unwrap_or(0)
    }

    /// Reads CPU usage in nanoseconds from `<cgroup_dir>/cpu.stat` (`usage_usec * 1000`).
    ///
    /// Returns 0 if the file is missing, unreadable, or `usage_usec` is absent.
    #[must_use]
    pub fn read_cpu_usage_ns(&self, unit: &str) -> u64 {
        let path = self.unit_cgroup_dir(unit).join("cpu.stat");
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return 0,
        };
        parse_cpu_usage_usec(&content).map_or(0, |usec| usec.saturating_mul(1_000))
    }
}

/// Parses the `usage_usec` integer from `cpu.stat` file content.
#[must_use]
pub fn parse_cpu_usage_usec(content: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(val_str) = line.strip_prefix("usage_usec ")
            && let Ok(val) = val_str.trim().parse::<u64>()
        {
            return Some(val);
        }
    }
    None
}

/// Detects the user ID, preferring parsing `/proc/self/cgroup` or falling back to `getuid`.
#[must_use]
pub fn detect_uid() -> u32 {
    if let Ok(cgroup) = fs::read_to_string("/proc/self/cgroup")
        && let Some(uid) = parse_uid_from_cgroup(&cgroup)
    {
        return uid;
    }
    rustix::process::getuid().as_raw()
}

/// Extracts UID from cgroup content like `0::/user.slice/user-1000.slice/...`.
#[must_use]
pub fn parse_uid_from_cgroup(content: &str) -> Option<u32> {
    for line in content.lines() {
        if let Some(idx) = line.find("user.slice/user-") {
            let rest = &line[idx + "user.slice/user-".len()..];
            if let Some(end) = rest.find(".slice")
                && let Ok(uid) = rest[..end].parse::<u32>()
            {
                return Some(uid);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_uid_from_cgroup() {
        let sample = "0::/user.slice/user-1000.slice/session-3.scope\n";
        assert_eq!(parse_uid_from_cgroup(sample), Some(1000));

        let app_sample =
            "0::/user.slice/user-1001.slice/user@1001.service/app.slice/neuroos-healthd.service\n";
        assert_eq!(parse_uid_from_cgroup(app_sample), Some(1001));

        let root_sample = "0::/\n";
        assert_eq!(parse_uid_from_cgroup(root_sample), None);
    }

    #[test]
    fn test_parse_cpu_usage_usec() {
        let content = "usage_usec 14716\nuser_usec 6090\nsystem_usec 8626\n";
        assert_eq!(parse_cpu_usage_usec(content), Some(14716));

        let empty = "other_stat 1234\n";
        assert_eq!(parse_cpu_usage_usec(empty), None);
    }

    #[test]
    fn test_cgroup_reader_with_fixtures() {
        let tmp = tempdir().unwrap();
        let cgroup_root = tmp.path().to_path_buf();
        let reader = CgroupReader::new(cgroup_root.clone(), 1000);

        let user_unit = "neuroos-monitor.service";
        let user_dir = reader.unit_cgroup_dir(user_unit);
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("memory.current"), b"430080\n").unwrap();
        fs::write(user_dir.join("cpu.stat"), b"usage_usec 5000\n").unwrap();

        assert_eq!(reader.read_memory_bytes(user_unit), 430080);
        assert_eq!(reader.read_cpu_usage_ns(user_unit), 5_000_000);

        // Fetcher system slice
        let fetcher_unit = "neuroos-fetcher.service";
        let fetcher_dir = reader.unit_cgroup_dir(fetcher_unit);
        fs::create_dir_all(&fetcher_dir).unwrap();
        fs::write(fetcher_dir.join("memory.current"), b"1048576\n").unwrap();
        fs::write(fetcher_dir.join("cpu.stat"), b"usage_usec 12000\n").unwrap();

        assert_eq!(reader.read_memory_bytes(fetcher_unit), 1048576);
        assert_eq!(reader.read_cpu_usage_ns(fetcher_unit), 12_000_000);

        // Non-existent unit returns 0
        assert_eq!(reader.read_memory_bytes("neuroos-unknown.service"), 0);
        assert_eq!(reader.read_cpu_usage_ns("neuroos-unknown.service"), 0);
    }
}
