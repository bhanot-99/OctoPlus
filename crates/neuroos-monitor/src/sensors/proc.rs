//! Process tree of the focused window (FR-MON-08, P3-S04).
//!
//! No Wayland protocol gives a window's process id (lessons-from-v1 §5), so the pid is a
//! *hint* found by matching the window's `app_id` against running processes. Matching is
//! conservative: an empty app id, a name that is too short, or two unrelated candidates
//! all yield "no match" rather than a guess. The parsers are pure functions over text.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{ProcessNode, ProcessTree};
use tokio::sync::watch;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// Most processes one tree may hold, so a fork bomb cannot blow the frame limit (FR-MON-08).
pub const MAX_TREE_NODES: usize = 256;
/// Wait after a focus change before scanning, so the new window's processes exist (FR-MON-08).
pub const SETTLE: Duration = Duration::from_millis(300);
/// The kernel truncates `comm` to this many bytes.
const COMM_MAX: usize = 15;
/// App ids shorter than this are never matched: too many unrelated processes would fit.
const MIN_NAME_LEN: usize = 3;

/// What `/proc` says about one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    /// Process id.
    pub pid: u32,
    /// Parent process id.
    pub ppid: u32,
    /// Kernel `comm` (truncated to 15 bytes).
    pub comm: String,
    /// Basename of the executable, when readable.
    pub exe: Option<String>,
    /// Basename of the first command-line argument, when readable.
    pub arg0: Option<String>,
    /// Start time in clock ticks since boot.
    pub start: u64,
}

/// Parses `/proc/<pid>/stat`: `(ppid, comm, starttime)`. The command name sits in
/// parentheses and may itself contain spaces and parentheses, so the *last* `)` ends it.
#[must_use]
pub fn parse_stat(text: &str) -> Option<(u32, String, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = text.get(open + 1..close)?.to_owned();
    let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // After the name: state(0) ppid(1) ... starttime is field 22 overall = index 19 here.
    let ppid = rest.get(1)?.parse().ok()?;
    let start = rest.get(19)?.parse().ok()?;
    Some((ppid, comm, start))
}

/// Basename of `cmdline`'s first NUL-separated argument.
#[must_use]
pub fn arg0_basename(cmdline: &[u8]) -> Option<String> {
    let first = cmdline.split(|b| *b == 0).next()?;
    let text = String::from_utf8_lossy(first);
    let base = text.rsplit('/').next()?.trim();
    (!base.is_empty()).then(|| base.to_owned())
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The names an `app_id` may go by: the whole id and its last dotted segment
/// (`org.mozilla.firefox` → `firefox`), `.desktop` stripped, punctuation ignored.
fn app_names(app_id: &str) -> Vec<String> {
    let id = app_id.trim().trim_end_matches(".desktop");
    let mut names = vec![norm(id)];
    if let Some(last) = id.rsplit('.').next() {
        names.push(norm(last));
    }
    names.retain(|n| n.len() >= MIN_NAME_LEN);
    names.dedup();
    names
}

/// How well a process fits the names: 3 exact, 2 truncated-`comm` prefix, 0 none.
fn score(p: &ProcInfo, names: &[String]) -> u8 {
    let mut best = 0;
    let candidates = [Some(p.comm.as_str()), p.exe.as_deref(), p.arg0.as_deref()];
    for cand in candidates.into_iter().flatten() {
        let c = norm(cand);
        if c.len() < MIN_NAME_LEN {
            continue;
        }
        for n in names {
            if c == *n {
                best = best.max(3);
            } else if cand.len() >= COMM_MAX && n.starts_with(&c) {
                // `comm` was cut by the kernel: the full name starts with what is left.
                best = best.max(2);
            }
        }
    }
    best
}

/// The pid most likely to be the window's application, or `None` if it is unknown or
/// ambiguous. Of all processes matching equally well, only the topmost of each family
/// counts (children of a matching parent are the same application); if two unrelated
/// families remain, the answer is `None`.
#[must_use]
pub fn match_window(app_id: &str, procs: &[ProcInfo]) -> Option<u32> {
    let names = app_names(app_id);
    if names.is_empty() {
        return None;
    }
    let scored: Vec<(&ProcInfo, u8)> = procs
        .iter()
        .map(|p| (p, score(p, &names)))
        .filter(|(_, s)| *s > 0)
        .collect();
    let top = scored.iter().map(|(_, s)| *s).max()?;
    let best: Vec<&ProcInfo> = scored
        .iter()
        .filter(|(_, s)| *s == top)
        .map(|(p, _)| *p)
        .collect();
    let pids: HashSet<u32> = best.iter().map(|p| p.pid).collect();
    let mut roots: Vec<&ProcInfo> = best
        .into_iter()
        .filter(|p| !pids.contains(&p.ppid))
        .collect();
    roots.sort_by_key(|p| (p.start, p.pid));
    match roots.as_slice() {
        [only] => Some(only.pid),
        _ => None,
    }
}

/// `root` and its descendants, breadth first, at most [`MAX_TREE_NODES`]; safe against
/// cycles and against `root` having vanished (`None`).
#[must_use]
pub fn snapshot_tree(procs: &[ProcInfo], root: u32) -> Option<ProcessTree> {
    let by_pid: HashMap<u32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    by_pid.get(&root)?;
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if p.pid != p.ppid {
            children.entry(p.ppid).or_default().push(p.pid);
        }
    }
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut nodes = Vec::new();
    while let Some(pid) = queue.pop_front() {
        let Some(p) = by_pid.get(&pid) else { continue };
        nodes.push(ProcessNode {
            pid,
            ppid: p.ppid,
            name: p.comm.clone(),
        });
        if nodes.len() >= MAX_TREE_NODES {
            break;
        }
        let mut kids = children.get(&pid).cloned().unwrap_or_default();
        kids.sort_unstable();
        for k in kids {
            if seen.insert(k) {
                queue.push_back(k);
            }
        }
    }
    Some(ProcessTree {
        root_pid: root,
        nodes,
    })
}

