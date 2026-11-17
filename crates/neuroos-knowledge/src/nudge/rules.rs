//! Nudge rules loader and compiled-in v1.0 default rules (FR-NUD-02/03, design.md §8.5).

use super::types::{ArgSource, Delivery, Direction, Priority, Rule, Signal};

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_RULES: usize = 64;

/// The C6 notification templates a rule may name, with the argument names each declares.
pub const TEMPLATES: &[(&str, &[&str])] = &[
    ("disk.nearly_full", &["percent"]),
    ("disk.critical", &["free_gib"]),
    ("disk.filling_fast", &["hours"]),
    ("battery.low", &["percent"]),
    ("thermal.hot", &[]),
    ("memory.pressure", &[]),
    ("session.long", &["minutes"]),
    ("part.degraded", &[]),
];

/// Skills a rule may OFFER (the offer is dispatched only from the user's reply, never by the engine).
pub const OFFERABLE_SKILLS: &[&str] = &["system.disk_report", "system.read_metadata"];

pub const DEFAULT_RULES_TOML: &str = r#"[rule.disk_nearly_full]
signal = "disk.used_percent"
direction = "at_least"
fire_at = 90
rearm = 85
hold_for = "2m"
cooldown = "24h"
priority = "important"
template = "disk.nearly_full"
offer_skill = "system.disk_report"

[rule.disk_critical]
signal = "disk.used_percent"
direction = "at_least"
fire_at = 95
rearm = 92
hold_for = "1m"
cooldown = "24h"
priority = "urgent"
template = "disk.critical"
offer_skill = "system.disk_report"

[rule.disk_filling_fast]
signal = "disk.fill_hours"
direction = "at_most"
fire_at = 24
rearm = 36
hold_for = "5m"
cooldown = "24h"
priority = "important"
template = "disk.filling_fast"
offer_skill = "system.disk_report"

[rule.battery_low]
signal = "battery.percent"
direction = "at_most"
fire_at = 15
rearm = 20
hold_for = "0s"
cooldown = "2h"
priority = "important"
template = "battery.low"

[rule.running_hot]
signal = "temperature.c"
direction = "at_least"
fire_at = 90
rearm = 80
hold_for = "2m"
cooldown = "24h"
priority = "important"
template = "thermal.hot"
offer_skill = "system.read_metadata"

[rule.memory_pressure]
signal = "memory.available_percent"
direction = "at_most"
fire_at = 10
rearm = 15
hold_for = "2m"
cooldown = "24h"
priority = "info"
template = "memory.pressure"
offer_skill = "system.read_metadata"

[rule.long_session]
signal = "session.active_minutes"
direction = "at_least"
fire_at = 90
rearm = 5
hold_for = "0s"
cooldown = "3h"
priority = "info"
deliver = "notify"
template = "session.long"

[rule.part_degraded]
signal = "parts.degraded"
direction = "at_least"
fire_at = 1
rearm = 0.5
hold_for = "2m"
cooldown = "24h"
priority = "info"
template = "part.degraded"
"#;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("rules file larger than the limit")]
    TooLarge,
    #[error("rules file is not valid TOML")]
    Parse,
    #[error("unknown key `{key}` in rule `{rule}`")]
    UnknownKey { rule: String, key: String },
    #[error("rule `{rule}` is missing `{field}`")]
    Missing { rule: String, field: &'static str },
    #[error("rule `{rule}`: bad `{field}`: {reason}")]
    Invalid {
        rule: String,
        field: &'static str,
        reason: &'static str,
    },
    #[error("more than {MAX_RULES} rules")]
    TooManyRules,
}

/// Parses duration strings like "90s", "2m", "24h", "7d".
///
/// Digits + unit only, no spaces, no sign, no decimals. Saturating arithmetic, never panics.
pub fn parse_duration(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let last = chars.next_back()?;
    let mult: u64 = match last {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return None,
    };
    let digits_part = &text[..text.len() - last.len_utf8()];
    if digits_part.is_empty() {
        return None;
    }
    let mut val: u64 = 0;
    for c in digits_part.chars() {
        if !c.is_ascii_digit() {
            return None;
        }
        let d = (c as u8 - b'0') as u64;
        val = val.saturating_mul(10).saturating_add(d);
    }
    Some(val.saturating_mul(mult))
}

