//! Native modules: functions written in Rust that scripts import like a
//! package (`import jbuild`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::trace::AccessKind;
use crate::error::{ErrorKind, QueError, Signal};
use crate::interpreter::Interpreter;
use crate::permissions::{Capability, Policy};
use crate::value::Value;

/// The signature of a native function.
pub type NativeFn =
    Arc<dyn Fn(&mut NativeContext<'_>, Vec<Value>) -> Result<Value, NativeError> + Send + Sync>;

/// A named set of native functions, importable with `import <name>`.
///
/// ```
/// use que_lang::embed::{NativeModule, NativeError};
/// use que_lang::value::Value;
///
/// let m = NativeModule::new("host").function("twice", |_ctx, args| {
///     match args.first() {
///         Some(Value::Int(n)) => Ok(Value::Int(n * 2)),
///         _ => Err(NativeError::new("twice() expects an Int")),
///     }
/// });
/// assert_eq!(m.name(), "host");
/// ```
#[derive(Clone)]
pub struct NativeModule {
    name: String,
    functions: BTreeMap<String, NativeFn>,
}

impl NativeModule {
    pub fn new(name: impl Into<String>) -> Self {
        NativeModule {
            name: name.into(),
            functions: BTreeMap::new(),
        }
    }

    /// Add (or replace) the function `name`.
    pub fn function<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: Fn(&mut NativeContext<'_>, Vec<Value>) -> Result<Value, NativeError>
            + Send
            + Sync
            + 'static,
    {
        self.functions.insert(name.into(), Arc::new(f));
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }
}

impl fmt::Debug for NativeModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeModule")
            .field("name", &self.name)
            .field("functions", &self.functions.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The native modules one engine provides, shared by every interpreter it
/// creates (imported modules, `parallel` branches).
#[derive(Default)]
pub(crate) struct NativeRegistry {
    modules: BTreeMap<String, NativeModule>,
}

impl NativeRegistry {
    pub fn insert(&mut self, module: NativeModule) {
        self.modules.insert(module.name.clone(), module);
    }

    pub fn contains_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    /// Look up `module.function`, the name a native function's
    /// `Value::BuiltinFn` carries.
    pub fn get(&self, qualified: &str) -> Option<NativeFn> {
        let (module, function) = qualified.split_once('.')?;
        self.modules.get(module)?.functions.get(function).cloned()
    }

    /// The value `import <name>` binds.
    pub fn module_value(&self, name: &str) -> Option<Value> {
        let module = self.modules.get(name)?;
        let entries = module
            .functions
            .keys()
            .map(|f| (f.clone(), Value::BuiltinFn(format!("{}.{}", name, f))))
            .collect();
        Some(Value::Module {
            name: name.to_string(),
            entries,
        })
    }
}

/// Where in a script something happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLocation {
    /// The script's file name (without directory).
    pub file: Option<String>,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ScriptLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}:{}:{}", file, self.line, self.col),
            None => write!(f, "line {}:{}", self.line, self.col),
        }
    }
}

/// What a native function can see of the interpreter calling it.
pub struct NativeContext<'a> {
    pub(crate) interp: &'a mut Interpreter,
    pub(crate) function: &'a str,
}

