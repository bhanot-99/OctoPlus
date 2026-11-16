//! File activity in the folders the user configured (FR-MON-11, P3-S07, OQ-05): git
//! repositories, notes and calendar files. inotify through `rustix`; nothing outside the
//! configured folders is watched, symlinks are never followed, and file *contents* are never
//! read. Events are coalesced so a burst of writes is one event.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use neuroos_common::config::{FolderConfig, FolderKind, MonitorConfig};
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{FileActivity, FileActivityKind};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::io::Errno;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::sensor::{Sensor, SensorError, SensorOut, StopOnDrop};

/// Most watches one sensor holds (the kernel's per-user limit is shared with everything else).
pub const MAX_WATCHES: usize = 1024;
/// Deepest subfolder watched under a notes or calendar folder.
pub const MAX_DEPTH: usize = 8;
/// A path and kind seen again within this window is not emitted again.
pub const COALESCE: Duration = Duration::from_millis(500);
/// Entries remembered by the coalescer before old ones are dropped (bounded memory, rules §9.7).
const COALESCE_CAP: usize = 4096;

/// File names in `.git` whose change means "something happened in this repository".
const GIT_FILES: &[&str] = &["HEAD", "index", "ORIG_HEAD", "FETCH_HEAD", "MERGE_HEAD"];

/// Whether `rel` (relative to the configured folder) is worth an event for `kind`.
#[must_use]
pub fn relevant(kind: FolderKind, rel: &Path) -> bool {
    let comps: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let Some(last) = comps.last() else {
        return false;
    };
    if comps.len() != rel.components().count() {
        return false; // a non-UTF-8 name: not ours to describe
    }
    match kind {
        FolderKind::Git => match comps.as_slice() {
            [".git", name] => GIT_FILES.contains(name),
            [".git", "refs", "heads", ..] => true,
            _ => false,
        },
        FolderKind::Notes | FolderKind::Calendar => {
            if comps.iter().any(|c| c.starts_with('.')) {
                return false; // hidden files and folders, `.git` included
            }
            let ext = Path::new(last)
                .extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase);
            matches!(
                (kind, ext.as_deref()),
                (FolderKind::Notes, Some("md" | "markdown" | "txt" | "org"))
                    | (FolderKind::Calendar, Some("ics"))
            )
        }
    }
}

/// The wire kind for an inotify mask, if it is one we report.
#[must_use]
pub fn kind_of(mask: ReadFlags) -> Option<FileActivityKind> {
    if mask.intersects(ReadFlags::CREATE | ReadFlags::MOVED_TO) {
        Some(FileActivityKind::Created)
    } else if mask.intersects(ReadFlags::DELETE | ReadFlags::MOVED_FROM) {
        Some(FileActivityKind::Removed)
    } else if mask.intersects(ReadFlags::MODIFY | ReadFlags::CLOSE_WRITE) {
        Some(FileActivityKind::Modified)
    } else {
        None
    }
}

/// Drops repeats of the same (path, kind) inside a window. Time is an argument, so tests are exact.
#[derive(Debug)]
pub struct Coalescer {
    window: Duration,
    seen: HashMap<(String, i32), Instant>,
}

impl Coalescer {
    /// A coalescer with the given window.
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            seen: HashMap::new(),
        }
    }

    /// True if this (path, kind) should be emitted at `now`.
    pub fn admit(&mut self, path: &str, kind: FileActivityKind, now: Instant) -> bool {
        if self.seen.len() >= COALESCE_CAP {
            let window = self.window;
            self.seen.retain(|_, t| now.duration_since(*t) < window);
            if self.seen.len() >= COALESCE_CAP {
                self.seen.clear(); // a flood of distinct paths: forget rather than grow
            }
        }
        let key = (path.to_owned(), kind as i32);
        match self.seen.get(&key) {
            Some(t) if now.duration_since(*t) < self.window => false,
            _ => {
                self.seen.insert(key, now);
                true
            }
        }
    }
}

/// One watched directory.
#[derive(Debug, Clone)]
struct Watched {
    root: usize,
    dir: PathBuf,
}

/// Emits `FileActivity` for configured folders.
#[derive(Debug)]
pub struct FoldersSensor {
    folders: Vec<FolderConfig>,
    max_watches: usize,
}

impl FoldersSensor {
    /// Watches `[[monitor.folders]]`.
    #[must_use]
    pub fn new(cfg: &MonitorConfig) -> Self {
        Self {
            folders: cfg.folders.clone(),
            max_watches: MAX_WATCHES,
        }
    }

    /// Like [`FoldersSensor::new`] with a smaller watch limit (tests).
    #[must_use]
    pub fn with_max_watches(cfg: &MonitorConfig, max_watches: usize) -> Self {
        Self {
            folders: cfg.folders.clone(),
            max_watches,
        }
    }
}

fn flags() -> WatchFlags {
    WatchFlags::CREATE
        | WatchFlags::DELETE
        | WatchFlags::MODIFY
        | WatchFlags::CLOSE_WRITE
        | WatchFlags::MOVED_FROM
        | WatchFlags::MOVED_TO
        | WatchFlags::ONLYDIR
        | WatchFlags::DONT_FOLLOW
}

/// The set of watches, and what they cost.
struct Watches {
    fd: OwnedFd,
    by_wd: HashMap<i32, Watched>,
    max: usize,
    limit_hit: bool,
}

