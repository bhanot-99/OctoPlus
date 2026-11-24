#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The nudge engine against fake C3 and C6 (P6-S01…S07): it fires once, stays quiet at bad
//! moments and delivers afterwards, merges what piled up, rebuilds its state after a restart,
//! idles when C3 is down, applies replies, and never dispatches anything by itself.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use octoplus_common::config::NudgesConfig;
use octoplus_ipc::server::{ServerConfig, ensure_private_dir};
use octoplus_ipc::{Client, Handler, HandlerError, ReconnectingClient, Request, Server, StreamSink};
use octoplus_knowledge::nudge::engine::{NudgeHandle, NudgePeers};
use octoplus_knowledge::nudge::rules::defaults;
use octoplus_knowledge::turn::{KnowledgeHandler, Metrics, Peers, Timing};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{
    Ack, AskRequest, BatterySample, ChargeState, DiskSample, DispatchResult, DispatchStatus,
    ErrorCode, EvaluateResult, HealthSignals, MonitorEvent, NudgeListRequest, NudgeLogEntries,
    NudgeLogEntry, NudgeOutcome, NudgeSetEnabled, NudgeTest, Origin, Taint, Tier, VoiceStatus,
};

const S: u64 = 1_000_000_000;
const GIB: u64 = 1_073_741_824;
/// 2027-01-01 12:00:00 UTC: outside the default quiet hours.
const NOON: u64 = (1_798_761_600 + 12 * 3600) * S;
/// 2027-01-01 23:30:00 UTC: inside them.
const LATE: u64 = (1_798_761_600 + 23 * 3600 + 30 * 60) * S;

// ---- fakes ------------------------------------------------------------------------------------

type Notify = (String, Vec<(String, f64)>, bool);

#[derive(Debug, Default)]
struct Rec {
    notifies: Vec<Notify>,
    dispatches: Vec<(String, String, u32)>,
    log: Vec<(String, i32, u64)>,
    spoken: Vec<Notify>,
}

type Shared = Arc<Mutex<Rec>>;

struct FakeC6 {
    rec: Shared,
    notify_fails: bool,
}

