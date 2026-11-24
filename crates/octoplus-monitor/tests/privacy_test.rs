//! W2 / P3-S05: pause and exclusion are the only filters and they hold for every input
//! (FR-MON-10, FR-PRV-01/02). The property test has a negative control (rules §8.9).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use octoplus_health::Health;
use octoplus_monitor::Privacy;
use octoplus_monitor::bus::Gate;
use octoplus_monitor::clock::{Clock, FakeClock};
use octoplus_monitor::exclusion::Exclusion;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{FocusChanged, MonitorEvent, ProcessNode, ProcessTree, ResourceSample};
use proptest::prelude::*;

const SECRET_APP: &str = "org.example.SecretVault";
const SECRET_TITLE: &str = "my-sentinel-private-tab";

fn exclusion() -> Exclusion {
    Exclusion::new(&[SECRET_APP, "vault*"], &[SECRET_TITLE])
}

fn privacy(clock: &Arc<FakeClock>) -> Arc<Privacy> {
    Privacy::new(
        Arc::clone(clock) as Arc<dyn Clock>,
        exclusion(),
        Health::new("octoplus-monitor", "test"),
    )
}

fn focus(app: &str, title: &str) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: 1,
        event: Some(Event::Focus(FocusChanged {
            app_id: app.into(),
            title: title.into(),
            pid: 42,
            fullscreen: true,
            fullscreen_known: true,
        })),
    }
}

fn tree() -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: 2,
        event: Some(Event::ProcessTree(ProcessTree {
            root_pid: 1,
            nodes: vec![ProcessNode {
                pid: 1,
                ppid: 0,
                name: "x".into(),
            }],
        })),
    }
}

fn resources(top: &str) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: 3,
        event: Some(Event::Resources(ResourceSample {
            top_process: top.into(),
            top_process_rss_bytes: 99,
            ..Default::default()
        })),
    }
}

// ---- exclusion matching (UT) ----

#[test]
fn matching_is_case_insensitive_exact_prefix_and_title_substring() {
    let ex = exclusion();
    assert!(ex.matches_app("org.example.secretvault"));
    assert!(ex.matches_app("VAULT-desktop"));
    assert!(
        !ex.matches_app("org.example.SecretVaultX"),
        "exact rule is not a prefix rule"
    );
    assert!(ex.is_excluded("firefox", &format!("tab - {}", SECRET_TITLE.to_uppercase())));
    assert!(!ex.is_excluded("firefox", "ordinary"));
}

#[test]
fn empty_inputs_and_empty_rules_never_match() {
    let ex = Exclusion::new(&["", "*", "  "], &["", "  "]);
    assert!(!ex.matches_app(""));
    assert!(
        !ex.matches_app("anything"),
        "a bare * must not exclude everything"
    );
    assert!(!ex.matches_title(""));
    assert!(!ex.matches_title("anything"));
    assert!(
        !exclusion().matches_app(""),
        "an empty app id never matches a real rule"
    );
}

#[test]
fn defaults_cover_password_managers_and_private_windows() {
    let ex = Exclusion::from_config(&octoplus_common::config::MonitorConfig::default());
    for app in [
        "com.bitwarden.desktop",
        "org.keepassxc.KeePassXC",
        "1Password",
        "org.gnome.seahorse.Application",
    ] {
        assert!(ex.matches_app(app), "{app}");
    }
    for title in [
        "Mozilla Firefox — Private Browsing",
        "New Tab - Google Chrome (Incognito)",
        "Microsoft Edge InPrivate",
    ] {
        assert!(ex.matches_title(title), "{title}");
    }
    let cfg = octoplus_common::config::MonitorConfig {
        drop_default_exclusions: true,
        ..Default::default()
    };
    assert!(!Exclusion::from_config(&cfg).matches_app("1password"));
}

// ---- pause (UT, fake clock) ----

#[test]
fn pause_drops_everything_then_resume_publishes_again() {
    let clock = Arc::new(FakeClock::new(1_000_000_000));
    let p = privacy(&clock);
    assert!(p.admit(resources("firefox")).is_some());
    let st = p.pause(0);
    assert!(st.paused && st.paused_until_ns == 0);
    assert!(p.admit(focus("firefox", "x")).is_none());
    assert!(p.admit(resources("firefox")).is_none(), "samples stop too");
    assert!(!p.resume().paused);
    assert!(p.admit(resources("firefox")).is_some());
}

