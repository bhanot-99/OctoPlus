//! W7 / P3-S07: file activity in configured folders (FR-MON-11, OQ-05). Real inotify on
//! temp folders; classification and coalescing are also tested as pure functions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use octoplus_common::config::{FolderConfig, FolderKind, MonitorConfig};
use octoplus_health::Health;
use octoplus_monitor::bus::{Bus, PassGate, Subscription};
use octoplus_monitor::clock::{Clock, SystemClock};
use octoplus_monitor::sensors::folders::{COALESCE, Coalescer, FoldersSensor, kind_of, relevant};
use octoplus_monitor::supervise;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{FileActivity, FileActivityKind, HealthStatus};
use proptest::prelude::*;
use rustix::fs::inotify::ReadFlags;
use tokio::sync::watch;

// ---------------------------------------------------------------- pure

#[test]
fn what_counts_as_activity_depends_on_the_folder_kind() {
    let n = FolderKind::Notes;
    assert!(relevant(n, Path::new("a.md")));
    assert!(relevant(n, Path::new("sub/dir/Plan.TXT")));
    assert!(!relevant(n, Path::new("image.png")));
    assert!(!relevant(n, Path::new(".hidden.md")));
    assert!(
        !relevant(n, Path::new(".git/notes.md")),
        "hidden folders are skipped"
    );
    assert!(!relevant(n, Path::new("")));
    assert!(relevant(FolderKind::Calendar, Path::new("work/cal.ics")));
    assert!(!relevant(FolderKind::Calendar, Path::new("a.md")));
    let g = FolderKind::Git;
    assert!(relevant(g, Path::new(".git/HEAD")));
    assert!(relevant(g, Path::new(".git/index")));
    assert!(relevant(g, Path::new(".git/refs/heads/main")));
    assert!(!relevant(g, Path::new(".git/objects/ab/cdef")));
    assert!(
        !relevant(g, Path::new("src/main.rs")),
        "working-tree edits are not repository events"
    );
    assert!(!relevant(g, Path::new(".git/config")));
}

#[test]
fn inotify_masks_map_to_the_three_wire_kinds() {
    assert_eq!(kind_of(ReadFlags::CREATE), Some(FileActivityKind::Created));
    assert_eq!(
        kind_of(ReadFlags::MOVED_TO),
        Some(FileActivityKind::Created)
    );
    assert_eq!(kind_of(ReadFlags::DELETE), Some(FileActivityKind::Removed));
    assert_eq!(
        kind_of(ReadFlags::MOVED_FROM),
        Some(FileActivityKind::Removed)
    );
    assert_eq!(kind_of(ReadFlags::MODIFY), Some(FileActivityKind::Modified));
    assert_eq!(
        kind_of(ReadFlags::CLOSE_WRITE),
        Some(FileActivityKind::Modified)
    );
    assert_eq!(kind_of(ReadFlags::ATTRIB), None);
    assert_eq!(kind_of(ReadFlags::empty()), None);
}

#[test]
fn a_burst_is_one_event_and_the_same_path_later_is_a_new_one() {
    let mut c = Coalescer::new(COALESCE);
    let t0 = tokio::time::Instant::now();
    assert!(c.admit("/n/a.md", FileActivityKind::Modified, t0));
    for i in 1..50 {
        assert!(!c.admit(
            "/n/a.md",
            FileActivityKind::Modified,
            t0 + Duration::from_millis(i)
        ));
    }
    assert!(
        c.admit("/n/a.md", FileActivityKind::Created, t0),
        "another kind is separate"
    );
    assert!(
        c.admit("/n/b.md", FileActivityKind::Modified, t0),
        "another path is separate"
    );
    assert!(c.admit(
        "/n/a.md",
        FileActivityKind::Modified,
        t0 + COALESCE + Duration::from_millis(1)
    ));
}

#[test]
fn the_coalescer_stays_bounded_under_a_flood_of_distinct_paths() {
    let mut c = Coalescer::new(COALESCE);
    let t0 = tokio::time::Instant::now();
    for i in 0..20_000 {
        let _ = c.admit(&format!("/n/{i}.md"), FileActivityKind::Created, t0);
    }
    // Still working, and a fresh path is admitted: memory did not grow without bound.
    assert!(c.admit("/n/fresh.md", FileActivityKind::Created, t0));
}

proptest! {
    #[test]
    fn classification_never_panics(rel in "[a-zA-Z0-9./ _.-]{0,60}") {
        for k in [FolderKind::Git, FolderKind::Notes, FolderKind::Calendar] {
            let _ = relevant(k, Path::new(&rel));
        }
    }
}

// ---------------------------------------------------------------- real inotify

