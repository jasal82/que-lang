/// Config file parsing and manipulation for the Que language.
///
/// Supports JSON, YAML, and TOML formats with a universal path syntax
/// for navigating and modifying nested structures.
///
/// Path syntax:
///   "key"              — top-level key
///   "key1.key2"        — nested map key
///   "array[0]"         — array index
///   "array[0].key"     — array index then map key
///   "array[*].key"     — wildcard: collect from all array elements

use crate::value::Value;
use std::collections::BTreeMap;

// ── JSON ↔ Value ─────────────────────────────────────────────────────

fn json_to_value(j: serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            Value::List(arr.into_iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: BTreeMap<String, Value> = obj
                .into_iter()
                .map(|(k, v)| (k, json_to_value(v)))
                .collect();
            Value::Map(map)
        }
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        Value::String(s) | Value::Path(s) | Value::Glob(s)
        | Value::Regex(s) | Value::Semver(s) => serde_json::Value::String(s.clone()),
        // A secret serialised into a config file is a secret written to
        // disk, and `json.stringify(config)` is not where anyone decides to
        // do that. Callers who mean it write `.expose()`.
        Value::Secret(_) => serde_json::Value::String(crate::value::REDACTED.to_string()),
        Value::Stream(s) => serde_json::Value::String(s.materialize_eager().unwrap_or_default()),
        Value::List(items) => {
            serde_json::Value::Array(items.iter().map(value_to_json).collect())
        }
        Value::Set(items) => {
            serde_json::Value::Array(items.iter().map(value_to_json).collect())
        }
        Value::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Tuple(items) => {
            serde_json::Value::Array(items.iter().map(value_to_json).collect())
        }
        Value::Ok(inner) => value_to_json(inner),
        Value::Err(inner) => value_to_json(inner),
        _ => serde_json::Value::String(v.display_string()),
    }
}

// ── YAML ↔ Value ─────────────────────────────────────────────────────

fn yaml_to_value(y: serde_yaml::Value) -> Value {
    match y {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            Value::List(seq.into_iter().map(yaml_to_value).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let btree: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        other => format!("{:?}", other),
                    };
                    (key, yaml_to_value(v))
                })
                .collect();
            Value::Map(btree)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_value(tagged.value),
    }
}

fn value_to_yaml(v: &Value) -> serde_yaml::Value {
    match v {
        Value::Null => serde_yaml::Value::Null,
        Value::Bool(b) => serde_yaml::Value::Bool(*b),
        Value::Int(n) => serde_yaml::Value::Number(serde_yaml::Number::from(*n)),
        Value::Float(f) => {
            serde_yaml::Value::Number(serde_yaml::Number::from(*f))
        }
        Value::String(s) | Value::Path(s) | Value::Glob(s)
        | Value::Regex(s) | Value::Semver(s) => serde_yaml::Value::String(s.clone()),
        Value::Secret(_) => serde_yaml::Value::String(crate::value::REDACTED.to_string()),
        Value::Stream(s) => serde_yaml::Value::String(s.materialize_eager().unwrap_or_default()),
        Value::List(items) => {
            serde_yaml::Value::Sequence(items.iter().map(value_to_yaml).collect())
        }
        Value::Set(items) => {
            serde_yaml::Value::Sequence(items.iter().map(value_to_yaml).collect())
        }
        Value::Map(map) => {
            let mut m = serde_yaml::Mapping::new();
            for (k, v) in map {
                m.insert(
                    serde_yaml::Value::String(k.clone()),
                    value_to_yaml(v),
                );
            }
            serde_yaml::Value::Mapping(m)
        }
        Value::Tuple(items) => {
            serde_yaml::Value::Sequence(items.iter().map(value_to_yaml).collect())
        }
        Value::Ok(inner) => value_to_yaml(inner),
        Value::Err(inner) => value_to_yaml(inner),
        _ => serde_yaml::Value::String(v.display_string()),
    }
}

// ── TOML ↔ Value ─────────────────────────────────────────────────────

fn toml_to_value(t: toml::Value) -> Value {
    match t {
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Integer(n) => Value::Int(n),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::String(s) => Value::String(s),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            Value::List(arr.into_iter().map(toml_to_value).collect())
        }
        toml::Value::Table(table) => {
            let map: BTreeMap<String, Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_value(v)))
                .collect();
            Value::Map(map)
        }
    }
}

