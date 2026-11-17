//! The nudge engine task (FR-NUD-01, 05, 09, 11, 12; Architecture §6.3): every tick it reads
//! C3's health signals, runs the pure evaluator and the pure policy gate, asks C6 to show the
//! notification from a template id and numbers, and logs what happened in C3.
//!
//! What this file must never do (R0-6, FR-NUD-04, FR-NUD-10): act. It has no import of the
//! action route, `DispatchRequest` or evidence types, and it never builds text: the only things
//! it sends C6 are a template id and numbers. An offered skill is kept as a *name*; the turn
//! engine dispatches it only when the user's reply says yes. `tests/nudge_isolation_test.rs`
//! checks the sources.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_common::config::NudgesConfig;
use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    NotifyRequest, NudgeOutcome, NudgeRuleState, QueryHealthSignals, QueryNudgeLog, StoreNudgeLog,
    TemplateArg,
};
use tracing::{debug, warn};

use super::core::Core as PureCore;
use super::eval::EvalConfig;
use super::reply::Reply;
use super::signals::{self, ContextInputs};
use super::types::{Action, ArgSource, Decision, PolicyConfig, Rule, RuleStatus};

const NS_PER_S: u64 = 1_000_000_000;
/// How far back each tick asks C3 for samples (C1 samples at most every minute).
const SAMPLE_LOOKBACK_S: u64 = 120;
/// How far back the start-up fold reads the nudge log (retention is 90 days, PRD §6.5).
const RESTORE_LOOKBACK_S: u64 = 90 * 86_400;
/// Longest wait for C3, in ticks that matter to the user.
const C3_DEADLINE: Duration = Duration::from_millis(500);
/// Longest wait for C6 to show a notification.
const NOTIFY_DEADLINE: Duration = Duration::from_secs(2);

/// The connections the engine uses.
#[derive(Debug, Clone)]
pub struct NudgePeers {
    /// C3 (signals, log).
    pub storage: Arc<ReconnectingClient>,
    /// C6 (notifications).
    pub kernel: Arc<ReconnectingClient>,
    /// C4, only to know whether it is reachable ("a part is degraded").
    pub inference: Arc<ReconnectingClient>,
}

/// The reply window opened by a delivered nudge.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    rule_id: String,
    offer_skill: Option<String>,
    opened_ns: u64,
}

struct Core {
    pure: PureCore,
    window: Option<Window>,
    restored: bool,
    quiet_start: u32,
    quiet_end: u32,
}

/// One row to write to C3's nudge log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    /// The rule.
    pub rule_id: String,
    /// What happened.
    pub outcome: NudgeOutcome,
    /// When.
    pub at_ns: u64,
}

/// What a reply did, for the turn engine to speak and, for a yes, to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// A fixed sentence to say.
    pub say: &'static str,
    /// For "yes": the skill the nudge offered, to be dispatched under the normal tiers.
    pub offered_skill: Option<String>,
    /// Rows to write to the log.
    pub log: Vec<LogRow>,
}

/// Shared by the engine task, the turn engine and the CLI handler.
#[derive(Clone)]
pub struct NudgeHandle {
    core: Arc<Mutex<Core>>,
    cfg: NudgesConfig,
    peers: NudgePeers,
    in_turn: Arc<AtomicU32>,
    own_pause_until_ns: Arc<AtomicU64>,
    idle: Arc<AtomicBool>,
    counters: Counters,
}

/// Counters for the health report (shared with the health registry by `init`).
#[derive(Debug, Default, Clone)]
pub struct Counters {
    /// Nudges shown.
    pub notified: Arc<AtomicU64>,
    /// Nudges held for a bad moment.
    pub held: Arc<AtomicU64>,
    /// Nudges thrown away (delivery off, replaced, overflow).
    pub dropped: Arc<AtomicU64>,
    /// Nudges that could not be shown (C6 down).
    pub failed: Arc<AtomicU64>,
}

impl std::fmt::Debug for NudgeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NudgeHandle").finish_non_exhaustive()
    }
}

