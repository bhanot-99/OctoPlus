//! Focused window, title and fullscreen from COSMIC's `zcosmic_toplevel_info_v1`
//! (FR-MON-01, P3-S01; ADR-0007 decision 2, spike S-05).
//!
//! [`Tracker`] is the whole decision logic and knows nothing about Wayland: it turns the
//! protocol's per-window events into "what is focused now". The thread and the sensor are
//! thin shells around it, so restarts, closes and unique ids are tested without a compositor.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1 as handle, zcosmic_toplevel_info_v1 as info,
};
use neuroos_proto::v1::FocusChanged;
use neuroos_proto::v1::monitor_event::Event;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use super::proc::ProcTable;
use super::wl::{self, Lost};
use crate::clock::Clock;
use crate::sensor::{Sensor, SensorError, SensorOut, StopOnDrop};

/// A same-window title or fullscreen change is held back until this long after the last
/// emission, and only the latest is sent: terminals rewrite their title many times a second.
/// A change of window is never delayed.
pub const TITLE_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// The protocol's per-window object id (valid for one connection only).
pub type Handle = u32;

/// One decoded protocol event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// A window appeared.
    New(Handle),
    /// Its application id.
    AppId(Handle, String),
    /// Its title.
    Title(Handle, String),
    /// Its state flags.
    State(Handle, WinState),
    /// All changes since the last `Done` are complete: commit them.
    Done(Handle),
    /// The window is gone.
    Closed(Handle),
}

/// The state flags C1 cares about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinState {
    /// The window has keyboard focus.
    pub activated: bool,
    /// The window is fullscreen.
    pub fullscreen: bool,
}

/// What is focused. `uid == 0` means nothing observable is focused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    /// C1's own window id: never reused, never shared with the compositor's ids.
    pub uid: u64,
    /// Application id.
    pub app_id: String,
    /// Window title.
    pub title: String,
    /// Fullscreen state.
    pub fullscreen: bool,
}

#[derive(Debug, Default)]
struct Win {
    uid: u64,
    app_id: String,
    title: String,
    state: WinState,
    /// Order in which the window became activated (latest wins if two claim focus).
    activated_seq: u64,
    committed: bool,
}

/// Turns window events into "what is focused", with ids that stay unique for the whole
/// life of the monitor.
#[derive(Debug)]
pub struct Tracker {
    next_uid: u64,
    seq: u64,
    windows: HashMap<Handle, Win>,
    current: Focus,
}

impl Tracker {
    /// A tracker whose first window gets `first_uid` (at least 1).
    #[must_use]
    pub fn new(first_uid: u64) -> Self {
        Self {
            next_uid: first_uid.max(1),
            seq: 0,
            windows: HashMap::new(),
            current: Focus::default(),
        }
    }

    /// The uid the next window will get. A restarted tracker starts here, so ids never repeat.
    #[must_use]
    pub fn next_uid(&self) -> u64 {
        self.next_uid
    }

    /// The last focus reported.
    #[must_use]
    pub fn current(&self) -> &Focus {
        &self.current
    }

    /// Applies one event; returns the new focus only if it changed.
    pub fn apply(&mut self, update: Update) -> Option<Focus> {
        match update {
            Update::New(h) => {
                let uid = self.next_uid;
                self.next_uid += 1;
                self.windows.insert(
                    h,
                    Win {
                        uid,
                        ..Win::default()
                    },
                );
                None
            }
            Update::AppId(h, v) => {
                self.windows.entry(h).or_default().app_id = v;
                None
            }
            Update::Title(h, v) => {
                self.windows.entry(h).or_default().title = v;
                None
            }
            Update::State(h, s) => {
                self.seq += 1;
                let seq = self.seq;
                let w = self.windows.entry(h).or_default();
                if s.activated && !w.state.activated {
                    w.activated_seq = seq;
                }
                w.state = s;
                None
            }
            Update::Done(h) => {
                self.windows.entry(h).or_default().committed = true;
                self.recompute()
            }
            Update::Closed(h) => {
                self.windows.remove(&h);
                self.recompute()
            }
        }
    }

    /// The compositor went away: every open window is closed. Returns "nothing focused"
    /// if something was.
    pub fn close_all(&mut self) -> Option<Focus> {
        self.windows.clear();
        self.recompute()
    }

