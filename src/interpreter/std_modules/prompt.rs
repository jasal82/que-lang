//! std.prompt module — low-level terminal input primitives.
//!
//! Only contains the primitives that require Rust support:
//!
//!   * `read_line(opts?)` — read a line from stdin, optionally without echo
//!     (for passwords).
//!   * `read_key()` — read a single key press in raw mode and return its name
//!     (e.g. `"up"`, `"enter"`, `"a"`, `"ctrl+c"`).
//!
//! Higher-level widgets (line editing with completions/history, select menus,
//! multi-select, yes/no confirms, password-with-confirmation, …) can be built
//! on top of these in pure Que using ANSI escape sequences through `print`.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

use console::{Key, Term};

pub(super) fn module() -> StdModule {
    StdModule {
        name: "prompt",
        functions: &["read_line", "read_key"],
    }
}

impl Interpreter {
    pub(crate) fn call_prompt(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "read_line" => prompt_read_line(args),
            "read_key"  => prompt_read_key(args),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'prompt.{}'", func),
            ))),
        }
    }
}

fn prompt_read_line(args: &[Value]) -> IResult {
    if args.len() > 1 {
        return Err(Signal::Error(QueError::new(
            ErrorKind::Runtime,
            format!("prompt.read_line() takes 0 or 1 arguments, got {}", args.len()),
        )));
    }

    let mut echo = true;
    if let Some(opts) = args.first() {
        let map = match opts {
            Value::Map(m) => m,
            _ => return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!("prompt.read_line(): options must be a Map, got {}", opts.type_name()),
            ))),
        };
        if let Some(v) = map.get("echo") {
            echo = match v {
                Value::Bool(b) => *b,
                _ => return Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!("prompt.read_line(): option 'echo' must be a Bool, got {}", v.type_name()),
                ))),
            };
        }
    }

    let line = if echo {
        // Plain stdin read works for both TTYs and pipes.
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(0) => Ok(String::new()),
            Ok(_) => {
                // Strip a single trailing newline (and optional \r).
                if buf.ends_with('\n') { buf.pop(); }
                if buf.ends_with('\r') { buf.pop(); }
                Ok(buf)
            }
            Err(e) => Err(e),
        }
    } else {
        // For hidden input we need a real terminal handle.
        Term::stdout().read_secure_line()
    };

    line
        .map(Value::String)
        .map_err(|e| Signal::Error(QueError::new(
            ErrorKind::IoError,
            format!("prompt.read_line(): {}", e),
        )))
}

fn prompt_read_key(args: &[Value]) -> IResult {
    if !args.is_empty() {
        return Err(Signal::Error(QueError::new(
            ErrorKind::Runtime,
            format!("prompt.read_key() takes 0 arguments, got {}", args.len()),
        )));
    }

    let term = Term::stdout();
    let key = term.read_key().map_err(|e| Signal::Error(QueError::new(
        ErrorKind::IoError,
        format!("prompt.read_key(): {}", e),
    )))?;

    Ok(Value::String(key_name(&key)))
}

fn key_name(key: &Key) -> String {
    match key {
        Key::ArrowUp     => "up".to_string(),
        Key::ArrowDown   => "down".to_string(),
        Key::ArrowLeft   => "left".to_string(),
        Key::ArrowRight  => "right".to_string(),
        Key::Enter       => "enter".to_string(),
        Key::Escape      => "escape".to_string(),
        Key::Backspace   => "backspace".to_string(),
        Key::Home        => "home".to_string(),
        Key::End         => "end".to_string(),
        Key::Tab         => "tab".to_string(),
        Key::BackTab     => "backtab".to_string(),
        Key::Insert      => "insert".to_string(),
        Key::Del         => "delete".to_string(),
        Key::PageUp      => "pageup".to_string(),
        Key::PageDown    => "pagedown".to_string(),
        Key::CtrlC       => "ctrl+c".to_string(),
        Key::Char(c)     => c.to_string(),
        Key::UnknownEscSeq(_) | Key::Unknown => "unknown".to_string(),
        _                => "unknown".to_string(),
    }
}
