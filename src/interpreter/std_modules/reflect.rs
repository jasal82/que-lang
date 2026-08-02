//! std.reflect module — asking a program about itself.
//!
//! These are debugging and tooling tools, not everyday vocabulary, so they
//! sit behind an import rather than in the global namespace every script
//! pays for. `help`, `dbg` and `typeof` stay global: they are typed at a
//! REPL prompt or dropped into a line mid-edit, and an import statement is
//! exactly the friction you do not want there.
//!
//! The reflection questions that a *value* can answer — `inspect`,
//! `methods`, `is_type`, `type_name` — are methods on every value and are
//! not repeated here.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "reflect",
        functions: &[
            "type_info", "fields", "has_method", "vars", "var_info",
            "scope_depth", "modules",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_reflect(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "type_info" => {
                let val = args.first().unwrap_or(&Value::Null);
                let mut fields = BTreeMap::new();
                fields.insert("type_name".into(), Value::String(val.type_name().into()));
                fields.insert("is_collection".into(), Value::Bool(matches!(
                    val,
                    Value::List(_) | Value::Map(_) | Value::Tuple(_)
                )));
                fields.insert("is_numeric".into(), Value::Bool(matches!(
                    val,
                    Value::Int(_) | Value::Float(_)
                )));
                fields.insert("is_callable".into(), Value::Bool(matches!(
                    val,
                    Value::Function { .. } | Value::BuiltinFn(_)
                )));
                fields.insert("is_result".into(), Value::Bool(matches!(
                    val,
                    Value::Ok(_) | Value::Err(_)
                )));
                fields.insert("is_null".into(), Value::Bool(matches!(val, Value::Null)));
                fields.insert("is_iterable".into(), Value::Bool(matches!(
                    val,
                    Value::List(_) | Value::Map(_) | Value::String(_) | Value::Tuple(_)
                )));
                fields.insert("methods".into(), Value::List(
                    val.available_methods().iter().map(|s| Value::String(s.to_string())).collect(),
                ));
                Ok(Value::Instance { type_name: "TypeInfo".to_string(), fields })
            }
            "fields" => {
                let val = args.first().unwrap_or(&Value::Null);
                match val {
                    Value::Map(map) => Ok(Value::List(
                        map.keys().map(|k| Value::String(k.clone())).collect(),
                    )),
                    Value::Tuple(items) => Ok(Value::List(
                        (0..items.len()).map(|i| Value::Int(i as i64)).collect(),
                    )),
                    Value::ProcessResult { .. } => Ok(Value::List(vec![
                        Value::String("exit_code".into()),
                        Value::String("stdout".into()),
                        Value::String("stderr".into()),
                    ])),
                    Value::Function { params, .. } => Ok(Value::List(
                        params.iter().map(|p| Value::String(p.name.clone())).collect(),
                    )),
                    _ => Ok(Value::List(vec![])),
                }
            }
            "has_method" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "reflect.has_method requires 2 arguments: value, method_name",
                    )));
                }
                let method_name = match &args[1] {
                    Value::String(s) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "reflect.has_method second argument must be a string",
                    ))),
                };
                Ok(Value::Bool(
                    args[0].available_methods().contains(&method_name.as_str()),
                ))
            }
            "vars" => {
                // User-defined bindings only; the builtins are always there
                // and listing them would bury what the script actually made.
                // Imported modules go too: you wrote the import, you know it
                // is there, and `reflect` itself would otherwise always show
                // up in its own answer.
                let mut result = BTreeMap::new();
                for (name, value, _mutable) in self.env.list_vars() {
                    if matches!(value, Value::BuiltinFn(_) | Value::Module { .. }) {
                        continue;
                    }
                    result.insert(name, value);
                }
                Ok(Value::Map(result))
            }
            "var_info" => {
                let name = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "reflect.var_info requires a string argument (variable name)",
                    ))),
                };
                match self.env.get(&name) {
                    Some(val) => {
                        let mut fields = BTreeMap::new();
                        fields.insert("name".into(), Value::String(name.clone()));
                        fields.insert("type_name".into(), Value::String(val.type_name().into()));
                        fields.insert("value".into(), Value::String(val.display_string()));
                        fields.insert(
                            "mutable".into(),
                            Value::Bool(self.env.is_mutable(&name).unwrap_or(false)),
                        );
                        fields.insert(
                            "is_builtin".into(),
                            Value::Bool(matches!(&val, Value::BuiltinFn(_))),
                        );
                        Ok(Value::Instance { type_name: "VarInfo".to_string(), fields })
                    }
                    None => Ok(Value::Null),
                }
            }
            "scope_depth" => Ok(Value::Int(self.env.scope_depth() as i64)),
            "modules" => {
                let mut map = BTreeMap::new();
                for m in super::all_modules() {
                    let funcs: Vec<Value> = m
                        .functions
                        .iter()
                        .map(|f| Value::String(f.to_string()))
                        .collect();
                    map.insert(m.name.to_string(), Value::List(funcs));
                }
                Ok(Value::Map(map))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'reflect.{}'", func),
            ))),
        }
    }
}
