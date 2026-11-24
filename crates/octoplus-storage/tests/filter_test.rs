//! W4 / P4-S01, P4-S02: the four-stage ingest filter and the adapters (FR-STO-01/02).
//! Security-critical (NFR-QA-01): every decision outcome is a table or property test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_common::config::StorageConfig;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{
    DiskSample, Domain, FileActivity, FileActivityKind, FocusChanged, IdleChanged, MediaChanged,
    MonitorEvent, PlaybackState, ProcessNode, ProcessTree, ResourceSample,
};
use octoplus_storage::ingest::adapters::{classify_file, health_points};
use octoplus_storage::ingest::filter::{IngestFilter, Stages, same_program};
use octoplus_storage::ingest::{Demotion, Item, Output};
use proptest::prelude::*;

const S: u64 = 1_000_000_000;

fn cfg() -> StorageConfig {
    StorageConfig::default()
}

fn ev(at: u64, e: Event) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: at,
        event: Some(e),
    }
}

fn focus(at: u64, app: &str, title: &str) -> MonitorEvent {
    ev(
        at,
        Event::Focus(FocusChanged {
            app_id: app.into(),
            title: title.into(),
            pid: 1,
            fullscreen: false,
            fullscreen_known: true,
        }),
    )
}

fn idle(at: u64, i: bool) -> MonitorEvent {
    ev(at, Event::Idle(IdleChanged { idle: i }))
}

fn media(at: u64, player: &str, state: PlaybackState, title: &str) -> MonitorEvent {
    ev(
        at,
        Event::Media(MediaChanged {
            player: player.into(),
            state: state as i32,
            title: title.into(),
            artist: "A".into(),
        }),
    )
}

fn tree(at: u64, root: &str, kids: &[&str]) -> MonitorEvent {
    let mut nodes = vec![ProcessNode {
        pid: 1,
        ppid: 0,
        name: root.into(),
    }];
    nodes.extend(kids.iter().enumerate().map(|(i, k)| ProcessNode {
        pid: 2 + i as u32,
        ppid: 1,
        name: (*k).into(),
    }));
    ev(at, Event::ProcessTree(ProcessTree { root_pid: 1, nodes }))
}

fn file(at: u64, path: &str, kind: FileActivityKind) -> MonitorEvent {
    ev(
        at,
        Event::File(FileActivity {
            path: path.into(),
            kind: kind as i32,
        }),
    )
}

fn run(f: &mut IngestFilter, events: &[MonitorEvent]) -> Vec<Output> {
    events.iter().flat_map(|e| f.push(e)).collect()
}

