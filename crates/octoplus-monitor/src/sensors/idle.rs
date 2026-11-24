//! Idle and active from `ext_idle_notifier_v1` (FR-MON-02, P3-S02; ADR-0007 decision 2).
//! Idle here means "no keyboard or pointer input for the timeout". The protocol's version-2
//! *input* notification is preferred because it ignores idle inhibitors: C1 reports raw facts,
//! and a playing video is already reported by the media sensor, so mixing it into "idle" would
//! hide the difference from the nudge hold rules. On a compositor that only has version 1 the
//! inhibitor-aware notification is used instead.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use octoplus_common::config::MonitorConfig;
use octoplus_proto::v1::IdleChanged;
use octoplus_proto::v1::monitor_event::Event;
use tokio::sync::{mpsc, watch};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1 as notification, ext_idle_notifier_v1 as notifier,
};

use super::wl::{self, Lost};
use crate::clock::Clock;
use crate::sensor::{Sensor, SensorError, SensorOut, StopOnDrop};

/// A change reported by the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    /// The idle notification is armed.
    Ready,
    /// The user went idle or came back, stamped on receipt (AB-8).
    Changed(u64, bool),
    /// The compositor is gone or lacks the protocol.
    Lost(&'static str),
}

struct WlState {
    tx: mpsc::Sender<Msg>,
    clock: Arc<dyn Clock>,
    notifier: Option<notifier::ExtIdleNotifierV1>,
    /// The protocol version actually bound (1 or 2).
    notifier_version: u32,
    seat: Option<wl_seat::WlSeat>,
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
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "ext_idle_notifier_v1" if state.notifier.is_none() => {
                state.notifier_version = version.min(2);
                state.notifier = Some(registry.bind::<notifier::ExtIdleNotifierV1, _, _>(
                    name,
                    state.notifier_version,
                    qh,
                    (),
                ));
            }
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<notifier::ExtIdleNotifierV1, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &notifier::ExtIdleNotifierV1,
        _: notifier::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<notification::ExtIdleNotificationV1, ()> for WlState {
    fn event(
        state: &mut Self,
        _: &notification::ExtIdleNotificationV1,
        event: notification::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let idle = match event {
            notification::Event::Idled => true,
            notification::Event::Resumed => false,
            _ => return,
        };
        let _ = state
            .tx
            .blocking_send(Msg::Changed(state.clock.now_ns(), idle));
    }
}

fn thread_main(tx: &mpsc::Sender<Msg>, clock: Arc<dyn Clock>, timeout_ms: u32, stop: &AtomicBool) {
    let lost = |why| {
        let _ = tx.blocking_send(Msg::Lost(why));
    };
    let Ok(conn) = wl::connect() else {
        return lost("no compositor to connect to");
    };
    let mut queue = conn.new_event_queue::<WlState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut state = WlState {
        tx: tx.clone(),
        clock,
        notifier: None,
        notifier_version: 1,
        seat: None,
    };
    if queue.roundtrip(&mut state).is_err() {
        return lost("compositor did not answer");
    }
    let (Some(notifier), Some(seat)) = (state.notifier.clone(), state.seat.clone()) else {
        return lost("compositor lacks the idle protocol");
    };
    // Kept alive for the whole loop: dropping the proxy would stop the notifications.
    let _armed = if state.notifier_version >= 2 {
        notifier.get_input_idle_notification(timeout_ms, &seat, &qh, ())
    } else {
        notifier.get_idle_notification(timeout_ms, &seat, &qh, ())
    };
    let _ = tx.blocking_send(Msg::Ready);
    if wl::pump(&mut queue, &mut state, stop) == Err(Lost::Connection) {
        lost("compositor connection lost");
    }
}

/// Emits `IdleChanged` when the user goes idle (after `idle_timeout_s`) or returns.
#[derive(Debug)]
pub struct IdleSensor {
    timeout_ms: u32,
    snapshot: watch::Receiver<u64>,
    idle: bool,
}

impl IdleSensor {
    /// `snapshot` asks for a re-emit of the current state after a pause (ADR-0011).
    #[must_use]
    pub fn new(cfg: &MonitorConfig, snapshot: watch::Receiver<u64>) -> Self {
        let ms = Duration::from_secs(cfg.idle_timeout_s).as_millis();
        Self {
            timeout_ms: u32::try_from(ms).unwrap_or(u32::MAX),
            snapshot,
            idle: false,
        }
    }

    fn emit(out: &SensorOut, at_ns: u64, idle: bool) {
        out.emit(at_ns, Event::Idle(IdleChanged { idle }));
    }

    /// The decision loop, separated from the thread so tests can feed it messages. The user
    /// is assumed active when the sensor starts: they just started it.
    ///
    /// # Errors
    /// [`SensorError::Unavailable`] when the compositor is lost.
    pub async fn run_messages(
        &mut self,
        out: &SensorOut,
        mut rx: mpsc::Receiver<Msg>,
    ) -> Result<(), SensorError> {
        self.idle = false;
        Self::emit(out, out.now_ns(), false);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Ready) => out.set_ok(),
                    Some(Msg::Changed(at, idle)) => {
                        self.idle = idle;
                        Self::emit(out, at, idle);
                    }
                    Some(Msg::Lost(why)) => return Err(SensorError::Unavailable(why)),
                    None => return Err(SensorError::Unavailable("compositor thread ended")),
                },
                changed = self.snapshot.changed() => {
                    if changed.is_ok() {
                        Self::emit(out, out.now_ns(), self.idle);
                    }
                }
            }
        }
    }
}

impl Sensor for IdleSensor {
    fn name(&self) -> &'static str {
        "idle"
    }

    async fn run(&mut self, out: &SensorOut) -> Result<(), SensorError> {
        let (tx, rx) = mpsc::channel(32);
        let stop = Arc::new(AtomicBool::new(false));
        let _guard = StopOnDrop(Arc::clone(&stop));
        let clock = Arc::clone(out.clock());
        let timeout_ms = self.timeout_ms;
        std::thread::Builder::new()
            .name("c1-wayland-idle".into())
            .spawn(move || thread_main(&tx, clock, timeout_ms, &stop))
            .map_err(|source| SensorError::Io {
                detail: "cannot start the compositor thread",
                source,
            })?;
        self.run_messages(out, rx).await
    }
}