fn value_to_toml(v: &Value) -> Result<toml::Value, String> {
    match v {
        Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Value::Int(n) => Ok(toml::Value::Integer(*n)),
        Value::Float(f) => Ok(toml::Value::Float(*f)),
        Value::String(s) => Ok(toml::Value::String(s.clone())),
        Value::Path(s) | Value::Glob(s) | Value::Regex(s)
        | Value::Semver(s) => {
            Ok(toml::Value::String(s.clone()))
        }
        Value::Secret(_) => Ok(toml::Value::String(crate::value::REDACTED.to_string())),
        Value::Stream(s) => Ok(toml::Value::String(s.materialize_eager().unwrap_or_default())),
        Value::List(items) => {
            let arr: Result<Vec<toml::Value>, String> =
                items.iter().map(value_to_toml).collect();
            Ok(toml::Value::Array(arr?))
        }
        Value::Set(items) => {
            let arr: Result<Vec<toml::Value>, String> =
                items.iter().map(value_to_toml).collect();
            Ok(toml::Value::Array(arr?))
        }
        Value::Map(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k.clone(), value_to_toml(v)?);
            }
            Ok(toml::Value::Table(table))
        }
        Value::Tuple(items) => {
            let arr: Result<Vec<toml::Value>, String> =
                items.iter().map(value_to_toml).collect();
            Ok(toml::Value::Array(arr?))
        }
        Value::Null => {
            // TOML has no null — represent as empty string
            Ok(toml::Value::String(String::new()))
        }
        Value::Ok(inner) => value_to_toml(inner),
        Value::Err(inner) => value_to_toml(inner),
        _ => Ok(toml::Value::String(v.display_string())),
    }
}

// ── Public parse / serialize API ─────────────────────────────────────

pub fn parse_json(s: &str) -> Result<Value, String> {
    serde_json::from_str::<serde_json::Value>(s)
        .map(json_to_value)
        .map_err(|e| format!("JSON parse error: {}", e))
}

pub fn parse_yaml(s: &str) -> Result<Value, String> {
    serde_yaml::from_str::<serde_yaml::Value>(s)
        .map(yaml_to_value)
        .map_err(|e| format!("YAML parse error: {}", e))
}

pub fn parse_toml(s: &str) -> Result<Value, String> {
    s.parse::<toml::Value>()
        .map(toml_to_value)
        .map_err(|e| format!("TOML parse error: {}", e))
}

pub fn to_json(value: &Value, indent: Option<usize>) -> Result<String, String> {
    let jv = value_to_json(value);
    match indent {
        Some(_n) => {
            // serde_json::to_string_pretty uses 2-space indent by default.
            // For custom indent we post-process.
            let pretty = serde_json::to_string_pretty(&jv)
                .map_err(|e| format!("JSON serialize error: {}", e))?;
            if _n == 2 {
                Ok(pretty)
            } else {
                // Re-indent: replace leading groups of 2 spaces with n spaces
                let mut result = String::new();
                for line in pretty.lines() {
                    let trimmed = line.trim_start();
                    let leading_spaces = line.len() - trimmed.len();
                    let indent_level = leading_spaces / 2;
                    let new_indent = " ".repeat(indent_level * _n);
                    result.push_str(&new_indent);
                    result.push_str(trimmed);
                    result.push('\n');
                }
                // Remove trailing newline to match serde_json behavior
                if result.ends_with('\n') {
                    result.pop();
                }
                Ok(result)
            }
        }
        None => serde_json::to_string(&jv)
            .map_err(|e| format!("JSON serialize error: {}", e)),
    }
}

pub fn to_yaml(value: &Value) -> Result<String, String> {
    let yv = value_to_yaml(value);
    serde_yaml::to_string(&yv)
        .map_err(|e| format!("YAML serialize error: {}", e))
}

pub fn to_toml(value: &Value) -> Result<String, String> {
    let tv = value_to_toml(value)?;
    toml::to_string_pretty(&tv)
        .map_err(|e| format!("TOML serialize error: {}", e))
}

// ── Order-preserving serialization (for edit functions) ──────────────
//
// When editing a config file, we want the output key order to match the
// original file.  The `*_ordered` helpers convert our `Value` to the
// format-native type, inserting keys in the order they appear in a
// "template" parsed from the original content.  New keys are appended.

fn value_to_json_ordered(v: &Value, template: &serde_json::Value) -> serde_json::Value {
    match (v, template) {
        (Value::Map(map), serde_json::Value::Object(orig_obj)) => {
            let mut obj = serde_json::Map::new();
            // Keys that existed in original — preserve their order
            for (k, orig_v) in orig_obj {
                if let Some(new_v) = map.get(k) {
                    obj.insert(k.clone(), value_to_json_ordered(new_v, orig_v));
                }
            }
            // New keys — append at end
            for (k, new_v) in map {
                if !orig_obj.contains_key(k) {
                    obj.insert(k.clone(), value_to_json(new_v));
                }
            }
            serde_json::Value::Object(obj)
        }
        (Value::List(items), serde_json::Value::Array(orig_arr)) => {
            serde_json::Value::Array(
                items.iter().enumerate().map(|(i, item)| {
                    if let Some(orig_item) = orig_arr.get(i) {
                        value_to_json_ordered(item, orig_item)
                    } else {
                        value_to_json(item)
                    }
                }).collect()
            )
        }
        _ => value_to_json(v),
    }
}