impl Handler for FakeC6 {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::NotifyRequest(n) => {
                if self.notify_fails {
                    return Err(HandlerError::new(ErrorCode::Unavailable, "no bus", true));
                }
                self.rec.lock().unwrap().notifies.push((
                    n.template_id,
                    n.args.into_iter().map(|a| (a.name, a.value)).collect(),
                    n.silent,
                ));
                Ok(Body::Ack(Ack {}))
            }
            Body::DispatchRequest(d) => {
                self.rec.lock().unwrap().dispatches.push((
                    d.skill,
                    d.args_json,
                    d.context_taint.map_or(u32::MAX, |t| t.flags),
                ));
                Ok(Body::DispatchResult(DispatchResult {
                    status: DispatchStatus::Executed as i32,
                    result_json: r#"{"filesystem":{"total_bytes":100,"available_bytes":10}}"#
                        .into(),
                    tier: Tier::Safe as i32,
                    result_taint: Some(Taint { flags: 0 }),
                }))
            }
            Body::EvaluateRequest(_) => Ok(Body::EvaluateResult(EvaluateResult {
                tier: Tier::Safe as i32,
                allowed_without_dialog: true,
            })),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

/// A stand-in C2: answers the status question and takes spoken nudges.
struct FakeC2 {
    rec: Shared,
    status: Arc<Mutex<VoiceStatus>>,
    speak_fails: bool,
}

impl Handler for FakeC2 {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::VoiceStatusRequest(_) => Ok(Body::VoiceStatus(*self.status.lock().unwrap())),
            Body::SpeakNudge(n) => {
                if self.speak_fails {
                    return Err(HandlerError::new(
                        ErrorCode::Unavailable,
                        "no speaker",
                        true,
                    ));
                }
                self.rec.lock().unwrap().spoken.push((
                    n.template_id,
                    n.args.into_iter().map(|a| (a.name, a.value)).collect(),
                    false,
                ));
                Ok(Body::Ack(Ack {}))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

struct FakeC4;

impl Handler for FakeC4 {
    async fn handle(&self, _req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        Err(HandlerError::unsupported())
    }
}

struct FakeC3 {
    rec: Shared,
    signals: Arc<Mutex<HealthSignals>>,
    stored: Arc<Mutex<Vec<NudgeLogEntry>>>,
}

impl Handler for FakeC3 {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::QueryHealthSignals(_) => {
                Ok(Body::HealthSignals(self.signals.lock().unwrap().clone()))
            }
            Body::QueryNudgeLog(_) => Ok(Body::NudgeLogEntries(NudgeLogEntries {
                entries: self.stored.lock().unwrap().clone(),
            })),
            Body::StoreNudgeLog(n) => {
                self.rec
                    .lock()
                    .unwrap()
                    .log
                    .push((n.rule_id, n.outcome, n.at_ns));
                Ok(Body::Ack(Ack {}))
            }
            Body::QueryHybrid(_) => Ok(Body::HybridResult(
                octoplus_proto::v1::HybridResult::default(),
            )),
            Body::ProfileListRequest(_) => Ok(Body::ProfileFacts(
                octoplus_proto::v1::ProfileFacts::default(),
            )),
            Body::StoreInteraction(_) => Ok(Body::Ack(Ack {})),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

// ---- the stack ----------------------------------------------------------------------------------

struct Stack {
    _dir: tempfile::TempDir,
    c5: std::path::PathBuf,
    rec: Shared,
    signals: Arc<Mutex<HealthSignals>>,
    stored: Arc<Mutex<Vec<NudgeLogEntry>>>,
    voice_status: Arc<Mutex<VoiceStatus>>,
    nudge: NudgeHandle,
    stop: tokio::sync::watch::Sender<bool>,
}

struct Opts {
    c3_up: bool,
    c6_up: bool,
    c4_up: bool,
    notify_fails: bool,
    voice_up: bool,
    speak_fails: bool,
    cfg: NudgesConfig,
    stored: Vec<NudgeLogEntry>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            c3_up: true,
            c6_up: true,
            c4_up: true,
            notify_fails: false,
            voice_up: false,
            speak_fails: false,
            cfg: NudgesConfig::default(),
            stored: Vec::new(),
        }
    }
}

async fn serve<H: Handler>(sock: &Path, h: H, stop: &tokio::sync::watch::Sender<bool>) {
    let server = Server::bind(sock, ServerConfig::new(1 << 20, Duration::from_secs(5)))
        .await
        .unwrap();
    let mut rx = stop.subscribe();
    tokio::spawn(server.serve(Arc::new(h), async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
}

fn quiet_signals() -> HealthSignals {
    HealthSignals {
        samples: vec![],
        idle: false,
        locked: false,
        fullscreen: false,
        media_playing: false,
        active_since_ns: 0,
        locked_known: false,
    }
}

impl Stack {
    async fn start(o: Opts) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socks = dir.path().join("s");
        ensure_private_dir(&socks).unwrap();
        let (stop, _) = tokio::sync::watch::channel(false);
        let rec: Shared = Arc::default();
        let signals = Arc::new(Mutex::new(quiet_signals()));
        let stored = Arc::new(Mutex::new(o.stored.clone()));
        let voice_status = Arc::new(Mutex::new(VoiceStatus {
            capture_running: true,
            ..VoiceStatus::default()
        }));
        let c2 = socks.join("c2.sock");
        if o.voice_up {
            serve(
                &c2,
                FakeC2 {
                    rec: rec.clone(),
                    status: voice_status.clone(),
                    speak_fails: o.speak_fails,
                },
                &stop,
            )
            .await;
        }
        let (c6, c3, c4, c5) = (
            socks.join("c6.sock"),
            socks.join("c3.sock"),
            socks.join("c4.sock"),
            socks.join("c5.sock"),
        );
        if o.c6_up {
            serve(
                &c6,
                FakeC6 {
                    rec: rec.clone(),
                    notify_fails: o.notify_fails,
                },
                &stop,
            )
            .await;
        }
        if o.c3_up {
            serve(
                &c3,
                FakeC3 {
                    rec: rec.clone(),
                    signals: signals.clone(),
                    stored: stored.clone(),
                },
                &stop,
            )
            .await;
        }
        if o.c4_up {
            serve(&c4, FakeC4, &stop).await;
        }
        let c = |p: &Path| Arc::new(ReconnectingClient::new(p, 1 << 20));
        let peers = Peers {
            kernel: c(&c6),
            storage: c(&c3),
            inference: c(&c4),
        };
        // In production the maintainer task keeps these connected; the tests do it once.
        let _ = peers.kernel.get().await;
        let _ = peers.inference.get().await;
        let voice = c(&c2);
        let _ = voice.get().await;
        let nudge = NudgeHandle::new(
            defaults().unwrap(),
            o.cfg.clone(),
            NudgePeers {
                storage: peers.storage.clone(),
                kernel: peers.kernel.clone(),
                inference: peers.inference.clone(),
                voice,
            },
        );
        let counter = || Arc::new(std::sync::atomic::AtomicU64::new(0));
        let metrics = Metrics {
            first_token: Arc::new(octoplus_health::LatencyHistogram::default()),
            compute: Arc::new(octoplus_health::LatencyHistogram::default()),
            requests: counter(),
            completed: counter(),
            c4_unavailable: counter(),
            c3_degraded: counter(),
            route_action: counter(),
            route_desktop: counter(),
            route_general: counter(),
            evidence_no_taint: counter(),
            barge_ins: counter(),
        };
        let handler = KnowledgeHandler::new(
            peers,
            octoplus_common::config::KnowledgeConfig::default(),
            Timing {
                kernel_check: Duration::from_millis(500),
                storage_query: Duration::from_millis(300),
                generation: Duration::from_secs(5),
                control: Duration::from_millis(250),
                dispatch: Duration::from_secs(5),
                web_ingest: Duration::from_millis(600),
            },
            metrics,
            nudge.clone(),
        );
        serve(&c5, handler, &stop).await;
        Self {
            _dir: dir,
            c5,
            rec,
            signals,
            stored,
            voice_status,
            nudge,
            stop,
        }
    }

    async fn tick(&self, now: u64) {
        self.nudge.tick(now, &jiff::tz::TimeZone::UTC, false).await;
    }

    /// A tick that first asks C2 about the moment, as the engine loop does.
    async fn tick_voice(&self, now: u64) {
        let view = self.nudge.voice_view().await;
        self.nudge
            .tick_with(now, &jiff::tz::TimeZone::UTC, view)
            .await;
    }

    async fn run_disk_voice(&self, from: u64, secs: u64, used_pct: u64) {
        let mut t = 0;
        while t <= secs {
            self.set_disk(from + t * S, used_pct);
            self.tick_voice(from + t * S).await;
            t += 30;
        }
    }

    fn set_disk(&self, now: u64, used_pct: u64) {
        let mut s = self.signals.lock().unwrap();
        s.samples = vec![MonitorEvent {
            observed_at_ns: now,
            event: Some(Event::Disk(DiskSample {
                mount_point: "/".into(),
                total_bytes: 100 * GIB,
                used_bytes: used_pct * GIB,
                free_bytes: (100 - used_pct) * GIB,
            })),
        }];
    }

    async fn ask(&self, text: &str, at_ns: u64) -> (String, octoplus_proto::v1::AnswerDone) {
        let client = Client::connect(&self.c5, 1 << 20).await.unwrap();
        let mut stream = client
            .stream(
                Body::AskRequest(AskRequest {
                    text: text.into(),
                    origin: Origin::Cli as i32,
                    at_ns,
                }),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let mut said = String::new();
        let mut done = None;
        while let Some(item) = stream.next().await {
            // A stand-in C4 that cannot answer ends the stream with an error: not what is tested.
            let Ok(body) = item else {
                break;
            };
            match body {
                Body::AnswerChunk(c) => said.push_str(&c.text),
                Body::AnswerDone(d) => {
                    done = Some(d);
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        (said, done.unwrap_or_default())
    }

    async fn admin(&self, body: Body) -> Result<Body, octoplus_ipc::IpcError> {
        let client = Client::connect(&self.c5, 1 << 20).await.unwrap();
        client.call(body, Duration::from_secs(5)).await
    }

    fn rec(&self) -> std::sync::MutexGuard<'_, Rec> {
        self.rec.lock().unwrap()
    }

    fn logged(&self, rule: &str, outcome: NudgeOutcome) -> usize {
        self.rec()
            .log
            .iter()
            .filter(|(r, o, _)| r == rule && *o == outcome as i32)
            .count()
    }

    /// Ticks every 30 s from `from` for `secs` seconds with a disk at `used_pct`.
    async fn run_disk(&self, from: u64, secs: u64, used_pct: u64) {
        let mut t = 0;
        while t <= secs {
            self.set_disk(from + t * S, used_pct);
            self.tick(from + t * S).await;
            t += 30;
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

fn entry(rule: &str, outcome: NudgeOutcome, at: u64) -> NudgeLogEntry {
    NudgeLogEntry {
        rule_id: rule.into(),
        outcome: outcome as i32,
        at_ns: at,
    }
}

// ---- firing ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_full_disk_fires_once_after_its_hold_time_and_nothing_acts() {
    let s = Stack::start(Opts::default()).await;
    s.run_disk(NOON, 1800, 92).await;
    let rec = s.rec();
    assert_eq!(rec.notifies.len(), 1, "{:?}", rec.notifies);
    let (template, args, silent) = &rec.notifies[0];
    assert_eq!(template, "disk.nearly_full");
    assert_eq!(args, &vec![("percent".to_string(), 92.0)]);
    assert!(!silent);
    assert!(rec.dispatches.is_empty(), "a nudge never acts (R0-6)");
    drop(rec);
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Notified), 1);
}

#[tokio::test]
async fn it_stays_silent_below_the_thresholds_and_for_a_blip() {
    let s = Stack::start(Opts::default()).await;
    s.run_disk(NOON, 600, 85).await;
    // 90 % for only one minute (the hold is two) and then back down.
    s.set_disk(NOON + 700 * S, 91);
    s.tick(NOON + 700 * S).await;
    s.set_disk(NOON + 730 * S, 91);
    s.tick(NOON + 730 * S).await;
    s.set_disk(NOON + 760 * S, 80);
    s.tick(NOON + 760 * S).await;
    assert!(s.rec().notifies.is_empty());
}

#[tokio::test]
async fn nothing_is_stored_or_said_without_data() {
    // No samples at all: the monitor looks stale, so nothing sample-based is reported.
    let s = Stack::start(Opts::default()).await;
    for i in 0..10 {
        s.tick(NOON + i * 30 * S).await;
    }
    assert!(s.rec().notifies.is_empty());
}

// ---- bad moments -----------------------------------------------------------------------------------

#[tokio::test]
async fn a_pause_holds_the_nudge_and_it_arrives_when_the_pause_ends() {
    let s = Stack::start(Opts::default()).await;
    s.nudge.note_pause(NOON, 10); // C5 dispatched "pause for 10 minutes"
    s.run_disk(NOON, 480, 92).await; // fires at 120 s, inside the pause
    assert!(s.rec().notifies.is_empty(), "held during the pause");
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Held), 1);
    // The pause is over at +600 s; the next tick releases the held nudge.
    s.set_disk(NOON + 660 * S, 92);
    s.tick(NOON + 660 * S).await;
    assert_eq!(s.rec().notifies.len(), 1);
    assert_eq!(s.rec().notifies[0].0, "disk.nearly_full");
}

#[tokio::test]
async fn quiet_hours_hold_important_nudges_and_make_urgent_ones_silent() {
    let s = Stack::start(Opts::default()).await;
    s.run_disk(LATE, 150, 92).await; // important
    assert!(s.rec().notifies.is_empty());
    s.run_disk(LATE + 300 * S, 120, 96).await; // critical: urgent
    let rec = s.rec();
    assert!(
        rec.notifies
            .iter()
            .any(|(t, _, silent)| t == "disk.critical" && *silent),
        "{:?}",
        rec.notifies
    );
}

#[tokio::test]
async fn a_turn_in_progress_holds_nudges() {
    let s = Stack::start(Opts::default()).await;
    let guard = s.nudge.turn_started();
    s.run_disk(NOON, 240, 92).await;
    assert!(s.rec().notifies.is_empty());
    drop(guard);
    s.set_disk(NOON + 300 * S, 92);
    s.tick(NOON + 300 * S).await;
    assert_eq!(s.rec().notifies.len(), 1);
}

#[tokio::test]
async fn being_away_holds_and_several_held_nudges_become_one_line() {
    let s = Stack::start(Opts::default()).await;
    {
        let mut sig = s.signals.lock().unwrap();
        sig.idle = true;
    }
    for t in (0..=240).step_by(30) {
        let now = NOON + t * S;
        {
            let mut sig = s.signals.lock().unwrap();
            sig.samples = vec![
                MonitorEvent {
                    observed_at_ns: now,
                    event: Some(Event::Disk(DiskSample {
                        mount_point: "/".into(),
                        total_bytes: 100 * GIB,
                        used_bytes: 92 * GIB,
                        free_bytes: 8 * GIB,
                    })),
                },
                MonitorEvent {
                    observed_at_ns: now,
                    event: Some(Event::Battery(BatterySample {
                        percent: 9.0,
                        state: ChargeState::Discharging as i32,
                        time_estimate_s: 0,
                        time_estimate_known: false,
                        on_ac: false,
                    })),
                },
            ];
        }
        s.tick(now).await;
    }
    assert!(s.rec().notifies.is_empty(), "held while away");
    // The user comes back.
    s.signals.lock().unwrap().idle = false;
    s.tick(NOON + 270 * S).await;
    let rec = s.rec();
    assert_eq!(rec.notifies.len(), 1, "{:?}", rec.notifies);
    assert_eq!(rec.notifies[0].0, "nudge.while_away");
    assert_eq!(rec.notifies[0].1, vec![("count".to_string(), 2.0)]);
}

// ---- parts that are down ------------------------------------------------------------------------------

#[tokio::test]
async fn with_c3_down_the_engine_idles_and_reports_it() {
    let s = Stack::start(Opts {
        c3_up: false,
        ..Opts::default()
    })
    .await;
    assert!(!s.nudge.is_idle());
    s.tick(NOON).await;
    assert!(s.nudge.is_idle(), "reported DEGRADED, not guessing");
    assert!(s.rec().notifies.is_empty());
}

#[tokio::test]
async fn a_failed_notification_is_counted_and_logged_as_dropped() {
    let s = Stack::start(Opts {
        notify_fails: true,
        ..Opts::default()
    })
    .await;
    s.run_disk(NOON, 150, 92).await;
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Dropped), 1);
    assert!(
        !s.nudge.reply_window_open(NOON + 150 * S),
        "no window for a nudge nobody saw"
    );
}

#[tokio::test]
async fn a_clock_jump_does_not_cause_a_burst() {
    let s = Stack::start(Opts::default()).await;
    s.run_disk(NOON, 60, 92).await; // 60 s of the 120 s hold
    // The clock jumps an hour ahead: nothing fires on that tick or the next, and the hold
    // starts over.
    let jump = NOON + 3600 * S;
    for (i, t) in [jump, jump + 30 * S, jump + 60 * S].into_iter().enumerate() {
        s.set_disk(t, 92);
        s.tick(t).await;
        assert!(s.rec().notifies.is_empty(), "tick {i} after the jump");
    }
    s.set_disk(jump + 90 * S, 92);
    s.tick(jump + 90 * S).await;
    s.set_disk(jump + 150 * S, 92);
    s.tick(jump + 150 * S).await;
    assert_eq!(s.rec().notifies.len(), 1);
}

// ---- state survives a restart ----------------------------------------------------------------------------

#[tokio::test]
async fn cooldowns_snoozes_and_switches_are_rebuilt_from_the_log() {
    let recent = NOON - 3600 * S;
    let s = Stack::start(Opts {
        stored: vec![
            entry(
                "running_hot",
                NudgeOutcome::Disabled,
                NOON - 400 * 86_400 * S,
            ),
            entry("disk_nearly_full", NudgeOutcome::Notified, recent),
            entry("battery_low", NudgeOutcome::Snoozed, recent),
        ],
        ..Opts::default()
    })
    .await;
    // Disk at 92 % is inside its 24 h cooldown; the hot laptop's rule was switched off long
    // ago; the battery rule is snoozed for 4 h.
    for t in (0..=300).step_by(30) {
        let now = NOON + t * S;
        s.signals.lock().unwrap().samples = vec![
            MonitorEvent {
                observed_at_ns: now,
                event: Some(Event::Disk(DiskSample {
                    mount_point: "/".into(),
                    total_bytes: 100 * GIB,
                    used_bytes: 92 * GIB,
                    free_bytes: 8 * GIB,
                })),
            },
            MonitorEvent {
                observed_at_ns: now,
                event: Some(Event::Thermal(octoplus_proto::v1::ThermalSample {
                    sensor: "cpu".into(),
                    celsius: 97.0,
                })),
            },
            MonitorEvent {
                observed_at_ns: now,
                event: Some(Event::Battery(BatterySample {
                    percent: 9.0,
                    state: ChargeState::Discharging as i32,
                    time_estimate_s: 0,
                    time_estimate_known: false,
                    on_ac: false,
                })),
            },
        ];
        s.tick(now).await;
    }
    assert!(s.rec().notifies.is_empty());
    let status = s.nudge.list();
    assert!(
        !status
            .iter()
            .find(|r| r.rule_id == "running_hot")
            .unwrap()
            .enabled
    );
    // Five hours later the snooze is over and the battery rule speaks.
    let later = NOON + 5 * 3600 * S;
    {
        let mut sig = s.signals.lock().unwrap();
        sig.samples = vec![MonitorEvent {
            observed_at_ns: later,
            event: Some(Event::Battery(BatterySample {
                percent: 9.0,
                state: ChargeState::Discharging as i32,
                time_estimate_s: 0,
                time_estimate_known: false,
                on_ac: false,
            })),
        }];
    }
    s.tick(later).await;
    s.tick(later + 30 * S).await;
    assert_eq!(
        s.rec()
            .notifies
            .iter()
            .filter(|n| n.0 == "battery.low")
            .count(),
        1
    );
}

// ---- replies ----------------------------------------------------------------------------------------------

async fn with_open_window() -> Stack {
    let s = Stack::start(Opts::default()).await;
    s.run_disk(NOON, 150, 92).await;
    assert_eq!(s.rec().notifies.len(), 1);
    s
}

#[tokio::test]
async fn yes_asks_for_the_offered_skill_only_through_the_reply() {
    let s = with_open_window().await;
    assert!(s.rec().dispatches.is_empty());
    let (said, _) = s.ask("show me", NOON + 160 * S).await;
    assert!(said.starts_with("Okay."), "{said}");
    let rec = s.rec();
    assert_eq!(rec.dispatches.len(), 1);
    assert_eq!(rec.dispatches[0].0, "system.disk_report");
    assert_eq!(rec.dispatches[0].1, "{}");
    assert_ne!(
        rec.dispatches[0].2 & (1 << 1),
        0,
        "the user's own word is in the taint"
    );
    drop(rec);
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Accepted), 1);
}

