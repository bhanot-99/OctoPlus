//! W4 / P3-S04: pid matching and the focused window's process tree (FR-MON-08).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use neuroos_common::config::MonitorConfig;
use neuroos_health::Health;
use neuroos_monitor::bus::{Bus, PassGate};
use neuroos_monitor::clock::{Clock, FakeClock};
use neuroos_monitor::sensors::proc::{
    MAX_TREE_NODES, ProcInfo, ProcSensor, arg0_basename, match_window, parse_stat, resolve_pid,
    scan, snapshot_tree,
};
use neuroos_monitor::supervisor::supervise;
use neuroos_proto::v1::monitor_event::Event;
use proptest::prelude::*;

fn p(pid: u32, ppid: u32, comm: &str) -> ProcInfo {
    ProcInfo {
        pid,
        ppid,
        comm: comm.into(),
        exe: None,
        arg0: None,
        start: u64::from(pid),
    }
}

const STAT: &str = "4242 (cosmic-term) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 123456 1000 50 18446744073709551615";

#[test]
fn stat_parsing_handles_odd_names_and_junk() {
    assert_eq!(parse_stat(STAT), Some((1, "cosmic-term".into(), 123_456)));
    let weird = STAT.replace("(cosmic-term)", "(a b) (c))");
    assert_eq!(parse_stat(&weird).unwrap().1, "a b) (c)");
    for bad in ["", "no parens", ") (", "1 (x)", "1 (x) S", "1 (x) S 1 2 3"] {
        assert_eq!(parse_stat(bad), None, "{bad:?}");
    }
    assert_eq!(
        parse_stat("1 (x) S notanumber 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 5"),
        None
    );
}

#[test]
fn arg0_is_the_basename_of_the_first_argument() {
    assert_eq!(
        arg0_basename(b"/usr/bin/firefox\0--new-window\0"),
        Some("firefox".into())
    );
    assert_eq!(arg0_basename(b"code\0"), Some("code".into()));
    assert_eq!(arg0_basename(b""), None);
    assert_eq!(arg0_basename(b"\0"), None);
    assert_eq!(arg0_basename(b"/"), None);
}

#[test]
fn empty_and_tiny_app_ids_never_match() {
    let procs = vec![p(10, 1, "a"), p(11, 1, "firefox")];
    assert_eq!(match_window("", &procs), None);
    assert_eq!(match_window("   ", &procs), None);
    assert_eq!(match_window("a", &procs), None);
    assert_eq!(match_window(".desktop", &procs), None);
}

#[test]
fn reverse_dns_ids_match_their_last_segment() {
    let procs = vec![p(10, 1, "bash"), p(20, 1, "firefox")];
    assert_eq!(match_window("org.mozilla.firefox", &procs), Some(20));
    assert_eq!(match_window("firefox.desktop", &procs), Some(20));
    assert_eq!(match_window("Firefox", &procs), Some(20));
}

#[test]
fn a_truncated_comm_matches_the_longer_app_id() {
    // The kernel keeps 15 bytes: "cosmic-files-applet" shows as "cosmic-files-ap".
    let procs = vec![p(30, 1, "cosmic-files-ap")];
    assert_eq!(
        match_window("com.system76.CosmicFilesApplet", &procs),
        Some(30)
    );
    // A short, complete comm is NOT a prefix match: "cosmic" must not claim "cosmic-files".
    let procs = vec![p(31, 1, "cosmic")];
    assert_eq!(match_window("com.system76.CosmicFiles", &procs), None);
}

#[test]
fn arg0_and_exe_count_when_comm_differs() {
    let mut proc = p(40, 1, "MainThread");
    proc.exe = Some("slack".into());
    assert_eq!(match_window("slack", &[proc.clone()]), Some(40));
    proc.exe = None;
    proc.arg0 = Some("slack".into());
    assert_eq!(match_window("Slack", &[proc]), Some(40));
}

#[test]
fn one_family_picks_the_topmost_and_two_unrelated_families_are_ambiguous() {
    let family = vec![
        p(100, 1, "firefox"),
        p(101, 100, "firefox"),
        p(102, 100, "firefox"),
    ];
    assert_eq!(match_window("firefox", &family), Some(100));
    let two = vec![p(100, 1, "firefox"), p(200, 1, "firefox")];
    assert_eq!(
        match_window("firefox", &two),
        None,
        "two separate instances: do not guess"
    );
}

