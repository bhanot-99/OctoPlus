//! W7: focus history, activity, health signals, profile, interactions and nudge log
//! (FR-STO-07/08/09; PRD §5.3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{
    Domain, FocusChanged, IdleChanged, MediaChanged, MonitorEvent, PlaybackState, ProfileAdd,
    QueryActivity, QueryFocusHistory, QueryHealthSignals, ResourceSample, StoreInteraction,
    StoreNudgeLog, Taint,
};
use neuroos_storage::db::DbError;
use neuroos_storage::embed::HashEmbedder;
use neuroos_storage::ingest::{FocusSegment, HealthPoint, Item, Output};
use neuroos_storage::store::Store;
use neuroos_taint::TaintFlags;

const S: u64 = 1_000_000_000;

async fn store() -> (Arc<Store>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(
        &neuroos_storage::store::StorePaths::under(dir.path()),
        StorageConfig::default(),
        Arc::new(HashEmbedder),
        Health::new("neuroos-storage", "test"),
    )
    .await
    .unwrap();
    (Arc::new(s), dir)
}

fn seg(app: &str, title: &str, a: u64, b: u64) -> Output {
    Output::Segment(FocusSegment {
        app_id: app.into(),
        title: title.into(),
        pid: 1,
        start_ns: a * S,
        end_ns: b * S,
        dwell_ns: (b - a) * S,
    })
}

fn ev(at: u64, e: Event) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: at * S,
        event: Some(e),
    }
}

async fn focus_at(s: &Arc<Store>, at: u64, window: u64) -> Vec<(String, bool)> {
    s.focus_history(QueryFocusHistory {
        at_ns: at * S,
        window_ns: window * S,
    })
    .await
    .unwrap()
    .windows
    .into_iter()
    .map(|w| (w.app_id, w.contains_instant))
    .collect()
}

#[tokio::test]
async fn focus_history_returns_the_window_containing_the_instant_else_the_nearest_edge() {
    let (s, _d) = store().await;
    s.ingest(vec![
        seg("a", "A", 100, 200),
        seg("b", "B", 200, 300),
        seg("c", "C", 500, 600),
    ])
    .await
    .unwrap();
    assert_eq!(focus_at(&s, 150, 0).await, vec![("a".into(), true)]);
    assert_eq!(
        focus_at(&s, 200, 0).await,
        vec![("b".into(), true)],
        "on a boundary the later window wins"
    );
    assert_eq!(
        focus_at(&s, 350, 100).await,
        vec![("b".into(), false)],
        "nearest edge within the window"
    );
    assert_eq!(
        focus_at(&s, 450, 100).await,
        vec![("c".into(), false)],
        "c's start is 50 s away, b's end 150 s"
    );
    assert!(
        focus_at(&s, 350, 10).await.is_empty(),
        "nothing within ±10 s"
    );
    assert!(
        focus_at(&s, 50, 0).await.is_empty(),
        "before everything, no window"
    );
    assert!(focus_at(&s, 700, 50).await.is_empty());
}

#[tokio::test]
async fn an_open_window_that_is_not_yet_history_still_answers() {
    let (s, _d) = store().await;
    s.ingest(vec![seg("old", "Old", 10, 20)]).await.unwrap();
    s.observe(&ev(
        100,
        Event::Focus(FocusChanged {
            app_id: "now".into(),
            title: "Live".into(),
            pid: 1,
            fullscreen: false,
            fullscreen_known: true,
        }),
    ));
    assert_eq!(focus_at(&s, 120, 0).await, vec![("now".into(), true)]);
    assert_eq!(
        focus_at(&s, 15, 0).await,
        vec![("old".into(), true)],
        "earlier instants still use history"
    );
    s.observe(&ev(130, Event::Focus(FocusChanged::default())));
    assert!(
        focus_at(&s, 140, 0).await.is_empty(),
        "an empty focus closes the open window"
    );
}

#[tokio::test]
async fn activity_lists_segments_and_chunks_in_time_order_inside_the_range() {
    let (s, _d) = store().await;
    let note = |at: u64, text: &str| {
        Output::Promote(Item {
            domain: Domain::Notes,
            kind: "note".into(),
            label: text.into(),
            app_id: String::new(),
            text: text.into(),
            taint: TaintFlags::empty(),
            at_ns: at * S,
        })
    };
    s.ingest(vec![
        seg("a", "A", 100, 200),
        note(150, "Note x changed"),
        note(900, "Note late changed"),
        seg("b", "B", 300, 400),
    ])
    .await
    .unwrap();
    let r = s
        .activity(QueryActivity {
            from_ns: 90 * S,
            to_ns: 350 * S,
        })
        .await
        .unwrap();
    let labels: Vec<_> = r.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["Used a: A", "Note x changed", "Used b: B"]);
    assert!(
        r.items.iter().all(|i| i.taint.is_some()),
        "taint is always present"
    );
    let none = s
        .activity(QueryActivity {
            from_ns: 1000 * S,
            to_ns: 2000 * S,
        })
        .await
        .unwrap();
    assert!(none.items.is_empty());
}