fn items(out: &[Output]) -> Vec<&Item> {
    out.iter()
        .filter_map(|o| {
            if let Output::Promote(i) = o {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn count(out: &[Output], d: Demotion) -> usize {
    out.iter()
        .filter(|o| matches!(o, Output::Demoted(x) if *x == d))
        .count()
}

// ---- stage 2: own windows -------------------------------------------------------------------

#[test]
fn octoplus_own_windows_are_dropped_and_their_process_trees_too() {
    let mut f = IngestFilter::new(&cfg());
    let out = run(
        &mut f,
        &[
            focus(S, "org.octoplus.Confirm", "Allow cleanup?"),
            tree(2 * S, "octoplus-confirm", &["helper"]),
            focus(30 * S, "firefox", "x"),
        ],
    );
    assert!(
        items(&out)
            .iter()
            .all(|i| !i.app_id.to_lowercase().contains("octoplus")),
        "{out:?}"
    );
    assert!(
        !out.iter()
            .any(|o| matches!(o, Output::Segment(s) if s.app_id.contains("octoplus")))
    );
    assert_eq!(
        count(&out, Demotion::OwnWindow),
        2,
        "the window and its tree"
    );
    // A prefix must match at the start: an app merely containing the word is not ours.
    let out = run(
        &mut IngestFilter::new(&cfg()),
        &[
            focus(S, "com.example.org.octoplus.fake", "t"),
            focus(40 * S, "a", "b"),
        ],
    );
    assert!(
        out.iter().any(
            |o| matches!(o, Output::Segment(s) if s.app_id == "com.example.org.octoplus.fake")
        )
    );
}

// ---- stage 3: promotion -----------------------------------------------------------------------

#[test]
fn an_app_is_promoted_on_its_third_focus_not_before() {
    let mut f = IngestFilter::new(&cfg());
    let mut promoted_at = vec![];
    for (n, t) in [(1, 0u64), (2, 10), (3, 20)] {
        let out = run(
            &mut f,
            &[
                focus(t * S, "editor", "a"),
                focus(t * S + 2 * S, "other", "b"),
            ],
        );
        if items(&out).iter().any(|i| i.label == "editor") {
            promoted_at.push(n);
        }
    }
    assert_eq!(
        promoted_at,
        vec![3],
        "two sightings of 2 s each are noise, the third promotes"
    );
}

#[test]
fn dwell_must_exceed_the_threshold_strictly_and_idle_time_does_not_count() {
    let promoted = |secs_x1000: u64, idle_ms: Option<(u64, u64)>| {
        let mut f = IngestFilter::new(&cfg());
        let mut evs = vec![focus(100 * S, "viewer", "doc")];
        if let Some((from, to)) = idle_ms {
            evs.push(idle(100 * S + from * 1_000_000, true));
            evs.push(idle(100 * S + to * 1_000_000, false));
        }
        evs.push(focus(100 * S + secs_x1000 * 1_000_000, "z", ""));
        let out = run(&mut f, &evs);
        items(&out).iter().any(|i| i.label == "viewer")
    };
    assert!(!promoted(4_999, None), "4.999 s of dwell");
    assert!(
        !promoted(5_000, None),
        "exactly 5.000 s: the rule says more than 5 s"
    );
    assert!(promoted(5_001, None));
    assert!(
        !promoted(20_000, Some((1_000, 17_000))),
        "20 s of which 16 s idle is 4 s of active dwell"
    );
    assert!(
        promoted(20_000, Some((1_000, 8_000))),
        "7 s idle leaves 13 s"
    );
}

#[test]
fn a_long_titled_visit_makes_a_chunk_and_a_short_one_only_a_history_row() {
    let mut f = IngestFilter::new(&cfg());
    run(
        &mut f,
        &[
            focus(0, "term", "t"),
            focus(S, "x", ""),
            focus(2 * S, "term", "t"),
            focus(3 * S, "x", ""),
        ],
    );
    let out = run(
        &mut f,
        &[focus(10 * S, "term", "make test"), focus(30 * S, "x", "")],
    );
    let it = items(&out);
    assert!(
        it.iter().any(|i| i.text == "Used term: make test"),
        "{out:?}"
    );
    let out = run(
        &mut f,
        &[
            focus(40 * S, "term", "ls"),
            focus(42 * S, "quick-filler", ""),
        ],
    );
    assert!(
        items(&out).iter().all(|i| i.label != "term"),
        "2 s is a history row, not a memory: {out:?}"
    );
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Segment(s) if s.title == "ls" && s.dwell_ns == 2 * S))
    );
}

#[test]
fn an_empty_focus_means_no_window_and_never_becomes_an_entity() {
    let mut f = IngestFilter::new(&cfg());
    let out = run(
        &mut f,
        &[
            focus(0, "a", "t"),
            focus(60 * S, "", ""),
            focus(120 * S, "", ""),
        ],
    );
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Segment(s) if s.app_id == "a" && s.end_ns == 60 * S))
    );
    assert!(
        !out.iter()
            .any(|o| matches!(o, Output::Segment(s) if s.app_id.is_empty()))
    );
    assert!(items(&out).iter().all(|i| !i.label.is_empty()));
}

