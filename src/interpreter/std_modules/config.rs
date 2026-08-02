//! std.config module — read and write config files.
//!
//! Everything else about a config is a Map operation, and lives on Map:
//! `get_path`, `set_path`, `delete_path`, `has_path`, `deep_merge`, `paths`.
//! What is left here is the part that touches the disk, and the part that
//! knows JSON from YAML from TOML by looking at the file extension.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "config",
        functions: &["read", "write"],
    }
}

impl Interpreter {
    pub(crate) fn call_config(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "read" => {
                let path = expect_path(args.first(), "config.read")?;
                match crate::config::config_read(&path) {
                    Ok(v) => Ok(Value::Ok(Box::new(v))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "write" => {
                let path = expect_path(args.first(), "config.write")?;
                let value = args.get(1).ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "config.write requires 2-3 arguments (path, value, indent?)",
                    ))
                })?;
                let indent = match args.get(2) {
                    Some(Value::Int(n)) => Some(*n as usize),
                    _ => None,
                };
                if self.dry_run_skip(format!("write config {}", path)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match crate::config::config_write(&path, value, indent) {
                    Ok(()) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'config.{}'", func),
            ))),
        }
    }
}

fn expect_path(arg: Option<&Value>, who: &str) -> Result<String, Signal> {
    match arg {
        Some(val) => crate::interpreter::helpers::path_arg(val, who),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{} requires a path or string argument", who),
        ))),
    }
}
