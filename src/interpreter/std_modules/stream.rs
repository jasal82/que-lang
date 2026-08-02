//! std.stream module — stream constructors.
//!
//! The global `stream(x)` decided what to build by looking at the type of
//! its argument, which meant one name covered "read this file" and "wrap
//! this text I already have" — two different things, one of which touches
//! the disk. Splitting them makes the reading obvious at the call site and
//! lets the capability policy classify each without inspecting arguments.

use crate::error::*;
use crate::value::{Stream, StreamSink, Value};
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "stream",
        functions: &["file", "of", "stdout", "stderr", "stdin"],
    }
}

impl Interpreter {
    pub(crate) fn call_stream(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            // Reads a file. Always a Path, so `--allow read=...` covers it.
            "file" => match args.first() {
                Some(Value::Path(p)) => Ok(Value::Stream(Stream::from_file(p.clone()))),
                Some(other) => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!(
                        "stream.file expects a Path, got {}; use stream.of() for text, a list or a file handle",
                        other.type_name()
                    ),
                ))),
                None => Err(Signal::Error(QueError::new(
                    ErrorKind::ArityMismatch,
                    "stream.file requires 1 argument (a Path)",
                ))),
            },
            // In-memory, or an already-opened handle. Touches nothing new.
            "of" => match args.first() {
                Some(Value::String(s)) => Ok(Value::Stream(Stream::from_string(s.clone()))),
                Some(Value::List(items)) => {
                    let lines: Vec<String> = items.iter().map(|v| v.display_string()).collect();
                    Ok(Value::Stream(Stream::from_string(lines.join("\n"))))
                }
                Some(Value::Stream(s)) => Ok(Value::Stream(s.clone())),
                Some(Value::FileHandle(fh)) => Ok(Value::Stream(Stream::with_sink(
                    StreamSink::FileHandle(fh.clone()),
                ))),
                Some(other) => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!(
                        "stream.of expects a String, List, Stream or FileHandle, got {}; use stream.file() for a Path",
                        other.type_name()
                    ),
                ))),
                None => Err(Signal::Error(QueError::new(
                    ErrorKind::ArityMismatch,
                    "stream.of requires 1 argument",
                ))),
            },
            "stdout" => Ok(Value::Stream(Stream::with_sink(StreamSink::Stdout))),
            "stderr" => Ok(Value::Stream(Stream::with_sink(StreamSink::Stderr))),
            "stdin" => Ok(Value::Stream(Stream::from_stdin())),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'stream.{}'", func),
            ))),
        }
    }
}