#[test]
fn finish_closes_the_open_segment() {
    let mut f = IngestFilter::new(&cfg());
    f.push(&focus(0, "a", "t"));
    let out = f.finish(9 * S);
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Segment(s) if s.end_ns == 9 * S && s.dwell_ns == 9 * S))
    );
    assert!(f.finish(10 * S).is_empty(), "nothing open any more");
}

// ---- stage 4: media -----------------------------------------------------------------------------

#[test]
fn passive_media_is_counted_and_never_stored() {
    let mut f = IngestFilter::new(&cfg());
    let out = run(
        &mut f,
        &[
            focus(0, "editor", "code"),
            media(S, "spotify", PlaybackState::Playing, "Song"),
            media(100 * S, "spotify", PlaybackState::Paused, "Song"),
            media(200 * S, "spotify", PlaybackState::Playing, "Song 2"),
            media(300 * S, "spotify", PlaybackState::Paused, "Song 2"),
        ],
    );
    assert!(
        items(&out)
            .iter()
            .all(|i| i.domain != Domain::MediaPlayback),
        "{out:?}"
    );
    assert_eq!(count(&out, Demotion::PassiveMedia), 4);
}

#[test]
fn media_in_the_focused_program_counts_as_interaction_and_is_promoted_on_the_third_change() {
    let mut f = IngestFilter::new(&cfg());
    let out = run(
        &mut f,
        &[
            focus(0, "org.mozilla.firefox", "video"),
            media(S, "firefox", PlaybackState::Playing, "Clip"),
            media(2 * S, "firefox", PlaybackState::Paused, "Clip"),
            media(3 * S, "firefox", PlaybackState::Playing, "Clip"),
        ],
    );
    let m: Vec<_> = items(&out)
        .into_iter()
        .filter(|i| i.domain == Domain::MediaPlayback)
        .collect();
    assert_eq!(m.len(), 1, "promoted exactly on the third interaction");
    assert_eq!(m[0].text, "firefox playing: Clip by A");
}

#[test]
fn the_interaction_window_after_leaving_the_player_is_exactly_ten_seconds() {
    let went_passive = |gap_ms: u64| {
        let mut f = IngestFilter::new(&cfg());
        let out = run(
            &mut f,
            &[
                focus(0, "brave-browser", "tab"),
                focus(S, "editor", "code"),
                media(S + gap_ms * 1_000_000, "brave", PlaybackState::Playing, "x"),
            ],
        );
        count(&out, Demotion::PassiveMedia) == 1
    };
    assert!(
        !went_passive(10_000),
        "10.000 s after focusing the player is still interaction"
    );
    assert!(went_passive(10_001));
}

#[test]
fn program_matching_is_conservative() {
    assert!(same_program("firefox", "org.mozilla.firefox"));
    assert!(same_program("brave", "brave-browser"));
    assert!(same_program("Spotify", "spotify"));
    assert!(!same_program("vlc", "org.mozilla.firefox"));
    assert!(
        !same_program("go", "org.gnome.go"),
        "names under three letters never match"
    );
    assert!(!same_program("", "firefox"));
    assert!(!same_program("firefox", ""));
}

// ---- stage 1: process trees ---------------------------------------------------------------------

const COMPILERS: [&str; 5] = ["rustc", "cc1", "ld", "cargo", "ninja"];

fn build_trees(n: u64) -> Vec<MonitorEvent> {
    let mut v = vec![focus(0, "konsole", "build")];
    for i in 0..n {
        v.push(tree(
            S + i * 5 * S,
            "konsole",
            &[
                "bash", "cargo", "rustc", "cc1", "ld", "helper-1", "helper-2",
            ],
        ));
    }
    v
}

fn entity_labels(out: &[Output]) -> Vec<String> {
    items(out)
        .iter()
        .filter(|i| matches!(i.domain, Domain::ProcessActivity | Domain::BuildJob))
        .map(|i| i.label.clone())
        .collect()
}

