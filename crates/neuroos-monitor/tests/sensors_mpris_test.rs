//! W6 / P3-S02: media players over MPRIS (FR-MON-03). The parsers are tested on constructed
//! values; the sensor runs against a private `dbus-daemon` with a fake player, so no real
//! player or session bus is needed. Skipped (loudly) if `dbus-daemon` is not installed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use neuroos_health::Health;
use neuroos_monitor::bus::{Bus, PassGate, Subscription};
use neuroos_monitor::clock::{Clock, SystemClock};
use neuroos_monitor::sensors::mpris::{
    MAX_TEXT_CHARS, MprisSensor, media_from_props, metadata_text, parse_playback, player_label,
    sanitize, vanished,
};
use neuroos_monitor::supervise;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{MediaChanged, PlaybackState};
use proptest::prelude::*;
use tokio::sync::watch;
use zbus::OwnedVariant;
use zbus::Variant as Value;
use zbus::{Array, Dict, Signature};

fn ov(v: Value<'_>) -> OwnedVariant {
    OwnedVariant::try_from(v).unwrap()
}

fn meta(title: Option<&str>, artist: Option<Value<'static>>) -> HashMap<String, OwnedVariant> {
    let mut m = HashMap::new();
    if let Some(t) = title {
        m.insert("xesam:title".to_owned(), ov(Value::from(t.to_owned())));
    }
    if let Some(a) = artist {
        m.insert("xesam:artist".to_owned(), ov(a));
    }
    m
}

fn artist_list(names: &[&str]) -> Value<'static> {
    let mut arr = Array::new(&Signature::Str);
    for n in names {
        arr.append(Value::from((*n).to_owned())).unwrap();
    }
    Value::Array(arr)
}

#[test]
fn text_is_cleaned_and_capped_because_players_are_untrusted() {
    assert_eq!(sanitize("Song\u{7}\n\tTitle\u{1b}[31m"), "SongTitle[31m");
    assert_eq!(
        sanitize(&"x".repeat(10_000)).chars().count(),
        MAX_TEXT_CHARS
    );
    assert_eq!(sanitize("ünïcödé 🎵"), "ünïcödé 🎵");
    assert_eq!(sanitize(""), "");
}

#[test]
fn playback_status_and_labels() {
    assert_eq!(parse_playback("Playing"), PlaybackState::Playing);
    assert_eq!(parse_playback("Paused"), PlaybackState::Paused);
    assert_eq!(parse_playback("Stopped"), PlaybackState::Stopped);
    assert_eq!(
        parse_playback("playing"),
        PlaybackState::Unspecified,
        "exact spelling only"
    );
    assert_eq!(parse_playback(""), PlaybackState::Unspecified);
    assert_eq!(player_label("org.mpris.MediaPlayer2.vlc"), "vlc");
    assert_eq!(
        player_label("org.mpris.MediaPlayer2.firefox.instance1234"),
        "firefox"
    );
    assert_eq!(player_label("weird"), "weird");
}

#[test]
fn metadata_handles_list_string_missing_and_wrong_types() {
    let (t, a) = metadata_text(&meta(Some("Song"), Some(artist_list(&["A", "B"]))));
    assert_eq!((t.as_str(), a.as_str()), ("Song", "A, B"));
    let (_, a) = metadata_text(&meta(None, Some(Value::from("Solo".to_owned()))));
    assert_eq!(a, "Solo", "some players send a plain string");
    assert_eq!(
        metadata_text(&HashMap::new()),
        (String::new(), String::new())
    );
    let mut wrong = HashMap::new();
    wrong.insert("xesam:title".to_owned(), ov(Value::from(42u32)));
    wrong.insert("xesam:artist".to_owned(), ov(Value::from(true)));
    assert_eq!(metadata_text(&wrong), (String::new(), String::new()));
    let (t, _) = metadata_text(&meta(Some(&"y".repeat(5000)), None));
    assert_eq!(t.chars().count(), MAX_TEXT_CHARS);
}

#[test]
fn a_property_map_becomes_a_media_event_and_a_vanished_player_is_stopped() {
    let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
    dict.append(
        Value::from("xesam:title".to_owned()),
        Value::Variant(Box::new(Value::from("Song".to_owned()))),
    )
    .unwrap();
    let mut props = HashMap::new();
    props.insert(
        "PlaybackStatus".to_owned(),
        ov(Value::from("Playing".to_owned())),
    );
    props.insert("Metadata".to_owned(), ov(Value::Dict(dict)));
    let m = media_from_props("vlc", &props);
    assert_eq!(
        (m.player.as_str(), m.state, m.title.as_str()),
        ("vlc", PlaybackState::Playing as i32, "Song")
    );
    let empty = media_from_props("x", &HashMap::new());
    assert_eq!(empty.state, PlaybackState::Unspecified as i32);
    let gone = vanished("vlc");
    assert_eq!(
        (gone.state, gone.title.as_str()),
        (PlaybackState::Stopped as i32, "")
    );
}