    fn recompute(&mut self) -> Option<Focus> {
        let now = self
            .windows
            .values()
            .filter(|w| w.committed && w.state.activated)
            .max_by_key(|w| w.activated_seq)
            .map_or_else(Focus::default, |w| Focus {
                uid: w.uid,
                app_id: w.app_id.clone(),
                title: w.title.clone(),
                fullscreen: w.state.fullscreen,
            });
        (now != self.current).then(|| {
            self.current = now.clone();
            now
        })
    }
}

/// A message from the Wayland thread to the sensor.
#[derive(Debug)]
pub enum Msg {
    /// The protocol is bound and the first windows have been announced.
    Ready,
    /// A window event, stamped when it was received (AB-8).
    Update(u64, Update),
    /// The compositor is gone or unusable.
    Lost(&'static str),
}

struct WlState {
    tx: mpsc::Sender<Msg>,
    clock: Arc<dyn Clock>,
    info: Option<info::ZcosmicToplevelInfoV1>,
}

impl WlState {
    fn send(&self, u: Update) {
        // Block rather than drop: a lost `Done` or `Closed` would leave the Tracker out of step
        // with the compositor for good. This thread has nothing else to do, a slow consumer
        // only makes the compositor buffer events briefly, and a gone receiver makes this
        // return at once (the thread then ends through the stop flag).
        let _ = self.tx.blocking_send(Msg::Update(self.clock.now_ns(), u));
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WlState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
            && interface == "zcosmic_toplevel_info_v1"
            && state.info.is_none()
        {
            // Version 1 announces windows directly (spike S-05).
            state.info = Some(registry.bind::<info::ZcosmicToplevelInfoV1, _, _>(name, 1, qh, ()));
        }
    }
}

impl Dispatch<info::ZcosmicToplevelInfoV1, ()> for WlState {
    fn event(
        state: &mut Self,
        _: &info::ZcosmicToplevelInfoV1,
        event: info::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let info::Event::Toplevel { toplevel } = event {
            state.send(Update::New(toplevel.id().protocol_id()));
        }
    }

    fn event_created_child(
        _: u16,
        qh: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_client::backend::ObjectData> {
        qh.make_data::<handle::ZcosmicToplevelHandleV1, _>(())
    }
}

/// The `state` event carries an array of u32 flags in native byte order.
fn decode_state(raw: &[u8]) -> WinState {
    let mut s = WinState::default();
    let (words, _) = raw.as_chunks::<4>();
    for word in words {
        match handle::State::try_from(u32::from_ne_bytes(*word)) {
            Ok(handle::State::Activated) => s.activated = true,
            Ok(handle::State::Fullscreen) => s.fullscreen = true,
            _ => {}
        }
    }
    s
}

impl Dispatch<handle::ZcosmicToplevelHandleV1, ()> for WlState {
    fn event(
        state: &mut Self,
        proxy: &handle::ZcosmicToplevelHandleV1,
        event: handle::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let h = proxy.id().protocol_id();
        match event {
            handle::Event::AppId { app_id } => state.send(Update::AppId(h, app_id)),
            handle::Event::Title { title } => state.send(Update::Title(h, title)),
            handle::Event::State { state: raw } => state.send(Update::State(h, decode_state(&raw))),
            handle::Event::Done => state.send(Update::Done(h)),
            handle::Event::Closed => state.send(Update::Closed(h)),
            _ => {}
        }
    }
}

/// The Wayland thread body: bind, announce readiness, dispatch until stopped or lost.
fn thread_main(tx: &mpsc::Sender<Msg>, clock: Arc<dyn Clock>, stop: &AtomicBool) {
    let lost = |why| {
        let _ = tx.blocking_send(Msg::Lost(why));
    };
    let Ok(conn) = wl::connect() else {
        return lost("no compositor to connect to");
    };
    let mut queue = conn.new_event_queue::<WlState>();
    let _registry = conn.display().get_registry(&queue.handle(), ());
    let mut state = WlState {
        tx: tx.clone(),
        clock,
        info: None,
    };
    if queue.roundtrip(&mut state).is_err() || queue.roundtrip(&mut state).is_err() {
        return lost("compositor did not answer");
    }
    if state.info.is_none() {
        return lost("compositor lacks the toplevel protocol");
    }
    let _ = tx.blocking_send(Msg::Ready);
    if wl::pump(&mut queue, &mut state, stop) == Err(Lost::Connection) {
        lost("compositor connection lost");
    }
}

/// Emits `FocusChanged` when the focused window, its title or its fullscreen state changes.
#[derive(Debug)]
pub struct FocusSensor {
    table: Arc<ProcTable>,
    snapshot: watch::Receiver<u64>,
    next_uid: u64,
}

impl FocusSensor {
    /// `table` supplies the pid hint and `snapshot` asks for a re-emit after a pause
    /// (ADR-0011 decision 2).
    #[must_use]
    pub fn new(table: Arc<ProcTable>, snapshot: watch::Receiver<u64>) -> Self {
        Self {
            table,
            snapshot,
            next_uid: 1,
        }
    }
}

/// Holds back same-window changes that arrive too soon after the last emission.
#[derive(Debug, Default)]
struct Pacer {
    last_uid: u64,
    last_at: Option<Instant>,
    pending: Option<Focus>,
}

impl Pacer {
    /// `Some(focus)` to emit now; otherwise the change is parked in `pending`.
    fn offer(&mut self, f: Focus, now: Instant) -> Option<Focus> {
        let quick_same_window = f.uid == self.last_uid
            && self
                .last_at
                .is_some_and(|t| now.duration_since(t) < TITLE_MIN_INTERVAL);
        if quick_same_window {
            self.pending = Some(f);
            return None;
        }
        self.sent(&f, now);
        Some(f)
    }

