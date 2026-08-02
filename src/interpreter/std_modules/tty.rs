//! std.tty module — terminal/TTY introspection.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;
use std::io::IsTerminal;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "tty",
        functions: &["is_stdin", "is_stdout", "is_stderr", "size"],
    }
}

impl Interpreter {
    pub(crate) fn call_tty(&mut self, func: &str, _args: &[Value]) -> IResult {
        match func {
            "is_stdin"  => Ok(Value::Bool(std::io::stdin().is_terminal())),
            "is_stdout" => Ok(Value::Bool(std::io::stdout().is_terminal())),
            "is_stderr" => Ok(Value::Bool(std::io::stderr().is_terminal())),
            "size" => {
                // Returns a Map { "cols": Int, "rows": Int } when attached to
                // a terminal, or `null` when stdout is not a TTY.
                if let Some((cols, rows)) = term_size() {
                    let mut m = BTreeMap::new();
                    m.insert("cols".to_string(), Value::Int(cols as i64));
                    m.insert("rows".to_string(), Value::Int(rows as i64));
                    Ok(Value::Map(m))
                } else {
                    Ok(Value::Null)
                }
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'tty.{}'", func),
            ))),
        }
    }
}

/// Query terminal size from stdout. Uses the `console` crate which already
/// handles Unix/Windows transparently.
fn term_size() -> Option<(u16, u16)> {
    let term = console::Term::stdout();
    let (rows, cols) = term.size_checked()?;
    Some((cols, rows))
}
