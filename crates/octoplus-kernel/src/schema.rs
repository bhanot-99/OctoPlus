//! In-house argument schema validation and canonical JSON serializer.
//!
//! Satisfies FR-KER-01, FR-KER-08, and rules.md §1 R0-10 (no user content in error messages).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum allowed payload size for argument JSON strings (64 KiB).
pub const MAX_ARGS_JSON_BYTES: usize = 64 * 1024;

/// Argument schema definition for a single parameter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArgSchema {
    /// String parameter with a maximum byte length and required flag.
    String {
        max_len: usize,
        #[serde(default)]
        required: bool,
    },
    /// Integer parameter with optional minimum and maximum bounds.
    Int {
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
        #[serde(default)]
        required: bool,
    },
    /// Boolean flag.
    Bool {
        #[serde(default)]
        required: bool,
    },
    /// Enumerated string value chosen from an explicit list.
    #[serde(rename = "enum")]
    Enum {
        values: Vec<String>,
        #[serde(default)]
        required: bool,
    },
    /// List of absolute filesystem paths.
    PathList {
        max_items: usize,
        max_len: usize,
        #[serde(default)]
        required: bool,
    },
}

impl ArgSchema {
    /// Returns true if this argument is required.
    #[must_use]
    pub fn is_required(&self) -> bool {
        match self {
            Self::String { required, .. }
            | Self::Int { required, .. }
            | Self::Bool { required }
            | Self::Enum { required, .. }
            | Self::PathList { required, .. } => *required,
        }
    }
}

/// Errors occurring during schema validation.
///
/// Contains no user content in error messages (R0-10).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("payload exceeds maximum size of 64 KiB")]
    PayloadTooLarge,
    #[error("invalid JSON syntax")]
    InvalidJson,
    #[error("arguments must be a JSON object")]
    NotAnObject,
    #[error("unknown field in arguments")]
    UnknownField,
    #[error("missing required field in arguments")]
    MissingRequired,
    #[error("argument has invalid type")]
    WrongType,
    #[error("argument value out of allowed range")]
    OutOfRange,
    #[error("argument value exceeds maximum length")]
    TooLong,
    #[error("string contains forbidden control characters")]
    ControlChar,
    #[error("path is not absolute or contains parent component")]
    InvalidPath,
    #[error("path list item exceeds maximum length")]
    PathTooLong,
    #[error("path list item count out of allowed range")]
    InvalidItemCount,
}

/// Validates whether a string contains ASCII control characters (< 0x20 or 0x7F).
fn has_control_chars(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 0x20 || c == '\x7F')
}

/// Validates a single argument field against its schema.
fn validate_field(schema: &ArgSchema, val: &Value) -> Result<(), SchemaError> {
    match schema {
        ArgSchema::String { max_len, .. } => {
            let s = val.as_str().ok_or(SchemaError::WrongType)?;
            if has_control_chars(s) {
                return Err(SchemaError::ControlChar);
            }
            if s.len() > *max_len {
                return Err(SchemaError::TooLong);
            }
            Ok(())
        }
        ArgSchema::Int { min, max, .. } => {
            let n = val.as_i64().ok_or(SchemaError::WrongType)?;
            if let Some(min_val) = min
                && n < *min_val
            {
                return Err(SchemaError::OutOfRange);
            }
            if let Some(max_val) = max
                && n > *max_val
            {
                return Err(SchemaError::OutOfRange);
            }
            Ok(())
        }
        ArgSchema::Bool { .. } => {
            if !val.is_boolean() {
                return Err(SchemaError::WrongType);
            }
            Ok(())
        }
        ArgSchema::Enum { values, .. } => {
            let s = val.as_str().ok_or(SchemaError::WrongType)?;
            if has_control_chars(s) {
                return Err(SchemaError::ControlChar);
            }
            if !values.iter().any(|v| v == s) {
                return Err(SchemaError::OutOfRange);
            }
            Ok(())
        }
        ArgSchema::PathList {
            max_items, max_len, ..
        } => {
            let arr = val.as_array().ok_or(SchemaError::WrongType)?;
            if arr.is_empty() || arr.len() > *max_items {
                return Err(SchemaError::InvalidItemCount);
            }
            for item in arr {
                let path_str = item.as_str().ok_or(SchemaError::WrongType)?;
                if path_str.len() > *max_len {
                    return Err(SchemaError::PathTooLong);
                }
                if has_control_chars(path_str) {
                    return Err(SchemaError::ControlChar);
                }
                let p = Path::new(path_str);
                if !p.is_absolute() {
                    return Err(SchemaError::InvalidPath);
                }
                if p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(SchemaError::InvalidPath);
                }
            }
            Ok(())
        }
    }
}