#[test]
fn timed_pause_expires_on_the_monotonic_clock_and_requests_a_snapshot() {
    let clock = Arc::new(FakeClock::new(5_000_000_000));
    let p = privacy(&clock);
    let snaps = p.snapshot_requests();
    let st = p.pause(60_000_000_000);
    assert_eq!(st.paused_until_ns, 65_000_000_000);
    clock.advance(Duration::from_secs(59));
    assert!(p.admit(resources("x")).is_none());
    assert!(!snaps.has_changed().unwrap());
    clock.advance(Duration::from_secs(2));
    assert!(
        p.admit(resources("x")).is_some(),
        "expired by the time of the next event"
    );
    assert!(
        snaps.has_changed().unwrap(),
        "resume asks sensors to re-emit their state"
    );
    assert!(!p.state().paused);
}

#[test]
fn a_wall_clock_jump_cannot_extend_or_end_a_pause() {
    let clock = Arc::new(FakeClock::new(1_000_000_000));
    let p = privacy(&clock);
    p.pause(10_000_000_000);
    clock.jump_wall(9_000_000_000_000);
    assert!(p.state().paused, "wall clock jumped forward: still paused");
    clock.advance(Duration::from_secs(11));
    assert!(!p.state().paused);
}

#[test]
fn a_later_pause_replaces_an_earlier_one_and_stale_timers_do_nothing() {
    let clock = Arc::new(FakeClock::new(0));
    let p = privacy(&clock);
    p.pause(10_000_000_000);
    p.pause(100_000_000_000);
    clock.advance(Duration::from_secs(11));
    p.expire_if_due(); // the first request's timer firing late
    assert!(p.state().paused);
    p.pause(u64::MAX); // unrepresentable deadline pauses until resumed, never panics
    assert!(p.state().paused);
}

// ---- exclusion at the gate (UT) ----

#[test]
fn excluded_window_becomes_a_redacted_focus_and_hides_its_process_tree() {
    let clock = Arc::new(FakeClock::new(0));
    let p = privacy(&clock);
    let out = p.admit(focus(SECRET_APP, "Vault")).unwrap();
    let Some(Event::Focus(f)) = out.event else {
        panic!("focus expected")
    };
    assert_eq!((f.app_id.as_str(), f.title.as_str(), f.pid), ("", "", 0));
    assert!(!f.fullscreen && !f.fullscreen_known);
    assert_eq!(
        out.observed_at_ns, 1,
        "the time stays: it ends the previous span"
    );
    assert!(p.admit(tree()).is_none());
    let normal = p.admit(focus("firefox", "ok")).unwrap();
    assert!(matches!(normal.event, Some(Event::Focus(f)) if f.app_id == "firefox"));
    assert!(
        p.admit(tree()).is_some(),
        "tree flows again after focus leaves the excluded window"
    );
}

#[test]
fn an_excluded_app_is_blanked_as_top_process() {
    let clock = Arc::new(FakeClock::new(0));
    let p = privacy(&clock);
    let Some(Event::Resources(r)) = p.admit(resources("vault-daemon")).unwrap().event else {
        panic!()
    };
    assert!(r.top_process.is_empty() && r.top_process_rss_bytes == 0);
    let Some(Event::Resources(r)) = p.admit(resources("firefox")).unwrap().event else {
        panic!()
    };
    assert_eq!(r.top_process, "firefox");
}

// ---- property test with a negative control (PT) ----

#[derive(Debug, Clone)]
enum Step {
    Focus(usize, usize),
    Tree,
    Res(usize),
    Pause(u64),
    Resume,
    Wait(u64),
}

const APPS: [&str; 5] = [
    SECRET_APP,
    "ORG.EXAMPLE.SECRETVAULT",
    "vault-ui",
    "firefox",
    "",
];
const TITLES: [&str; 4] = ["", "plain", "tab - MY-SENTINEL-PRIVATE-TAB", "Doc"];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..APPS.len(), 0..TITLES.len()).prop_map(|(a, t)| Step::Focus(a, t)),
        Just(Step::Tree),
        (0..APPS.len()).prop_map(Step::Res),
        (0u64..120).prop_map(Step::Pause),
        Just(Step::Resume),
        (0u64..90).prop_map(Step::Wait),
    ]
}

