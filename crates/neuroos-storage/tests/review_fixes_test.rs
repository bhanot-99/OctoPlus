//! Regression tests for the agy review of Phase 4 (F1 live state survives forget, F6 reader pool).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{FocusChanged, ForgetRequest, MonitorEvent, QueryFocusHistory};
use neuroos_storage::embed::HashEmbedder;
use neuroos_storage::store::{Store, StorePaths};

const S: u64 = 1_000_000_000;

async fn store() -> (Arc<Store>, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(
        &StorePaths::under(d.path()),
        StorageConfig::default(),
        Arc::new(HashEmbedder),
        Health::new("c3", "t"),
    )
    .await
    .unwrap();
    (Arc::new(s), d)
}

#[tokio::test]
async fn a_forgotten_app_is_no_longer_served_from_the_open_window() {
    let (s, _d) = store().await;
    s.observe(&MonitorEvent {
        observed_at_ns: 100 * S,
        event: Some(Event::Focus(FocusChanged {
            app_id: "org.keepassxc.KeePassXC".into(),
            title: "Passwords".into(),
            pid: 1,
            fullscreen: false,
            fullscreen_known: true,
        })),
    });
    let q = || QueryFocusHistory {
        at_ns: 120 * S,
        window_ns: 0,
    };
    assert_eq!(s.focus_history(q()).await.unwrap().windows.len(), 1);
    s.forget(ForgetRequest {
        from_ns: 0,
        to_ns: 0,
        app_id: "org.keepassxc.KeePassXC".into(),
    })
    .await
    .unwrap();
    assert!(
        s.focus_history(q()).await.unwrap().windows.is_empty(),
        "the open window of a forgotten app is gone"
    );
}

#[tokio::test]
async fn a_panicking_read_does_not_shrink_the_reader_pool() {
    let (s, _d) = store().await;
    let before = s.db().readers().idle_count();
    for _ in 0..(before + 2) {
        let db = Arc::clone(s.db());
        let _ = std::thread::spawn(move || {
            let _ = db.readers().with(|_| -> Result<(), _> { panic!("boom") });
        })
        .join();
    }
    assert_eq!(s.db().readers().idle_count(), before);
    assert!(s.db().integrity_ok().unwrap());
}