/// Recursively apply changes from a Que `Value` onto a `toml_edit::Item`,
/// preserving the original formatting (inline tables stay inline, etc.).
fn apply_value_to_toml_item(item: &mut toml_edit::Item, value: &Value) {
    match value {
        Value::Bool(b) => {
            if let Some(v) = item.as_value_mut() {
                *v = (*b).into();
            } else {
                *item = toml_edit::Item::Value((*b).into());
            }
        }
        Value::Int(n) => {
            if let Some(v) = item.as_value_mut() {
                *v = (*n).into();
            } else {
                *item = toml_edit::Item::Value((*n).into());
            }
        }
        Value::Float(f) => {
            if let Some(v) = item.as_value_mut() {
                *v = (*f).into();
            } else {
                *item = toml_edit::Item::Value((*f).into());
            }
        }
        Value::String(s) | Value::Path(s) | Value::Glob(s)
        | Value::Regex(s) | Value::Semver(s) => {
            if let Some(v) = item.as_value_mut() {
                *v = s.as_str().into();
            } else {
                *item = toml_edit::Item::Value(s.as_str().into());
            }
        }
        Value::Secret(_) => {
            let r = crate::value::REDACTED;
            if let Some(v) = item.as_value_mut() {
                *v = r.into();
            } else {
                *item = toml_edit::Item::Value(r.into());
            }
        }
        Value::Stream(s) => {
            let buf = s.materialize_eager().unwrap_or_default();
            if let Some(v) = item.as_value_mut() {
                *v = buf.as_str().into();
            } else {
                *item = toml_edit::Item::Value(buf.as_str().into());
            }
        }
        Value::Map(map) => {
            // If the item is already a table, update it in place
            if let Some(table) = item.as_table_mut() {
                // Remove keys that no longer exist
                let existing: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
                for key in &existing {
                    if !map.contains_key(key) {
                        table.remove(key);
                    }
                }
                // Update existing and add new keys
                for (k, v) in map {
                    if let Some(child) = table.get_mut(k) {
                        apply_value_to_toml_item(child, v);
                    } else {
                        table.insert(k, que_value_to_toml_item(v));
                    }
                }
            } else if let Some(v) = item.as_value_mut() {
                // Inline table
                if let Some(inline) = v.as_inline_table_mut() {
                    let existing: Vec<String> = inline.iter().map(|(k, _)| k.to_string()).collect();
                    for key in &existing {
                        if !map.contains_key(key) {
                            inline.remove(key);
                        }
                    }
                    for (k, val) in map {
                        if let Some(child) = inline.get_mut(k) {
                            apply_value_to_toml_value(child, val);
                        } else {
                            inline.insert(k, que_value_to_toml_edit_value(val));
                        }
                    }
                } else {
                    *item = que_value_to_toml_item(value);
                }
            } else {
                *item = que_value_to_toml_item(value);
            }
        }
        Value::List(items) | Value::Tuple(items) => {
            // Handle array-of-tables ([[section]])
            if let Some(aot) = item.as_array_of_tables_mut() {
                for (i, val) in items.iter().enumerate() {
                    if let Value::Map(map) = val {
                        if let Some(table) = aot.get_mut(i) {
                            let existing: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
                            for key in &existing {
                                if !map.contains_key(key) {
                                    table.remove(key);
                                }
                            }
                            for (k, v) in map {
                                if let Some(child) = table.get_mut(k) {
                                    apply_value_to_toml_item(child, v);
                                } else {
                                    table.insert(k, que_value_to_toml_item(v));
                                }
                            }
                        }
                    }
                }
            } else if let Some(v) = item.as_value_mut() {
                if let Some(arr) = v.as_array_mut() {
                    arr.clear();
                    for val in items {
                        arr.push_formatted(que_value_to_toml_edit_value(val));
                    }
                } else {
                    *item = que_value_to_toml_item(value);
                }
            } else {
                *item = que_value_to_toml_item(value);
            }
        }
        Value::Null => {
            if let Some(v) = item.as_value_mut() {
                *v = "".into();
            } else {
                *item = toml_edit::Item::Value("".into());
            }
        }
        _ => {
            let s = value.display_string();
            if let Some(v) = item.as_value_mut() {
                *v = s.as_str().into();
            } else {
                *item = toml_edit::Item::Value(s.as_str().into());
            }
        }
    }
}