#[tokio::test]
async fn not_now_snoozes_stop_switches_off_and_no_dismisses() {
    let s = with_open_window().await;
    let (said, done) = s.ask("not now", NOON + 160 * S).await;
    assert_eq!(said, "Okay.");
    assert_eq!(done.route, octoplus_proto::v1::Route::NudgeReply as i32);
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Snoozed), 1);
    let st = s.nudge.list();
    let disk = st.iter().find(|r| r.rule_id == "disk_nearly_full").unwrap();
    assert!(disk.snoozed_until_ns >= NOON + 160 * S + 4 * 3600 * S);

    let s = with_open_window().await;
    let (said, _) = s.ask("stop telling me about this", NOON + 160 * S).await;
    assert!(said.contains("won't mention that again"), "{said}");
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Disabled), 1);
    assert!(
        !s.nudge
            .list()
            .iter()
            .find(|r| r.rule_id == "disk_nearly_full")
            .unwrap()
            .enabled
    );

    let s = with_open_window().await;
    s.ask("no", NOON + 160 * S).await;
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Dismissed), 1);
    assert!(s.rec().dispatches.is_empty());
}

#[tokio::test]
async fn a_new_question_or_a_late_reply_means_ignored_never_an_action() {
    let s = with_open_window().await;
    s.ask("what is the capital of France", NOON + 160 * S).await;
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Ignored), 1);
    assert!(s.rec().dispatches.is_empty());
    // A "yes" now is an ordinary turn, not a reply to anything.
    s.ask("yes", NOON + 170 * S).await;
    assert!(s.rec().dispatches.is_empty());

    // An unanswered nudge becomes ignored when its window runs out, and a late "yes" does nothing.
    let s = with_open_window().await;
    s.set_disk(NOON + 400 * S, 92);
    s.tick(NOON + 400 * S).await;
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Ignored), 1);
    s.ask("show me", NOON + 410 * S).await;
    assert!(s.rec().dispatches.is_empty());
}

