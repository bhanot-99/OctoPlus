//! The action route (Architecture §11 step 2, FR-KNO-03, FR-KER-10): C4 is asked for one
//! grammar-limited skill call, C5 validates it strictly, and only then does it become a
//! `Dispatch` to C6. The model never decides the route and never writes C6 arguments
//! directly: it fills the small *model schema* below, and [`parse_call`] turns that into the
//! exact arguments of the skill's manifest (so a model cannot invent a timestamp or a key).
//!
//! The table is closed on purpose. DANGEROUS and disabled skills are absent, so is
//! `system.cleanup` (its arguments are paths the model must not choose) and `web.*` (Phase 8).
//! The list is mirrored from `crates/neuroos-kernel/skills`; the contract test
//! `tests/action_test.rs` reads those manifests and fails when the two drift.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

/// Rolling windows `memory.forget` may name; C5 turns them into nanosecond bounds.
pub const RANGES: &[(&str, u64)] = &[
    ("last_hour", 3_600),
    ("last_day", 86_400),
    ("last_week", 7 * 86_400),
];

/// The "no skill fits" answer the grammar always allows.
pub const NONE_SKILL: &str = "none";

/// One model-fillable parameter.
#[derive(Debug, Clone, Copy)]
pub enum Param {
    /// Integer in `min..=max`.
    Int {
        name: &'static str,
        min: u64,
        max: u64,
    },
    /// Text of at most `max` bytes with no control characters.
    Text {
        name: &'static str,
        max: usize,
        required: bool,
    },
    /// Time range for `memory.forget` (see [`RANGES`]); optional when an app is named.
    Range { name: &'static str, required: bool },
}

impl Param {
    const fn name(&self) -> &'static str {
        match self {
            Self::Int { name, .. } | Self::Text { name, .. } | Self::Range { name, .. } => name,
        }
    }
    const fn required(&self) -> bool {
        match self {
            Self::Int { .. } => true,
            Self::Text { required, .. } | Self::Range { required, .. } => *required,
        }
    }
}

/// One skill the model may ask for.
#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    /// Capability name, equal to the kernel manifest's.
    pub skill: &'static str,
    /// Parameters in the fixed order the grammar emits them.
    pub params: &'static [Param],
}

/// Skills the action route can call. Order is the order the grammar and the persona list them.
pub const SPECS: &[ActionSpec] = &[
    ActionSpec {
        skill: "privacy.pause",
        params: &[Param::Int {
            name: "minutes",
            min: 1,
            max: 480,
        }],
    },
    ActionSpec {
        skill: "memory.forget",
        params: &[
            Param::Range {
                name: "range",
                required: false,
            },
            Param::Text {
                name: "app",
                max: 128,
                required: false,
            },
        ],
    },
    ActionSpec {
        skill: "profile.remember",
        params: &[Param::Text {
            name: "fact",
            max: 512,
            required: true,
        }],
    },
    ActionSpec {
        skill: "profile.forget",
        params: &[Param::Int {
            name: "fact_id",
            min: 1,
            max: u64::MAX >> 12,
        }],
    },
    ActionSpec {
        skill: "notes.search",
        params: &[Param::Text {
            name: "query",
            max: 256,
            required: true,
        }],
    },
    ActionSpec {
        skill: "notes.append",
        params: &[
            Param::Text {
                name: "note",
                max: 256,
                required: true,
            },
            Param::Text {
                name: "text",
                max: 4096,
                required: true,
            },
        ],
    },
    ActionSpec {
        skill: "calendar.read",
        params: &[Param::Int {
            name: "days",
            min: 1,
            max: 31,
        }],
    },
    ActionSpec {
        skill: "system.disk_report",
        params: &[],
    },
    ActionSpec {
        skill: "system.read_metadata",
        params: &[],
    },
];

/// Why a model answer was refused. Messages never carry the model's text.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ActionError {
    /// Not one JSON object.
    #[error("the model's answer is not a JSON object")]
    NotAnObject,
    /// The `skill` key is missing or not a string.
    #[error("no skill named")]
    NoSkill,
    /// The model said no skill fits (a valid, safe answer).
    #[error("no skill fits the request")]
    NoneFits,
    /// A skill that is not in [`SPECS`].
    #[error("skill is not callable from the action route")]
    UnknownSkill,
    /// A key that the skill's schema does not have.
    #[error("unknown argument")]
    UnknownArg,
    /// A required argument is missing.
    #[error("missing argument")]
    MissingArg,
    /// Wrong type, out of range, too long or contains control characters.
    #[error("bad argument value")]
    BadArg,
    /// `memory.forget` with neither a range nor an app would mean "everything".
    #[error("forget needs a range or an app")]
    UnboundedForget,
}