/// Apply a Que Value onto a toml_edit::Value (for inline table children).
fn apply_value_to_toml_value(target: &mut toml_edit::Value, value: &Value) {
    match value {
        Value::Bool(b) => *target = (*b).into(),
        Value::Int(n) => *target = (*n).into(),
        Value::Float(f) => *target = (*f).into(),
        Value::String(s) | Value::Path(s) | Value::Glob(s)
        | Value::Regex(s) | Value::Semver(s) => *target = s.as_str().into(),
        Value::Secret(_) => *target = crate::value::REDACTED.into(),
        Value::Stream(s) => *target = s.materialize_eager().unwrap_or_default().as_str().into(),
        Value::Map(map) => {
            if let Some(inline) = target.as_inline_table_mut() {
                let existing: Vec<String> = inline.iter().map(|(k, _)| k.to_string()).collect();
                for key in &existing {
                    if !map.contains_key(key) {
                        inline.remove(key);
                    }
                }
                for (k, val) in map {
                    if let Some(child) = inline.get_mut(k) {
                        apply_value_to_toml_value(child, val);
                    } else {
                        inline.insert(k, que_value_to_toml_edit_value(val));
                    }
                }
            } else {
                *target = que_value_to_toml_edit_value(value);
            }
        }
        Value::List(items) | Value::Tuple(items) => {
            if let Some(arr) = target.as_array_mut() {
                arr.clear();
                for val in items {
                    arr.push_formatted(que_value_to_toml_edit_value(val));
                }
            } else {
                *target = que_value_to_toml_edit_value(value);
            }
        }
        _ => *target = value.display_string().as_str().into(),
    }
}

/// Convert a Que Value to a toml_edit::Item (for new keys).
fn que_value_to_toml_item(value: &Value) -> toml_edit::Item {
    toml_edit::Item::Value(que_value_to_toml_edit_value(value))
}

/// Convert a Que Value to a toml_edit::Value.
fn que_value_to_toml_edit_value(value: &Value) -> toml_edit::Value {
    match value {
        Value::Bool(b) => (*b).into(),
        Value::Int(n) => (*n).into(),
        Value::Float(f) => (*f).into(),
        Value::String(s) | Value::Path(s) | Value::Glob(s)
        | Value::Regex(s) | Value::Semver(s) => s.as_str().into(),
        Value::Secret(_) => crate::value::REDACTED.into(),
        Value::Stream(s) => s.materialize_eager().unwrap_or_default().as_str().into(),
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            let mut arr = toml_edit::Array::new();
            for v in items {
                arr.push_formatted(que_value_to_toml_edit_value(v));
            }
            toml_edit::Value::Array(arr)
        }
        Value::Map(map) => {
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in map {
                inline.insert(k, que_value_to_toml_edit_value(v));
            }
            toml_edit::Value::InlineTable(inline)
        }
        Value::Null => "".into(),
        _ => value.display_string().as_str().into(),
    }
}

fn value_to_yaml_ordered(v: &Value, template: &serde_yaml::Value) -> serde_yaml::Value {
    match (v, template) {
        (Value::Map(map), serde_yaml::Value::Mapping(orig_map)) => {
            let mut m = serde_yaml::Mapping::new();
            for (orig_k, orig_v) in orig_map {
                if let serde_yaml::Value::String(k) = orig_k {
                    if let Some(new_v) = map.get(k) {
                        m.insert(
                            serde_yaml::Value::String(k.clone()),
                            value_to_yaml_ordered(new_v, orig_v),
                        );
                    }
                }
            }
            for (k, new_v) in map {
                let yaml_key = serde_yaml::Value::String(k.clone());
                if !orig_map.contains_key(&yaml_key) {
                    m.insert(yaml_key, value_to_yaml(new_v));
                }
            }
            serde_yaml::Value::Mapping(m)
        }
        (Value::List(items), serde_yaml::Value::Sequence(orig_seq)) => {
            serde_yaml::Value::Sequence(
                items.iter().enumerate().map(|(i, item)| {
                    if let Some(orig_item) = orig_seq.get(i) {
                        value_to_yaml_ordered(item, orig_item)
                    } else {
                        value_to_yaml(item)
                    }
                }).collect()
            )
        }
        _ => value_to_yaml(v),
    }
}