struct Rig {
    sub: Subscription,
    health: Arc<Health>,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

fn cfg(folders: &[(&Path, FolderKind)]) -> MonitorConfig {
    MonitorConfig {
        folders: folders
            .iter()
            .map(|(p, k)| FolderConfig {
                path: p.to_path_buf(),
                kind: *k,
            })
            .collect(),
        ..Default::default()
    }
}

async fn start(sensor: FoldersSensor) -> Rig {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let health = Health::new("octoplus-monitor", "test");
    let bus = Bus::new(
        256,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (stop, stop_rx) = watch::channel(false);
    let task = tokio::spawn(supervise(sensor, bus, clock, Arc::clone(&health), stop_rx));
    // Wait until the watches exist: the sensor reports OK or DEGRADED once it has set them up.
    for _ in 0..200 {
        let snap = health.snapshot();
        if snap
            .subsystems
            .iter()
            .any(|s| s.name == "folders" && s.status() != HealthStatus::Starting)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Rig {
        sub,
        health,
        stop,
        task,
    }
}

impl Rig {
    async fn file(&self) -> Option<FileActivity> {
        let ev = tokio::time::timeout(Duration::from_secs(3), self.sub.next())
            .await
            .ok()?;
        match ev.event {
            Some(Event::File(f)) => Some(f),
            other => panic!("file event expected: {other:?}"),
        }
    }

    /// Everything that arrives until 600 ms of quiet (a create is followed by its close-write).
    async fn all(&self) -> Vec<FileActivity> {
        let mut v = Vec::new();
        while let Ok(ev) = tokio::time::timeout(Duration::from_millis(600), self.sub.next()).await {
            if let Some(Event::File(f)) = ev.event {
                v.push(f);
            }
        }
        v
    }

    async fn quiet(&self) -> bool {
        tokio::time::timeout(Duration::from_millis(600), self.sub.next())
            .await
            .is_err()
    }

    async fn finish(self) {
        self.stop.send(true).unwrap();
        self.task.await.unwrap();
    }

    fn status(&self) -> (HealthStatus, String) {
        let snap = self.health.snapshot();
        let s = snap
            .subsystems
            .iter()
            .find(|s| s.name == "folders")
            .unwrap();
        (s.status(), s.detail.clone())
    }
}

#[tokio::test]
async fn notes_create_modify_remove_and_a_new_subfolder_are_seen_and_noise_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let rig = start(FoldersSensor::new(&cfg(&[(&root, FolderKind::Notes)]))).await;

    std::fs::write(root.join("plan.md"), "x").unwrap();
    let created = rig.file().await.expect("created");
    assert_eq!(
        (created.path.as_str(), created.kind),
        (
            root.join("plan.md").to_str().unwrap(),
            FileActivityKind::Created as i32
        )
    );
    let modified = rig.file().await.expect("close-write is a modification");
    assert_eq!(modified.kind, FileActivityKind::Modified as i32);
    assert!(rig.quiet().await);

    std::fs::write(root.join("picture.png"), "x").unwrap();
    std::fs::write(root.join(".secret.md"), "x").unwrap();
    assert!(
        rig.quiet().await,
        "irrelevant and hidden files make no events"
    );

    std::fs::remove_file(root.join("plan.md")).unwrap();
    assert_eq!(
        rig.file().await.expect("removed").kind,
        FileActivityKind::Removed as i32
    );

    std::fs::create_dir(root.join("2026")).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await; // the new folder gets its watch
    std::fs::write(root.join("2026/day.txt"), "x").unwrap();
    let nested = rig
        .file()
        .await
        .expect("a file in a folder created after start");
    assert!(nested.path.ends_with("2026/day.txt"));
    assert_eq!(rig.status().0, HealthStatus::Ok);
    rig.finish().await;
}

#[tokio::test]
async fn a_burst_of_writes_becomes_few_events() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("log.md"), "").unwrap();
    let rig = start(FoldersSensor::new(&cfg(&[(&root, FolderKind::Notes)]))).await;
    for i in 0..200 {
        std::fs::write(root.join("log.md"), format!("line {i}")).unwrap();
    }
    let mut n = 0;
    while rig.file().await.is_some() {
        n += 1;
        if n > 20 {
            break;
        }
    }
    assert!((1..=3).contains(&n), "200 writes produced {n} events");
    rig.finish().await;
}

#[tokio::test]
async fn a_symlink_out_of_the_folder_is_never_followed() {
    let outside = tempfile::tempdir().unwrap();
    let inside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), inside.path().join("link")).unwrap();
    let rig = start(FoldersSensor::new(&cfg(&[(
        inside.path(),
        FolderKind::Notes,
    )])))
    .await;
    std::fs::write(outside.path().join("private.md"), "x").unwrap();
    assert!(
        rig.quiet().await,
        "activity behind a symlink is outside the configured folder"
    );
    std::fs::write(inside.path().join("ok.md"), "x").unwrap();
    assert!(rig.file().await.is_some(), "the folder itself still works");
    rig.finish().await;
}

