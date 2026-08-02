//! std.toml module — TOML parsing and serialization.

use crate::error::*;
use crate::value::Value;
use super::super::helpers::arg_str;
use super::super::Interpreter;
use super::StdModule;
use super::fs::{path_str, atomic_write_str};

pub(super) fn module() -> StdModule {
    StdModule {
        name: "toml",
        functions: &["parse", "stringify", "edit"],
    }
}

impl Interpreter {
    pub(crate) fn call_toml(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "parse" => {
                let s = arg_str(args, 0, "toml.parse")?;
                match crate::config::parse_toml(s) {
                    Ok(v) => Ok(Value::Ok(Box::new(v))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "stringify" => {
                let value = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "toml.stringify requires 1 argument"))
                })?;
                match crate::config::to_toml(value) {
                    Ok(s) => Ok(Value::String(s)),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "edit" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "toml.edit requires 2 arguments: path, fn(doc) -> doc",
                    )));
                }
                let p = path_str(&args[0], "toml.edit")?;
                let func_val = args[1].clone();
                let content = std::fs::read_to_string(&p).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::IoError, e.to_string()))
                })?;
                let doc = crate::config::parse_toml(&content).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::Runtime, e))
                })?;
                let result = self.call_value(func_val, vec![doc])?;
                let serialized = crate::config::to_toml_ordered(&result, &content).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::Runtime, e))
                })?;
                atomic_write_str(&p, &serialized)?;
                Ok(Value::Null)
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'toml.{}'", func),
            ))),
        }
    }
}