/// Serialize to JSON, preserving key order from the original content.
pub fn to_json_ordered(value: &Value, original: &str, indent: Option<usize>) -> Result<String, String> {
    let orig_jv: serde_json::Value = serde_json::from_str(original)
        .map_err(|e| format!("JSON parse error: {}", e))?;
    let jv = value_to_json_ordered(value, &orig_jv);
    match indent {
        Some(n) => {
            let pretty = serde_json::to_string_pretty(&jv)
                .map_err(|e| format!("JSON serialize error: {}", e))?;
            if n == 2 {
                Ok(pretty)
            } else {
                let mut result = String::new();
                for line in pretty.lines() {
                    let trimmed = line.trim_start();
                    let leading_spaces = line.len() - trimmed.len();
                    let indent_level = leading_spaces / 2;
                    let new_indent = " ".repeat(indent_level * n);
                    result.push_str(&new_indent);
                    result.push_str(trimmed);
                    result.push('\n');
                }
                if result.ends_with('\n') {
                    result.pop();
                }
                Ok(result)
            }
        }
        None => serde_json::to_string(&jv)
            .map_err(|e| format!("JSON serialize error: {}", e)),
    }
}

/// Serialize to TOML, preserving formatting from the original content.
/// Uses `toml_edit` to keep inline tables, comments, and key order intact.
pub fn to_toml_ordered(value: &Value, original: &str) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = original.parse()
        .map_err(|e| format!("TOML parse error: {}", e))?;
    if let Value::Map(map) = value {
        let table = doc.as_table_mut();
        // Remove keys that no longer exist
        let existing: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
        for key in &existing {
            if !map.contains_key(key) {
                table.remove(key);
            }
        }
        // Update existing and add new keys
        for (k, v) in map {
            if let Some(child) = table.get_mut(k) {
                apply_value_to_toml_item(child, v);
            } else {
                table.insert(k, que_value_to_toml_item(v));
            }
        }
    }
    Ok(doc.to_string())
}

/// Serialize to YAML, preserving key order from the original content.
pub fn to_yaml_ordered(value: &Value, original: &str) -> Result<String, String> {
    let orig_yv: serde_yaml::Value = serde_yaml::from_str(original)
        .map_err(|e| format!("YAML parse error: {}", e))?;
    let yv = value_to_yaml_ordered(value, &orig_yv);
    serde_yaml::to_string(&yv)
        .map_err(|e| format!("YAML serialize error: {}", e))
}

// ── Config path syntax ───────────────────────────────────────────────

/// A parsed segment of a config path.
#[derive(Debug, Clone)]
enum PathSegment {
    /// Map key: `"foo"`
    Key(String),
    /// Array index: `[0]`, `[3]`
    Index(usize),
    /// Wildcard: `[*]`
    Wildcard,
}

/// Parse a config path like `"database.host"` or `"servers[0].name"` or `"items[*].id"`.
fn parse_config_path(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '.' => {
                if !current.is_empty() {
                    segments.push(PathSegment::Key(current.clone()));
                    current.clear();
                }
                i += 1;
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(PathSegment::Key(current.clone()));
                    current.clear();
                }
                i += 1; // skip '['
                if i < chars.len() && chars[i] == '*' {
                    segments.push(PathSegment::Wildcard);
                    i += 1; // skip '*'
                    if i < chars.len() && chars[i] == ']' {
                        i += 1; // skip ']'
                    }
                } else {
                    let mut num = String::new();
                    while i < chars.len() && chars[i] != ']' {
                        num.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1; // skip ']'
                    }
                    if let Ok(idx) = num.parse::<usize>() {
                        segments.push(PathSegment::Index(idx));
                    } else {
                        // Treat as string key inside brackets (e.g. ["key with spaces"])
                        let key = num.trim_matches('"').trim_matches('\'').to_string();
                        segments.push(PathSegment::Key(key));
                    }
                }
            }
            c => {
                current.push(c);
                i += 1;
            }
        }
    }
    if !current.is_empty() {
        segments.push(PathSegment::Key(current));
    }

    segments
}

/// Navigate into a value following the given path segments.
fn get_at_path(value: &Value, segments: &[PathSegment]) -> Value {
    if segments.is_empty() {
        return value.clone();
    }

    let (head, tail) = segments.split_first().unwrap();
    match (head, value) {
        (PathSegment::Key(key), Value::Map(map)) => {
            match map.get(key) {
                Some(child) => get_at_path(child, tail),
                None => Value::Null,
            }
        }
        (PathSegment::Index(idx), Value::List(items)) => {
            match items.get(*idx) {
                Some(child) => get_at_path(child, tail),
                None => Value::Null,
            }
        }
        (PathSegment::Wildcard, Value::List(items)) => {
            let results: Vec<Value> = items
                .iter()
                .map(|item| get_at_path(item, tail))
                .filter(|v| !matches!(v, Value::Null))
                .collect();
            Value::List(results)
        }
        _ => Value::Null,
    }
}