#[test]
fn a_tree_is_breadth_first_bounded_cycle_safe_and_none_for_a_vanished_root() {
    let procs = vec![
        p(1, 0, "init"),
        p(10, 1, "app"),
        p(11, 10, "child"),
        p(12, 10, "child"),
        p(13, 11, "leaf"),
    ];
    let t = snapshot_tree(&procs, 10).unwrap();
    assert_eq!(t.root_pid, 10);
    assert_eq!(
        t.nodes.iter().map(|n| n.pid).collect::<Vec<_>>(),
        vec![10, 11, 12, 13]
    );
    assert!(snapshot_tree(&procs, 999).is_none());

    let cyc = vec![p(1, 2, "a"), p(2, 1, "b")];
    assert_eq!(snapshot_tree(&cyc, 1).unwrap().nodes.len(), 2);

    let mut bomb = vec![p(1, 0, "root")];
    bomb.extend((2..5000).map(|i| p(i, 1, "x")));
    assert_eq!(snapshot_tree(&bomb, 1).unwrap().nodes.len(), MAX_TREE_NODES);
}

fn fake_proc(dir: &std::path::Path, pid: u32, ppid: u32, comm: &str, cmdline: &str) {
    let d = dir.join(pid.to_string());
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("stat"),
        format!("{pid} ({comm}) S {ppid} {pid} {pid} 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {pid}0 0 0"),
    )
    .unwrap();
    std::fs::write(d.join("cmdline"), cmdline).unwrap();
}

#[test]
fn scan_reads_a_fake_proc_and_skips_junk_and_vanished_entries() {
    let dir = tempfile::tempdir().unwrap();
    fake_proc(dir.path(), 50, 1, "myapp", "/opt/myapp/bin/myapp\0--flag\0");
    fake_proc(dir.path(), 51, 50, "helper", "helper\0");
    std::fs::create_dir(dir.path().join("self")).unwrap();
    std::fs::create_dir(dir.path().join("77")).unwrap(); // no stat: vanished
    std::fs::write(dir.path().join("78"), b"a file, not a dir").unwrap();
    let procs = scan(dir.path());
    assert_eq!(procs.len(), 2);
    assert_eq!(resolve_pid(dir.path(), "org.example.MyApp"), 50);
    assert_eq!(resolve_pid(dir.path(), "org.example.Nothing"), 0);
    assert_eq!(snapshot_tree(&procs, 50).unwrap().nodes.len(), 2);
    assert!(scan(&dir.path().join("does-not-exist")).is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_sensor_emits_the_tree_once_after_the_settle_delay() {
    let dir = tempfile::tempdir().unwrap();
    fake_proc(dir.path(), 50, 1, "myapp", "myapp\0");
    fake_proc(dir.path(), 51, 50, "helper", "helper\0");
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1));
    let health = Health::new("neuroos-monitor", "test");
    let bus = Bus::new(
        16,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (pid_tx, pid_rx) = tokio::sync::watch::channel(0u32);
    let sensor = ProcSensor::with_root(&MonitorConfig::default(), dir.path(), pid_rx);
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(supervise(sensor, bus, clock, health, stop_rx));

    pid_tx.send(0).unwrap(); // "no window": nothing to emit
    pid_tx.send(50).unwrap();
    let ev = tokio::time::timeout(Duration::from_secs(5), sub.next())
        .await
        .unwrap();
    let Some(Event::ProcessTree(t)) = ev.event else {
        panic!("tree expected")
    };
    assert_eq!((t.root_pid, t.nodes.len()), (50, 2));
    assert!(sub.try_next().is_none(), "exactly one tree");
    stop_tx.send(true).unwrap();
    task.await.unwrap();
}

#[test]
fn local_the_real_proc_contains_this_process_and_its_tree() {
    let me = std::process::id();
    let procs = scan(std::path::Path::new("/proc"));
    assert!(procs.iter().any(|x| x.pid == me));
    let tree = snapshot_tree(&procs, me).unwrap();
    assert_eq!(tree.nodes[0].pid, me);
}

proptest! {
    #[test]
    fn parsers_and_matcher_never_panic_and_stay_inside_their_inputs(
        text in ".{0,200}",
        app in ".{0,40}",
        names in proptest::collection::vec("[a-zA-Z.-]{0,18}", 0..8),
    ) {
        let _ = parse_stat(&text);
        let _ = arg0_basename(text.as_bytes());
        let procs: Vec<ProcInfo> = names.iter().enumerate()
            .map(|(i, n)| p(u32::try_from(i).unwrap() + 2, u32::try_from(i / 2).unwrap(), n))
            .collect();
        if let Some(pid) = match_window(&app, &procs) {
            prop_assert!(procs.iter().any(|x| x.pid == pid));
            prop_assert!(!app.trim().is_empty());
        }
        if let Some(root) = procs.first().map(|x| x.pid) {
            prop_assert!(snapshot_tree(&procs, root).unwrap().nodes.len() <= MAX_TREE_NODES);
        }
    }
}