proptest! {
    #[test]
    fn sanitize_never_panics_and_never_keeps_control_characters(text in ".{0,2000}") {
        let s = sanitize(&text);
        prop_assert!(s.chars().count() <= MAX_TEXT_CHARS);
        prop_assert!(!s.chars().any(char::is_control));
    }
}

// ---------------------------------------------------------------- the sensor on a private bus

struct Daemon {
    child: Child,
    address: String,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon() -> Option<Daemon> {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some(Daemon {
        child,
        address: line.trim().to_owned(),
    })
}

struct FakePlayer {
    status: String,
    title: String,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl FakePlayer {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        let mut m = HashMap::new();
        m.insert(
            "xesam:title".to_owned(),
            OwnedVariant::try_from(Value::from(self.title.clone())).unwrap(),
        );
        m.insert(
            "xesam:artist".to_owned(),
            OwnedVariant::try_from(artist_list(&["Band"])).unwrap(),
        );
        m
    }
}

async fn player_on(address: &str, name: &str, status: &str, title: &str) -> zbus::Connection {
    zbus::connection::Builder::address(address)
        .name(name.to_owned())
        .serve_at(
            "/org/mpris/MediaPlayer2",
            FakePlayer {
                status: status.into(),
                title: title.into(),
            },
        )
        .build()
        .await
        .unwrap()
}

async fn media(sub: &Subscription) -> MediaChanged {
    let ev = tokio::time::timeout(Duration::from_secs(10), sub.next())
        .await
        .expect("a media event");
    match ev.event {
        Some(Event::Media(m)) => m,
        other => panic!("media expected: {other:?}"),
    }
}

#[tokio::test]
async fn local_players_appear_change_and_vanish_without_leaking_watchers() {
    let Some(daemon) = start_daemon() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let health = Health::new("neuroos-monitor", "test");
    let bus = Bus::new(
        64,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (_snap_tx, snap_rx) = watch::channel(0u64);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(supervise(
        MprisSensor::with_address(snap_rx, &daemon.address),
        bus,
        clock,
        Arc::clone(&health),
        stop_rx,
    ));

    // A player that is already there when the sensor starts, with hostile text.
    let player = player_on(
        &daemon.address,
        "org.mpris.MediaPlayer2.testplayer",
        "Playing",
        "Song\u{7}One",
    )
    .await;
    let first = media(&sub).await;
    assert_eq!(first.player, "testplayer");
    assert_eq!(first.state, PlaybackState::Playing as i32);
    assert_eq!(first.title, "SongOne", "control characters are removed");
    assert_eq!(first.artist, "Band");

    // It pauses: a PropertiesChanged signal arrives.
    let iface = player
        .object_server()
        .interface::<_, FakePlayer>("/org/mpris/MediaPlayer2")
        .await
        .unwrap();
    iface.get_mut().await.status = "Paused".into();
    iface
        .get()
        .await
        .playback_status_changed(iface.signal_emitter())
        .await
        .unwrap();
    assert_eq!(media(&sub).await.state, PlaybackState::Paused as i32);

    let gauge = |h: &Health| {
        h.snapshot()
            .gauges
            .iter()
            .find(|g| g.name == "media_players")
            .map(|g| g.value)
    };
    assert_eq!(gauge(&health), Some(1.0));

    // It disappears: STOPPED once, and the watcher is gone.
    drop(iface);
    drop(player);
    let gone = media(&sub).await;
    assert_eq!(
        (gone.player.as_str(), gone.state),
        ("testplayer", PlaybackState::Stopped as i32)
    );
    assert!(gone.title.is_empty());
    assert_eq!(
        gauge(&health),
        Some(0.0),
        "no watcher left for a vanished player"
    );
    assert!(sub.try_next().is_none(), "exactly one stop event");

    stop_tx.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn negative_control_a_missing_bus_degrades_instead_of_hanging() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let health = Health::new("neuroos-monitor", "test");
    let bus = Bus::new(
        8,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let (_t, snap_rx) = watch::channel(0u64);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(supervise(
        MprisSensor::with_address(snap_rx, "unix:path=/definitely/not/a/bus"),
        bus,
        clock,
        Arc::clone(&health),
        stop_rx,
    ));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let snap = health.snapshot();
    let row = snap.subsystems.iter().find(|s| s.name == "media").unwrap();
    assert_eq!(row.status(), neuroos_proto::v1::HealthStatus::Degraded);
    assert_eq!(row.detail, "session bus unavailable");
    stop_tx.send(true).unwrap();
    task.await.unwrap();
}
