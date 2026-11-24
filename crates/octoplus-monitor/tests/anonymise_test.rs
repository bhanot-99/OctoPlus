//! W8 / P3-S06: the anonymiser and the recorder (FR-MON-12). Property tests plus a negative control.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use octoplus_health::Health;
use octoplus_monitor::anonymise::Anonymiser;
use octoplus_monitor::bus::{Bus, PassGate};
use octoplus_monitor::clock::{Clock, FakeClock};
use octoplus_proto::dump::DumpReader;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{
    DiskSample, FileActivity, FocusChanged, MediaChanged, MonitorEvent, ProcessNode, ProcessTree,
    ResourceSample,
};
use proptest::prelude::*;
use zeroize::Zeroizing;

fn anon(byte: u8) -> Anonymiser {
    Anonymiser::with_key(Zeroizing::new([byte; 32]), &[])
}

fn ev(e: Event) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: 9,
        event: Some(e),
    }
}

fn focus(app: &str, title: &str) -> MonitorEvent {
    ev(Event::Focus(FocusChanged {
        app_id: app.into(),
        title: title.into(),
        pid: 7,
        fullscreen: true,
        fullscreen_known: true,
    }))
}

fn text(e: &MonitorEvent) -> String {
    format!("{e:?}")
}

#[test]
fn same_text_same_placeholder_in_a_run_and_a_different_one_with_another_key() {
    let (a, b) = (anon(1), anon(2));
    assert_eq!(a.placeholder("secret title"), a.placeholder("secret title"));
    assert_ne!(a.placeholder("secret title"), a.placeholder("other title"));
    assert_ne!(a.placeholder("secret title"), b.placeholder("secret title"));
    assert!(
        a.placeholder("secret title").starts_with("t_")
            && a.placeholder("secret title").len() == 14
    );
    assert_eq!(
        a.placeholder(""),
        "",
        "empty stays empty (a redacted focus stays visibly empty)"
    );
}

#[test]
fn a_placeholder_is_not_a_plain_hash_so_it_cannot_be_recomputed_without_the_key() {
    use sha2::{Digest, Sha256};
    let a = anon(7);
    let p = a.placeholder("my private window");
    let plain = Sha256::digest(b"my private window");
    let plain_hex: String = plain.iter().take(6).map(|b| format!("{b:02x}")).collect();
    assert_ne!(p, format!("t_{plain_hex}"));
    // A guesser with the wrong key gets nothing.
    assert_ne!(p, anon(8).placeholder("my private window"));
}

#[test]
fn every_text_field_is_replaced_and_numbers_and_public_apps_survive() {
    let a = anon(3);
    let f = a.anonymise(focus("com.example.Private", "Bank - Statement.pdf"));
    let Some(Event::Focus(f)) = f.event else {
        panic!()
    };
    assert!(f.app_id.starts_with("t_"), "unknown app ids are hashed");
    assert!(f.title.starts_with("t_"));
    assert_eq!((f.pid, f.fullscreen, f.fullscreen_known), (7, true, true));

    let Some(Event::Focus(f)) = a.anonymise(focus("firefox", "x")).event else {
        panic!()
    };
    assert_eq!(
        f.app_id, "firefox",
        "public apps stay readable for the noise gate"
    );

    let m = a.anonymise(ev(Event::Media(MediaChanged {
        player: "myplayer".into(),
        title: "Song".into(),
        artist: "Band".into(),
        ..Default::default()
    })));
    assert!(
        !text(&m).contains("Song") && !text(&m).contains("Band") && !text(&m).contains("myplayer")
    );

    let r = a.anonymise(ev(Event::Resources(ResourceSample {
        top_process: "tax-return".into(),
        top_process_rss_bytes: 5,
        ..Default::default()
    })));
    assert!(!text(&r).contains("tax-return") && text(&r).contains("top_process_rss_bytes: 5"));

    let t = a.anonymise(ev(Event::ProcessTree(ProcessTree {
        root_pid: 1,
        nodes: vec![ProcessNode {
            pid: 1,
            ppid: 0,
            name: "secretd".into(),
        }],
    })));
    assert!(!text(&t).contains("secretd"));
}