    fn sent(&mut self, f: &Focus, now: Instant) {
        self.last_uid = f.uid;
        self.last_at = Some(now);
        self.pending = None;
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref()?;
        self.last_at.map(|t| t + TITLE_MIN_INTERVAL)
    }
}

impl FocusSensor {
    fn emit(&self, out: &SensorOut, at_ns: u64, f: &Focus) {
        // A pid hint from the cached table: no `/proc` walk on the capture path.
        let pid = if f.uid == 0 {
            0
        } else {
            self.table.resolve(&f.app_id)
        };
        out.emit(
            at_ns,
            Event::Focus(FocusChanged {
                app_id: f.app_id.clone(),
                title: f.title.clone(),
                pid,
                fullscreen: f.fullscreen,
                fullscreen_known: f.uid != 0,
            }),
        );
    }

    /// The uid the next window will get; grows across restarts, never repeats.
    #[must_use]
    pub fn next_uid(&self) -> u64 {
        self.next_uid
    }

    /// The decision loop, separated from the thread so tests can feed it messages.
    ///
    /// # Errors
    /// [`SensorError::Unavailable`] when the compositor is lost; open windows are closed first.
    pub async fn run_messages(
        &mut self,
        out: &SensorOut,
        mut rx: mpsc::Receiver<Msg>,
    ) -> Result<(), SensorError> {
        let mut tracker = Tracker::new(self.next_uid);
        let mut pacer = Pacer::default();
        let result = loop {
            let wait = pacer.deadline();
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Ready) => out.set_ok(),
                    Some(Msg::Update(at, u)) => {
                        if let Some(f) = tracker.apply(u)
                            && let Some(f) = pacer.offer(f, Instant::now())
                        {
                            self.emit(out, at, &f);
                        }
                    }
                    Some(Msg::Lost(why)) => break Err(SensorError::Unavailable(why)),
                    None => break Err(SensorError::Unavailable("compositor thread ended")),
                },
                () = async { tokio::time::sleep_until(wait.unwrap_or_else(Instant::now)).await }, if wait.is_some() => {
                    if let Some(f) = pacer.pending.take() {
                        pacer.sent(&f, Instant::now());
                        self.emit(out, out.now_ns(), &f);
                    }
                }
                changed = self.snapshot.changed() => {
                    if changed.is_ok() {
                        let f = tracker.current().clone();
                        pacer.sent(&f, Instant::now());
                        self.emit(out, out.now_ns(), &f);
                    }
                }
            }
        };
        // Windows of a lost compositor are closed; the next run starts with fresh, unique ids.
        if let Some(none) = tracker.close_all() {
            self.emit(out, out.now_ns(), &none);
        }
        self.next_uid = tracker.next_uid();
        result
    }
}

impl Sensor for FocusSensor {
    fn name(&self) -> &'static str {
        "focus"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let (tx, rx) = mpsc::channel(256);
        let stop = Arc::new(AtomicBool::new(false));
        let _guard = StopOnDrop(Arc::clone(&stop));
        let clock = Arc::clone(out.clock());
        std::thread::Builder::new()
            .name("c1-wayland-focus".into())
            .spawn(move || thread_main(&tx, clock, &stop))
            .map_err(|source| SensorError::Io {
                detail: "cannot start the compositor thread",
                source,
            })?;
        self.run_messages(out, rx).await
    }
}