impl NativeContext<'_> {
    /// The qualified name of the function being called (`jbuild.run`).
    pub fn function_name(&self) -> &str {
        self.function
    }

    /// The script location of the call.
    pub fn location(&self) -> Option<ScriptLocation> {
        self.interp.current_location()
    }

    /// The policy the script runs under (`None` only outside embedded mode).
    pub fn policy(&self) -> Option<&Policy> {
        self.interp.permissions.as_ref()
    }

    /// Check `subject` against the script's policy, with the same rules and
    /// wording as the built-in functions.
    pub fn check(&self, capability: Capability, subject: &str) -> Result<(), NativeError> {
        self.interp
            .check_permission(capability, subject)
            .map_err(NativeError::from_signal)
    }

    pub fn check_read(&self, path: &Path) -> Result<(), NativeError> {
        self.check(Capability::Read, &path.to_string_lossy())
    }

    pub fn check_write(&self, path: &Path) -> Result<(), NativeError> {
        self.check(Capability::Write, &path.to_string_lossy())
    }

    pub fn check_env(&self, name: &str) -> Result<(), NativeError> {
        self.check(Capability::Env, name)
    }

    pub fn check_net(&self, host: &str) -> Result<(), NativeError> {
        self.check(Capability::Net, host)
    }

    /// Check that the program at the absolute path `program` is on the exec
    /// allow-list (for natives that spawn processes without a shell).
    pub fn check_exec_program(&self, program: &Path) -> Result<(), NativeError> {
        match &self.interp.permissions {
            None => Ok(()),
            Some(policy) => policy
                .check_exec_program(program)
                .map_err(|d| NativeError::new(d.message())),
        }
    }

    /// Report an access the native function performed to the engine's
    /// tracer, as if a built-in had done it.
    pub fn record_access(&self, kind: AccessKind, requested: &str, outcome: Result<(), String>) {
        self.interp.trace(kind, self.function, requested, outcome);
    }

    /// Fail if the script has been cancelled or ran out of time. Natives that
    /// block for long should call this regularly.
    pub fn check_cancelled(&self) -> Result<(), NativeError> {
        match self.interp.host.limits.exceeded() {
            Some(reason) => Err(NativeError::from_signal(reason.into_signal())),
            None => Ok(()),
        }
    }

    /// Time left before the engine's timeout, if one is set.
    pub fn remaining_time(&self) -> Option<Duration> {
        self.interp
            .host
            .limits
            .deadline
            .map(|(at, _)| at.saturating_duration_since(Instant::now()))
    }

    /// Call a que function value (for example a callback the script passed
    /// in). An error it raises keeps its location and stack trace when the
    /// native function returns it.
    pub fn call(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, NativeError> {
        match self.interp.call_value(callee.clone(), args) {
            Ok(v) | Err(Signal::Return(v)) => Ok(v),
            Err(signal) => Err(NativeError::from_signal(signal)),
        }
    }
}

/// An error raised by a native function. Surfaces in the script as an
/// ordinary runtime error (catchable with `try`), located at the call.
pub struct NativeError {
    repr: Repr,
}

enum Repr {
    Message(ErrorKind, String),
    /// An error or unwind that came from the interpreter (a failing
    /// callback, a cancellation), passed through untouched. Boxed because a
    /// `Signal` is large and natives return `NativeError` on every path.
    Signal(Box<Signal>),
}

impl NativeError {
    pub fn new(message: impl Into<String>) -> Self {
        NativeError {
            repr: Repr::Message(ErrorKind::Runtime, message.into()),
        }
    }

    /// An argument had the wrong type.
    pub fn type_mismatch(message: impl Into<String>) -> Self {
        NativeError {
            repr: Repr::Message(ErrorKind::TypeMismatch, message.into()),
        }
    }

    pub fn message(&self) -> String {
        match &self.repr {
            Repr::Message(_, m) => m.clone(),
            Repr::Signal(signal) => match signal.as_ref() {
                Signal::Error(e) => e.message.clone(),
                Signal::Interrupted(super::limits::SIGNAL_TIMED_OUT) => {
                    "script exceeded its time limit".to_string()
                }
                Signal::Interrupted(_) => "script was cancelled".to_string(),
                other => format!("{:?}", other),
            },
        }
    }

    pub(crate) fn from_signal(signal: Signal) -> Self {
        NativeError {
            repr: Repr::Signal(Box::new(signal)),
        }
    }

    pub(crate) fn into_signal(self) -> Signal {
        match self.repr {
            Repr::Message(kind, m) => Signal::Error(QueError::new(kind, m)),
            Repr::Signal(s) => *s,
        }
    }
}

impl fmt::Debug for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NativeError").field(&self.message()).finish()
    }
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for NativeError {}

impl From<String> for NativeError {
    fn from(message: String) -> Self {
        NativeError::new(message)
    }
}

impl From<&str> for NativeError {
    fn from(message: &str) -> Self {
        NativeError::new(message)
    }
}

impl From<super::convert::ConvertError> for NativeError {
    fn from(e: super::convert::ConvertError) -> Self {
        NativeError::type_mismatch(e.to_string())
    }
}
