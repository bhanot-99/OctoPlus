//! Disk usage of user-relevant filesystems (FR-MON-05, P3-S03). Mounts come from
//! `/proc/self/mountinfo`; sizes from `statvfs` (ADR-0007).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use octoplus_common::config::MonitorConfig;
use octoplus_proto::v1::DiskSample;
use octoplus_proto::v1::monitor_event::Event;
use tokio::time::MissedTickBehavior;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// Filesystem types that hold a user's files (FR-MON-05). Pseudo, tmpfs, overlay and
/// squashfs are not here, so they never appear.
const REAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "zfs",
    "fuseblk",
];
/// Mount points under these are never reported (virtual, runtime or container storage).
const SKIP_PREFIXES: &[&str] = &["/proc", "/sys", "/dev", "/run", "/snap", "/var/lib/docker"];

/// One line of `mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Where it is mounted, escapes decoded.
    pub mount_point: String,
    /// Filesystem type.
    pub fstype: String,
    /// `major:minor` of the device.
    pub dev: String,
    /// The root of the mount inside the filesystem (differs for bind mounts).
    pub root: String,
}

/// `\ooo` at the start of `b` as a byte, if it is a valid three-digit octal escape.
fn octal_escape(b: &[u8]) -> Option<u8> {
    let digits = b.strip_prefix(b"\\")?.get(..3)?;
    if !digits.iter().all(|c| (b'0'..=b'7').contains(c)) {
        return None;
    }
    let v = digits.iter().fold(0u16, |a, c| a * 8 + u16::from(c - b'0'));
    u8::try_from(v).ok()
}

/// Decodes the `\040`-style octal escapes `mountinfo` uses for spaces and tabs.
#[must_use]
pub fn decode_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(byte) = octal_escape(&b[i..]) {
            out.push(byte);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `mountinfo`; malformed lines are skipped.
#[must_use]
pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let f: Vec<&str> = left.split_whitespace().collect();
            let fstype = right.split_whitespace().next()?;
            Some(Mount {
                dev: (*f.get(2)?).to_owned(),
                root: decode_octal(f.get(3)?),
                mount_point: decode_octal(f.get(4)?),
                fstype: fstype.to_owned(),
            })
        })
        .collect()
}

/// Whether FR-MON-05 wants this mount reported.
#[must_use]
pub fn user_relevant(m: &Mount) -> bool {
    REAL_FS.contains(&m.fstype.as_str())
        && !SKIP_PREFIXES.iter().any(|p| {
            m.mount_point == *p
                || m.mount_point
                    .strip_prefix(p)
                    .is_some_and(|r| r.starts_with('/'))
        })
}

/// User-relevant mounts, each mount point once and each *filesystem* once: a folder bound
/// from the same device (the unit's own bind mounts, `mount --bind`) reports the whole
/// filesystem again, so it is dropped. Btrfs is the exception: its subvolumes (`/`, `/home`)
/// are mounted from one device with different roots and are separate mounts to the user, so
/// there the (device, root) pair is the identity (review F4).
#[must_use]
pub fn relevant_mounts(text: &str) -> Vec<Mount> {
    let mut points = HashSet::new();
    let mut fs: HashSet<(String, String)> = HashSet::new();
    parse_mountinfo(text)
        .into_iter()
        .filter(user_relevant)
        .filter(|m| {
            let root = if m.fstype == "btrfs" {
                m.root.clone()
            } else {
                String::new()
            };
            points.insert(m.mount_point.clone()) && fs.insert((m.dev.clone(), root))
        })
        .collect()
}

/// `(total, used, free)` bytes from `statvfs` counters. `free` is what an ordinary user can
/// still write (`f_bavail`), which is less than the filesystem's free space when blocks are
/// reserved; `used` counts the reserved blocks as free, like `df`. `None` on overflow.
#[must_use]
pub fn usage_from(blocks: u64, frsize: u64, bfree: u64, bavail: u64) -> Option<(u64, u64, u64)> {
    let total = blocks.checked_mul(frsize)?;
    let free = bavail.checked_mul(frsize)?;
    let used = total.saturating_sub(bfree.checked_mul(frsize)?);
    Some((total, used, free))
}

/// `(total, used, free)` bytes for a mount point.
fn usage(mount_point: &str) -> Option<(u64, u64, u64)> {
    let s = rustix::fs::statvfs(mount_point).ok()?;
    usage_from(s.f_blocks, s.f_frsize, s.f_bfree, s.f_bavail)
}

/// One reading of every relevant mount. A mount whose `statvfs` fails (gone, no permission)
/// is skipped this time; an unreadable mountinfo is an error.
fn sample_all(mountinfo: &Path) -> std::io::Result<Vec<DiskSample>> {
    let text = std::fs::read_to_string(mountinfo)?;
    Ok(relevant_mounts(&text)
        .into_iter()
        .filter_map(|m| {
            let (total, used, free) = usage(&m.mount_point)?;
            Some(DiskSample {
                mount_point: m.mount_point,
                total_bytes: total,
                used_bytes: used,
                free_bytes: free,
            })
        })
        .collect())
}

/// Samples every relevant mount every `disk_interval_s`.
#[derive(Debug)]
pub struct DiskSensor {
    mountinfo: PathBuf,
    interval: Duration,
}

impl DiskSensor {
    /// The real `/proc/self/mountinfo`.
    #[must_use]
    pub fn new(cfg: &MonitorConfig) -> Self {
        Self::with_mountinfo(cfg, Path::new("/proc/self/mountinfo"))
    }

    /// A mount list read from `mountinfo` (tests point it at a fixture whose mount points
    /// are real folders, so `statvfs` still works).
    #[must_use]
    pub fn with_mountinfo(cfg: &MonitorConfig, mountinfo: &Path) -> Self {
        Self {
            mountinfo: mountinfo.to_path_buf(),
            interval: Duration::from_secs(cfg.disk_interval_s),
        }
    }
}

impl Sensor for DiskSensor {
    fn name(&self) -> &'static str {
        "disk"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let mut tick = tokio::time::interval(self.interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // Reading mountinfo and `statvfs` can stall on a hung drive or FUSE mount, so the
            // whole sample runs on the blocking pool (AB-11, review F5).
            let path = self.mountinfo.clone();
            let samples = tokio::task::spawn_blocking(move || sample_all(&path))
                .await
                .map_err(|_| SensorError::Unavailable("disk scan failed"))?
                .map_err(|source| SensorError::Io {
                    detail: "mountinfo unreadable",
                    source,
                })?;
            out.set_ok();
            for d in samples {
                out.emit(out.now_ns(), Event::Disk(d));
            }
        }
    }
}
