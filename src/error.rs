/// Error types for the Que language toolchain.
use crate::token::Span;
use std::fmt;

/// A single frame in a call stack backtrace.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Name of the function or task being called.
    pub name: String,
    /// File where the call was made (call site).
    pub call_file: Option<String>,
    /// Source position of the call site.
    pub call_span: Option<Span>,
}

/// A Que error with source location and message.
#[derive(Debug, Clone)]
pub struct QueError {
    pub kind: ErrorKind,
    pub message: String,
    pub span: Option<Span>,
    /// Source file name (filename only, not full path) for error reporting.
    pub file: Option<String>,
    /// Call stack at the point the error was raised.
    /// Only populated when `QUE_BACKTRACE=1` is set or the interpreter has a call stack.
    pub backtrace: Vec<CallFrame>,
    /// Process exit code to use if this error reaches the top level.
    /// `None` means "use the default for this error kind" (see
    /// [`QueError::process_exit_code`]). Set explicitly by `fail(msg, code)`
    /// and by failing commands, which forward the child's own exit code.
    pub exit_code: Option<i32>,
}

/// Exit code for a usage problem: bad CLI arguments, a missing file, or
/// source that does not lex/parse. Distinct from a *running* script that
/// fails, so CI can tell "you invoked me wrong" from "the build broke".
pub const EXIT_USAGE: i32 = 2;

/// Exit code for a script that failed while running, when nothing more
/// specific applies.
pub const EXIT_FAILURE: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    // Lexer errors
    UnexpectedChar,
    UnterminatedString,
    UnterminatedCommand,
    InvalidNumber,
    InvalidEscape,

    // Parser errors
    UnexpectedToken,
    ExpectedToken,
    ExpectedExpression,
    ExpectedPattern,
    InvalidAssignmentTarget,

    // Interpreter errors
    UndefinedVariable,
    TypeMismatch,
    DivisionByZero,
    IndexOutOfBounds,
    KeyNotFound,
    ArityMismatch,
    NotCallable,
    NotIterable,
    ImmutableVariable,
    GuardFailed,
    CommandFailed,
    IoError,

    // General
    Runtime,
}

impl QueError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            span: None,
            file: None,
            backtrace: Vec::new(),
            exit_code: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn lexer(message: impl Into<String>, span: Span) -> Self {
        Self {
            kind: ErrorKind::UnexpectedChar,
            message: message.into(),
            span: Some(span),
            file: None,
            backtrace: Vec::new(),
            exit_code: None,
        }
    }

    pub fn parser(kind: ErrorKind, message: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            message: message.into(),
            span: Some(span),
            file: None,
            backtrace: Vec::new(),
            exit_code: None,
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Runtime,
            message: message.into(),
            span: None,
            file: None,
            backtrace: Vec::new(),
            exit_code: None,
        }
    }

    /// Pin the process exit code this error should produce at the top level.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// The process exit code for this error.
    ///
    /// An explicit code wins; otherwise the kind decides. Lex and parse
    /// errors mean the input was never runnable, which is a usage problem;
    /// everything else is a runtime failure.
    pub fn process_exit_code(&self) -> i32 {
        if let Some(code) = self.exit_code {
            return code;
        }
        match self.kind {
            ErrorKind::UnexpectedChar
            | ErrorKind::UnterminatedString
            | ErrorKind::UnterminatedCommand
            | ErrorKind::InvalidNumber
            | ErrorKind::InvalidEscape
            | ErrorKind::UnexpectedToken
            | ErrorKind::ExpectedToken
            | ErrorKind::ExpectedExpression
            | ErrorKind::ExpectedPattern
            | ErrorKind::InvalidAssignmentTarget => EXIT_USAGE,
            _ => EXIT_FAILURE,
        }
    }
}

impl fmt::Display for QueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.span, &self.file) {
            (Some(span), Some(file)) => write!(
                f,
                "{}:{}:{}: {}",
                file, span.line, span.col, self.message
            )?,
            (Some(span), None) => write!(
                f,
                "line {}:{}: {}",
                span.line, span.col, self.message
            )?,
            (None, _) => write!(f, "{}", self.message)?,
        }
        if !self.backtrace.is_empty() && std::env::var("QUE_BACKTRACE").is_ok() {
            write!(f, "\ncall stack:")?;
            for frame in self.backtrace.iter().rev() {
                match (&frame.call_span, &frame.call_file) {
                    (Some(span), Some(file)) => write!(
                        f, "\n  in {} (called from {}:{}:{})",
                        frame.name, file, span.line, span.col
                    )?,
                    (Some(span), None) => write!(
                        f, "\n  in {} (called from line {}:{})",
                        frame.name, span.line, span.col
                    )?,
                    _ => write!(f, "\n  in {}", frame.name)?,
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for QueError {}

/// Control flow signals used by the interpreter.
/// These are not errors per se, but signals that unwind the call stack.
#[derive(Debug, Clone)]
pub enum Signal {
    Return(crate::value::Value),
    Break(Option<crate::value::Value>),
    Continue,
    Error(QueError),
    /// Process exit requested via `os.exit(code)`. Unwinds the entire call
    /// stack; caught at the top level (CLI) to call `std::process::exit`.
    Exit(i32),
    /// SIGINT or SIGTERM arrived. Unwinds like `Exit`, so every `defer` on the
    /// stack still runs, but is deliberately **not** catchable by `try`/`catch`:
    /// a script must not be able to swallow Ctrl-C.
    Interrupted(i32),
}

impl From<QueError> for Signal {
    fn from(e: QueError) -> Self {
        Signal::Error(e)
    }
}

/// Result type for interpreter evaluation.
pub type IResult = Result<crate::value::Value, Signal>;
