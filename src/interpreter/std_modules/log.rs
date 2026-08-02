//! std.log module — Structured logging with levels, formats, and sinks.

use crate::error::*;
use crate::value::Value;
use super::super::helpers::{arg_str, parse_log_level, parse_log_format, default_console_sink};
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "log",
        functions: &[
            "debug", "info", "warn", "error",
            "set_level", "set_format",
            "add_file_sink", "add_console_sink", "remove_sinks",
            "new",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_log(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "debug" => {
                let empty = BTreeMap::new();
                self.log_emit_to_sinks("DEBUG", 0, &empty, args)
            }
            "info" => {
                let empty = BTreeMap::new();
                self.log_emit_to_sinks("INFO", 1, &empty, args)
            }
            "warn" => {
                let empty = BTreeMap::new();
                self.log_emit_to_sinks("WARN", 2, &empty, args)
            }
            "error" => {
                let empty = BTreeMap::new();
                self.log_emit_to_sinks("ERROR", 3, &empty, args)
            }
            "set_level" => {
                let level_str = arg_str(args, 0, "log.set_level")?;
                self.log_level = parse_log_level(level_str).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "set_format" => {
                let fmt_str = arg_str(args, 0, "log.set_format")?;
                self.log_format = parse_log_format(fmt_str).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "add_file_sink" => {
                let path = arg_str(args, 0, "log.add_file_sink")?.to_string();
                let opts = args.get(1);
                let mut sink = super::super::helpers::LogSink {
                    kind: super::super::helpers::LogSinkKind::File(path),
                    level: None,
                    format: None,
                    filter: None,
                };
                if let Some(Value::Map(m)) = opts {
                    if let Some(Value::String(l)) = m.get("level") {
                        sink.level = Some(parse_log_level(l).map_err(|e| sig_err(e))?);
                    }
                    if let Some(Value::String(f)) = m.get("format") {
                        sink.format = Some(parse_log_format(f).map_err(|e| sig_err(e))?);
                    }
                    if let Some(Value::Map(filter)) = m.get("filter") {
                        sink.filter = Some(filter.clone());
                    }
                }
                self.log_sinks.push(sink);
                Ok(Value::Null)
            }
            "add_console_sink" => {
                let opts = args.first();
                let mut sink = super::super::helpers::LogSink {
                    kind: super::super::helpers::LogSinkKind::Console,
                    level: None,
                    format: None,
                    filter: None,
                };
                if let Some(Value::Map(m)) = opts {
                    if let Some(Value::String(l)) = m.get("level") {
                        sink.level = Some(parse_log_level(l).map_err(|e| sig_err(e))?);
                    }
                    if let Some(Value::String(f)) = m.get("format") {
                        sink.format = Some(parse_log_format(f).map_err(|e| sig_err(e))?);
                    }
                    if let Some(Value::Map(filter)) = m.get("filter") {
                        sink.filter = Some(filter.clone());
                    }
                }
                self.log_sinks.push(sink);
                Ok(Value::Null)
            }
            "remove_sinks" => {
                self.log_sinks.clear();
                self.log_sinks.push(default_console_sink());
                Ok(Value::Null)
            }
            "new" => {
                let context = match args.first() {
                    Some(Value::Map(m)) => m.clone(),
                    Some(Value::Null) | None => BTreeMap::new(),
                    Some(other) => return Err(sig_type("log.new", &format!("Map, got {}", other.type_name()))),
                };
                let mut fields = BTreeMap::new();
                fields.insert("context".to_string(), Value::Map(context));
                Ok(Value::Instance {
                    type_name: "Logger".to_string(),
                    fields,
                })
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'log.{}'", func),
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