/// Reads every readable process under `proc_root` (a process that vanishes mid-scan is
/// skipped, never an error).
#[must_use]
pub fn scan(proc_root: &Path) -> Vec<ProcInfo> {
    let Ok(dir) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    dir.filter_map(Result::ok)
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let base = e.path();
            let (ppid, comm, start) =
                parse_stat(&std::fs::read_to_string(base.join("stat")).ok()?)?;
            let exe = std::fs::read_link(base.join("exe"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            let arg0 = std::fs::read(base.join("cmdline"))
                .ok()
                .and_then(|c| arg0_basename(&c));
            Some(ProcInfo {
                pid,
                ppid,
                comm,
                exe,
                arg0,
                start,
            })
        })
        .collect()
}

/// A cached process table. The focus sensor looks a window's pid up here in microseconds
/// (NFR-PERF-07 forbids a `/proc` walk on the capture path); [`ProcSensor`] keeps it fresh.
#[derive(Debug)]
pub struct ProcTable {
    root: PathBuf,
    procs: RwLock<Arc<Vec<ProcInfo>>>,
}

impl ProcTable {
    /// A table over `proc_root`, filled once at creation.
    #[must_use]
    pub fn new(proc_root: &Path) -> Arc<Self> {
        let t = Arc::new(Self {
            root: proc_root.to_path_buf(),
            procs: RwLock::new(Arc::new(Vec::new())),
        });
        t.refresh();
        t
    }

    /// Re-reads `/proc` (blocking: call from the blocking pool).
    pub fn refresh(&self) {
        let fresh = Arc::new(scan(&self.root));
        *self
            .procs
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = fresh;
    }

    /// The current table.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Vec<ProcInfo>> {
        Arc::clone(
            &self
                .procs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Best-effort pid for a window's app id; 0 when unknown or ambiguous (a hint only).
    #[must_use]
    pub fn resolve(&self, app_id: &str) -> u32 {
        match_window(app_id, &self.snapshot()).unwrap_or(0)
    }
}

/// Re-reads the table this often even without a focus change, so a freshly started
/// application is known by the time it takes focus.
pub const REFRESH_EVERY: Duration = Duration::from_secs(5);

/// Emits the focused application's process tree after each change of focused app.
#[derive(Debug)]
pub struct ProcSensor {
    table: Arc<ProcTable>,
    focused_app: watch::Receiver<String>,
    settle: Duration,
}

impl ProcSensor {
    /// `focused_app` carries the app id of the focused window (empty = none).
    #[must_use]
    pub fn new(table: Arc<ProcTable>, focused_app: watch::Receiver<String>) -> Self {
        Self {
            table,
            focused_app,
            settle: SETTLE,
        }
    }
}

impl Sensor for ProcSensor {
    fn name(&self) -> &'static str {
        "proc"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        out.set_ok();
        let mut refresh = tokio::time::interval(REFRESH_EVERY);
        refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                changed = self.focused_app.changed() => {
                    changed.map_err(|_| SensorError::Unavailable("focus source closed"))?;
                    tokio::time::sleep(self.settle).await;
                    let app = self.focused_app.borrow_and_update().clone();
                    self.emit_tree(out, &app).await?;
                }
                _ = refresh.tick() => {
                    let table = Arc::clone(&self.table);
                    tokio::task::spawn_blocking(move || table.refresh())
                        .await
                        .map_err(|_| SensorError::Unavailable("process scan failed"))?;
                }
            }
        }
    }
}

impl ProcSensor {
    /// Refreshes the table (the new window's processes now exist) and emits the tree.
    async fn emit_tree(&self, out: &SensorOut, app_id: &str) -> Result<(), SensorError> {
        if app_id.is_empty() {
            return Ok(());
        }
        let table = Arc::clone(&self.table);
        let app = app_id.to_owned();
        let tree = tokio::task::spawn_blocking(move || {
            table.refresh();
            let procs = table.snapshot();
            match_window(&app, &procs).and_then(|pid| snapshot_tree(&procs, pid))
        })
        .await
        .map_err(|_| SensorError::Unavailable("process scan failed"))?;
        // The observation is the finished scan, not its start (NFR-PERF-07 times capture, not the read).
        if let Some(tree) = tree {
            out.emit(out.now_ns(), Event::ProcessTree(tree));
        }
        Ok(())
    }
}