/// Set a value at the given path, returning a new value with the modification.
fn set_at_path(value: &Value, segments: &[PathSegment], new_val: Value) -> Value {
    if segments.is_empty() {
        return new_val;
    }

    let (head, tail) = segments.split_first().unwrap();
    match (head, value) {
        (PathSegment::Key(key), Value::Map(map)) => {
            let mut new_map = map.clone();
            let child = map.get(key).cloned().unwrap_or(
                if tail.is_empty() {
                    Value::Null
                } else {
                    // Auto-create intermediate structure based on next segment
                    match tail.first() {
                        Some(PathSegment::Key(_)) => Value::Map(BTreeMap::new()),
                        Some(PathSegment::Index(_)) | Some(PathSegment::Wildcard) => Value::List(Vec::new()),
                        None => Value::Null,
                    }
                }
            );
            new_map.insert(key.clone(), set_at_path(&child, tail, new_val));
            Value::Map(new_map)
        }
        (PathSegment::Key(key), _) => {
            // Current value is not a map — create one
            let mut new_map = BTreeMap::new();
            let child = match tail.first() {
                Some(PathSegment::Key(_)) => Value::Map(BTreeMap::new()),
                Some(PathSegment::Index(_)) | Some(PathSegment::Wildcard) => Value::List(Vec::new()),
                None => Value::Null,
            };
            new_map.insert(key.clone(), set_at_path(&child, tail, new_val));
            Value::Map(new_map)
        }
        (PathSegment::Index(idx), Value::List(items)) => {
            let mut new_items = items.clone();
            // Extend the list if needed
            while new_items.len() <= *idx {
                new_items.push(Value::Null);
            }
            new_items[*idx] = set_at_path(&new_items[*idx], tail, new_val.clone());
            Value::List(new_items)
        }
        (PathSegment::Index(idx), _) => {
            // Current value is not a list — create one
            let mut new_items = Vec::new();
            while new_items.len() <= *idx {
                new_items.push(Value::Null);
            }
            let child = match tail.first() {
                Some(PathSegment::Key(_)) => Value::Map(BTreeMap::new()),
                Some(PathSegment::Index(_)) | Some(PathSegment::Wildcard) => Value::List(Vec::new()),
                None => Value::Null,
            };
            new_items[*idx] = set_at_path(&child, tail, new_val);
            Value::List(new_items)
        }
        (PathSegment::Wildcard, Value::List(items)) => {
            // Set on all elements
            let new_items: Vec<Value> = items
                .iter()
                .map(|item| set_at_path(item, tail, new_val.clone()))
                .collect();
            Value::List(new_items)
        }
        _ => value.clone(),
    }
}

/// Delete a value at the given path, returning a new value without it.
fn delete_at_path(value: &Value, segments: &[PathSegment]) -> Value {
    if segments.is_empty() {
        return Value::Null;
    }

    if segments.len() == 1 {
        // Terminal segment — remove it
        match (&segments[0], value) {
            (PathSegment::Key(key), Value::Map(map)) => {
                let mut new_map = map.clone();
                new_map.remove(key);
                Value::Map(new_map)
            }
            (PathSegment::Index(idx), Value::List(items)) => {
                let mut new_items = items.clone();
                if *idx < new_items.len() {
                    new_items.remove(*idx);
                }
                Value::List(new_items)
            }
            (PathSegment::Wildcard, Value::List(_)) => {
                Value::List(Vec::new())
            }
            _ => value.clone(),
        }
    } else {
        let (head, tail) = segments.split_first().unwrap();
        match (head, value) {
            (PathSegment::Key(key), Value::Map(map)) => {
                if let Some(child) = map.get(key) {
                    let mut new_map = map.clone();
                    new_map.insert(key.clone(), delete_at_path(child, tail));
                    Value::Map(new_map)
                } else {
                    value.clone()
                }
            }
            (PathSegment::Index(idx), Value::List(items)) => {
                if let Some(child) = items.get(*idx) {
                    let mut new_items = items.clone();
                    new_items[*idx] = delete_at_path(child, tail);
                    Value::List(new_items)
                } else {
                    value.clone()
                }
            }
            (PathSegment::Wildcard, Value::List(items)) => {
                let new_items: Vec<Value> = items
                    .iter()
                    .map(|item| delete_at_path(item, tail))
                    .collect();
                Value::List(new_items)
            }
            _ => value.clone(),
        }
    }
}

/// Check if a value exists at the given path.
fn has_at_path(value: &Value, segments: &[PathSegment]) -> bool {
    if segments.is_empty() {
        return true;
    }

    let (head, tail) = segments.split_first().unwrap();
    match (head, value) {
        (PathSegment::Key(key), Value::Map(map)) => {
            match map.get(key) {
                Some(child) => has_at_path(child, tail),
                None => false,
            }
        }
        (PathSegment::Index(idx), Value::List(items)) => {
            match items.get(*idx) {
                Some(child) => has_at_path(child, tail),
                None => false,
            }
        }
        (PathSegment::Wildcard, Value::List(items)) => {
            items.iter().any(|item| has_at_path(item, tail))
        }
        _ => false,
    }
}