// ---- the command line -----------------------------------------------------------------------------------------

#[tokio::test]
async fn nudges_list_enable_disable_and_test() {
    let s = Stack::start(Opts::default()).await;
    let Body::NudgeList(l) = s
        .admin(Body::NudgeListRequest(NudgeListRequest {}))
        .await
        .unwrap()
    else {
        panic!("not a list");
    };
    assert_eq!(l.rules.len(), 8);
    assert!(l.rules.iter().all(|r| r.enabled));

    let ack = s
        .admin(Body::NudgeSetEnabled(NudgeSetEnabled {
            rule_id: "running_hot".into(),
            enabled: false,
        }))
        .await
        .unwrap();
    assert!(matches!(ack, Body::Ack(_)));
    assert_eq!(s.logged("running_hot", NudgeOutcome::Disabled), 1);
    let Body::NudgeList(l) = s
        .admin(Body::NudgeListRequest(NudgeListRequest {}))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(
        !l.rules
            .iter()
            .find(|r| r.rule_id == "running_hot")
            .unwrap()
            .enabled
    );
    s.admin(Body::NudgeSetEnabled(NudgeSetEnabled {
        rule_id: "running_hot".into(),
        enabled: true,
    }))
    .await
    .unwrap();
    assert_eq!(s.logged("running_hot", NudgeOutcome::Enabled), 1);

    let err = s
        .admin(Body::NudgeSetEnabled(NudgeSetEnabled {
            rule_id: "nope".into(),
            enabled: true,
        }))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        octoplus_ipc::IpcError::Remote {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    s.admin(Body::NudgeTest(NudgeTest {
        rule_id: "disk_critical".into(),
        speak: false,
    }))
    .await
    .unwrap();
    {
        let rec = s.rec();
        let (t, a, silent) = rec.notifies.last().unwrap();
        assert_eq!(
            (t.as_str(), a.as_slice(), *silent),
            ("disk.critical", &[("free_gib".to_string(), 5.0)][..], true)
        );
    }
    assert!(
        s.admin(Body::NudgeTest(NudgeTest {
            rule_id: "x".into(),
            speak: false,
        }))
        .await
        .is_err()
    );
    // A test line is not a real nudge: nothing is logged for it and no reply window opens.
    assert_eq!(s.logged("disk_critical", NudgeOutcome::Notified), 0);
    let _ = &s.stored;
}

#[tokio::test]
async fn a_part_that_is_down_is_reported_after_its_hold_time() {
    let s = Stack::start(Opts {
        c4_up: false,
        ..Opts::default()
    })
    .await;
    for t in (0..=150).step_by(30) {
        let now = NOON + t * S;
        s.set_disk(now, 50);
        s.tick(now).await;
    }
    let rec = s.rec();
    assert_eq!(rec.notifies.len(), 1, "{:?}", rec.notifies);
    assert_eq!(rec.notifies[0].0, "part.degraded");
}

#[tokio::test]
async fn switching_a_rule_on_clears_an_earlier_snooze() {
    let s = with_open_window().await;
    s.ask("not now", NOON + 160 * S).await;
    let snoozed = |s: &Stack| {
        s.nudge
            .list()
            .iter()
            .find(|r| r.rule_id == "disk_nearly_full")
            .unwrap()
            .snoozed_until_ns
    };
    assert!(snoozed(&s) > 0);
    s.admin(Body::NudgeSetEnabled(NudgeSetEnabled {
        rule_id: "disk_nearly_full".into(),
        enabled: true,
    }))
    .await
    .unwrap();
    assert_eq!(snoozed(&s), 0, "an explicit enable ends the snooze");
}

#[test]
fn the_log_fold_ends_a_snooze_at_a_later_enable_and_keeps_a_later_snooze() {
    use octoplus_knowledge::nudge::engine::fold_log;
    let e = |outcome, at| entry("r", outcome, at);
    let hour = 3600 * S;
    let enabled_after = fold_log(
        &[
            e(NudgeOutcome::Snoozed, 10 * S),
            e(NudgeOutcome::Enabled, 20 * S),
        ],
        4 * hour,
    );
    assert_eq!(enabled_after, vec![("r".to_string(), 0, 0, Some(true))]);
    let snoozed_after = fold_log(
        &[
            e(NudgeOutcome::Enabled, 10 * S),
            e(NudgeOutcome::Snoozed, 20 * S),
        ],
        4 * hour,
    );
    assert_eq!(
        snoozed_after,
        vec![("r".to_string(), 0, 20 * S + 4 * hour, Some(true))]
    );
    // Only Spoken and Notified start a cooldown.
    let fired = fold_log(
        &[
            e(NudgeOutcome::Held, 5 * S),
            e(NudgeOutcome::Dropped, 6 * S),
            e(NudgeOutcome::Notified, 7 * S),
            e(NudgeOutcome::Accepted, 8 * S),
        ],
        hour,
    );
    assert_eq!(fired, vec![("r".to_string(), 7 * S, 0, None)]);
}

// ---- spoken nudges (Phase 7, FR-NUD-05, FR-VOI-10/11) -----------------------------------------------

#[tokio::test]
async fn a_good_moment_with_voice_ready_speaks_the_nudge_and_shows_nothing() {
    let s = Stack::start(Opts {
        voice_up: true,
        ..Opts::default()
    })
    .await;
    s.run_disk_voice(NOON, 150, 92).await;
    let rec = s.rec();
    assert_eq!(rec.spoken.len(), 1, "{:?}", rec.spoken);
    assert_eq!(rec.spoken[0].0, "disk.nearly_full");
    assert_eq!(rec.spoken[0].1, vec![("percent".to_string(), 92.0)]);
    assert!(
        rec.notifies.is_empty(),
        "spoken instead of a popup: {:?}",
        rec.notifies
    );
    assert!(rec.dispatches.is_empty(), "a nudge never acts (R0-6)");
    drop(rec);
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Spoken), 1);
}