const KNOWN_RULE_KEYS: &[&str] = &[
    "signal",
    "direction",
    "fire_at",
    "rearm",
    "hold_for",
    "cooldown",
    "priority",
    "deliver",
    "template",
    "args",
    "offer_skill",
    "enabled",
];

fn template_args(template_name: &str) -> Option<&'static [&'static str]> {
    TEMPLATES
        .iter()
        .find(|(name, _)| *name == template_name)
        .map(|(_, args)| *args)
}

fn default_direction(signal: Signal) -> Direction {
    match signal {
        Signal::BatteryPercent | Signal::MemoryAvailablePercent | Signal::DiskFillHours => {
            Direction::AtMost
        }
        Signal::DiskUsedPercent
        | Signal::TemperatureC
        | Signal::ActiveMinutes
        | Signal::PartsDegraded => Direction::AtLeast,
    }
}

fn default_args_for(template_name: &str) -> Vec<(String, ArgSource)> {
    if let Some(arg_names) = template_args(template_name) {
        arg_names
            .iter()
            .map(|&name| {
                let src = if template_name == "disk.critical" && name == "free_gib" {
                    ArgSource::Extra
                } else {
                    ArgSource::Value
                };
                (name.to_string(), src)
            })
            .collect()
    } else {
        Vec::new()
    }
}

fn parse_signal(rule_id: &str, s: &str) -> Result<Signal, RuleError> {
    match s {
        "disk.used_percent" => Ok(Signal::DiskUsedPercent),
        "disk.fill_hours" => Ok(Signal::DiskFillHours),
        "battery.percent" => Ok(Signal::BatteryPercent),
        "temperature.c" => Ok(Signal::TemperatureC),
        "memory.available_percent" => Ok(Signal::MemoryAvailablePercent),
        "session.active_minutes" => Ok(Signal::ActiveMinutes),
        "parts.degraded" => Ok(Signal::PartsDegraded),
        _ => Err(RuleError::Invalid {
            rule: rule_id.to_string(),
            field: "signal",
            reason: "unknown signal",
        }),
    }
}

fn parse_direction(rule_id: &str, s: &str) -> Result<Direction, RuleError> {
    match s {
        "at_least" => Ok(Direction::AtLeast),
        "at_most" => Ok(Direction::AtMost),
        _ => Err(RuleError::Invalid {
            rule: rule_id.to_string(),
            field: "direction",
            reason: "direction must be at_least or at_most",
        }),
    }
}

fn parse_priority(rule_id: &str, s: &str) -> Result<Priority, RuleError> {
    match s {
        "info" => Ok(Priority::Info),
        "important" => Ok(Priority::Important),
        "urgent" => Ok(Priority::Urgent),
        _ => Err(RuleError::Invalid {
            rule: rule_id.to_string(),
            field: "priority",
            reason: "priority must be info, important, or urgent",
        }),
    }
}

fn parse_deliver(rule_id: &str, s: &str) -> Result<Delivery, RuleError> {
    match s {
        "voice" => Ok(Delivery::Voice),
        "notify" => Ok(Delivery::Notify),
        "off" => Ok(Delivery::Off),
        _ => Err(RuleError::Invalid {
            rule: rule_id.to_string(),
            field: "deliver",
            reason: "deliver must be voice, notify, or off",
        }),
    }
}