// ── Public path operations ───────────────────────────────────────────

/// Get a value at a config path.
pub fn config_get(value: &Value, path: &str) -> Value {
    let segments = parse_config_path(path);
    get_at_path(value, &segments)
}

/// Set a value at a config path, returning a new value.
pub fn config_set(value: &Value, path: &str, new_val: Value) -> Value {
    let segments = parse_config_path(path);
    set_at_path(value, &segments, new_val)
}

/// Delete a value at a config path, returning a new value.
pub fn config_delete(value: &Value, path: &str) -> Value {
    let segments = parse_config_path(path);
    delete_at_path(value, &segments)
}

/// Check whether a config path exists in a value.
pub fn config_has(value: &Value, path: &str) -> bool {
    let segments = parse_config_path(path);
    has_at_path(value, &segments)
}

/// Deep-merge two maps. The overlay takes priority for leaf values.
/// When both base and overlay have a map at the same key, they are merged recursively.
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Map(base_map), Value::Map(overlay_map)) => {
            let mut result = base_map.clone();
            for (k, v) in overlay_map {
                if let Some(base_v) = result.get(k) {
                    result.insert(k.clone(), deep_merge(base_v, v));
                } else {
                    result.insert(k.clone(), v.clone());
                }
            }
            Value::Map(result)
        }
        // For non-map values, overlay wins
        (_, overlay) => overlay.clone(),
    }
}

// ── Auto-detect format from file extension ───────────────────────────

/// Detect config format from file extension.
fn detect_format(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".json") {
        Some("json")
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        Some("yaml")
    } else if lower.ends_with(".toml") {
        Some("toml")
    } else {
        None
    }
}

/// Read a config file, auto-detecting format from extension.
pub fn config_read(path: &str) -> Result<Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path, e))?;

    match detect_format(path) {
        Some("json") => parse_json(&content),
        Some("yaml") => parse_yaml(&content),
        Some("toml") => parse_toml(&content),
        _ => {
            // Try each format in order
            parse_json(&content)
                .or_else(|_| parse_toml(&content))
                .or_else(|_| parse_yaml(&content))
                .map_err(|_| format!(
                    "cannot determine config format for '{}'; use .json, .yaml, or .toml extension",
                    path,
                ))
        }
    }
}

/// Write a config file, auto-detecting format from extension.
pub fn config_write(path: &str, value: &Value, indent: Option<usize>) -> Result<(), String> {
    let content = match detect_format(path) {
        Some("json") => to_json(value, indent.or(Some(2)))?,
        Some("yaml") => to_yaml(value)?,
        Some("toml") => to_toml(value)?,
        _ => {
            // Default to JSON with 2-space indent
            to_json(value, indent.or(Some(2)))?
        }
    };

    std::fs::write(path, &content)
        .map_err(|e| format!("cannot write '{}': {}", path, e))
}

// ── List available config path keys ──────────────────────────────────

/// Collect all leaf paths from a value (useful for introspection).
pub fn config_paths(value: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_paths(value, &mut String::new(), &mut paths);
    paths
}