/// Holds the in-turn count up while a turn is being answered.
#[derive(Debug)]
pub struct TurnGuard(Arc<AtomicU32>);

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lock(core: &Mutex<Core>) -> std::sync::MutexGuard<'_, Core> {
    core.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn policy_config(cfg: &NudgesConfig) -> PolicyConfig {
    PolicyConfig {
        quiet_mode: cfg.quiet_mode,
        deliver_info: cfg.deliver_info,
        deliver_important: cfg.deliver_important,
        deliver_urgent: cfg.deliver_urgent,
        max_spoken_per_hour: cfg.max_spoken_per_hour,
        max_spoken_per_day: cfg.max_spoken_per_day,
        max_held: cfg.max_held,
    }
}

/// Wire form of a candidate's numbers.
fn template_args(args: &[(String, f64)]) -> Vec<TemplateArg> {
    args.iter()
        .map(|(name, value)| TemplateArg {
            name: name.clone(),
            value: *value,
        })
        .collect()
}

/// Folds the stored log into per-rule state (oldest first).
/// `(rule, last_fired_ns, snoozed_until_ns, enabled)`.
#[must_use]
pub fn fold_log(
    entries: &[neuroos_proto::v1::NudgeLogEntry],
    snooze_ns: u64,
) -> Vec<(String, u64, u64, Option<bool>)> {
    let mut out: Vec<(String, u64, u64, Option<bool>)> = Vec::new();
    for e in entries {
        let slot = match out.iter().position(|(r, ..)| *r == e.rule_id) {
            Some(i) => i,
            None => {
                out.push((e.rule_id.clone(), 0, 0, None));
                out.len() - 1
            }
        };
        let row = &mut out[slot];
        match NudgeOutcome::try_from(e.outcome) {
            Ok(NudgeOutcome::Spoken | NudgeOutcome::Notified) => row.1 = row.1.max(e.at_ns),
            Ok(NudgeOutcome::Snoozed) => row.2 = e.at_ns.saturating_add(snooze_ns),
            Ok(NudgeOutcome::Disabled) => row.3 = Some(false),
            Ok(NudgeOutcome::Enabled) => row.3 = Some(true),
            _ => {}
        }
    }
    out
}

impl NudgeHandle {
    /// Builds the engine's shared core from the rules and config.
    #[must_use]
    pub fn new(rules: Vec<Rule>, cfg: NudgesConfig, peers: NudgePeers) -> Self {
        let core = Core {
            pure: PureCore::new(
                rules,
                EvalConfig {
                    max_gap_s: cfg.max_gap_s,
                },
                policy_config(&cfg),
            ),
            window: None,
            restored: false,
            quiet_start: cfg.quiet_start_min,
            quiet_end: cfg.quiet_end_min,
        };
        Self {
            core: Arc::new(Mutex::new(core)),
            cfg,
            peers,
            in_turn: Arc::new(AtomicU32::new(0)),
            own_pause_until_ns: Arc::new(AtomicU64::new(0)),
            idle: Arc::new(AtomicBool::new(false)),
            counters: Counters::default(),
        }
    }

    /// Uses the health registry's counters instead of private ones.
    #[must_use]
    pub fn with_counters(mut self, counters: Counters) -> Self {
        self.counters = counters;
        self
    }