impl Watches {
    fn add(&mut self, root: usize, dir: &Path) -> bool {
        if self.by_wd.len() >= self.max {
            self.limit_hit = true;
            return false;
        }
        match inotify::add_watch(&self.fd, dir, flags()) {
            Ok(wd) => {
                self.by_wd.insert(
                    wd,
                    Watched {
                        root,
                        dir: dir.to_path_buf(),
                    },
                );
                true
            }
            Err(_) => false, // vanished or unreadable: skip it, never fail the sensor
        }
    }

    /// Watches `dir` and its real subfolders (symlinks are skipped, hidden folders too).
    fn add_tree(&mut self, root: usize, dir: &Path, depth: usize) {
        if !self.add(root, dir) || depth >= MAX_DEPTH {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else { continue };
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if ft.is_dir() && !ft.is_symlink() && !hidden {
                self.add_tree(root, &entry.path(), depth + 1);
            }
        }
    }

    fn add_folder(&mut self, idx: usize, f: &FolderConfig) -> bool {
        if !f.path.is_dir() {
            return false;
        }
        match f.kind {
            FolderKind::Git => {
                let git = f.path.join(".git");
                self.add(idx, &git);
                self.add(idx, &git.join("refs/heads"));
            }
            FolderKind::Notes | FolderKind::Calendar => self.add_tree(idx, &f.path, 0),
        }
        true
    }
}

/// One decoded event: where, what.
struct Seen {
    wd: i32,
    name: Option<String>,
    mask: ReadFlags,
}

fn drain(fd: &OwnedFd) -> std::io::Result<Vec<Seen>> {
    let mut buf = [MaybeUninit::<u8>::uninit(); 4096];
    let mut reader = inotify::Reader::new(fd, &mut buf);
    let mut out = Vec::new();
    loop {
        match reader.next() {
            Ok(ev) => out.push(Seen {
                wd: ev.wd(),
                name: ev.file_name().map(|n| n.to_string_lossy().into_owned()),
                mask: ev.events(),
            }),
            Err(Errno::WOULDBLOCK) => return Ok(out),
            Err(e) => return Err(e.into()),
        }
    }
}

impl FoldersSensor {
    fn handle(&self, w: &mut Watches, co: &mut Coalescer, out: &SensorOut, ev: &Seen) {
        let Some(dir) = w.by_wd.get(&ev.wd).cloned() else {
            return;
        };
        let Some(name) = ev.name.as_deref() else {
            return;
        };
        let full = dir.dir.join(name);
        let is_dir = ev.mask.contains(ReadFlags::ISDIR);
        let folder = &self.folders[dir.root];
        // A new subfolder in a notes or calendar tree is watched too.
        if is_dir && ev.mask.intersects(ReadFlags::CREATE | ReadFlags::MOVED_TO) {
            if folder.kind != FolderKind::Git && !name.starts_with('.') {
                w.add_tree(dir.root, &full, MAX_DEPTH.saturating_sub(1));
            }
            return;
        }
        let Some(kind) = kind_of(ev.mask).filter(|_| !is_dir) else {
            return;
        };
        let Ok(rel) = full.strip_prefix(&folder.path) else {
            return;
        };
        if !relevant(folder.kind, rel) {
            return;
        }
        let path = full.to_string_lossy().into_owned();
        if co.admit(&path, kind, Instant::now()) {
            out.emit(
                out.now_ns(),
                Event::File(FileActivity {
                    path,
                    kind: kind as i32,
                }),
            );
        }
    }
}

/// How long the reader thread waits for events before re-checking its stop flag.
const WAKE: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 500_000_000,
};

/// Reads inotify on its own thread (blocking is fine there) and forwards batches of events.
fn reader_thread(fd: &OwnedFd, tx: &mpsc::Sender<std::io::Result<Vec<Seen>>>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        let mut fds = [PollFd::new(fd, PollFlags::IN)];
        if poll(&mut fds, Some(&WAKE)).unwrap_or(0) == 0 {
            continue;
        }
        let batch = drain(fd);
        let failed = batch.is_err();
        if tx.blocking_send(batch).is_err() || failed {
            return;
        }
    }
}

impl Sensor for FoldersSensor {
    fn name(&self) -> &'static str {
        "folders"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let io = |detail| move |source| SensorError::Io { detail, source };
        let fd = inotify::init(CreateFlags::NONBLOCK | CreateFlags::CLOEXEC)
            .map_err(|e| io("inotify unavailable")(e.into()))?;
        let mut w = Watches {
            fd,
            by_wd: HashMap::new(),
            max: self.max_watches,
            limit_hit: false,
        };
        let mut missing = false;
        for (i, f) in self.folders.iter().enumerate() {
            missing |= !w.add_folder(i, f);
        }
        let (tx, mut rx) = mpsc::channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let _guard = StopOnDrop(Arc::clone(&stop));
        let reader_fd = w.fd.try_clone().map_err(io("inotify descriptor"))?;
        std::thread::Builder::new()
            .name("c1-inotify".into())
            .spawn(move || reader_thread(&reader_fd, &tx, &stop))
            .map_err(io("cannot start the inotify thread"))?;
        let mut co = Coalescer::new(COALESCE);
        let report = |out: &SensorOut, w: &Watches| match (missing, w.limit_hit) {
            (_, true) => out.degraded("watch limit reached"),
            (true, false) => out.degraded("a configured folder is missing"),
            (false, false) => out.set_ok(),
        };
        report(out, &w);
        while let Some(batch) = rx.recv().await {
            for ev in &batch.map_err(io("inotify read"))? {
                self.handle(&mut w, &mut co, out, ev);
            }
            report(out, &w);
        }
        Err(SensorError::Unavailable("inotify thread ended"))
    }
}