/// A validated call, ready for `DispatchRequest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCall {
    /// Capability name.
    pub skill: &'static str,
    /// Arguments exactly as the kernel manifest expects them.
    pub args_json: String,
}

/// The GBNF grammar that limits C4's output to the model schema (FR-INF-05). Generated from
/// [`SPECS`], so the grammar and the validator cannot disagree about which skills exist.
#[must_use]
pub fn grammar() -> String {
    let mut g = String::new();
    g.push_str("root ::= call\n");
    let mut alts = vec![format!("\"{{\\\"skill\\\":\\\"{NONE_SKILL}\\\"}}\"")];
    for spec in SPECS {
        let mut rule = format!("\"{{\\\"skill\\\":\\\"{}\\\"", spec.skill);
        for p in spec.params {
            // Optional parameters are emitted too (possibly empty); the validator drops empties.
            let _ = write!(rule, " \",\\\"{}\\\":\" ", p.name());
            match p {
                Param::Int { .. } => rule.push_str("int"),
                Param::Text { max, .. } => {
                    let _ = write!(rule, "text{}", bucket(*max));
                }
                Param::Range { .. } => rule.push_str("range"),
            }
        }
        rule.push_str(" \"}\"");
        alts.push(rule);
    }
    let _ = writeln!(g, "call ::= {}", alts.join(" | "));
    g.push_str("int ::= [0-9]{1,6}\n");
    let ranges: Vec<String> = RANGES
        .iter()
        .map(|(n, _)| format!("\"\\\"{n}\\\"\""))
        .collect();
    let _ = writeln!(g, "range ::= {} | \"\\\"\\\"\"", ranges.join(" | "));
    for b in [128, 256, 512, 4096] {
        let _ = writeln!(
            g,
            "text{b} ::= \"\\\"\" [^\"\\\\\\x00-\\x1f]{{0,{b}}} \"\\\"\""
        );
    }
    g
}

/// Grammar buckets for text lengths; the validator enforces the exact limit afterwards.
const fn bucket(max: usize) -> usize {
    if max <= 128 {
        128
    } else if max <= 256 {
        256
    } else if max <= 512 {
        512
    } else {
        4096
    }
}

/// One line per skill for the persona's skill list.
#[must_use]
pub fn skill_list() -> String {
    let mut s = String::new();
    for spec in SPECS {
        let args: Vec<&str> = spec.params.iter().map(Param::name).collect();
        let _ = writeln!(s, "{}({})", spec.skill, args.join(", "));
    }
    s
}

/// Validates the model's answer and builds the exact manifest arguments.
///
/// `now_ns` is the turn's time (AB-13): it only matters for `memory.forget` ranges.
///
/// # Errors
/// [`ActionError`]: nothing the model says can name a skill outside [`SPECS`], add a key, or
/// leave the declared ranges.
pub fn parse_call(model_text: &str, now_ns: u64) -> Result<SkillCall, ActionError> {
    let value: Value =
        serde_json::from_str(model_text.trim()).map_err(|_| ActionError::NotAnObject)?;
    let Value::Object(mut obj) = value else {
        return Err(ActionError::NotAnObject);
    };
    let name = match obj.remove("skill") {
        Some(Value::String(s)) => s,
        _ => return Err(ActionError::NoSkill),
    };
    if name == NONE_SKILL && obj.is_empty() {
        return Err(ActionError::NoneFits);
    }
    let spec = SPECS
        .iter()
        .find(|s| s.skill == name)
        .ok_or(ActionError::UnknownSkill)?;
    if obj
        .keys()
        .any(|k| !spec.params.iter().any(|p| p.name() == k))
    {
        return Err(ActionError::UnknownArg);
    }
    let mut args = Map::new();
    for p in spec.params {
        let raw = obj.remove(p.name());
        if let Some(v) = check(p, raw)? {
            args.insert(p.name().to_string(), v);
        } else if p.required() {
            return Err(ActionError::MissingArg);
        }
    }
    let args = if spec.skill == "memory.forget" {
        forget_args(args, now_ns)?
    } else {
        args
    };
    Ok(SkillCall {
        skill: spec.skill,
        args_json: Value::Object(args).to_string(),
    })
}

