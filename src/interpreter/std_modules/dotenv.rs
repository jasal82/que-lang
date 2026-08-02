//! std.dotenv module — Load, parse, and write .env files.

use crate::error::*;
use crate::value::Value;
use crate::interpreter::helpers::arg_path_str;
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "dotenv",
        functions: &["load", "load_overwrite", "parse", "write"],
    }
}

impl Interpreter {
    pub(crate) fn call_dotenv(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "load" => {
                let path_str = arg_path_str(args, 0, "dotenv.load")?;
                let vars = dotenv_parse_file(&path_str).map_err(|e| sig_err(e))?;
                for (k, v) in &vars {
                    std::env::set_var(k, v);
                }
                Ok(Value::Null)
            }
            "load_overwrite" => {
                let path_str = arg_path_str(args, 0, "dotenv.load_overwrite")?;
                let vars = dotenv_parse_file(&path_str).map_err(|e| sig_err(e))?;
                for (k, v) in &vars {
                    std::env::set_var(k, v);
                }
                Ok(Value::Null)
            }
            "parse" => {
                let path_str = arg_path_str(args, 0, "dotenv.parse")?;
                let vars = dotenv_parse_file(&path_str).map_err(|e| sig_err(e))?;
                let map: BTreeMap<String, Value> = vars.into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                Ok(Value::Map(map))
            }
            "write" => {
                let map = match args.first() {
                    Some(Value::Map(m)) => m.clone(),
                    _ => return Err(sig_type("dotenv.write", "Map")),
                };
                let path_str = arg_path_str(args, 1, "dotenv.write")?;
                let content = dotenv_write_map(&map);
                std::fs::write(&path_str, content).map_err(|e| sig_err(e.to_string()))?;
                Ok(Value::Null)
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'dotenv.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn sig_type(name: &str, expected: &str) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::TypeMismatch,
        format!("{}: expected {}", name, expected),
    ))
}

fn dotenv_parse_file(path: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    dotenv_parse_str(&content)
}

fn dotenv_parse_str(content: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key_val, _comment)) = line.split_once(" #") {
            let line = key_val.trim();
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim().to_string();
                let v = unquote_dotenv_value(v.trim());
                map.insert(k, v);
            }
        } else if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_string();
            let v = unquote_dotenv_value(v.trim());
            map.insert(k, v);
        }
    }
    Ok(map)
}

fn unquote_dotenv_value(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len()-1].to_string()
    } else {
        s.to_string()
    }
}

fn dotenv_write_map(map: &BTreeMap<String, Value>) -> String {
    let mut lines = Vec::new();
    for (k, v) in map {
        let val = v.display_string();
        if val.contains(' ') || val.contains('"') || val.contains('\'') {
            lines.push(format!("{}=\"{}\"", k, val.replace('"', "\\\"")));
        } else {
            lines.push(format!("{}={}", k, val));
        }
    }
    lines.join("\n") + "\n"
}
