//! Pattern matching for the Que interpreter.

use super::helpers::glob_matches;
use super::Interpreter;
use crate::ast::*;
use crate::error::*;
use crate::value::Value;

use std::collections::BTreeMap;

impl Interpreter {
    // ── Pattern matching ─────────────────────────────────────────────

    pub(crate) fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Value,
    ) -> Option<Vec<(String, Value)>> {
        match pattern {
            Pattern::Wildcard => Some(vec![]),
            Pattern::Ident(name) => Some(vec![(name.clone(), value.clone())]),
            Pattern::IntLit(n) => {
                if let Value::Int(v) = value {
                    if v == n {
                        Some(vec![])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Pattern::FloatLit(n) => {
                if let Value::Float(v) = value {
                    if (v - n).abs() < f64::EPSILON {
                        Some(vec![])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Pattern::StringLit(s) => {
                if let Value::String(v) = value {
                    if v == s {
                        Some(vec![])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Pattern::BoolLit(b) => {
                if let Value::Bool(v) = value {
                    if v == b {
                        Some(vec![])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Pattern::NullLit => {
                if matches!(value, Value::Null) {
                    Some(vec![])
                } else {
                    None
                }
            }
            Pattern::Glob(pattern) => {
                // Match glob pattern against strings and paths
                let s = match value {
                    Value::String(s) => s.as_str(),
                    Value::Path(p) => p.as_str(),
                    _ => return None,
                };
                if glob_matches(pattern, s) {
                    Some(vec![])
                } else {
                    None
                }
            }
            Pattern::List(patterns, rest) => {
                if let Value::List(items) = value {
                    if rest.is_some() {
                        if items.len() < patterns.len() {
                            return None;
                        }
                    } else if items.len() != patterns.len() {
                        return None;
                    }
                    let mut bindings = Vec::new();
                    for (pat, val) in patterns.iter().zip(items.iter()) {
                        bindings.extend(self.match_pattern(pat, val)?);
                    }
                    if let Some(rest_pat) = rest {
                        let rest_val =
                            Value::List(items[patterns.len()..].to_vec());
                        bindings.extend(self.match_pattern(rest_pat, &rest_val)?);
                    }
                    Some(bindings)
                } else {
                    None
                }
            }
            Pattern::Tuple(patterns) => {
                if let Value::Tuple(items) = value {
                    if items.len() != patterns.len() {
                        return None;
                    }
                    let mut bindings = Vec::new();
                    for (pat, val) in patterns.iter().zip(items.iter()) {
                        bindings.extend(self.match_pattern(pat, val)?);
                    }
                    Some(bindings)
                } else {
                    None
                }
            }
            Pattern::Struct(fields, rest) => {
                if let Value::Map(map) = value {
                    let mut bindings = Vec::new();
                    let mut matched_keys = std::collections::HashSet::new();
                    for (name, inner_pat) in fields {
                        let val = map.get(name.as_str())?;
                        matched_keys.insert(name.clone());
                        if let Some(pat) = inner_pat {
                            bindings.extend(self.match_pattern(pat, val)?);
                        } else {
                            bindings.push((name.clone(), val.clone()));
                        }
                    }
                    if let Some(rest_name) = rest {
                        let rest_map: BTreeMap<String, Value> = map
                            .iter()
                            .filter(|(k, _)| !matched_keys.contains(*k))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        bindings.push((rest_name.clone(), Value::Map(rest_map)));
                    }
                    Some(bindings)
                } else {
                    None
                }
            }
            Pattern::Instance(enum_name, type_name, field_pats, rest) => {
                // Match struct instances
                if let Value::Instance { type_name: inst_type, fields } = value {
                    if enum_name.is_some() || inst_type != type_name {
                        return None;
                    }
                    let mut bindings = Vec::new();
                    let mut matched_keys = std::collections::HashSet::new();
                    for (name, inner_pat) in field_pats {
                        let val = fields.get(name.as_str())?;
                        matched_keys.insert(name.clone());
                        if let Some(pat) = inner_pat {
                            bindings.extend(self.match_pattern(pat, val)?);
                        } else {
                            bindings.push((name.clone(), val.clone()));
                        }
                    }
                    if let Some(rest_name) = rest {
                        let rest_map: BTreeMap<String, Value> = fields
                            .iter()
                            .filter(|(k, _)| !matched_keys.contains(*k))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        bindings.push((rest_name.clone(), Value::Map(rest_map)));
                    }
                    return Some(bindings);
                }
                // Also match enum variants by variant name: Circle { radius } matches Value::Enum { variant: "Circle", .. }
                if let Value::Enum { enum_name: actual_enum_name, variant, fields } = value {
                    if variant != type_name {
                        return None;
                    }
                    if enum_name.as_ref().is_some_and(|expected| expected != actual_enum_name) {
                        return None;
                    }
                    let mut bindings = Vec::new();
                    let mut matched_keys = std::collections::HashSet::new();
                    for (name, inner_pat) in field_pats {
                        let val = fields.get(name.as_str())?;
                        matched_keys.insert(name.clone());
                        if let Some(pat) = inner_pat {
                            bindings.extend(self.match_pattern(pat, val)?);
                        } else {
                            bindings.push((name.clone(), val.clone()));
                        }
                    }
                    if let Some(rest_name) = rest {
                        let rest_map: BTreeMap<String, Value> = fields
                            .iter()
                            .filter(|(k, _)| !matched_keys.contains(*k))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        bindings.push((rest_name.clone(), Value::Map(rest_map)));
                    }
                    return Some(bindings);
                }
                None
            }
            Pattern::Enum(enum_name, name, fields) => {
                // Built-in result/option wrappers
                match (enum_name.as_deref(), name.as_str(), value) {
                    (None | Some("Result"), "Ok", Value::Ok(inner)) => {
                        if fields.len() == 1 {
                            return self.match_pattern(&fields[0], inner);
                        } else if fields.is_empty() {
                            return Some(vec![]);
                        } else {
                            return None;
                        }
                    }
                    (None | Some("Result"), "Err", Value::Err(inner)) => {
                        if fields.len() == 1 {
                            return self.match_pattern(&fields[0], inner);
                        } else if fields.is_empty() {
                            return Some(vec![]);
                        } else {
                            return None;
                        }
                    }
                    _ => {}
                }
                // User-defined enum: Variant(field1, field2, ...) — positional destructuring
                if let Value::Enum { enum_name: actual_enum_name, variant, fields: enum_fields } = value {
                    if variant != name {
                        return None;
                    }
                    if enum_name.as_ref().is_some_and(|expected| expected != actual_enum_name) {
                        return None;
                    }
                    // Get ordered field values for positional matching
                    let field_vals: Vec<Value> = enum_fields.values().cloned().collect();
                    if fields.len() == 0 {
                        return Some(vec![]);
                    }
                    if fields.len() != field_vals.len() {
                        return None;
                    }
                    let mut bindings = Vec::new();
                    for (pat, val) in fields.iter().zip(field_vals.iter()) {
                        bindings.extend(self.match_pattern(pat, val)?);
                    }
                    return Some(bindings);
                }
                None
            }
            Pattern::Range(start, end, inclusive) => {
                if let Value::Int(v) = value {
                    let start_ok = match start {
                        Some(s) => {
                            if let Pattern::IntLit(n) = s.as_ref() {
                                v >= n
                            } else {
                                true
                            }
                        }
                        None => true,
                    };
                    let end_ok = match end {
                        Some(e) => {
                            if let Pattern::IntLit(n) = e.as_ref() {
                                if *inclusive {
                                    v <= n
                                } else {
                                    v < n
                                }
                            } else {
                                true
                            }
                        }
                        None => true,
                    };
                    if start_ok && end_ok {
                        Some(vec![])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Pattern::Or(alternatives) => {
                for alt in alternatives {
                    if let Some(bindings) = self.match_pattern(alt, value) {
                        return Some(bindings);
                    }
                }
                None
            }
            Pattern::Binding(name, inner) => {
                if let Some(mut bindings) = self.match_pattern(inner, value) {
                    bindings.push((name.clone(), value.clone()));
                    Some(bindings)
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        mutable: bool,
    ) -> Result<(), Signal> {
        match pattern {
            Pattern::Wildcard => Ok(()),
            Pattern::Ident(name) => {
                self.env.define(name, value, mutable);
                Ok(())
            }
            Pattern::List(patterns, rest) => {
                if let Value::List(items) = value {
                    if rest.is_some() {
                        if items.len() < patterns.len() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                "list pattern mismatch: too few elements",
                            )));
                        }
                    } else if items.len() != patterns.len() {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            format!(
                                "list pattern mismatch: expected {} elements, got {}",
                                patterns.len(),
                                items.len()
                            ),
                        )));
                    }
                    let mut items = items;
                    for pat in patterns {
                        let val = items.remove(0);
                        self.bind_pattern(pat, val, mutable)?;
                    }
                    if let Some(rest_pat) = rest {
                        self.bind_pattern(rest_pat, Value::List(items), mutable)?;
                    }
                    Ok(())
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "cannot destructure non-list with list pattern",
                    )))
                }
            }
            Pattern::Tuple(patterns) => {
                if let Value::Tuple(items) = value {
                    if items.len() != patterns.len() {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            "tuple pattern mismatch",
                        )));
                    }
                    for (pat, val) in patterns.iter().zip(items.into_iter()) {
                        self.bind_pattern(pat, val, mutable)?;
                    }
                    Ok(())
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "cannot destructure non-tuple with tuple pattern",
                    )))
                }
            }
            Pattern::Struct(fields, rest) => {
                if let Value::Map(map) = value {
                    let mut matched_keys = std::collections::HashSet::new();
                    for (name, inner_pat) in fields {
                        let val = map.get(name).cloned().unwrap_or(Value::Null);
                        matched_keys.insert(name.clone());
                        if let Some(pat) = inner_pat {
                            self.bind_pattern(pat, val, mutable)?;
                        } else {
                            self.env.define(name, val, mutable);
                        }
                    }
                    if let Some(rest_name) = rest {
                        let rest_map: BTreeMap<String, Value> = map
                            .iter()
                            .filter(|(k, _)| !matched_keys.contains(*k))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        self.env.define(rest_name, Value::Map(rest_map), mutable);
                    }
                    Ok(())
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "cannot destructure non-map with struct pattern",
                    )))
                }
            }
            Pattern::Instance(enum_name, type_name, field_pats, rest) => {
                let inst_fields = match &value {
                    Value::Instance { type_name: inst_type, fields } => {
                        if enum_name.is_some() || inst_type != type_name {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch,
                                format!("pattern expected '{}', got '{}'", type_name, inst_type),
                            )));
                        }
                        fields.clone()
                    }
                    Value::Enum { enum_name: actual_enum_name, variant, fields } => {
                        if variant != type_name {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch,
                                format!("pattern expected variant '{}', got '{}'", type_name, variant),
                            )));
                        }
                        if let Some(expected) = enum_name.as_ref() {
                            if expected != actual_enum_name {
                                return Err(Signal::Error(QueError::new(
                                    ErrorKind::TypeMismatch,
                                    format!("pattern expected enum '{}', got '{}'", expected, actual_enum_name),
                                )));
                            }
                        }
                        fields.clone()
                    }
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("cannot destructure {} with instance pattern '{}'", value.type_name(), type_name),
                    ))),
                };
                let mut matched_keys = std::collections::HashSet::new();
                for (name, inner_pat) in field_pats {
                    let val = inst_fields.get(name).cloned().unwrap_or(Value::Null);
                    matched_keys.insert(name.clone());
                    if let Some(pat) = inner_pat {
                        self.bind_pattern(pat, val, mutable)?;
                    } else {
                        self.env.define(name, val, mutable);
                    }
                }
                if let Some(rest_name) = rest {
                    let rest_map: BTreeMap<String, Value> = inst_fields
                        .iter()
                        .filter(|(k, _)| !matched_keys.contains(*k))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.env.define(rest_name, Value::Map(rest_map), mutable);
                }
                Ok(())
            }
            // For literal patterns in let bindings, just verify match.
            _ => {
                if self.match_pattern(pattern, &value).is_some() {
                    Ok(())
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "pattern mismatch in let binding",
                    )))
                }
            }
        }
    }

}
