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
        functions: &["is_stdin", "is_stdout", "is_stderr", "size", "supports_ansi"],
    }
}

impl Interpreter {
    pub(crate) fn call_tty(&mut self, func: &str, _args: &[Value]) -> IResult {
        match func {
            "is_stdin"  => Ok(Value::Bool(std::io::stdin().is_terminal())),
            "is_stdout" => Ok(Value::Bool(std::io::stdout().is_terminal())),
            "is_stderr" => Ok(Value::Bool(std::io::stderr().is_terminal())),
            "supports_ansi" => Ok(Value::Bool(supports_ansi())),
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

/// Whether stdout will actually interpret ANSI escape sequences.
///
/// This is a stronger claim than `is_stdout()`: a script that moves the cursor
/// or clears lines needs escape *handling*, not just a terminal. It is also
/// deliberately independent of `NO_COLOR` — that variable is a color policy,
/// and suppressing color must not disable cursor positioning.
///
/// On Windows the call has a side effect, by necessity: legacy conhost
/// ignores escape sequences until a process turns on
/// `ENABLE_VIRTUAL_TERMINAL_PROCESSING`, so asking whether escapes work means
/// requesting that they do. `colors_supported()` is the only public entry
/// point in `console` that reaches `SetConsoleMode`, and on Windows it applies
/// no color-policy checks of its own.
pub fn supports_ansi() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }

    #[cfg(windows)]
    {
        console::Term::stdout().features().colors_supported()
    }

    #[cfg(not(windows))]
    {
        // A terminal that does not name itself, or names itself `dumb`, is
        // taken at its word.
        std::env::var("TERM").map(|t| t != "dumb").unwrap_or(false)
    }
}

/// Best-effort enable of ANSI escape handling on stdout, called once at
/// process start.
///
/// Que's `print` writes straight to stdout, so without this a script that
/// emits raw escapes — rather than asking `supports_ansi()` first — would have
/// them shown literally on a legacy Windows console. A no-op on Unix.
pub fn enable_ansi() {
    let _ = supports_ansi();
}
