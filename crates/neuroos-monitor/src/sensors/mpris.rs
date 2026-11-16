//! Media players over MPRIS on the **session** bus (FR-MON-03, P3-S02; ADR-0007: never the
//! system bus). One small task per player; when a player's bus name vanishes its task is
//! aborted, so nothing leaks. Metadata comes from another program and is untrusted text:
//! control characters are removed and the length is capped.

use std::collections::HashMap;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Poll;

use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{MediaChanged, PlaybackState};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use zbus::Connection;
use zbus::OwnedVariant;
use zbus::export::futures_core::Stream;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::InterfaceName;

use crate::sensor::{Sensor, SensorError, SensorOut};

/// Every MPRIS player owns a name with this prefix.
pub const NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
/// Longest title or artist kept, in characters (a bound on what another program can make us hold).
pub const MAX_TEXT_CHARS: usize = 256;

/// Removes control characters and caps the length.
#[must_use]
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT_CHARS)
        .collect()
}

/// `PlaybackStatus` text to the wire enum; anything unknown is unspecified.
#[must_use]
pub fn parse_playback(status: &str) -> PlaybackState {
    match status {
        "Playing" => PlaybackState::Playing,
        "Paused" => PlaybackState::Paused,
        "Stopped" => PlaybackState::Stopped,
        _ => PlaybackState::Unspecified,
    }
}

/// The player name without the MPRIS prefix and without an instance suffix
/// (`org.mpris.MediaPlayer2.vlc.instance123` → `vlc`).
#[must_use]
pub fn player_label(bus_name: &str) -> String {
    let rest = bus_name.strip_prefix(NAME_PREFIX).unwrap_or(bus_name);
    let first = rest.split(".instance").next().unwrap_or(rest);
    sanitize(first)
}

fn string_of(v: &OwnedVariant) -> Option<String> {
    <&str>::try_from(v).ok().map(sanitize)
}

/// `(title, artist)` from an MPRIS `Metadata` dictionary. `xesam:artist` is a list of
/// strings, but some players send a single string; both work.
#[must_use]
pub fn metadata_text(meta: &HashMap<String, OwnedVariant>) -> (String, String) {
    let title = meta
        .get("xesam:title")
        .and_then(string_of)
        .unwrap_or_default();
    let artist = meta
        .get("xesam:artist")
        .and_then(|v| {
            Vec::<String>::try_from(v.try_clone().ok()?)
                .ok()
                .map(|a| a.iter().map(|s| sanitize(s)).collect::<Vec<_>>().join(", "))
                .or_else(|| string_of(v))
        })
        .unwrap_or_default();
    (title, artist)
}

/// One player's state from its `GetAll(Player)` reply.
#[must_use]
pub fn media_from_props(label: &str, props: &HashMap<String, OwnedVariant>) -> MediaChanged {
    let state = props
        .get("PlaybackStatus")
        .and_then(|v| <&str>::try_from(v).ok())
        .map_or(PlaybackState::Unspecified, parse_playback);
    let meta: HashMap<String, OwnedVariant> = props
        .get("Metadata")
        .and_then(|v| HashMap::<String, OwnedVariant>::try_from(v.try_clone().ok()?).ok())
        .unwrap_or_default();
    let (title, artist) = metadata_text(&meta);
    MediaChanged {
        player: label.to_owned(),
        state: state as i32,
        title,
        artist,
    }
}

/// The event for a player whose bus name disappeared.
#[must_use]
pub fn vanished(label: &str) -> MediaChanged {
    MediaChanged {
        player: label.to_owned(),
        state: PlaybackState::Stopped as i32,
        title: String::new(),
        artist: String::new(),
    }
}

/// Awaits the next item of a stream without needing a stream-extension crate.
async fn next<S: Stream + Unpin>(s: &mut S) -> Option<S::Item> {
    poll_fn(|cx| match Pin::new(&mut *s).poll_next(cx) {
        Poll::Ready(v) => Poll::Ready(v),
        Poll::Pending => Poll::Pending,
    })
    .await
}

type Update = (String, MediaChanged);

/// Reads one player's properties and forwards every change until the task is aborted.
async fn watch_player(conn: Connection, bus_name: String, tx: mpsc::Sender<Update>) {
    let label = player_label(&bus_name);
    let builder = PropertiesProxy::builder(&conn)
        .destination(bus_name.clone())
        .path(OBJECT_PATH);
    let Ok(props) = builder.build().await else {
        return;
    };
    let Ok(iface) = InterfaceName::try_from(PLAYER_INTERFACE) else {
        return;
    };
    let Ok(mut changes) = props.receive_properties_changed().await else {
        return;
    };
    loop {
        // Read after subscribing, so no change is missed between the two.
        if let Ok(all) = props.get_all(iface.clone()).await
            && tx
                .send((bus_name.clone(), media_from_props(&label, &all)))
                .await
                .is_err()
        {
            return;
        }
        if next(&mut changes).await.is_none() {
            return;
        }
    }
}