/// Validates a parsed JSON value against a skill argument schema.
///
/// # Errors
/// Returns [`SchemaError`] if validation fails.
pub fn validate_args(schema: &BTreeMap<String, ArgSchema>, val: &Value) -> Result<(), SchemaError> {
    let map = val.as_object().ok_or(SchemaError::NotAnObject)?;

    for key in map.keys() {
        if !schema.contains_key(key) {
            return Err(SchemaError::UnknownField);
        }
    }

    for (name, arg_schema) in schema {
        if arg_schema.is_required() {
            match map.get(name) {
                None | Some(Value::Null) => return Err(SchemaError::MissingRequired),
                Some(_) => {}
            }
        }
    }

    for (key, v) in map {
        if v.is_null() {
            continue;
        }
        if let Some(arg_schema) = schema.get(key) {
            validate_field(arg_schema, v)?;
        }
    }

    Ok(())
}

/// Parses and validates an argument JSON string against a skill schema.
///
/// Enforces the 64 KiB total payload size limit before parsing.
///
/// # Errors
/// Returns [`SchemaError`] on payload size limit, parse syntax errors, or schema violations.
pub fn validate_args_json(
    schema: &BTreeMap<String, ArgSchema>,
    json_str: &str,
) -> Result<Value, SchemaError> {
    if json_str.len() > MAX_ARGS_JSON_BYTES {
        return Err(SchemaError::PayloadTooLarge);
    }
    let val: Value = serde_json::from_str(json_str).map_err(|_| SchemaError::InvalidJson)?;
    validate_args(schema, &val)?;
    Ok(val)
}

/// Serializes a JSON Value into canonical JSON format.
///
/// Keys in objects are sorted lexicographically, no whitespace is emitted,
/// and standard JSON escapes are used.
#[must_use]
pub fn canonical_json(val: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, val);
    out
}