/// Checks one value. `Ok(None)` means "absent or empty optional".
fn check(p: &Param, raw: Option<Value>) -> Result<Option<Value>, ActionError> {
    match (p, raw) {
        (_, None | Some(Value::Null)) => Ok(None),
        (Param::Int { min, max, .. }, Some(v)) => {
            let n = v.as_u64().ok_or(ActionError::BadArg)?;
            if (*min..=*max).contains(&n) {
                Ok(Some(json!(n)))
            } else {
                Err(ActionError::BadArg)
            }
        }
        (Param::Text { max, required, .. }, Some(Value::String(s))) => {
            let t = s.trim();
            if t.len() > *max || t.chars().any(char::is_control) {
                return Err(ActionError::BadArg);
            }
            if t.is_empty() {
                return if *required {
                    Err(ActionError::BadArg)
                } else {
                    Ok(None)
                };
            }
            Ok(Some(Value::String(t.to_string())))
        }
        (Param::Range { .. }, Some(Value::String(s))) => {
            if s.is_empty() {
                return Ok(None);
            }
            if RANGES.iter().any(|(n, _)| *n == s) {
                Ok(Some(Value::String(s)))
            } else {
                Err(ActionError::BadArg)
            }
        }
        _ => Err(ActionError::BadArg),
    }
}

/// Turns the model's `range` into the manifest's `since_ns`/`until_ns` (never "everything").
fn forget_args(
    mut args: Map<String, Value>,
    now_ns: u64,
) -> Result<Map<String, Value>, ActionError> {
    let range = args.remove("range");
    let has_app = args.contains_key("app");
    match range {
        Some(Value::String(name)) => {
            let secs = RANGES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .ok_or(ActionError::BadArg)?;
            let since = now_ns
                .saturating_sub(secs.saturating_mul(1_000_000_000))
                .max(1);
            args.insert("since_ns".into(), json!(since));
            args.insert("until_ns".into(), json!(now_ns));
        }
        None if has_app => {}
        _ => return Err(ActionError::UnboundedForget),
    }
    Ok(args)
}

/// Longest raw result text spoken for the read-only skills.
const MAX_SPOKEN_RESULT: usize = 400;

/// Fixed sentence for a refused or failed dispatch.
#[must_use]
pub const fn speak_not_done(status: DispatchOutcome) -> &'static str {
    match status {
        DispatchOutcome::Rejected => "Okay, I won't do that.",
        DispatchOutcome::Expired => "That timed out, so I didn't do it.",
        DispatchOutcome::Failed => "That didn't work.",
    }
}

/// Why a dispatch did not execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The user said no (or policy did).
    Rejected,
    /// The confirmation timed out.
    Expired,
    /// The skill failed.
    Failed,
}

/// The sentence for an executed skill. Write skills get a fixed line; read-only skills get
/// their result flattened to plain words and neutralised, because the result is data the
/// kernel returned, not our own template (FR-KNO-06).
#[must_use]
pub fn speak_result(skill: &str, result_json: &str) -> String {
    let n = serde_json::from_str::<Value>(result_json)
        .ok()
        .and_then(|v| v.get("rows_deleted").and_then(Value::as_u64));
    match skill {
        "privacy.pause" => "Okay, I've paused.".to_string(),
        "profile.remember" => "Okay, I'll remember that.".to_string(),
        "profile.forget" => "Okay, I've forgotten that.".to_string(),
        "memory.forget" => n.map_or_else(
            || "Done, I've forgotten that.".to_string(),
            |n| format!("Done, I removed {n} items."),
        ),
        "notes.append" => "Okay, I added it to your note.".to_string(),
        _ => {
            let flat = flatten(result_json);
            let cut: String = flat.chars().take(MAX_SPOKEN_RESULT).collect();
            if cut.is_empty() {
                "Done.".to_string()
            } else {
                format!("Here's what I found: {}", crate::sanitize::neutralise(&cut))
            }
        }
    }
}