#[tokio::test]
async fn a_spoken_nudge_can_be_answered_by_a_reply() {
    let s = Stack::start(Opts {
        voice_up: true,
        ..Opts::default()
    })
    .await;
    s.run_disk_voice(NOON, 150, 92).await;
    let (said, _) = s.ask("show me", NOON + 160 * S).await;
    assert!(said.starts_with("Okay."), "{said}");
    assert_eq!(s.rec().dispatches[0].0, "system.disk_report");
}

#[tokio::test]
async fn another_app_on_the_microphone_or_a_turn_in_progress_means_no_speech() {
    for status in [
        VoiceStatus {
            capture_running: true,
            mic_in_use_by_other: true,
            ..VoiceStatus::default()
        },
        VoiceStatus {
            capture_running: true,
            turn_in_progress: true,
            ..VoiceStatus::default()
        },
        VoiceStatus {
            capture_running: true,
            speaking: true,
            ..VoiceStatus::default()
        },
    ] {
        let s = Stack::start(Opts {
            voice_up: true,
            ..Opts::default()
        })
        .await;
        *s.voice_status.lock().unwrap() = status;
        s.run_disk_voice(NOON, 300, 92).await;
        let rec = s.rec();
        assert!(rec.spoken.is_empty(), "{status:?}: {:?}", rec.spoken);
        assert!(rec.notifies.is_empty(), "{status:?}: held, not shown");
    }
}