#[tokio::test]
async fn git_reports_repository_events_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
    std::fs::create_dir_all(root.join(".git/objects")).unwrap();
    let rig = start(FoldersSensor::new(&cfg(&[(&root, FolderKind::Git)]))).await;

    std::fs::write(root.join("main.rs"), "x").unwrap();
    std::fs::write(root.join(".git/objects/blob"), "x").unwrap();
    assert!(
        rig.quiet().await,
        "working-tree files and objects are not repository events"
    );
    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    let head = rig.all().await;
    assert!(
        !head.is_empty() && head.iter().all(|f| f.path.ends_with(".git/HEAD")),
        "{head:?}"
    );
    std::fs::write(root.join(".git/refs/heads/main"), "abc").unwrap();
    let branch = rig.all().await;
    assert!(
        !branch.is_empty() && branch.iter().all(|f| f.path.ends_with("refs/heads/main")),
        "{branch:?}"
    );
    rig.finish().await;
}

#[tokio::test]
async fn calendar_ics_files_are_seen() {
    let dir = tempfile::tempdir().unwrap();
    let rig = start(FoldersSensor::new(&cfg(&[(
        dir.path(),
        FolderKind::Calendar,
    )])))
    .await;
    std::fs::write(dir.path().join("work.ics"), "BEGIN:VCALENDAR").unwrap();
    let ics = rig.all().await;
    assert!(
        !ics.is_empty() && ics.iter().all(|f| f.path.ends_with("work.ics")),
        "{ics:?}"
    );
    std::fs::write(dir.path().join("notes.md"), "x").unwrap();
    assert!(rig.quiet().await);
    rig.finish().await;
}

#[tokio::test]
async fn the_watch_limit_degrades_health_and_the_sensor_keeps_working() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        std::fs::create_dir(dir.path().join(format!("d{i}"))).unwrap();
    }
    let rig = start(FoldersSensor::with_max_watches(
        &cfg(&[(dir.path(), FolderKind::Notes)]),
        3,
    ))
    .await;
    assert_eq!(
        rig.status(),
        (HealthStatus::Degraded, "watch limit reached".into())
    );
    std::fs::write(dir.path().join("top.md"), "x").unwrap();
    assert!(rig.file().await.is_some(), "the watched part still reports");
    rig.finish().await;
}

#[tokio::test]
async fn a_missing_folder_is_reported_not_fatal() {
    let good = tempfile::tempdir().unwrap();
    let missing = PathBuf::from("/definitely/not/here");
    let rig = start(FoldersSensor::new(&cfg(&[
        (&missing, FolderKind::Notes),
        (good.path(), FolderKind::Notes),
    ])))
    .await;
    assert_eq!(
        rig.status(),
        (
            HealthStatus::Degraded,
            "a configured folder is missing".into()
        )
    );
    std::fs::write(good.path().join("a.md"), "x").unwrap();
    assert!(rig.file().await.is_some(), "the other folder still works");
    rig.finish().await;
}

#[tokio::test]
async fn no_folders_configured_is_healthy_and_silent() {
    let rig = start(FoldersSensor::new(&MonitorConfig::default())).await;
    assert_eq!(rig.status().0, HealthStatus::Ok);
    assert!(rig.quiet().await);
    rig.finish().await;
}

/// Review F8 (agy): a deleted watched folder frees its watch, so a busy notes tree never
/// exhausts the limit with dead entries.
#[tokio::test]
async fn deleting_a_watched_folder_frees_its_watch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir(root.join("a")).unwrap();
    // Two watches only: the root and `a`.
    let rig = start(FoldersSensor::with_max_watches(
        &cfg(&[(&root, FolderKind::Notes)]),
        2,
    ))
    .await;
    assert_eq!(rig.status().0, HealthStatus::Ok);
    std::fs::remove_dir(root.join("a")).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await; // the kernel's IN_IGNORED arrives
    std::fs::create_dir(root.join("b")).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await; // `b` gets the freed watch
    std::fs::write(root.join("b/note.md"), "x").unwrap();
    let seen = rig.all().await;
    assert!(
        seen.iter().any(|f| f.path.ends_with("b/note.md")),
        "a file in the new folder must be seen once the dead watch is gone: {seen:?}"
    );
    assert_eq!(
        rig.status().0,
        HealthStatus::Ok,
        "no false 'watch limit reached'"
    );
    rig.finish().await;
}
