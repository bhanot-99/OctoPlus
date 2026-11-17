//! Nudge replay scenario engine (FR-NUD-01..12, KPI-10, phases.md 9.1).
//!
//! Feeds declarative scenario scripts through the pure [`Core`] evaluator and policy gate
//! over virtual time, collecting events and validating expectations.
//! Pure code: no I/O, no clock (time is virtual).

use serde::{Deserialize, Serialize};

use super::core::Core;
use super::eval::EvalConfig;
use super::rules;
use super::types::{Action, Delivery, PolicyConfig, Reading, Reason, Signal, UserContext};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("scenario is not valid JSON")]
    Parse,
    #[error("bad signal name `{0}`")]
    Signal(String),
    #[error("rules: {0}")]
    Rules(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedNotify {
    pub rule: String,
    pub at_s: u64,
    #[serde(default)]
    pub silent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectJson {
    #[serde(default)]
    pub notified: Vec<ExpectedNotify>,
    #[serde(default)]
    pub spoken: u32,
    #[serde(default)]
    pub held_at_end: Option<usize>,
    #[serde(default)]
    pub dropped: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryJson {
    Voice,
    Notify,
    Off,
}

impl From<DeliveryJson> for Delivery {
    fn from(d: DeliveryJson) -> Self {
        match d {
            DeliveryJson::Voice => Delivery::Voice,
            DeliveryJson::Notify => Delivery::Notify,
            DeliveryJson::Off => Delivery::Off,
        }
    }
}

fn default_max_spoken_per_hour() -> u32 {
    3
}
fn default_max_spoken_per_day() -> u32 {
    8
}
fn default_max_held() -> usize {
    16
}
fn default_deliver_info() -> DeliveryJson {
    DeliveryJson::Notify
}
fn default_deliver_important() -> DeliveryJson {
    DeliveryJson::Voice
}
fn default_deliver_urgent() -> DeliveryJson {
    DeliveryJson::Voice
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyJson {
    #[serde(default)]
    pub quiet_mode: bool,
    #[serde(default = "default_max_spoken_per_hour")]
    pub max_spoken_per_hour: u32,
    #[serde(default = "default_max_spoken_per_day")]
    pub max_spoken_per_day: u32,
    #[serde(default = "default_max_held")]
    pub max_held: usize,
    #[serde(default = "default_deliver_info")]
    pub deliver_info: DeliveryJson,
    #[serde(default = "default_deliver_important")]
    pub deliver_important: DeliveryJson,
    #[serde(default = "default_deliver_urgent")]
    pub deliver_urgent: DeliveryJson,
}

impl Default for PolicyJson {
    fn default() -> Self {
        Self {
            quiet_mode: false,
            max_spoken_per_hour: default_max_spoken_per_hour(),
            max_spoken_per_day: default_max_spoken_per_day(),
            max_held: default_max_held(),
            deliver_info: default_deliver_info(),
            deliver_important: default_deliver_important(),
            deliver_urgent: default_deliver_urgent(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadingJson {
    pub signal: String,
    #[serde(default)]
    pub key: String,
    pub value: f64,
    #[serde(default)]
    pub extra: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextJson {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub quiet_hours: bool,
    #[serde(default)]
    pub in_turn: bool,
    #[serde(default)]
    pub mic_busy: bool,
    #[serde(default)]
    pub media_playing: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub away: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StepJson {
    pub at_s: u64,
    #[serde(default)]
    pub every_s: Option<u64>,
    #[serde(default)]
    pub until_s: Option<u64>,
    #[serde(default)]
    pub readings: Vec<ReadingJson>,
    #[serde(default)]
    pub context: ContextJson,
}

fn default_max_gap_s() -> u64 {
    300
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub voice_available: bool,
    #[serde(default)]
    pub rules_toml: Option<String>,
    #[serde(default)]
    pub policy: PolicyJson,
    #[serde(default = "default_max_gap_s")]
    pub max_gap_s: u64,
    pub steps: Vec<StepJson>,
    pub expect: ExpectJson,
}

fn parse_signal_name(sig: &str) -> Result<Signal, ReplayError> {
    match sig {
        "disk.used_percent" => Ok(Signal::DiskUsedPercent),
        "disk.fill_hours" => Ok(Signal::DiskFillHours),
        "battery.percent" => Ok(Signal::BatteryPercent),
        "temperature.c" => Ok(Signal::TemperatureC),
        "memory.available_percent" => Ok(Signal::MemoryAvailablePercent),
        "session.active_minutes" => Ok(Signal::ActiveMinutes),
        "parts.degraded" => Ok(Signal::PartsDegraded),
        _ => Err(ReplayError::Signal(sig.to_string())),
    }
}

pub fn parse(json: &str) -> Result<Scenario, ReplayError> {
    let s: Scenario = serde_json::from_str(json).map_err(|_| ReplayError::Parse)?;
    for step in &s.steps {
        for reading in &step.readings {
            parse_signal_name(&reading.signal)?;
        }
    }
    Ok(s)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub at_s: u64,
    pub rule: String,
    #[serde(serialize_with = "serialize_action")]
    pub action: Action,
    #[serde(serialize_with = "serialize_reason")]
    pub reason: Reason,
}

fn serialize_action<S>(action: &Action, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match action {
        Action::Speak => {
            #[derive(Serialize)]
            struct SpeakAct {
                #[serde(rename = "type")]
                act_type: &'static str,
            }
            SpeakAct { act_type: "speak" }.serialize(s)
        }
        Action::Notify { silent } => {
            #[derive(Serialize)]
            struct NotifyAct {
                #[serde(rename = "type")]
                act_type: &'static str,
                silent: bool,
            }
            NotifyAct {
                act_type: "notify",
                silent: *silent,
            }
            .serialize(s)
        }
        Action::Hold => {
            #[derive(Serialize)]
            struct HoldAct {
                #[serde(rename = "type")]
                act_type: &'static str,
            }
            HoldAct { act_type: "hold" }.serialize(s)
        }
        Action::Drop => {
            #[derive(Serialize)]
            struct DropAct {
                #[serde(rename = "type")]
                act_type: &'static str,
            }
            DropAct { act_type: "drop" }.serialize(s)
        }
    }
}

fn serialize_reason<S>(reason: &Reason, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_str(&format!("{reason:?}"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub events: Vec<Event>,
    pub held_at_end: usize,
}

/// Expands repeating steps (`every_s`/`until_s`) and stably sorts all steps by `at_s`.
pub fn expand_and_sort_steps(steps: &[StepJson]) -> Vec<(u64, StepJson)> {
    let mut expanded: Vec<(u64, StepJson)> = Vec::new();
    for step in steps {
        if let (Some(every), Some(until)) = (step.every_s, step.until_s) {
            if every > 0 && step.at_s <= until {
                let mut t = step.at_s;
                while t <= until {
                    let mut single_step = step.clone();
                    single_step.at_s = t;
                    single_step.every_s = None;
                    single_step.until_s = None;
                    expanded.push((t, single_step));
                    match t.checked_add(every) {
                        Some(next) => t = next,
                        None => break,
                    }
                }
            } else {
                expanded.push((step.at_s, step.clone()));
            }
        } else {
            expanded.push((step.at_s, step.clone()));
        }
    }
    expanded.sort_by_key(|&(at_s, _)| at_s);
    expanded
}

pub fn run(s: &Scenario) -> Result<Report, ReplayError> {
    let rules =
        rules::load(s.rules_toml.as_deref()).map_err(|e| ReplayError::Rules(e.to_string()))?;
    let eval_cfg = EvalConfig {
        max_gap_s: s.max_gap_s,
    };
    let policy_cfg = PolicyConfig {
        quiet_mode: s.policy.quiet_mode,
        deliver_info: s.policy.deliver_info.into(),
        deliver_important: s.policy.deliver_important.into(),
        deliver_urgent: s.policy.deliver_urgent.into(),
        max_spoken_per_hour: s.policy.max_spoken_per_hour,
        max_spoken_per_day: s.policy.max_spoken_per_day,
        max_held: s.policy.max_held,
    };
    let mut core = Core::new(rules, eval_cfg, policy_cfg);

    let expanded = expand_and_sort_steps(&s.steps);
    const BASE_S: u64 = 1_000_000;
    let mut events = Vec::new();

    for (at_s, step) in expanded {
        let now_s = BASE_S.saturating_add(at_s);
        let now_ns = now_s.saturating_mul(1_000_000_000);

        let readings = step
            .readings
            .iter()
            .map(|r| {
                let signal = parse_signal_name(&r.signal)?;
                Ok(Reading {
                    signal,
                    key: r.key.clone(),
                    value: r.value,
                    extra: r.extra,
                })
            })
            .collect::<Result<Vec<_>, ReplayError>>()?;

        let ctx = UserContext {
            paused: step.context.paused,
            quiet_hours: step.context.quiet_hours,
            in_turn: step.context.in_turn,
            mic_busy: step.context.mic_busy,
            media_playing: step.context.media_playing,
            fullscreen: step.context.fullscreen,
            away: step.context.away,
            voice_available: s.voice_available,
        };

        let step_out = core.step(now_ns, &readings, &ctx);
        for d in step_out.decisions {
            events.push(Event {
                at_s,
                rule: d.candidate.rule_id,
                action: d.action,
                reason: d.reason,
            });
        }
        for d in step_out.dropped {
            events.push(Event {
                at_s,
                rule: d.candidate.rule_id,
                action: d.action,
                reason: d.reason,
            });
        }
    }

    Ok(Report {
        events,
        held_at_end: core.policy.held().len(),
    })
}

pub fn check(s: &Scenario, r: &Report) -> Vec<String> {
    let mut mismatches = Vec::new();

    // 1. Notified: EXACT ordered list
    let actual_notified: Vec<ExpectedNotify> = r
        .events
        .iter()
        .filter_map(|e| match e.action {
            Action::Notify { silent } => Some(ExpectedNotify {
                rule: e.rule.clone(),
                at_s: e.at_s,
                silent,
            }),
            _ => None,
        })
        .collect();

    if actual_notified.len() != s.expect.notified.len() {
        mismatches.push(format!(
            "notified count mismatch: expected {}, got {}",
            s.expect.notified.len(),
            actual_notified.len()
        ));
    }
    let min_len = actual_notified.len().min(s.expect.notified.len());
    for i in 0..min_len {
        let exp = &s.expect.notified[i];
        let act = &actual_notified[i];
        if exp != act {
            mismatches.push(format!(
                "notified[{i}] mismatch: expected rule `{}` at_s {} silent {}, got rule `{}` at_s {} silent {}",
                exp.rule, exp.at_s, exp.silent, act.rule, act.at_s, act.silent
            ));
        }
    }

    // 2. Spoken count
    let actual_spoken = r
        .events
        .iter()
        .filter(|e| e.action == Action::Speak)
        .count() as u32;
    if actual_spoken != s.expect.spoken {
        mismatches.push(format!(
            "spoken count mismatch: expected {}, got {}",
            s.expect.spoken, actual_spoken
        ));
    }

    // 3. Held at end
    if let Some(expected_held) = s.expect.held_at_end
        && r.held_at_end != expected_held
    {
        mismatches.push(format!(
            "held_at_end mismatch: expected {}, got {}",
            expected_held, r.held_at_end
        ));
    }

    // 4. Dropped count
    if let Some(expected_dropped) = s.expect.dropped {
        let actual_dropped = r.events.iter().filter(|e| e.action == Action::Drop).count();
        if actual_dropped != expected_dropped {
            mismatches.push(format!(
                "dropped count mismatch: expected {}, got {}",
                expected_dropped, actual_dropped
            ));
        }
    }

    mismatches
}