/// Runs a script through `gate` and counts leaks: an event published while paused, an
/// excluded app id or title text in any published event, or a tree while an excluded
/// window is focused.
fn violations(gate: &dyn Gate, clock: &FakeClock, privacy: &Privacy, script: &[Step]) -> usize {
    let ex = exclusion();
    let mut leaks = 0;
    // An independent model of the pause timer, so the checker does not trust the gate:
    // `now` in whole seconds, and the second the current pause ends (None = running).
    let mut now = 0u64;
    let mut paused_until: Option<u64> = None;
    let mut excluded_focus = false;
    for s in script {
        let ev = match s {
            Step::Focus(a, t) => {
                excluded_focus = ex.is_excluded(APPS[*a], TITLES[*t]);
                focus(APPS[*a], TITLES[*t])
            }
            Step::Tree => tree(),
            Step::Res(a) => resources(APPS[*a]),
            Step::Pause(secs) => {
                privacy.pause(secs * 1_000_000_000);
                paused_until = Some(if *secs == 0 { u64::MAX } else { now + secs });
                continue;
            }
            Step::Resume => {
                privacy.resume();
                paused_until = None;
                continue;
            }
            Step::Wait(secs) => {
                clock.advance(Duration::from_secs(*secs));
                now += secs;
                continue;
            }
        };
        let paused_now = paused_until.is_some_and(|u| u > now);
        let is_tree = matches!(ev.event, Some(Event::ProcessTree(_)));
        let Some(out) = gate.admit(ev) else { continue };
        if paused_now || (is_tree && excluded_focus) {
            leaks += 1;
        }
        let text = format!("{out:?}").to_lowercase();
        if text.contains(&SECRET_APP.to_lowercase())
            || text.contains(SECRET_TITLE)
            || text.contains("vault-ui")
        {
            leaks += 1;
        }
    }
    leaks
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn no_excluded_or_paused_event_ever_leaves(script in proptest::collection::vec(step(), 0..40)) {
        let clock = Arc::new(FakeClock::new(1));
        let p = privacy(&clock);
        prop_assert_eq!(violations(p.as_ref(), &clock, &p, &script), 0);
    }
}

/// Negative control: a gate that lets everything through must be caught by the same checker.
struct Leaky;
impl Gate for Leaky {
    fn admit(&self, e: MonitorEvent) -> Option<MonitorEvent> {
        Some(e)
    }
}

#[test]
fn negative_control_the_property_checker_catches_a_leaky_gate() {
    let script = vec![Step::Focus(0, 1), Step::Tree, Step::Pause(30), Step::Res(3)];
    let clock = Arc::new(FakeClock::new(1));
    let p = privacy(&clock);
    assert!(violations(&Leaky, &clock, &p, &script) >= 3);
    let clock2 = Arc::new(FakeClock::new(1));
    let p2 = privacy(&clock2);
    assert_eq!(violations(p2.as_ref(), &clock2, &p2, &script), 0);
}

/// IT: through a real socket, an excluded window and a paused period never reach a subscriber.
#[tokio::test]
async fn excluded_and_paused_events_never_reach_a_socket_subscriber() {
    use octoplus_ipc::server::ServerConfig;
    use octoplus_ipc::{Client, Server};
    use octoplus_monitor::bus::Bus;
    use octoplus_proto::v1::MonitorSubscribe;
    use octoplus_proto::v1::envelope::Body;

    let clock = Arc::new(FakeClock::new(1));
    let p = privacy(&clock);
    let bus = Bus::new(
        32,
        Arc::clone(&p) as Arc<dyn Gate>,
        Arc::clone(&clock) as Arc<dyn Clock>,
        Health::new("octoplus-monitor", "test"),
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handler = Arc::new(octoplus_monitor::StreamHandler::new(
        Arc::clone(&bus),
        stop_rx.clone(),
    ));
    let dir = tempfile::tempdir().unwrap();
    let sock_dir = dir.path().join("stream");
    octoplus_ipc::server::ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("stream.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(5)))
        .await
        .unwrap();
    let mut srv_stop = stop_rx;
    let serve = tokio::spawn(server.serve(handler, async move {
        let _ = srv_stop.wait_for(|s| *s).await;
    }));
    let client = Client::connect(&sock, 1 << 20).await.unwrap();
    let mut stream = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    while bus.subscriber_count() == 0 {
        tokio::task::yield_now().await;
    }
    bus.publish(focus(SECRET_APP, SECRET_TITLE)); // redacted
    bus.publish(tree()); // dropped: excluded window focused
    p.pause(0);
    bus.publish(focus("firefox", "while paused")); // dropped
    p.resume();
    bus.publish(focus("firefox", "visible")); // delivered
    let mut seen = Vec::new();
    for _ in 0..2 {
        let Body::MonitorEvent(e) = stream.next().await.unwrap().unwrap() else {
            panic!()
        };
        seen.push(format!("{e:?}"));
    }
    let all = seen.join("|").to_lowercase();
    assert!(all.contains("visible"));
    assert!(
        !all.contains("secretvault")
            && !all.contains(SECRET_TITLE)
            && !all.contains("while paused")
    );
    assert!(!all.contains("processtree"), "no tree reached the stream");
    stop_tx.send(true).unwrap();
    let _ = serve.await;
}
