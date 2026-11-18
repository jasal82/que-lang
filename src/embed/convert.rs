//! Conversions between que [`Value`]s and serde data.
//!
//! Lossless for the data the formats have in common: null, booleans,
//! integers, floats, strings, lists and string-keyed maps round-trip
//! unchanged. The lossy cases are deliberate and listed here:
//!
//! | Direction | Case | Result |
//! |---|---|---|
//! | JSON → que | integer above `i64::MAX` | `Float` (precision may be lost) |
//! | TOML → que | datetime | `String` in TOML syntax |
//! | que → JSON/TOML | `Path`, `Glob`, `Regex`, `Semver` | string (the type is lost) |
//! | que → JSON/TOML | `Set`, `Tuple` | array |
//! | que → JSON/TOML | struct instance | map of its fields (the type name is lost) |
//! | que → JSON | `NaN`, `±inf` | error (JSON can't represent them) |
//! | que → TOML | `Null` | error (TOML has no null) |
//! | que → any | `Secret` | error (a secret never leaves as plain text) |
//! | que → any | functions, commands, streams, handles, `Ok`/`Err`, enums, durations | error |
//! | Rust → que ([`to_value`]) | map with non-string keys | integer and bool keys become strings; other key types are an error |
//!
//! [`to_value`] and [`from_value`] go through `serde_json::Value`, so any
//! `Serialize`/`Deserialize` type works with the JSON rules above.

use std::collections::BTreeMap;
use std::fmt;

use crate::value::Value;

/// A value that can't be represented in the target format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertError {
    /// Where in the value the problem is, e.g. `targets.app.defines[2]`
    /// (empty for the top level).
    pub path: String,
    pub message: String,
}

impl ConvertError {
    fn at(path: &str, message: impl Into<String>) -> Self {
        ConvertError {
            path: path.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "at {}: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for ConvertError {}

fn child(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", path, key)
    }
}

fn index(path: &str, i: usize) -> String {
    format!("{}[{}]", path, i)
}

/// Why a que value has no data representation, or `None` if it has one.
fn unrepresentable(v: &Value) -> Option<String> {
    let what =
        match v {
            Value::Secret(_) => return Some(
                "a Secret can't be converted; call .expose() if the plain text is meant to leave"
                    .to_string(),
            ),
            Value::Ok(_) | Value::Err(_) => {
                return Some(format!(
                    "a {} result can't be converted; unwrap it first",
                    v.type_name()
                ))
            }
            Value::Function { .. } | Value::BuiltinFn(_) => "a function",
            Value::Cmd(..) => "a command",
            Value::ProcessResult { .. } => "a process result",
            Value::Task(_) => "a task",
            Value::Stream(_) => "a stream",
            Value::ProcessHandle(_) => "a process handle",
            Value::FileHandle(_) => "a file handle",
            Value::Enum { .. } => "an enum value",
            Value::TypeRef(_) => "a type",
            Value::Module { .. } => "a module",
            Value::Duration(..) => "a duration",
            _ => return None,
        };
    Some(format!(
        "{} ({}) can't be converted to data",
        what,
        v.type_name()
    ))
}

// ── JSON ─────────────────────────────────────────────────────────────

/// Convert JSON to a que value. Never fails; see the module docs for the
/// one lossy case (integers above `i64::MAX`).
pub fn from_json(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::List(items.iter().map(from_json).collect()),
        serde_json::Value::Object(map) => {
            Value::Map(map.iter().map(|(k, v)| (k.clone(), from_json(v))).collect())
        }
    }
}

/// Convert a que value to JSON.
pub fn to_json(value: &Value) -> Result<serde_json::Value, ConvertError> {
    to_json_at(value, "")
}

fn to_json_at(value: &Value, path: &str) -> Result<serde_json::Value, ConvertError> {
    if let Some(why) = unrepresentable(value) {
        return Err(ConvertError::at(path, why));
    }
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => match serde_json::Number::from_f64(*f) {
            Some(n) => serde_json::Value::Number(n),
            None => {
                return Err(ConvertError::at(
                    path,
                    format!("{} can't be represented in JSON", f),
                ))
            }
        },
        Value::String(s) | Value::Path(s) | Value::Glob(s) | Value::Regex(s) | Value::Semver(s) => {
            serde_json::Value::String(s.clone())
        }
        Value::List(items) | Value::Set(items) | Value::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                out.push(to_json_at(item, &index(path, i))?);
            }
            serde_json::Value::Array(out)
        }
        Value::Map(map) | Value::Instance { fields: map, .. } => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), to_json_at(v, &child(path, k))?);
            }
            serde_json::Value::Object(out)
        }
        other => {
            return Err(ConvertError::at(
                path,
                format!("{} can't be converted to data", other.type_name()),
            ))
        }
    })
}

// ── TOML ─────────────────────────────────────────────────────────────

/// Convert TOML to a que value. Never fails; datetimes become strings.
pub fn from_toml(toml: &toml::Value) -> Value {
    match toml {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f) => Value::Float(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(items) => Value::List(items.iter().map(from_toml).collect()),
        toml::Value::Table(table) => Value::Map(
            table
                .iter()
                .map(|(k, v)| (k.clone(), from_toml(v)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}

/// Convert a que value to TOML.
pub fn to_toml(value: &Value) -> Result<toml::Value, ConvertError> {
    to_toml_at(value, "")
}

fn to_toml_at(value: &Value, path: &str) -> Result<toml::Value, ConvertError> {
    if let Some(why) = unrepresentable(value) {
        return Err(ConvertError::at(path, why));
    }
    Ok(match value {
        Value::Null => return Err(ConvertError::at(path, "TOML has no null")),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Int(i) => toml::Value::Integer(*i),
        Value::Float(f) => toml::Value::Float(*f),
        Value::String(s) | Value::Path(s) | Value::Glob(s) | Value::Regex(s) | Value::Semver(s) => {
            toml::Value::String(s.clone())
        }
        Value::List(items) | Value::Set(items) | Value::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                out.push(to_toml_at(item, &index(path, i))?);
            }
            toml::Value::Array(out)
        }
        Value::Map(map) | Value::Instance { fields: map, .. } => {
            let mut out = toml::map::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), to_toml_at(v, &child(path, k))?);
            }
            toml::Value::Table(out)
        }
        other => {
            return Err(ConvertError::at(
                path,
                format!("{} can't be converted to data", other.type_name()),
            ))
        }
    })
}

// ── serde ────────────────────────────────────────────────────────────

/// Convert any `Serialize` type to a que value (via JSON).
pub fn to_value<T: serde::Serialize + ?Sized>(value: &T) -> Result<Value, ConvertError> {
    serde_json::to_value(value)
        .map(|json| from_json(&json))
        .map_err(|e| ConvertError::at("", e.to_string()))
}

/// Convert a que value to any `Deserialize` type (via JSON).
pub fn from_value<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, ConvertError> {
    let json = to_json(value)?;
    serde_json::from_value(json).map_err(|e| ConvertError::at("", e.to_string()))
}