#[tokio::test]
async fn voice_that_is_not_capturing_means_a_notification_not_speech() {
    let s = Stack::start(Opts {
        voice_up: true,
        ..Opts::default()
    })
    .await;
    *s.voice_status.lock().unwrap() = VoiceStatus::default();
    s.run_disk_voice(NOON, 150, 92).await;
    assert!(s.rec().spoken.is_empty());
    assert_eq!(s.rec().notifies.len(), 1);
}

#[tokio::test]
async fn a_speaker_that_fails_falls_back_to_the_notification() {
    let s = Stack::start(Opts {
        voice_up: true,
        speak_fails: true,
        ..Opts::default()
    })
    .await;
    s.run_disk_voice(NOON, 150, 92).await;
    assert!(s.rec().spoken.is_empty());
    assert_eq!(s.rec().notifies.len(), 1, "the user still hears about it");
    assert_eq!(s.logged("disk_nearly_full", NudgeOutcome::Notified), 1);
}

#[tokio::test]
async fn with_no_voice_part_at_all_the_notification_path_is_unchanged() {
    let s = Stack::start(Opts::default()).await;
    s.run_disk_voice(NOON, 150, 92).await;
    assert!(s.rec().spoken.is_empty());
    assert_eq!(s.rec().notifies.len(), 1);
}

#[tokio::test]
async fn a_spoken_test_line_goes_to_the_voice_and_fails_when_it_cannot() {
    let s = Stack::start(Opts {
        voice_up: true,
        ..Opts::default()
    })
    .await;
    s.admin(Body::NudgeTest(NudgeTest {
        rule_id: "disk_critical".into(),
        speak: true,
    }))
    .await
    .unwrap();
    {
        let rec = s.rec();
        assert_eq!(rec.spoken.len(), 1);
        assert_eq!(rec.spoken[0].0, "disk.critical");
        assert!(rec.notifies.is_empty(), "said aloud, not shown");
    }
    let none = Stack::start(Opts::default()).await;
    assert!(
        none.admin(Body::NudgeTest(NudgeTest {
            rule_id: "disk_critical".into(),
            speak: true,
        }))
        .await
        .is_err(),
        "no voice part: an error, never a silent success"
    );
}