fn is_valid_rule_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn validate_rule(rule: &Rule) -> Result<(), RuleError> {
    if !is_valid_rule_id(&rule.id) {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "id",
            reason: "rule id must match [a-z0-9_]{1,40}",
        });
    }

    if !rule.fire_at.is_finite() {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "fire_at",
            reason: "must be finite",
        });
    }

    if !rule.rearm.is_finite() {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "rearm",
            reason: "must be finite",
        });
    }

    match rule.direction {
        Direction::AtLeast if rule.rearm > rule.fire_at => {
            return Err(RuleError::Invalid {
                rule: rule.id.clone(),
                field: "rearm",
                reason: "at_least requires rearm <= fire_at",
            });
        }
        Direction::AtMost if rule.rearm < rule.fire_at => {
            return Err(RuleError::Invalid {
                rule: rule.id.clone(),
                field: "rearm",
                reason: "at_most requires rearm >= fire_at",
            });
        }
        _ => {}
    }

    // Range checks by signal
    match rule.signal {
        Signal::DiskUsedPercent | Signal::BatteryPercent | Signal::MemoryAvailablePercent => {
            if !(0.0..=100.0).contains(&rule.fire_at) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "fire_at",
                    reason: "percent signals must be between 0 and 100",
                });
            }
            if !(0.0..=100.0).contains(&rule.rearm) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "rearm",
                    reason: "percent signals must be between 0 and 100",
                });
            }
        }
        Signal::TemperatureC => {
            if !(0.0..=150.0).contains(&rule.fire_at) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "fire_at",
                    reason: "temperature must be between 0 and 150",
                });
            }
            if !(0.0..=150.0).contains(&rule.rearm) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "rearm",
                    reason: "temperature must be between 0 and 150",
                });
            }
        }
        Signal::DiskFillHours => {
            if !(0.0..=10000.0).contains(&rule.fire_at) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "fire_at",
                    reason: "disk.fill_hours must be between 0 and 10000",
                });
            }
            if !(0.0..=10000.0).contains(&rule.rearm) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "rearm",
                    reason: "disk.fill_hours must be between 0 and 10000",
                });
            }
        }
        Signal::ActiveMinutes => {
            if !(0.0..=100000.0).contains(&rule.fire_at) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "fire_at",
                    reason: "session.active_minutes must be between 0 and 100000",
                });
            }
            if !(0.0..=100000.0).contains(&rule.rearm) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "rearm",
                    reason: "session.active_minutes must be between 0 and 100000",
                });
            }
        }
        Signal::PartsDegraded => {
            if !(0.0..=100.0).contains(&rule.fire_at) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "fire_at",
                    reason: "parts.degraded must be between 0 and 100",
                });
            }
            if !(0.0..=100.0).contains(&rule.rearm) {
                return Err(RuleError::Invalid {
                    rule: rule.id.clone(),
                    field: "rearm",
                    reason: "parts.degraded must be between 0 and 100",
                });
            }
        }
    }

    if rule.hold_for_s > 24 * 3600 {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "hold_for",
            reason: "hold_for must be at most 24h",
        });
    }

    if rule.cooldown_s > 30 * 86400 {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "cooldown",
            reason: "cooldown must be at most 30d",
        });
    }

    let expected_args = template_args(&rule.template).ok_or_else(|| RuleError::Invalid {
        rule: rule.id.clone(),
        field: "template",
        reason: "unknown template",
    })?;

    if rule.args.len() != expected_args.len() {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "args",
            reason: "args keys must equal template argument names",
        });
    }

    for (name, src) in &rule.args {
        if !expected_args.contains(&name.as_str()) {
            return Err(RuleError::Invalid {
                rule: rule.id.clone(),
                field: "args",
                reason: "args keys must equal template argument names",
            });
        }
        if *src == ArgSource::Extra
            && rule.signal != Signal::DiskUsedPercent
            && rule.signal != Signal::DiskFillHours
        {
            return Err(RuleError::Invalid {
                rule: rule.id.clone(),
                field: "args",
                reason: "extra only allowed for disk.used_percent and disk.fill_hours",
            });
        }
    }

    if rule
        .offer_skill
        .as_deref()
        .is_some_and(|skill| !OFFERABLE_SKILLS.contains(&skill))
    {
        return Err(RuleError::Invalid {
            rule: rule.id.clone(),
            field: "offer_skill",
            reason: "unknown offer skill",
        });
    }

    Ok(())
}

struct ParsedRulePatch {
    signal: Option<Signal>,
    direction: Option<Direction>,
    fire_at: Option<f64>,
    rearm: Option<f64>,
    hold_for: Option<u64>,
    cooldown: Option<u64>,
    priority: Option<Priority>,
    deliver: Option<Option<Delivery>>,
    template: Option<String>,
    args: Option<Vec<(String, ArgSource)>>,
    offer_skill: Option<Option<String>>,
    enabled: Option<bool>,
}