/// JSON to plain words: keys and scalars joined with spaces, markup characters dropped.
fn flatten(json: &str) -> String {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Number(n) => out.push(n.to_string()),
            Value::Bool(b) => out.push(b.to_string()),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.iter().for_each(|(k, x)| {
                out.push(k.replace('_', " "));
                walk(x, out);
            }),
            Value::Null => {}
        }
    }
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return String::new();
    };
    let mut parts = Vec::new();
    walk(&v, &mut parts);
    parts
        .join(", ")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | '*' | '#' | '`'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000_000_000;

    fn call(text: &str) -> Result<SkillCall, ActionError> {
        parse_call(text, NOW)
    }

    #[test]
    fn valid_calls_become_manifest_arguments() {
        let c = call(r#"{"skill":"privacy.pause","minutes":30}"#).unwrap();
        assert_eq!(
            (c.skill, c.args_json.as_str()),
            ("privacy.pause", r#"{"minutes":30}"#)
        );
        let c = call(r#"{"skill":"profile.remember","fact":"  my dog is Rex "}"#).unwrap();
        assert_eq!(c.args_json, r#"{"fact":"my dog is Rex"}"#);
        let c = call(r#"{"skill":"system.disk_report"}"#).unwrap();
        assert_eq!(c.args_json, "{}");
        let c = call(r#"{"skill":"memory.forget","range":"last_hour","app":""}"#).unwrap();
        let v: Value = serde_json::from_str(&c.args_json).unwrap();
        assert_eq!(v["until_ns"], json!(NOW));
        assert_eq!(v["since_ns"], json!(NOW - 3_600_000_000_000));
        let c = call(r#"{"skill":"memory.forget","range":"","app":"org.x"}"#).unwrap();
        assert_eq!(c.args_json, r#"{"app":"org.x"}"#);
    }

    #[test]
    fn refusals() {
        use ActionError::*;
        let cases: &[(&str, ActionError)] = &[
            ("", NotAnObject),
            ("[1]", NotAnObject),
            ("{}", NoSkill),
            (r#"{"skill":5}"#, NoSkill),
            (r#"{"skill":"none"}"#, NoneFits),
            (r#"{"skill":"shell.exec","command":"ls"}"#, UnknownSkill),
            (r#"{"skill":"system.cleanup","paths":[]}"#, UnknownSkill),
            (r#"{"skill":"web.search","query":"x"}"#, UnknownSkill),
            (
                r#"{"skill":"privacy.pause","minutes":30,"extra":1}"#,
                UnknownArg,
            ),
            (r#"{"skill":"privacy.pause"}"#, MissingArg),
            (r#"{"skill":"privacy.pause","minutes":0}"#, BadArg),
            (r#"{"skill":"privacy.pause","minutes":481}"#, BadArg),
            (r#"{"skill":"privacy.pause","minutes":"5"}"#, BadArg),
            (r#"{"skill":"privacy.pause","minutes":-1}"#, BadArg),
            (r#"{"skill":"profile.remember","fact":"   "}"#, BadArg),
            (r#"{"skill":"profile.remember","fact":"a\u0000b"}"#, BadArg),
            (r#"{"skill":"memory.forget"}"#, UnboundedForget),
            (
                r#"{"skill":"memory.forget","range":"","app":""}"#,
                UnboundedForget,
            ),
            (r#"{"skill":"memory.forget","range":"forever"}"#, BadArg),
            (r#"{"skill":"none","skill2":1}"#, UnknownSkill),
        ];
        for (text, want) in cases {
            assert_eq!(call(text), Err(want.clone()), "{text}");
        }
        let long = format!(
            r#"{{"skill":"profile.remember","fact":"{}"}}"#,
            "x".repeat(513)
        );
        assert_eq!(call(&long), Err(BadArg));
    }

    #[test]
    fn grammar_names_every_skill_and_none() {
        let g = grammar();
        assert!(g.starts_with("root ::= call\n"));
        assert!(g.contains(r#"\"skill\":\"none\""#));
        for spec in SPECS {
            assert!(
                g.contains(&format!(r#"\"skill\":\"{}\""#, spec.skill)),
                "{}",
                spec.skill
            );
        }
        for banned in ["shell.exec", "system.cleanup", "web.fetch", "web.search"] {
            assert!(!g.contains(banned), "{banned}");
        }
    }

    #[test]
    fn skill_list_is_one_line_per_skill() {
        assert_eq!(skill_list().lines().count(), SPECS.len());
    }
}