    /// True while the engine cannot read C3 and so says nothing (reported DEGRADED).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.idle.load(Ordering::Relaxed)
    }

    /// Marks a turn as in progress until the guard is dropped (FR-NUD-05).
    #[must_use]
    pub fn turn_started(&self) -> TurnGuard {
        self.in_turn.fetch_add(1, Ordering::SeqCst);
        TurnGuard(Arc::clone(&self.in_turn))
    }

    /// Remembers a pause C5 itself dispatched (C1's own pause cannot be asked, plan decision 5).
    pub fn note_pause(&self, now_ns: u64, minutes: u64) {
        let until = now_ns.saturating_add(minutes.saturating_mul(60 * NS_PER_S));
        self.own_pause_until_ns.fetch_max(until, Ordering::SeqCst);
    }

    /// Is a nudge's reply window open?
    #[must_use]
    pub fn reply_window_open(&self, now_ns: u64) -> bool {
        lock(&self.core).window.as_ref().is_some_and(|w| {
            now_ns.saturating_sub(w.opened_ns) <= self.cfg.reply_window_s * NS_PER_S
        })
    }

    /// Applies the user's reply to the open window, closing it. `None` when no window is open.
    pub fn apply_reply(&self, now_ns: u64, reply: Reply) -> Option<Applied> {
        let mut core = lock(&self.core);
        let w = core.window.take()?;
        if now_ns.saturating_sub(w.opened_ns) > self.cfg.reply_window_s * NS_PER_S {
            return None;
        }
        let row = |outcome| LogRow {
            rule_id: w.rule_id.clone(),
            outcome,
            at_ns: now_ns,
        };
        Some(match reply {
            Reply::Yes => Applied {
                say: "Okay.",
                offered_skill: w.offer_skill.clone(),
                log: vec![row(NudgeOutcome::Accepted)],
            },
            Reply::No => Applied {
                say: "Okay.",
                offered_skill: None,
                log: vec![row(NudgeOutcome::Dismissed)],
            },
            Reply::NotNow => {
                let until = now_ns.saturating_add(self.cfg.snooze_h * 3600 * NS_PER_S);
                core.pure.evaluator.snooze(&w.rule_id, until);
                Applied {
                    say: "Okay.",
                    offered_skill: None,
                    log: vec![row(NudgeOutcome::Snoozed)],
                }
            }
            Reply::Stop => {
                core.pure.evaluator.set_enabled(&w.rule_id, false);
                Applied {
                    say: "Okay, I won't mention that again. You can turn it back on any time.",
                    offered_skill: None,
                    log: vec![row(NudgeOutcome::Disabled)],
                }
            }
        })
    }

    /// The user moved on without answering: the open window closes as "ignored" (KPI-9).
    pub fn close_window_ignored(&self, now_ns: u64) -> Option<LogRow> {
        let w = lock(&self.core).window.take()?;
        Some(LogRow {
            rule_id: w.rule_id,
            outcome: NudgeOutcome::Ignored,
            at_ns: now_ns,
        })
    }

    /// `neuroosctl nudges list`.
    #[must_use]
    pub fn list(&self) -> Vec<NudgeRuleState> {
        lock(&self.core)
            .pure
            .evaluator
            .status()
            .into_iter()
            .map(
                |RuleStatus {
                     rule_id,
                     enabled,
                     snoozed_until_ns,
                     last_fired_ns,
                 }| NudgeRuleState {
                    rule_id,
                    enabled,
                    snoozed_until_ns,
                    last_fired_ns,
                },
            )
            .collect()
    }

    /// `neuroosctl nudges enable|disable`. `None` for an unknown rule.
    pub fn set_enabled(&self, rule_id: &str, on: bool, now_ns: u64) -> Option<LogRow> {
        if !lock(&self.core).pure.evaluator.set_enabled(rule_id, on) {
            return None;
        }
        Some(LogRow {
            rule_id: rule_id.to_string(),
            outcome: if on {
                NudgeOutcome::Enabled
            } else {
                NudgeOutcome::Disabled
            },
            at_ns: now_ns,
        })
    }

    /// The template and sample numbers `neuroosctl nudges test` shows for a rule: the real
    /// template, the rule's own threshold as the number (and 5 for a second number).
    #[must_use]
    pub fn test_line(&self, rule_id: &str) -> Option<(String, Vec<(String, f64)>)> {
        let core = lock(&self.core);
        let r = core
            .pure
            .evaluator
            .rules()
            .iter()
            .find(|r| r.id == rule_id)?;
        let args = r
            .args
            .iter()
            .map(|(name, src)| {
                (
                    name.clone(),
                    match src {
                        ArgSource::Value => r.fire_at,
                        ArgSource::Extra => 5.0,
                    },
                )
            })
            .collect();
        Some((r.template.clone(), args))
    }

    // ---- talking to C3 and C6 -------------------------------------------------------------

    /// Writes rows to the nudge log (best effort: a failure never stops a nudge).
    pub async fn record(&self, rows: Vec<LogRow>) {
        for r in rows {
            let body = Body::StoreNudgeLog(StoreNudgeLog {
                rule_id: r.rule_id,
                outcome: r.outcome as i32,
                at_ns: r.at_ns,
            });
            if let Err(e) = self.peers.storage.call(body, C3_DEADLINE).await {
                warn!("could not write the nudge log: {e}");
            }
        }
    }

    /// Shows a notification from a template id and numbers (FR-NUD-12); false if C6 is down.
    pub async fn notify(&self, template: &str, args: &[(String, f64)], silent: bool) -> bool {
        let body = Body::NotifyRequest(NotifyRequest {
            template_id: template.to_string(),
            args: template_args(args),
            silent,
        });
        match self.peers.kernel.call(body, NOTIFY_DEADLINE).await {
            Ok(_) => true,
            Err(e) => {
                warn!("could not show a nudge: {e}");
                false
            }
        }
    }

    /// Rebuilds cooldowns, snoozes and switches from the stored log. Returns false while C3 is
    /// unreachable (the engine stays idle and tries again next tick).
    async fn restore(&self, now_ns: u64) -> bool {
        if lock(&self.core).restored {
            return true;
        }
        let since = now_ns.saturating_sub(RESTORE_LOOKBACK_S * NS_PER_S);
        let Ok(Body::NudgeLogEntries(log)) = self
            .peers
            .storage
            .call(
                Body::QueryNudgeLog(QueryNudgeLog { since_ns: since }),
                C3_DEADLINE,
            )
            .await
        else {
            return false;
        };
        let snooze_ns = self.cfg.snooze_h * 3600 * NS_PER_S;
        let mut core = lock(&self.core);
        for (rule, fired, snoozed, enabled) in fold_log(&log.entries, snooze_ns) {
            core.pure.evaluator.restore(&rule, fired, snoozed, enabled);
        }
        core.restored = true;
        true
    }

    /// The engine loop: one [`NudgeHandle::tick`] every `tick_s` until `stop` fires.
    pub async fn run(&self, tz: jiff::tz::TimeZone, mut stop: tokio::sync::watch::Receiver<bool>) {
        let mut every = tokio::time::interval(Duration::from_secs(self.cfg.tick_s));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                res = stop.changed() => {
                    if res.is_err() || *stop.borrow() {
                        break;
                    }
                }
                _ = every.tick() => {
                    let now = neuroos_common::time::now_ns().unwrap_or(0);
                    // Voice arrives in Phase 7; until then a spoken decision is impossible.
                    self.tick(now, &tz, false).await;
                }
            }
        }
    }

    fn peers_degraded(&self) -> u32 {
        u32::from(!self.peers.kernel.is_connected())
            + u32::from(!self.peers.inference.is_connected())
    }

    /// One evaluation (FR-NUD-01). Exposed so tests can drive it with a chosen clock.
    pub async fn tick(&self, now_ns: u64, tz: &jiff::tz::TimeZone, voice_available: bool) {
        if !self.restore(now_ns).await {
            self.idle.store(true, Ordering::Relaxed);
            return;
        }
        let since = now_ns.saturating_sub(SAMPLE_LOOKBACK_S * NS_PER_S);
        let got = self
            .peers
            .storage
            .call(
                Body::QueryHealthSignals(QueryHealthSignals { since_ns: since }),
                C3_DEADLINE,
            )
            .await;
        let Ok(Body::HealthSignals(sig)) = got else {
            // C3 unreachable: say nothing and report DEGRADED (never guess).
            self.idle.store(true, Ordering::Relaxed);
            return;
        };
        self.idle.store(false, Ordering::Relaxed);

        let built =
            signals::build_readings(&sig, now_ns, self.cfg.stale_after_s, self.peers_degraded());
        let own_pause = now_ns < self.own_pause_until_ns.load(Ordering::SeqCst);
        let (decisions, dropped, expired) = {
            let mut core = lock(&self.core);
            let minute = signals::local_minute(now_ns, tz);
            let ctx = signals::build_context(ContextInputs {
                idle: sig.idle,
                fullscreen: sig.fullscreen,
                media_playing: sig.media_playing,
                locked: sig.locked,
                locked_known: sig.locked_known,
                monitor_stale: built.monitor_stale,
                own_pause_active: own_pause,
                in_turn: self.in_turn.load(Ordering::SeqCst) > 0,
                mic_busy: false,
                quiet_hours: signals::in_quiet_hours(minute, core.quiet_start, core.quiet_end),
                voice_available,
            });
            let out = core.pure.step(now_ns, &built.readings, &ctx);
            let (decisions, dropped) = (out.decisions, out.dropped);
            // A reply window nobody used is "ignored" (KPI-9).
            let expired = match &core.window {
                Some(w)
                    if now_ns.saturating_sub(w.opened_ns) > self.cfg.reply_window_s * NS_PER_S =>
                {
                    core.window.take().map(|w| LogRow {
                        rule_id: w.rule_id,
                        outcome: NudgeOutcome::Ignored,
                        at_ns: now_ns,
                    })
                }
                _ => None,
            };
            (decisions, dropped, expired)
        };

        let mut rows: Vec<LogRow> = expired.into_iter().collect();
        for d in dropped {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            rows.push(row_for(&d.candidate.rule_id, NudgeOutcome::Dropped, now_ns));
        }
        for d in decisions {
            rows.extend(self.carry_out(d, now_ns).await);
        }
        self.record(rows).await;
    }

    /// Does what one decision says and returns the rows to log.
    async fn carry_out(&self, d: Decision, now_ns: u64) -> Vec<LogRow> {
        let ids: Vec<String> = if d.merged_from.is_empty() {
            vec![d.candidate.rule_id.clone()]
        } else {
            d.merged_from.iter().map(|(r, _)| r.clone()).collect()
        };
        let all = |o: NudgeOutcome| ids.iter().map(|r| row_for(r, o, now_ns)).collect();
        match d.action {
            Action::Hold => {
                self.counters.held.fetch_add(1, Ordering::Relaxed);
                debug!("nudge {} held ({:?})", d.candidate.rule_id, d.reason);
                all(NudgeOutcome::Held)
            }
            Action::Drop => {
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                all(NudgeOutcome::Dropped)
            }
            Action::Speak | Action::Notify { .. } => {
                // Voice arrives in Phase 7: a spoken decision is only reachable when voice is
                // available, and until then the gate has already downgraded it.
                let silent = matches!(d.action, Action::Notify { silent: true });
                let spoken = matches!(d.action, Action::Speak);
                if self
                    .notify(&d.candidate.template, &d.candidate.args, silent)
                    .await
                {
                    self.counters.notified.fetch_add(1, Ordering::Relaxed);
                    // The reply window opens for what the user can answer (a single nudge).
                    if d.merged_from.is_empty() {
                        lock(&self.core).window = Some(Window {
                            rule_id: d.candidate.rule_id.clone(),
                            offer_skill: d.candidate.offer_skill.clone(),
                            opened_ns: now_ns,
                        });
                    }
                    all(if spoken {
                        NudgeOutcome::Spoken
                    } else {
                        NudgeOutcome::Notified
                    })
                } else {
                    self.counters.failed.fetch_add(1, Ordering::Relaxed);
                    all(NudgeOutcome::Dropped)
                }
            }
        }
    }
}

fn row_for(rule: &str, outcome: NudgeOutcome, at_ns: u64) -> LogRow {
    LogRow {
        rule_id: rule.to_string(),
        outcome,
        at_ns,
    }
}