#[test]
fn compiler_subprocesses_never_become_entities_and_a_build_becomes_one_build_job() {
    let mut f = IngestFilter::new(&cfg());
    let out = run(&mut f, &build_trees(50));
    let labels = entity_labels(&out);
    assert!(
        labels
            .iter()
            .all(|l| !COMPILERS.contains(&l.as_str()) && l != "helper-1"),
        "{labels:?}"
    );
    let builds: Vec<_> = items(&out)
        .into_iter()
        .filter(|i| i.domain == Domain::BuildJob)
        .collect();
    assert!(!builds.is_empty());
    assert!(
        builds
            .iter()
            .all(|b| b.label == "konsole" && b.text.starts_with("Build activity in konsole using")),
        "{builds:?}"
    );
    assert!(builds[0].text.contains("cargo") && builds[0].text.contains("rustc"));
    assert!(count(&out, Demotion::Subprocess) >= 50);
    // 50 trees at one per 5 s must not produce 50 chunks: re-announcement is rate limited.
    assert!(builds.len() <= 5, "{} build chunks", builds.len());
}

#[test]
fn a_plain_tree_is_one_process_entity_for_its_root() {
    let mut f = IngestFilter::new(&cfg());
    let evs: Vec<_> = (0..4)
        .map(|i| tree(i * S, "firefox", &["web-content", "gpu-helper"]))
        .collect();
    let out = run(&mut f, &evs);
    assert_eq!(entity_labels(&out), vec!["firefox"]);
}

#[test]
fn negative_control_without_the_collapse_stage_compilers_do_become_entities() {
    let stages = Stages {
        collapse: false,
        ..Stages::default()
    };
    let out = run(
        &mut IngestFilter::with_stages(&cfg(), stages),
        &build_trees(10),
    );
    let labels = entity_labels(&out);
    assert!(
        labels.iter().any(|l| COMPILERS.contains(&l.as_str())),
        "the check would not have noticed: {labels:?}"
    );
}

// ---- folders, health, idle ------------------------------------------------------------------------

#[test]
fn files_are_classified_and_promoted_on_the_third_event_and_removal_forgets() {
    assert_eq!(
        classify_file("/home/u/Notes/plan.md").unwrap().domain,
        Domain::Notes
    );
    assert_eq!(
        classify_file("/home/u/proj/.git/HEAD").unwrap().label,
        "proj"
    );
    assert_eq!(
        classify_file("/home/u/cal/work.ics").unwrap().domain,
        Domain::Calendar
    );
    assert!(classify_file("/home/u/pic.png").is_none());
    assert!(
        classify_file("/.git/HEAD").is_none(),
        "no folder name before .git"
    );
    assert!(classify_file("").is_none());
    let mut f = IngestFilter::new(&cfg());
    let p = "/home/u/Notes/plan.md";
    let a = run(
        &mut f,
        &[
            file(0, p, FileActivityKind::Created),
            file(S, p, FileActivityKind::Modified),
        ],
    );
    assert!(items(&a).is_empty());
    let b = run(&mut f, &[file(2 * S, p, FileActivityKind::Modified)]);
    assert_eq!(items(&b).len(), 1);
    assert_eq!(items(&b)[0].text, "Note plan.md changed");
    let c = run(&mut f, &[file(3 * S, p, FileActivityKind::Removed)]);
    assert!(
        matches!(&c[0], Output::SourceGone { domain: Domain::Notes, label, .. } if label == "plan.md")
    );
}

