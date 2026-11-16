//! W12 fault injection on the real binary (`local_`): kill during write → the database is intact
//! and C3 restarts with its data. Needs the built `neuroosctl` next to the test binaries.
//! (A real full-disk run in a private tmpfs was attempted and not completed: see the phase report.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn ctl() -> Option<PathBuf> {
    let me = PathBuf::from(env!("CARGO_BIN_EXE_neuroos-storage"));
    let p = me.parent()?.join("neuroosctl");
    p.exists().then_some(p)
}

fn dump() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monitor-dumps/synthetic-1.dump")
}

struct Env {
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn data(&self) -> PathBuf {
        self.root.path().join("data")
    }

    fn runtime(&self) -> PathBuf {
        self.root.path().join("run")
    }

    fn db(&self) -> PathBuf {
        self.data().join("neuroos/storage/neuroos.sqlite3")
    }

    fn storage(&self, wrap: Option<&str>) -> Child {
        let bin = env!("CARGO_BIN_EXE_neuroos-storage");
        let args = format!(
            "--runtime-dir {} --allow-uid {}",
            self.runtime().display(),
            neuroos_ipc::peer::current_uid()
        );
        let mut c = match wrap {
            Some(w) => {
                let mut c = Command::new("unshare");
                c.args(["-Urm", "sh", "-c", &format!("{w}; exec {bin} {args}")]);
                c
            }
            None => {
                let mut c = Command::new(bin);
                c.args(args.split(' '));
                c
            }
        };
        c.env("XDG_DATA_HOME", self.data())
            .env("HOME", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join("cfg"))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c.spawn().unwrap()
    }

    fn feed(&self, speed: &str) -> Child {
        let sock = self.runtime().join("monitor/stream/stream.sock");
        Command::new(ctl().unwrap())
            .args([
                "replay",
                dump().to_str().unwrap(),
                "--speed",
                speed,
                "--serve",
                sock.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn counts(db: &Path) -> (String, i64, i64, i64) {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let ok: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    let chunks: i64 = c
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    let fts: i64 = c
        .query_row("SELECT COUNT(*) FROM chunks_fts_docsize", [], |r| r.get(0))
        .unwrap();
    let hist: i64 = c
        .query_row("SELECT COUNT(*) FROM focus_history", [], |r| r.get(0))
        .unwrap();
    (ok, chunks, fts, hist)
}

fn search(env: &Env, text: &str) -> bool {
    Command::new(ctl().unwrap())
        .args([
            "--runtime-dir",
            env.runtime().to_str().unwrap(),
            "storage",
            "search",
            text,
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn local_a_kill_during_ingest_leaves_a_sound_database_that_reopens_with_its_data() {
    if ctl().is_none() {
        eprintln!("skipped: build neuroosctl first (cargo build -p neuroosctl)");
        return;
    }
    let env = Env::new();
    let mut feeder = env.feed("0");
    let mut s = env.storage(None);
    wait_for("the database to appear", 20, || env.db().exists());
    wait_for("some history to be written", 20, || {
        env.db().exists() && counts(&env.db()).3 > 20
    });
    s.kill().unwrap(); // SIGKILL in the middle of the stream
    s.wait().unwrap();
    let (ok, chunks, fts, hist) = counts(&env.db());
    assert_eq!(ok, "ok", "integrity after SIGKILL");
    assert_eq!(chunks, fts, "the text index is in step with the table");
    assert!(hist > 0);
    // Restart with no feed: C3 comes back, keeps its data and answers.
    feeder.kill().unwrap();
    let _ = feeder.wait();
    let mut s2 = env.storage(None);
    wait_for("C3 to answer after the restart", 20, || {
        search(&env, "Used")
    });
    s2.kill().unwrap();
    let _ = s2.wait();
    let (ok, chunks2, _, _) = counts(&env.db());
    assert_eq!(ok, "ok");
    assert!(chunks2 >= chunks, "nothing was lost by the restart");
}
