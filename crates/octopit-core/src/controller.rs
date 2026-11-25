//! The background half of the window: polls the parts, runs the chat and the mic button, and
//! sends everything the screen needs as [`Update`] messages (ADR-0017). No GTK here.

use std::path::Path;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

use crate::chat::ChatEvent;
use crate::companion::{CompanionCommand, CompanionView};
use crate::gateway::{AuditRow, ChainStatus, EgressRow, Gateway};
use crate::gpu::{self, GpuState};
use crate::health::Snapshot;
use crate::voice::{MicAction, VoiceView};

/// Something the screen should show.
#[derive(Debug, Clone)]
pub enum Update {
    /// The state of every part.
    Snapshot(Snapshot),
    /// The voice part.
    Voice(VoiceView),
    /// The GPU.
    Gpu(GpuState),
    /// The companion: rhythm, talks, memories.
    Companion(CompanionView),
    /// A chat event for the running question.
    Chat(ChatEvent),
    /// The privacy ledgers, refreshed.
    Ledgers {
        /// Newest egress records, newest first.
        egress: Result<Vec<EgressRow>, String>,
        /// Newest audit records, newest first.
        audit: Result<Vec<AuditRow>, String>,
        /// Hash-chain check.
        chain: Result<ChainStatus, String>,
    },
    /// A short notice to show (a refused mic press, say).
    Notice(String),
}

/// What the window asks the background half to do.
#[derive(Debug, Clone)]
pub enum Command {
    /// Ask a question (already cleaned by `Conversation::begin`).
    Ask(String),
    /// Stop the running answer.
    StopAsk,
    /// Press the mic button.
    Mic(MicAction),
    /// Refresh the ledgers now.
    RefreshLedgers,
    /// Press a companion button.
    Companion(CompanionCommand),
}

/// How often each source is read.
#[derive(Debug, Clone, Copy)]
pub struct Rates {
    /// Part health.
    pub health: Duration,
    /// Voice state and level.
    pub voice: Duration,
    /// GPU.
    pub gpu: Duration,
    /// Ledgers.
    pub ledgers: Duration,
    /// The companion's status and memory list.
    pub companion: Duration,
}

impl Default for Rates {
    fn default() -> Self {
        Self {
            health: Duration::from_millis(1000),
            voice: Duration::from_millis(200),
            gpu: Duration::from_millis(2000),
            ledgers: Duration::from_secs(10),
            companion: Duration::from_secs(3),
        }
    }
}

/// The window's handle on the background half.
#[derive(Debug, Clone)]
pub struct Controller {
    commands: UnboundedSender<Command>,
}

impl Controller {
    /// Sends a command. Returns `false` when the background half has stopped.
    #[must_use]
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }
}

/// Starts the pollers and the command loop on `rt`. Dropping every [`Controller`] and the
/// returned receiver ends them.
#[must_use]
pub fn start(
    rt: &Handle,
    runtime_root: &Path,
    rates: Rates,
) -> (Controller, UnboundedReceiver<Update>) {
    let (up_tx, up_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let gw = Gateway::new(runtime_root);

    spawn_poll(rt, up_tx.clone(), rates.health, {
        let gw = gw.clone();
        move || {
            let gw = gw.clone();
            async move { Some(Update::Snapshot(gw.snapshot().await)) }
        }
    });
    spawn_poll(rt, up_tx.clone(), rates.voice, {
        let gw = gw.clone();
        move || {
            let gw = gw.clone();
            async move { Some(Update::Voice(gw.voice().await)) }
        }
    });
    spawn_poll(rt, up_tx.clone(), rates.companion, {
        let gw = gw.clone();
        move || {
            let gw = gw.clone();
            async move { Some(Update::Companion(gw.companion().await)) }
        }
    });
    spawn_poll(rt, up_tx.clone(), rates.gpu, || async {
        let state = tokio::task::spawn_blocking(gpu::sample)
            .await
            .unwrap_or(GpuState::NotAvailable);
        Some(Update::Gpu(state))
    });
    spawn_poll(rt, up_tx.clone(), rates.ledgers, {
        let gw = gw.clone();
        move || {
            let gw = gw.clone();
            async move { Some(ledgers(&gw).await) }
        }
    });
    rt.spawn(command_loop(gw, cmd_rx, up_tx));
    (Controller { commands: cmd_tx }, up_rx)
}

async fn ledgers(gw: &Gateway) -> Update {
    Update::Ledgers {
        egress: gw.egress(50).await,
        audit: gw.audit(50).await,
        chain: gw.verify_audit().await,
    }
}

fn spawn_poll<F, Fut>(rt: &Handle, tx: UnboundedSender<Update>, every: Duration, make: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<Update>> + Send + 'static,
{
    rt.spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Some(update) = make().await
                && tx.send(update).is_err()
            {
                return;
            }
        }
    });
}

async fn command_loop(
    gw: Gateway,
    mut commands: UnboundedReceiver<Command>,
    updates: UnboundedSender<Update>,
) {
    let mut stop: Option<watch::Sender<bool>> = None;
    while let Some(command) = commands.recv().await {
        match command {
            Command::Ask(question) => {
                if let Some(old) = stop.take() {
                    let _ = old.send(true);
                }
                let (stop_tx, stop_rx) = watch::channel(false);
                stop = Some(stop_tx);
                let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
                let gw = gw.clone();
                tokio::spawn(async move { gw.ask(question, ev_tx, stop_rx).await });
                let updates = updates.clone();
                tokio::spawn(async move {
                    while let Some(e) = ev_rx.recv().await {
                        if updates.send(Update::Chat(e)).is_err() {
                            return;
                        }
                    }
                });
            }
            Command::StopAsk => {
                if let Some(s) = stop.take() {
                    let _ = s.send(true);
                }
            }
            Command::Mic(action) => {
                if let Err(why) = gw.mic(action).await {
                    let _ = updates.send(Update::Notice(why));
                }
                // Show the new state at once instead of waiting for the next poll.
                let _ = updates.send(Update::Voice(gw.voice().await));
            }
            Command::RefreshLedgers => {
                let _ = updates.send(ledgers(&gw).await);
            }
            Command::Companion(c) => {
                if let Err(why) = gw.companion_control(c).await {
                    let _ = updates.send(Update::Notice(why));
                }
                // Show the effect at once instead of waiting for the next poll.
                let _ = updates.send(Update::Companion(gw.companion().await));
            }
        }
    }
    if let Some(s) = stop {
        let _ = s.send(true);
    }
}
