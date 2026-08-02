//! std.json module — JSON parsing and serialization.

use crate::error::*;
use crate::value::Value;
use super::super::helpers::arg_str;
use super::super::Interpreter;
use super::StdModule;
use super::fs::{path_str, atomic_write_str};

pub(super) fn module() -> StdModule {
    StdModule {
        name: "json",
        functions: &["parse", "stringify", "edit"],
    }
}

impl Interpreter {
    pub(crate) fn call_json(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "parse" => {
                let s = arg_str(args, 0, "json.parse")?;
                match crate::config::parse_json(s) {
                    Ok(v) => Ok(Value::Ok(Box::new(v))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "stringify" => {
                let value = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "json.stringify requires 1-2 arguments"))
                })?;
                let indent = args.get(1).and_then(|v| if let Value::Int(n) = v { Some(*n as usize) } else { None });
                match crate::config::to_json(value, indent) {
                    Ok(s) => Ok(Value::String(s)),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "edit" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "json.edit requires 2 arguments: path, fn(doc) -> doc",
                    )));
                }
                let p = path_str(&args[0], "json.edit")?;
                let func_val = args[1].clone();
                let content = std::fs::read_to_string(&p).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::IoError, e.to_string()))
                })?;
                let doc = crate::config::parse_json(&content).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::Runtime, e))
                })?;
                let result = self.call_value(func_val, vec![doc])?;
                let serialized = crate::config::to_json_ordered(&result, &content, Some(2)).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::Runtime, e))
                })?;
                atomic_write_str(&p, &serialized)?;
                Ok(Value::Null)
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'json.{}'", func),
            ))),
        }
    }
}