fn parse_rule_table(
    rule_id: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<ParsedRulePatch, RuleError> {
    for k in table.keys() {
        if !KNOWN_RULE_KEYS.contains(&k.as_str()) {
            return Err(RuleError::UnknownKey {
                rule: rule_id.to_string(),
                key: k.clone(),
            });
        }
    }

    let signal = match table.get("signal") {
        Some(toml::Value::String(s)) => Some(parse_signal(rule_id, s)?),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "signal",
                reason: "signal must be a string",
            });
        }
        None => None,
    };

    let direction = match table.get("direction") {
        Some(toml::Value::String(s)) => Some(parse_direction(rule_id, s)?),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "direction",
                reason: "direction must be a string",
            });
        }
        None => None,
    };

    let fire_at = match table.get("fire_at") {
        Some(toml::Value::Float(f)) => {
            if !f.is_finite() {
                return Err(RuleError::Invalid {
                    rule: rule_id.to_string(),
                    field: "fire_at",
                    reason: "must be finite",
                });
            }
            Some(*f)
        }
        Some(toml::Value::Integer(i)) => Some(*i as f64),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "fire_at",
                reason: "fire_at must be a number",
            });
        }
        None => None,
    };

    let rearm = match table.get("rearm") {
        Some(toml::Value::Float(f)) => {
            if !f.is_finite() {
                return Err(RuleError::Invalid {
                    rule: rule_id.to_string(),
                    field: "rearm",
                    reason: "must be finite",
                });
            }
            Some(*f)
        }
        Some(toml::Value::Integer(i)) => Some(*i as f64),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "rearm",
                reason: "rearm must be a number",
            });
        }
        None => None,
    };

    let hold_for = match table.get("hold_for") {
        Some(toml::Value::String(s)) => {
            let dur = parse_duration(s).ok_or_else(|| RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "hold_for",
                reason: "invalid duration",
            })?;
            Some(dur)
        }
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "hold_for",
                reason: "hold_for must be a duration string",
            });
        }
        None => None,
    };

    let cooldown = match table.get("cooldown") {
        Some(toml::Value::String(s)) => {
            let dur = parse_duration(s).ok_or_else(|| RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "cooldown",
                reason: "invalid duration",
            })?;
            Some(dur)
        }
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "cooldown",
                reason: "cooldown must be a duration string",
            });
        }
        None => None,
    };

    let priority = match table.get("priority") {
        Some(toml::Value::String(s)) => Some(parse_priority(rule_id, s)?),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "priority",
                reason: "priority must be a string",
            });
        }
        None => None,
    };

    let deliver = match table.get("deliver") {
        Some(toml::Value::String(s)) => Some(Some(parse_deliver(rule_id, s)?)),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "deliver",
                reason: "deliver must be a string",
            });
        }
        None => None,
    };

    let template = match table.get("template") {
        Some(toml::Value::String(s)) => {
            if template_args(s).is_none() {
                return Err(RuleError::Invalid {
                    rule: rule_id.to_string(),
                    field: "template",
                    reason: "unknown template",
                });
            }
            Some(s.clone())
        }
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "template",
                reason: "template must be a string",
            });
        }
        None => None,
    };

    let offer_skill = match table.get("offer_skill") {
        Some(toml::Value::String(s)) => {
            if !OFFERABLE_SKILLS.contains(&s.as_str()) {
                return Err(RuleError::Invalid {
                    rule: rule_id.to_string(),
                    field: "offer_skill",
                    reason: "unknown offer skill",
                });
            }
            Some(Some(s.clone()))
        }
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "offer_skill",
                reason: "offer_skill must be a string",
            });
        }
        None => None,
    };

    let enabled = match table.get("enabled") {
        Some(toml::Value::Boolean(b)) => Some(*b),
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "enabled",
                reason: "enabled must be a boolean",
            });
        }
        None => None,
    };

    let args = match table.get("args") {
        Some(toml::Value::Table(args_table)) => {
            let mut parsed_args = Vec::new();
            for (arg_key, arg_val) in args_table {
                let src_str = match arg_val {
                    toml::Value::String(s) => s.as_str(),
                    _ => {
                        return Err(RuleError::Invalid {
                            rule: rule_id.to_string(),
                            field: "args",
                            reason: "arg source must be string",
                        });
                    }
                };
                let src = match src_str {
                    "value" => ArgSource::Value,
                    "extra" => ArgSource::Extra,
                    _ => {
                        return Err(RuleError::Invalid {
                            rule: rule_id.to_string(),
                            field: "args",
                            reason: "arg source must be value or extra",
                        });
                    }
                };
                parsed_args.push((arg_key.clone(), src));
            }
            Some(parsed_args)
        }
        Some(_) => {
            return Err(RuleError::Invalid {
                rule: rule_id.to_string(),
                field: "args",
                reason: "args must be a table",
            });
        }
        None => None,
    };

    Ok(ParsedRulePatch {
        signal,
        direction,
        fire_at,
        rearm,
        hold_for,
        cooldown,
        priority,
        deliver,
        template,
        args,
        offer_skill,
        enabled,
    })
}