#[tokio::test]
async fn health_signals_carry_recent_samples_and_the_live_flags() {
    let (s, _d) = store().await;
    for t in [10, 20, 30] {
        s.observe(&ev(
            t,
            Event::Resources(ResourceSample {
                cpu_percent: f64::from(t as u32),
                ..Default::default()
            }),
        ));
    }
    s.observe(&ev(31, Event::Idle(IdleChanged { idle: true })));
    s.observe(&ev(
        32,
        Event::Media(MediaChanged {
            player: "vlc".into(),
            state: PlaybackState::Playing as i32,
            ..Default::default()
        }),
    ));
    s.observe(&ev(
        33,
        Event::Focus(FocusChanged {
            app_id: "v".into(),
            title: "x".into(),
            pid: 0,
            fullscreen: true,
            fullscreen_known: true,
        }),
    ));
    let h = s.health_signals(&QueryHealthSignals { since_ns: 15 * S });
    assert_eq!(
        h.samples.len(),
        2,
        "only samples since the cut-off, oldest first"
    );
    assert_eq!(h.samples[0].observed_at_ns, 20 * S);
    assert!(h.idle && h.media_playing && h.fullscreen);
    assert!(
        !h.locked_known,
        "lock state is unknown in v1.0 (ADR-0011) and the answer says so"
    );
    assert_eq!(h.active_since_ns, 0, "idle: not active");
    s.observe(&ev(40, Event::Idle(IdleChanged { idle: false })));
    assert_eq!(
        s.health_signals(&QueryHealthSignals { since_ns: 0 })
            .active_since_ns,
        40 * S
    );
}

#[tokio::test]
async fn the_recent_sample_ring_is_bounded() {
    let (s, _d) = store().await;
    for t in 0..5000_u64 {
        s.observe(&ev(t, Event::Resources(ResourceSample::default())));
    }
    let n = s
        .health_signals(&QueryHealthSignals { since_ns: 0 })
        .samples
        .len();
    assert_eq!(n, neuroos_storage::ingest::live::RING_CAP);
}

#[tokio::test]
async fn health_points_are_stored_for_the_rollup() {
    let (s, _d) = store().await;
    s.ingest(vec![Output::Health(HealthPoint {
        metric: "cpu_percent".into(),
        value: 12.0,
        at_ns: 5 * S,
    })])
    .await
    .unwrap();
    let n: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM health_samples WHERE metric = 'cpu_percent'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn profile_facts_need_a_taint_and_can_be_listed_filtered_and_removed() {
    let (s, _d) = store().await;
    let user = Some(Taint {
        flags: TaintFlags::USER_PROVIDED.bits(),
    });
    let a = s
        .profile_add(
            ProfileAdd {
                text: "My dog is called Rex".into(),
                taint: user,
            },
            5 * S,
        )
        .await
        .unwrap();
    let b = s
        .profile_add(
            ProfileAdd {
                text: "I prefer tea".into(),
                taint: user,
            },
            6 * S,
        )
        .await
        .unwrap();
    assert_ne!(a, b);
    assert!(
        matches!(
            s.profile_add(
                ProfileAdd {
                    text: "x".into(),
                    taint: None
                },
                1
            )
            .await,
            Err(DbError::Taint)
        ),
        "a missing taint is an error, not clean"
    );
    assert!(
        matches!(
            s.profile_add(
                ProfileAdd {
                    text: "x".into(),
                    taint: Some(Taint { flags: 1 << 20 })
                },
                1
            )
            .await,
            Err(DbError::Taint)
        ),
        "unknown bits are an error"
    );
    assert_eq!(s.profile_list(String::new()).await.unwrap().len(), 2);
    let rel = s.profile_list("what is my dog named".into()).await.unwrap();
    assert_eq!(rel.len(), 1);
    assert_eq!(rel[0].text, "My dog is called Rex");
    assert_eq!(s.profile_remove(vec![a, 9999]).await.unwrap(), 1);
    assert_eq!(s.profile_list(String::new()).await.unwrap().len(), 1);
}

#[tokio::test]
async fn interactions_and_nudges_are_stored_with_their_taint() {
    let (s, _d) = store().await;
    let tainted = Some(Taint {
        flags: TaintFlags::EXTERNAL_UNTRUSTED.bits() | TaintFlags::MODEL_GENERATED.bits(),
    });
    s.store_interaction(StoreInteraction {
        question: "q".into(),
        answer_summary: "a".into(),
        taint: tainted,
        at_ns: 7 * S,
        origin: 0,
    })
    .await
    .unwrap();
    let t: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT taint FROM chunks WHERE domain = ?1",
                [Domain::VoiceInteraction as i32],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(t, 5, "the turn's taint is kept");
    assert!(matches!(
        s.store_interaction(StoreInteraction {
            question: "q".into(),
            answer_summary: "a".into(),
            taint: None,
            at_ns: 1,
            origin: 0
        })
        .await,
        Err(DbError::Taint)
    ));
    s.store_nudge(StoreNudgeLog {
        rule_id: "disk_low".into(),
        outcome: 1,
        at_ns: 9 * S,
    })
    .await
    .unwrap();
    let n: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM nudge_log WHERE rule_id = 'disk_low'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(n, 1);
}