fn collect_paths(value: &Value, prefix: &mut String, paths: &mut Vec<String>) {
    match value {
        Value::Map(map) => {
            for (key, child) in map {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                collect_paths(child, &mut new_prefix.clone(), paths);
            }
        }
        Value::List(items) => {
            for (i, child) in items.iter().enumerate() {
                let new_prefix = format!("{}[{}]", prefix, i);
                collect_paths(child, &mut new_prefix.clone(), paths);
            }
        }
        _ => {
            if !prefix.is_empty() {
                paths.push(prefix.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_path_simple() {
        let segs = parse_config_path("database.host");
        assert_eq!(segs.len(), 2);
        assert!(matches!(&segs[0], PathSegment::Key(k) if k == "database"));
        assert!(matches!(&segs[1], PathSegment::Key(k) if k == "host"));
    }

    #[test]
    fn test_parse_config_path_array() {
        let segs = parse_config_path("servers[0].name");
        assert_eq!(segs.len(), 3);
        assert!(matches!(&segs[0], PathSegment::Key(k) if k == "servers"));
        assert!(matches!(&segs[1], PathSegment::Index(0)));
        assert!(matches!(&segs[2], PathSegment::Key(k) if k == "name"));
    }

    #[test]
    fn test_parse_config_path_wildcard() {
        let segs = parse_config_path("items[*].id");
        assert_eq!(segs.len(), 3);
        assert!(matches!(&segs[0], PathSegment::Key(k) if k == "items"));
        assert!(matches!(&segs[1], PathSegment::Wildcard));
        assert!(matches!(&segs[2], PathSegment::Key(k) if k == "id"));
    }

    #[test]
    fn test_json_roundtrip() {
        let json = r#"{"name":"wisp","version":1,"tags":["build","devops"]}"#;
        let val = parse_json(json).unwrap();
        let back = to_json(&val, None).unwrap();
        let val2 = parse_json(&back).unwrap();
        assert_eq!(val, val2);
    }

    #[test]
    fn test_yaml_roundtrip() {
        let yaml = "name: wisp\nversion: 1\ntags:\n  - build\n  - devops\n";
        let val = parse_yaml(yaml).unwrap();
        assert_eq!(config_get(&val, "name"), Value::String("wisp".into()));
        assert_eq!(config_get(&val, "version"), Value::Int(1));
    }

    #[test]
    fn test_toml_roundtrip() {
        let toml_str = r#"
name = "wisp"
version = 1

[database]
host = "localhost"
port = 5432
"#;
        let val = parse_toml(toml_str).unwrap();
        assert_eq!(config_get(&val, "name"), Value::String("wisp".into()));
        assert_eq!(config_get(&val, "database.host"), Value::String("localhost".into()));
        assert_eq!(config_get(&val, "database.port"), Value::Int(5432));
    }

    #[test]
    fn test_config_get_nested() {
        let json = r#"{"a":{"b":{"c":42}}}"#;
        let val = parse_json(json).unwrap();
        assert_eq!(config_get(&val, "a.b.c"), Value::Int(42));
        assert_eq!(config_get(&val, "a.b.missing"), Value::Null);
    }

    #[test]
    fn test_config_get_array() {
        let json = r#"{"items":[{"id":1},{"id":2},{"id":3}]}"#;
        let val = parse_json(json).unwrap();
        assert_eq!(config_get(&val, "items[0].id"), Value::Int(1));
        assert_eq!(config_get(&val, "items[2].id"), Value::Int(3));
    }

    #[test]
    fn test_config_get_wildcard() {
        let json = r#"{"items":[{"id":1},{"id":2},{"id":3}]}"#;
        let val = parse_json(json).unwrap();
        let result = config_get(&val, "items[*].id");
        assert_eq!(result, Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
    }

    #[test]
    fn test_config_set() {
        let json = r#"{"database":{"host":"localhost","port":5432}}"#;
        let val = parse_json(json).unwrap();
        let updated = config_set(&val, "database.port", Value::Int(3306));
        assert_eq!(config_get(&updated, "database.port"), Value::Int(3306));
        assert_eq!(config_get(&updated, "database.host"), Value::String("localhost".into()));
    }

    #[test]
    fn test_config_set_creates_path() {
        let val = Value::Map(BTreeMap::new());
        let updated = config_set(&val, "new.nested.key", Value::String("value".into()));
        assert_eq!(config_get(&updated, "new.nested.key"), Value::String("value".into()));
    }

    #[test]
    fn test_config_delete() {
        let json = r#"{"a":1,"b":2,"c":3}"#;
        let val = parse_json(json).unwrap();
        let updated = config_delete(&val, "b");
        assert!(!config_has(&updated, "b"));
        assert!(config_has(&updated, "a"));
        assert!(config_has(&updated, "c"));
    }

    #[test]
    fn test_config_has() {
        let json = r#"{"database":{"host":"localhost"}}"#;
        let val = parse_json(json).unwrap();
        assert!(config_has(&val, "database.host"));
        assert!(config_has(&val, "database"));
        assert!(!config_has(&val, "database.port"));
        assert!(!config_has(&val, "missing"));
    }

    #[test]
    fn test_deep_merge() {
        let base = parse_json(r#"{"a":1,"nested":{"x":10,"y":20}}"#).unwrap();
        let overlay = parse_json(r#"{"b":2,"nested":{"y":99,"z":30}}"#).unwrap();
        let merged = deep_merge(&base, &overlay);
        assert_eq!(config_get(&merged, "a"), Value::Int(1));
        assert_eq!(config_get(&merged, "b"), Value::Int(2));
        assert_eq!(config_get(&merged, "nested.x"), Value::Int(10));
        assert_eq!(config_get(&merged, "nested.y"), Value::Int(99));
        assert_eq!(config_get(&merged, "nested.z"), Value::Int(30));
    }

    #[test]
    fn test_config_paths() {
        let json = r#"{"a":1,"b":{"c":2,"d":[3,4]}}"#;
        let val = parse_json(json).unwrap();
        let paths = config_paths(&val);
        assert!(paths.contains(&"a".to_string()));
        assert!(paths.contains(&"b.c".to_string()));
        assert!(paths.contains(&"b.d[0]".to_string()));
        assert!(paths.contains(&"b.d[1]".to_string()));
    }
}