fn extract_rule_order(text: &str, ids: &[String]) -> Vec<String> {
    let mut with_pos: Vec<(usize, String)> = ids
        .iter()
        .map(|id| {
            let needle1 = format!("[rule.{id}]");
            let needle2 = format!("[rule.\"{id}\"]");
            let needle3 = format!("[rule.'{id}']");
            let pos = text
                .find(&needle1)
                .or_else(|| text.find(&needle2))
                .or_else(|| text.find(&needle3))
                .unwrap_or(usize::MAX);
            (pos, id.clone())
        })
        .collect();
    with_pos.sort_by_key(|(pos, _)| *pos);
    with_pos.into_iter().map(|(_, id)| id).collect()
}

/// Parses the compiled-in default rules.
pub fn defaults() -> Result<Vec<Rule>, RuleError> {
    load(None)
}

/// Loads default rules merged with the optional user TOML file.
pub fn load(user: Option<&str>) -> Result<Vec<Rule>, RuleError> {
    // 1. Parse compiled-in default rules
    let default_val: toml::Value =
        toml::from_str(DEFAULT_RULES_TOML).map_err(|_| RuleError::Parse)?;
    let default_root = default_val.as_table().ok_or(RuleError::Parse)?;
    let default_rules_table = default_root
        .get("rule")
        .and_then(|v| v.as_table())
        .ok_or(RuleError::Parse)?;

    // Preserve default rules order as declared in DEFAULT_RULES_TOML
    let default_order = [
        "disk_nearly_full",
        "disk_critical",
        "disk_filling_fast",
        "battery_low",
        "running_hot",
        "memory_pressure",
        "long_session",
        "part_degraded",
    ];

    let mut default_rules = Vec::with_capacity(default_order.len());
    for id in default_order {
        let table = default_rules_table
            .get(id)
            .and_then(|v| v.as_table())
            .ok_or(RuleError::Parse)?;
        let patch = parse_rule_table(id, table)?;
        let sig = patch.signal.ok_or(RuleError::Missing {
            rule: id.to_string(),
            field: "signal",
        })?;
        let tmpl = patch.template.ok_or(RuleError::Missing {
            rule: id.to_string(),
            field: "template",
        })?;
        let args = patch.args.unwrap_or_else(|| default_args_for(&tmpl));
        let rule = Rule {
            id: id.to_string(),
            signal: sig,
            direction: patch.direction.unwrap_or_else(|| default_direction(sig)),
            fire_at: patch.fire_at.ok_or(RuleError::Missing {
                rule: id.to_string(),
                field: "fire_at",
            })?,
            rearm: patch.rearm.ok_or(RuleError::Missing {
                rule: id.to_string(),
                field: "rearm",
            })?,
            hold_for_s: patch.hold_for.unwrap_or(0),
            cooldown_s: patch.cooldown.unwrap_or(86400),
            priority: patch.priority.ok_or(RuleError::Missing {
                rule: id.to_string(),
                field: "priority",
            })?,
            deliver: patch.deliver.flatten(),
            template: tmpl,
            args,
            offer_skill: patch.offer_skill.flatten(),
            enabled: patch.enabled.unwrap_or(true),
        };
        validate_rule(&rule)?;
        default_rules.push(rule);
    }

    let user_text = match user {
        Some(u) => u,
        None => return Ok(default_rules),
    };

    if user_text.len() > MAX_FILE_BYTES {
        return Err(RuleError::TooLarge);
    }

    let user_val: toml::Value = toml::from_str(user_text).map_err(|_| RuleError::Parse)?;
    let user_root = user_val.as_table().ok_or(RuleError::Parse)?;

    // Top-level must have ONLY "rule" table
    for k in user_root.keys() {
        if k != "rule" {
            return Err(RuleError::UnknownKey {
                rule: String::new(),
                key: k.clone(),
            });
        }
    }

    let user_rule_table = match user_root.get("rule") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => {
            return Err(RuleError::UnknownKey {
                rule: "rule".to_string(),
                key: String::new(),
            });
        }
        None => {
            // No rule table defined; defaults remain unmodified
            return Ok(default_rules);
        }
    };

    // Under `rule`, check that each entry is a table
    for (k, v) in user_rule_table {
        if !v.is_table() {
            return Err(RuleError::UnknownKey {
                rule: "rule".to_string(),
                key: k.clone(),
            });
        }
    }

    // Determine the order of user rules
    let user_ids: Vec<String> = user_rule_table.keys().cloned().collect();
    let ordered_user_ids = extract_rule_order(user_text, &user_ids);

    let mut rules = default_rules;
    let mut new_rules = Vec::new();

    for id in &ordered_user_ids {
        let rule_val = &user_rule_table[id];
        let table = rule_val.as_table().ok_or(RuleError::Parse)?;
        let patch = parse_rule_table(id, table)?;

        // If rule id is a default rule id, merge/override fields
        if let Some(existing) = rules.iter_mut().find(|r| r.id == *id) {
            if let Some(s) = patch.signal {
                existing.signal = s;
            }
            if let Some(d) = patch.direction {
                existing.direction = d;
            }
            if let Some(f) = patch.fire_at {
                existing.fire_at = f;
            }
            if let Some(r) = patch.rearm {
                existing.rearm = r;
            }
            if let Some(h) = patch.hold_for {
                existing.hold_for_s = h;
            }
            if let Some(c) = patch.cooldown {
                existing.cooldown_s = c;
            }
            if let Some(p) = patch.priority {
                existing.priority = p;
            }
            if let Some(del) = patch.deliver {
                existing.deliver = del;
            }
            let template_changed = patch.template.is_some();
            if let Some(t) = patch.template {
                existing.template = t;
            }
            if let Some(a) = patch.args {
                // Check for unknown keys inside args against the template
                let t_args =
                    template_args(&existing.template).ok_or_else(|| RuleError::Invalid {
                        rule: id.clone(),
                        field: "template",
                        reason: "unknown template",
                    })?;
                for (arg_name, _) in &a {
                    if !t_args.contains(&arg_name.as_str()) {
                        return Err(RuleError::UnknownKey {
                            rule: id.clone(),
                            key: arg_name.clone(),
                        });
                    }
                }
                existing.args = a;
            } else if template_changed {
                // If template was changed and args was not provided, update default args
                existing.args = default_args_for(&existing.template);
            }
            if let Some(off) = patch.offer_skill {
                existing.offer_skill = off;
            }
            if let Some(en) = patch.enabled {
                existing.enabled = en;
            }

            validate_rule(existing)?;
        } else {
            // New rule ID
            if !is_valid_rule_id(id) {
                return Err(RuleError::Invalid {
                    rule: id.clone(),
                    field: "id",
                    reason: "rule id must match [a-z0-9_]{1,40}",
                });
            }

            let sig = patch.signal.ok_or(RuleError::Missing {
                rule: id.clone(),
                field: "signal",
            })?;
            let fire_at = patch.fire_at.ok_or(RuleError::Missing {
                rule: id.clone(),
                field: "fire_at",
            })?;
            let rearm = patch.rearm.ok_or(RuleError::Missing {
                rule: id.clone(),
                field: "rearm",
            })?;
            let priority = patch.priority.ok_or(RuleError::Missing {
                rule: id.clone(),
                field: "priority",
            })?;
            let template = patch.template.ok_or(RuleError::Missing {
                rule: id.clone(),
                field: "template",
            })?;

            let t_args = template_args(&template).ok_or_else(|| RuleError::Invalid {
                rule: id.clone(),
                field: "template",
                reason: "unknown template",
            })?;

            let args = if let Some(a) = patch.args {
                for (arg_name, _) in &a {
                    if !t_args.contains(&arg_name.as_str()) {
                        return Err(RuleError::UnknownKey {
                            rule: id.clone(),
                            key: arg_name.clone(),
                        });
                    }
                }
                a
            } else {
                default_args_for(&template)
            };

            let rule = Rule {
                id: id.clone(),
                signal: sig,
                direction: patch.direction.unwrap_or_else(|| default_direction(sig)),
                fire_at,
                rearm,
                hold_for_s: patch.hold_for.unwrap_or(0),
                cooldown_s: patch.cooldown.unwrap_or(86400),
                priority,
                deliver: patch.deliver.flatten(),
                template,
                args,
                offer_skill: patch.offer_skill.flatten(),
                enabled: patch.enabled.unwrap_or(true),
            };

            validate_rule(&rule)?;
            new_rules.push(rule);
        }
    }

    rules.extend(new_rules);

    if rules.len() > MAX_RULES {
        return Err(RuleError::TooManyRules);
    }

    Ok(rules)
}