/// Emits `MediaChanged` for every player on the session bus.
#[derive(Debug)]
pub struct MprisSensor {
    snapshot: watch::Receiver<u64>,
    /// A bus address to use instead of the session bus (tests run a private bus).
    address: Option<String>,
}

impl MprisSensor {
    /// `snapshot` asks for a re-emit of every player's state after a pause (ADR-0011).
    #[must_use]
    pub fn new(snapshot: watch::Receiver<u64>) -> Self {
        Self {
            snapshot,
            address: None,
        }
    }

    /// Like [`MprisSensor::new`] on a bus at `address` (a private test bus).
    #[must_use]
    pub fn with_address(snapshot: watch::Receiver<u64>, address: &str) -> Self {
        Self {
            snapshot,
            address: Some(address.to_owned()),
        }
    }

    async fn connect(&self) -> Result<Connection, SensorError> {
        let conn = match &self.address {
            Some(a) => zbus::connection::Builder::address(a.as_str()).build().await,
            None => Connection::session().await,
        };
        conn.map_err(|_| SensorError::Unavailable("session bus unavailable"))
    }
}

/// What the sensor knows about the players it watches.
#[derive(Default)]
struct Players {
    tasks: HashMap<String, JoinHandle<()>>,
    last: HashMap<String, MediaChanged>,
}

impl Drop for Players {
    fn drop(&mut self) {
        for t in self.tasks.values() {
            t.abort();
        }
    }
}

impl Players {
    fn start(&mut self, conn: &Connection, name: &str, tx: &mpsc::Sender<Update>) {
        if self.tasks.contains_key(name) {
            return;
        }
        let task = tokio::spawn(watch_player(conn.clone(), name.to_owned(), tx.clone()));
        self.tasks.insert(name.to_owned(), task);
    }

    /// Stops watching `name`; returns the "stopped" event if it was being watched.
    fn vanish(&mut self, name: &str) -> Option<MediaChanged> {
        let task = self.tasks.remove(name)?;
        task.abort();
        self.last.remove(name);
        Some(vanished(&player_label(name)))
    }

    /// How many players are watched; reported as a gauge so a leak shows in health.
    #[must_use]
    fn watching(&self) -> usize {
        self.tasks.len()
    }
}

fn emit(out: &SensorOut, m: MediaChanged) {
    out.emit(out.now_ns(), Event::Media(m));
}

impl Sensor for MprisSensor {
    fn name(&self) -> &'static str {
        "media"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        fn unavailable<E>(_: E) -> SensorError {
            SensorError::Unavailable("session bus unavailable")
        }
        let conn = self.connect().await?;
        let dbus = DBusProxy::new(&conn).await.map_err(unavailable)?;
        let mut owners = dbus
            .receive_name_owner_changed()
            .await
            .map_err(unavailable)?;
        let (tx, mut rx) = mpsc::channel::<Update>(64);
        let mut players = Players::default();
        for name in dbus.list_names().await.map_err(unavailable)? {
            if name.as_str().starts_with(NAME_PREFIX) {
                players.start(&conn, name.as_str(), &tx);
            }
        }
        out.set_ok();
        loop {
            tokio::select! {
                owner = next(&mut owners) => {
                    let signal = owner.ok_or(SensorError::Unavailable("session bus closed"))?;
                    let args = signal.args().map_err(|_| SensorError::Protocol("bad name-owner signal"))?;
                    let name = args.name().to_string();
                    if !name.starts_with(NAME_PREFIX) { continue; }
                    if args.new_owner().is_none() {
                        if let Some(stopped) = players.vanish(&name) { emit(out, stopped); }
                    } else {
                        players.start(&conn, &name, &tx);
                    }
                }
                Some((name, media)) = rx.recv() => {
                    // A late update from a player that already vanished is dropped.
                    if players.tasks.contains_key(&name)
                        && players.last.get(&name) != Some(&media)
                    {
                        players.last.insert(name, media.clone());
                        emit(out, media);
                    }
                }
                changed = self.snapshot.changed() => {
                    if changed.is_ok() {
                        for m in players.last.values() { emit(out, m.clone()); }
                    }
                }
            }
            out.set_ok();
            #[allow(clippy::cast_precision_loss)] // a handful of players
            out.gauge("media_players", players.watching() as f64);
        }
    }
}