#[test]
fn health_samples_bypass_the_gate_and_the_counters() {
    let mut f = IngestFilter::new(&cfg());
    let out = f.push(&ev(
        5,
        Event::Resources(ResourceSample {
            cpu_percent: 12.5,
            mem_total_bytes: 1000,
            mem_available_bytes: 250,
            ..Default::default()
        }),
    ));
    let names: Vec<_> = out
        .iter()
        .filter_map(|o| {
            if let Output::Health(h) = o {
                Some((h.metric.as_str(), h.value))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        names,
        vec![("cpu_percent", 12.5), ("mem_used_percent", 75.0)]
    );
    assert!(
        !out.iter()
            .any(|o| matches!(o, Output::Counter(_) | Output::Promote(_)))
    );
    let d = health_points(&ev(
        1,
        Event::Disk(DiskSample {
            mount_point: "/".into(),
            total_bytes: 0,
            ..Default::default()
        }),
    ));
    assert!(d.is_empty(), "no division by zero");
}

#[test]
fn coming_back_from_idle_is_remembered_after_three_returns() {
    let mut f = IngestFilter::new(&cfg());
    let mut stored = 0;
    for i in 0..4u64 {
        let base = i * 1000 * S;
        let out = run(&mut f, &[idle(base, true), idle(base + 600 * S, false)]);
        stored += items(&out).len();
    }
    assert_eq!(stored, 2, "the third and fourth return");
    let out = run(
        &mut f,
        &[idle(9000 * S, true), idle(9000 * S + 600 * S, false)],
    );
    assert!(items(&out)[0].text.contains("10 minutes"));
}

// ---- property tests -----------------------------------------------------------------------------------

fn any_event() -> impl Strategy<Value = (u64, u8, u8)> {
    (0u64..3, 0u8..9, 0u8..6)
}

fn build_event(at: u64, kind: u8, v: u8) -> MonitorEvent {
    let apps = [
        "editor",
        "term",
        "org.octoplus.Confirm",
        "",
        "brave-browser",
        "ORG.OCTOPLUS.dialog",
    ];
    let app = apps[usize::from(v) % apps.len()];
    match kind {
        0..=2 => focus(at, app, if v.is_multiple_of(2) { "title" } else { "" }),
        3 => idle(at, v.is_multiple_of(2)),
        4 => media(
            at,
            if v.is_multiple_of(2) {
                "brave"
            } else {
                "spotify"
            },
            PlaybackState::Playing,
            "x",
        ),
        5 => tree(at, "term", &["cargo", "rustc", "bash"]),
        6 => tree(at, "editor", &["lsp"]),
        7 => file(at, "/home/u/Notes/n.md", FileActivityKind::Modified),
        _ => ev(at, Event::Resources(ResourceSample::default())),
    }
}

proptest! {
    #[test]
    fn the_noise_invariants_hold_for_any_event_sequence(seq in proptest::collection::vec(any_event(), 0..120)) {
        let mut f = IngestFilter::new(&cfg());
        let mut t = 0u64;
        let mut promoted_keys = std::collections::HashSet::new();
        let mut all = Vec::new();
        for (dt, kind, v) in seq {
            t += dt * S + 1;
            all.extend(f.push(&build_event(t, kind, v)));
        }
        all.extend(f.finish(t + S));
        for o in &all {
            match o {
                Output::Promote(i) => {
                    prop_assert!(!i.label.is_empty());
                    prop_assert!(!i.label.to_lowercase().starts_with("org.octoplus."));
                    prop_assert!(!i.app_id.to_lowercase().starts_with("org.octoplus."));
                    if matches!(i.domain, Domain::ProcessActivity | Domain::BuildJob) {
                        prop_assert!(!["cargo", "rustc", "bash", "lsp"].contains(&i.label.as_str()));
                    }
                    prop_assert!(i.domain != Domain::MediaPlayback || i.text.contains("by A"));
                    promoted_keys.insert((i.domain as i32, i.kind.clone(), i.label.clone()));
                }
                Output::Segment(s) => {
                    prop_assert!(!s.app_id.to_lowercase().starts_with("org.octoplus."));
                    prop_assert!(s.end_ns >= s.start_ns && s.dwell_ns <= s.end_ns - s.start_ns);
                }
                Output::Counter(c) => {
                    // a promoted counter always met the gate
                    if c.promoted {
                        prop_assert!(c.occurrences >= 3 || c.dwell_ns > 5 * S);
                    }
                }
                _ => {}
            }
        }
        prop_assert!(promoted_keys.len() <= 30 + 10, "{} distinct entities from one random session", promoted_keys.len());
    }
}