#[test]
fn paths_keep_their_shape_and_extension_but_not_their_names() {
    let a = anon(4);
    let Some(Event::File(f)) = a
        .anonymise(ev(Event::File(FileActivity {
            path: "/home/alice/Notes/divorce-plan.md".into(),
            kind: 2,
        })))
        .event
    else {
        panic!()
    };
    assert!(f.path.starts_with("/home/t_"), "{}", f.path);
    assert!(f.path.ends_with(".md"));
    assert_eq!(f.path.matches('/').count(), 4, "depth is preserved");
    assert!(!f.path.contains("alice") && !f.path.contains("divorce"));

    let Some(Event::File(g)) = a
        .anonymise(ev(Event::File(FileActivity {
            path: "repo/.git/HEAD".into(),
            kind: 2,
        })))
        .event
    else {
        panic!()
    };
    assert!(
        g.path.ends_with("/.git/HEAD"),
        "git structure stays: {}",
        g.path
    );

    let Some(Event::Disk(d)) = a
        .anonymise(ev(Event::Disk(DiskSample {
            mount_point: "/media/alice/MY-BACKUP".into(),
            ..Default::default()
        })))
        .event
    else {
        panic!()
    };
    assert!(d.mount_point.starts_with("/media/t_") && !d.mount_point.contains("BACKUP"));
}

proptest! {
    /// No sentinel string put into any text field survives anonymisation, for any key.
    #[test]
    fn no_original_text_survives(
        key in any::<u8>(),
        title in "[A-Za-z0-9]{6,20}",
        artist in "[A-Za-z0-9]{6,20}",
        file in "[A-Za-z0-9]{6,20}",
    ) {
        let sentinel_title = format!("ZQ{title}");
        let sentinel_artist = format!("ZQ{artist}");
        let sentinel_file = format!("ZQ{file}");
        let a = anon(key);
        let out = [
            a.anonymise(focus("org.nobody.App", &sentinel_title)),
            a.anonymise(ev(Event::Media(MediaChanged { artist: sentinel_artist.clone(), ..Default::default() }))),
            a.anonymise(ev(Event::File(FileActivity { path: format!("/home/u/{sentinel_file}.md"), kind: 1 }))),
        ];
        let all = out.iter().map(text).collect::<String>();
        prop_assert!(!all.contains(&sentinel_title));
        prop_assert!(!all.contains(&sentinel_artist));
        prop_assert!(!all.contains(&sentinel_file));
    }
}

/// Negative control (rules §8.9): the same check fails if the anonymiser is bypassed.
#[test]
fn negative_control_the_leak_check_fails_without_anonymising() {
    let raw = focus("org.nobody.App", "ZQsentinel-title");
    assert!(
        text(&raw).contains("ZQsentinel-title"),
        "the checker can see a leak"
    );
    assert!(!text(&anon(1).anonymise(raw)).contains("ZQsentinel-title"));
}

#[tokio::test]
async fn recorder_writes_an_anonymised_complete_dump_and_the_key_is_nowhere_in_it() {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1));
    let health = Health::new("octoplus-monitor", "test");
    let bus = Bus::new(64, Arc::new(PassGate), clock, Arc::clone(&health));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.dump");
    let writer = octoplus_monitor::record::create_dump(&path).unwrap();
    let key = [0xAB; 32];
    let sub = bus.subscribe();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(octoplus_monitor::record::run_recorder(
        sub,
        writer,
        Anonymiser::with_key(Zeroizing::new(key), &[]),
        Arc::clone(&health),
        stop_rx,
    ));
    for i in 0..10 {
        bus.publish(focus("org.nobody.App", &format!("ZQprivate-{i}")));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    stop_tx.send(true).unwrap();
    assert_eq!(task.await.unwrap().unwrap(), 10);

    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("ZQprivate"));
    assert!(
        !bytes.windows(32).any(|w| w == key),
        "the key is never written"
    );
    let mut r = DumpReader::new(bytes.as_slice()).unwrap();
    let mut n = 0;
    while r.next_event().unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 10, "complete dump with a valid end record");
    // A second recorder must not overwrite an existing dump.
    assert!(octoplus_monitor::record::create_dump(&path).is_err());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

/// Review F3 (agy): `--record session.dump` (no folder) must resolve to a real folder.
#[test]
fn a_bare_record_file_name_resolves_to_the_working_folder() {
    let (file, dir) =
        octoplus_monitor::record::resolve_path(std::path::Path::new("session.dump")).unwrap();
    assert!(file.is_absolute() && dir.is_absolute());
    assert_eq!(file.parent().unwrap(), dir);
    assert!(dir.is_dir(), "Landlock needs a folder that exists");
    let (file, dir) =
        octoplus_monitor::record::resolve_path(std::path::Path::new("/tmp/x/y.dump")).unwrap();
    assert_eq!(
        (file.to_str().unwrap(), dir.to_str().unwrap()),
        ("/tmp/x/y.dump", "/tmp/x")
    );
    let (_, dir) = octoplus_monitor::record::resolve_path(std::path::Path::new("/y.dump")).unwrap();
    assert_eq!(dir.to_str().unwrap(), "/");
}