fn write_canonical(out: &mut String, val: &Value) {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            if let Ok(escaped) = serde_json::to_string(s) {
                out.push_str(&escaped);
            } else {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Ok(escaped) = serde_json::to_string(k) {
                    out.push_str(&escaped);
                } else {
                    out.push('"');
                    out.push_str(k);
                    out.push('"');
                }
                out.push(':');
                if let Some(v) = map.get(k) {
                    write_canonical(out, v);
                }
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    #[test]
    fn non_object_root_is_rejected() {
        let schema = BTreeMap::new();
        assert_eq!(
            validate_args(&schema, &json!([1, 2, 3])),
            Err(SchemaError::NotAnObject)
        );
        assert_eq!(
            validate_args(&schema, &json!("hello")),
            Err(SchemaError::NotAnObject)
        );
        assert_eq!(
            validate_args(&schema, &json!(42)),
            Err(SchemaError::NotAnObject)
        );
        assert_eq!(
            validate_args(&schema, &json!(null)),
            Err(SchemaError::NotAnObject)
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let schema = BTreeMap::new();
        assert_eq!(
            validate_args(&schema, &json!({"extra": 1})),
            Err(SchemaError::UnknownField)
        );
    }

    #[test]
    fn string_schema_validation() {
        let mut schema = BTreeMap::new();
        schema.insert(
            "name".into(),
            ArgSchema::String {
                max_len: 10,
                required: true,
            },
        );

        // Valid
        assert!(validate_args(&schema, &json!({"name": "valid"})).is_ok());
        // Missing required
        assert_eq!(
            validate_args(&schema, &json!({})),
            Err(SchemaError::MissingRequired)
        );
        assert_eq!(
            validate_args(&schema, &json!({"name": null})),
            Err(SchemaError::MissingRequired)
        );
        // Wrong type
        assert_eq!(
            validate_args(&schema, &json!({"name": 123})),
            Err(SchemaError::WrongType)
        );
        // Too long
        assert_eq!(
            validate_args(&schema, &json!({"name": "this is too long"})),
            Err(SchemaError::TooLong)
        );
        // Control char (NUL, newline, tab, DEL)
        assert_eq!(
            validate_args(&schema, &json!({"name": "bad\0name"})),
            Err(SchemaError::ControlChar)
        );
        assert_eq!(
            validate_args(&schema, &json!({"name": "bad\nname"})),
            Err(SchemaError::ControlChar)
        );
        assert_eq!(
            validate_args(&schema, &json!({"name": "bad\x7fname"})),
            Err(SchemaError::ControlChar)
        );
    }

    #[test]
    fn int_schema_validation() {
        let mut schema = BTreeMap::new();
        schema.insert(
            "count".into(),
            ArgSchema::Int {
                min: Some(1),
                max: Some(10),
                required: true,
            },
        );

        // Valid
        assert!(validate_args(&schema, &json!({"count": 5})).is_ok());
        assert!(validate_args(&schema, &json!({"count": 1})).is_ok());
        assert!(validate_args(&schema, &json!({"count": 10})).is_ok());
        // Missing required
        assert_eq!(
            validate_args(&schema, &json!({})),
            Err(SchemaError::MissingRequired)
        );
        // Wrong type
        assert_eq!(
            validate_args(&schema, &json!({"count": "5"})),
            Err(SchemaError::WrongType)
        );
        assert_eq!(
            validate_args(&schema, &json!({"count": 5.5})),
            Err(SchemaError::WrongType)
        );
        // Out of range
        assert_eq!(
            validate_args(&schema, &json!({"count": 0})),
            Err(SchemaError::OutOfRange)
        );
        assert_eq!(
            validate_args(&schema, &json!({"count": 11})),
            Err(SchemaError::OutOfRange)
        );
    }

    #[test]
    fn bool_schema_validation() {
        let mut schema = BTreeMap::new();
        schema.insert("enabled".into(), ArgSchema::Bool { required: true });

        assert!(validate_args(&schema, &json!({"enabled": true})).is_ok());
        assert!(validate_args(&schema, &json!({"enabled": false})).is_ok());
        assert_eq!(
            validate_args(&schema, &json!({})),
            Err(SchemaError::MissingRequired)
        );
        assert_eq!(
            validate_args(&schema, &json!({"enabled": "true"})),
            Err(SchemaError::WrongType)
        );
    }

    #[test]
    fn enum_schema_validation() {
        let mut schema = BTreeMap::new();
        schema.insert(
            "status".into(),
            ArgSchema::Enum {
                values: vec!["active".into(), "inactive".into()],
                required: true,
            },
        );

        assert!(validate_args(&schema, &json!({"status": "active"})).is_ok());
        assert!(validate_args(&schema, &json!({"status": "inactive"})).is_ok());
        assert_eq!(
            validate_args(&schema, &json!({"status": "unknown"})),
            Err(SchemaError::OutOfRange)
        );
        assert_eq!(
            validate_args(&schema, &json!({})),
            Err(SchemaError::MissingRequired)
        );
        assert_eq!(
            validate_args(&schema, &json!({"status": 1})),
            Err(SchemaError::WrongType)
        );
        assert_eq!(
            validate_args(&schema, &json!({"status": "active\n"})),
            Err(SchemaError::ControlChar)
        );
    }

    #[test]
    fn path_list_schema_validation() {
        let mut schema = BTreeMap::new();
        schema.insert(
            "paths".into(),
            ArgSchema::PathList {
                max_items: 2,
                max_len: 20,
                required: true,
            },
        );

        // Valid
        assert!(validate_args(&schema, &json!({"paths": ["/home/user/file"]})).is_ok());
        assert!(validate_args(&schema, &json!({"paths": ["/tmp/a", "/tmp/b"]})).is_ok());

        // Empty array
        assert_eq!(
            validate_args(&schema, &json!({"paths": []})),
            Err(SchemaError::InvalidItemCount)
        );
        // Too many items
        assert_eq!(
            validate_args(&schema, &json!({"paths": ["/a", "/b", "/c"]})),
            Err(SchemaError::InvalidItemCount)
        );
        // Relative path
        assert_eq!(
            validate_args(&schema, &json!({"paths": ["relative/path"]})),
            Err(SchemaError::InvalidPath)
        );
        // Parent component ..
        assert_eq!(
            validate_args(&schema, &json!({"paths": ["/var/log/../build"]})),
            Err(SchemaError::InvalidPath)
        );
        // NUL / control character
        assert_eq!(
            validate_args(&schema, &json!({"paths": ["/var/log\0/file"]})),
            Err(SchemaError::ControlChar)
        );
        // Path too long
        assert_eq!(
            validate_args(
                &schema,
                &json!({"paths": ["/extremely/long/path/exceeding/limit"]})
            ),
            Err(SchemaError::PathTooLong)
        );
        // Wrong type
        assert_eq!(
            validate_args(&schema, &json!({"paths": "/single/path"})),
            Err(SchemaError::WrongType)
        );
        assert_eq!(
            validate_args(&schema, &json!({"paths": [123]})),
            Err(SchemaError::WrongType)
        );
    }

    #[test]
    fn size_limit_and_canonical_json() {
        let schema = BTreeMap::new();
        let big = " ".repeat(MAX_ARGS_JSON_BYTES + 1);
        assert_eq!(
            validate_args_json(&schema, &big),
            Err(SchemaError::PayloadTooLarge)
        );

        // Canonical JSON key ordering and formatting
        let v: Value =
            serde_json::from_str(r#"{"z": 1, "a": {"d": 4, "b": 2}, "m": [3, 1]}"#).unwrap();
        let c = canonical_json(&v);
        assert_eq!(c, r#"{"a":{"b":2,"d":4},"m":[3,1],"z":1}"#);
    }

    // Proptest strategy to generate arbitrary JSON values
    fn arb_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(|n| Value::Number(n.into())),
            "[a-zA-Z0-9_-]{0,20}".prop_map(Value::String),
        ];
        leaf.prop_recursive(3, 16, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                prop::collection::btree_map("[a-z]{1,5}", inner, 0..4)
                    .prop_map(|m| { Value::Object(m.into_iter().collect()) })
            ]
        })
    }

    proptest! {
        #[test]
        fn canonical_json_is_stable(v in arb_json()) {
            let c1 = canonical_json(&v);
            let v2: Value = serde_json::from_str(&c1).unwrap();
            let c2 = canonical_json(&v2);
            prop_assert_eq!(c1, c2);
        }
    }
}
