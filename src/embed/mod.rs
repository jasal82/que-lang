//! Embedding que in a Rust program.
//!
//! An [`Engine`] is one interpreter configured by its host: a capability
//! [`Policy`](crate::permissions::Policy) (deny-by-default), native modules
//! written in Rust, explicit module search paths and in-memory module
//! sources, a timeout and a [`CancellationToken`], and an [`AccessTracer`]
//! that sees every filesystem access the script makes. Engines share no
//! process-global state, so several can run on different threads at once.
//!
//! ```
//! use que_lang::embed::{Engine, NativeModule};
//! use que_lang::permissions::Policy;
//! use que_lang::value::Value;
//!
//! let host = NativeModule::new("host").function("greeting", |_ctx, _args| {
//!     Ok(Value::String("hello".into()))
//! });
//! let mut engine = Engine::builder()
//!     .policy(Policy::deny_all())
//!     .native_module(host)
//!     .build()
//!     .unwrap();
//! engine
//!     .run_source("main.que", "import host\nfn greet(name) { host.greeting() + \", \" + name }")
//!     .unwrap();
//! let v = engine.call("greet", vec![Value::String("que".into())]).unwrap();
//! assert_eq!(v, Value::String("hello, que".into()));
//! ```
//!
//! See `docs/embedding.md` for the full guide.

pub mod convert;
pub mod limits;
pub mod native;
pub mod trace;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use convert::{from_json, from_toml, from_value, to_json, to_toml, to_value, ConvertError};
pub use limits::{CancelReason, CancellationToken};
pub use native::{NativeContext, NativeError, NativeFn, NativeModule, ScriptLocation};
pub use trace::{observe, AccessEvent, AccessKind, AccessTracer, EntryType, Observation};

use crate::error::{CallFrame, ErrorKind, QueError, Signal};
use crate::interpreter::Interpreter;
use crate::module_loader::ModuleLoader;
use crate::permissions::Policy;
use crate::value::Value;
use limits::Limits;
use native::NativeRegistry;

/// The path prefix of modules given as source text rather than files
/// ([`EngineBuilder::embedded_module`]). It never refers to the disk.
pub const EMBEDDED_ROOT: &str = "<embedded>";

/// What an interpreter inherited from its embedder. Cloned into every
/// interpreter a script causes to exist, so imported modules and `parallel`
/// branches run under the same limits, tracer and native modules.
#[derive(Clone, Default)]
pub(crate) struct HostContext {
    pub limits: Limits,
    pub tracer: Option<Arc<dyn AccessTracer>>,
    pub natives: Option<Arc<NativeRegistry>>,
    /// Embedded mode: refuse operations that change process-global state
    /// (working directory, environment variables) or read the terminal,
    /// because they would leak into the host and into other interpreters.
    pub isolated: bool,
}

impl fmt::Debug for HostContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostContext")
            .field("limits", &self.limits)
            .field("tracer", &self.tracer.is_some())
            .field("natives", &self.natives.is_some())
            .field("isolated", &self.isolated)
            .finish()
    }
}

impl HostContext {
    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    pub fn check_limits(&self) -> Result<(), Signal> {
        match self.limits.exceeded() {
            Some(reason) => Err(reason.into_signal()),
            None => Ok(()),
        }
    }

    /// Refuse `what` in embedded mode.
    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    pub fn refuse_global(&self, what: &str) -> Result<(), Signal> {
        if self.isolated {
            return Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!(
                    "{} is not available in embedded mode: it would change state shared \
                     with the host process",
                    what
                ),
            )));
        }
        Ok(())
    }
}

/// The version of the que language this crate implements: the semantics of
/// evaluation and of every standard-library function.
///
/// Embedders that cache script results put this in their cache keys. It
/// changes whenever a script could observe a difference: a changed or added
/// stdlib function, a changed evaluation rule, a changed error. Bug fixes
/// bump the patch level, compatible additions the minor version, changes
/// that can make existing scripts behave differently the major version. It
/// is independent of the crate version, which also changes for API-only
/// releases.
pub const LANGUAGE_VERSION: &str = "1.2.1";

/// [`LANGUAGE_VERSION`] as a function, for callers that prefer one.
pub fn language_version() -> &'static str {
    LANGUAGE_VERSION
}

// ── Errors ───────────────────────────────────────────────────────────

/// One frame of a script stack trace, innermost last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// The function or task that was called.
    pub function: String,
    /// Where it was called from, if known.
    pub call_site: Option<ScriptLocation>,
}

/// An error raised by a script: a parse error, a runtime error, a failed
/// command, a permission denial or an error from a native function.
#[derive(Debug, Clone)]
pub struct ScriptError {
    pub kind: ErrorKind,
    pub message: String,
    /// Where it was raised.
    pub location: Option<ScriptLocation>,
    /// The call stack at that point, outermost first.
    pub stack: Vec<StackFrame>,
}

impl ScriptError {
    fn from_que(e: QueError) -> Self {
        let location = e.span.map(|span| ScriptLocation {
            file: e.file.clone(),
            line: span.line,
            col: span.col,
        });
        let stack = e
            .backtrace
            .iter()
            .map(|frame: &CallFrame| StackFrame {
                function: frame.name.clone(),
                call_site: frame.call_span.map(|span| ScriptLocation {
                    file: frame.call_file.clone(),
                    line: span.line,
                    col: span.col,
                }),
            })
            .collect();
        ScriptError {
            kind: e.kind,
            message: e.message,
            location,
            stack,
        }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "{}: {}", loc, self.message)?,
            None => write!(f, "{}", self.message)?,
        }
        for frame in self.stack.iter().rev() {
            match &frame.call_site {
                Some(site) => write!(f, "\n  in {} (called from {})", frame.function, site)?,
                None => write!(f, "\n  in {}", frame.function)?,
            }
        }
        Ok(())
    }
}

impl std::error::Error for ScriptError {}

/// Everything that can go wrong running a script in an [`Engine`].
#[derive(Debug)]
pub enum EmbedError {
    /// The script failed (syntax error, runtime error, permission denial…).
    Script(ScriptError),
    /// The script was cancelled or ran out of time.
    Cancelled(CancelReason),
    /// The script called `exit(code)` / `os.exit(code)`.
    Exit(i32),
    /// The function to call isn't defined (or isn't exported by the module).
    FunctionNotFound(String),
    /// The name exists but isn't a function.
    NotCallable { name: String, type_name: String },
    /// A script file couldn't be read.
    Io { path: PathBuf, message: String },
    /// The engine configuration is invalid.
    Config(String),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmbedError::Script(e) => write!(f, "{}", e),
            EmbedError::Cancelled(reason) => write!(f, "{}", reason),
            EmbedError::Exit(code) => write!(f, "script called exit({})", code),
            EmbedError::FunctionNotFound(name) => write!(f, "function '{}' is not defined", name),
            EmbedError::NotCallable { name, type_name } => {
                write!(f, "'{}' is a {}, not a function", name, type_name)
            }
            EmbedError::Io { path, message } => {
                write!(f, "cannot read '{}': {}", path.display(), message)
            }
            EmbedError::Config(message) => write!(f, "invalid engine configuration: {}", message),
        }
    }
}

impl std::error::Error for EmbedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EmbedError::Script(e) => Some(e),
            _ => None,
        }
    }
}

impl EmbedError {
    fn from_signal(signal: Signal, timeout: Option<Duration>) -> Self {
        match signal {
            Signal::Error(e) => EmbedError::Script(ScriptError::from_que(e)),
            Signal::Interrupted(limits::SIGNAL_TIMED_OUT) => {
                EmbedError::Cancelled(CancelReason::TimedOut(timeout.unwrap_or_default()))
            }
            // A cancelled token; or a real signal if the embedder also
            // installed the CLI's handlers, which means the same thing.
            Signal::Interrupted(_) => EmbedError::Cancelled(CancelReason::Cancelled),
            Signal::Exit(code) => EmbedError::Exit(code),
            Signal::Break(_) => EmbedError::Script(ScriptError::from_que(QueError::runtime(
                "break outside of loop",
            ))),
            Signal::Continue => EmbedError::Script(ScriptError::from_que(QueError::runtime(
                "continue outside of loop",
            ))),
            // Callers turn a top-level return into a value before this.
            Signal::Return(_) => EmbedError::Script(ScriptError::from_que(QueError::runtime(
                "return outside of function",
            ))),
        }
    }
}

// ── Builder ──────────────────────────────────────────────────────────

/// Configures an [`Engine`]. Start with [`Engine::builder`].
pub struct EngineBuilder {
    policy: Policy,
    search_paths: Vec<PathBuf>,
    embedded: BTreeMap<String, String>,
    natives: Vec<NativeModule>,
    tracer: Option<Arc<dyn AccessTracer>>,
    cancel: CancellationToken,
    timeout: Option<Duration>,
    strict: bool,
    args: Vec<String>,
}

impl EngineBuilder {
    /// The capability policy. Defaults to [`Policy::deny_all`].
    pub fn policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    /// A directory searched for `import <package>`: it contains
    /// `<package>/mod.que` (like a `que_packages/` directory). Searched in
    /// the order added, after the embedded modules. No other directory is
    /// searched: not `que_packages/` next to the script, not the global
    /// packages directory, nothing from the environment.
    pub fn search_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.search_paths.push(dir.into());
        self
    }

    /// A module given as source text. `path` is relative to a virtual
    /// packages directory, with `/` separators: `kit/mod.que` is
    /// `import kit`, `kit/license.que` is `import kit.license`, and a
    /// script run with [`Engine::run_source`] can `import .helper` for
    /// `helper.que`. Embedded modules are found before the search paths.
    pub fn embedded_module(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        self.embedded.insert(path.into(), source.into());
        self
    }

    /// A native module, importable with `import <name>`. Its name must not
    /// be `std` or the name of a standard-library module.
    pub fn native_module(mut self, module: NativeModule) -> Self {
        self.natives.push(module);
        self
    }

    /// Receive an [`AccessEvent`] for every filesystem access.
    pub fn tracer(mut self, tracer: impl AccessTracer + 'static) -> Self {
        self.tracer = Some(Arc::new(tracer));
        self
    }

    /// Use this token for cancellation (by default the engine makes its own;
    /// see [`Engine::cancellation_token`]).
    pub fn cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancel = token;
        self
    }

    /// Wall-clock limit for each call into the engine ([`Engine::run_file`],
    /// [`Engine::call`], …), measured from the start of that call.
    pub fn timeout(mut self, limit: Duration) -> Self {
        self.timeout = Some(limit);
        self
    }

    /// Enforce type annotations (as `que --strict`).
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// What the script's `args()` returns.
    pub fn script_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn build(self) -> Result<Engine, EmbedError> {
        let std_names: Vec<&str> = crate::interpreter::std_module_names();
        let mut registry = NativeRegistry::default();
        for module in self.natives {
            let name = module.name().to_string();
            let valid_ident = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_ident {
                return Err(EmbedError::Config(format!(
                    "native module name '{}' is not an identifier",
                    name
                )));
            }
            if name == "std" || std_names.contains(&name.as_str()) {
                return Err(EmbedError::Config(format!(
                    "native module name '{}' is taken by the standard library",
                    name
                )));
            }
            if registry.contains_module(&name) {
                return Err(EmbedError::Config(format!(
                    "native module '{}' is registered twice",
                    name
                )));
            }
            registry.insert(module);
        }
        for path in self.embedded.keys() {
            if !path.ends_with(".que")
                || path.starts_with('/')
                || path.split('/').any(|c| c == ".." || c.is_empty())
            {
                return Err(EmbedError::Config(format!(
                    "embedded module path '{}' must be a relative path like 'kit/mod.que'",
                    path
                )));
            }
        }

        let host = HostContext {
            limits: Limits {
                cancel: Some(self.cancel.clone()),
                deadline: None,
            },
            tracer: self.tracer,
            natives: Some(Arc::new(registry)),
            isolated: true,
        };
        let mut interp = Interpreter::new();
        interp.strict = self.strict;
        interp.permissions = Some(self.policy.clone());
        interp.host = host;
        interp.set_script_args(self.args);
        interp.set_module_loader(ModuleLoader::embedded(
            self.search_paths,
            Arc::new(self.embedded),
        ));
        Ok(Engine {
            interp,
            policy: self.policy,
            timeout: self.timeout,
            cancel: self.cancel,
        })
    }
}

// ── Engine ───────────────────────────────────────────────────────────

/// A module's exports, from [`Engine::import`].
#[derive(Debug, Clone)]
pub struct ModuleHandle {
    /// The import path, e.g. `kit.license`.
    pub name: String,
    pub exports: BTreeMap<String, Value>,
}

/// One embedded interpreter. See the [module documentation](self).
///
/// Scripts run on the thread that calls into the engine. The engine is
/// `Send`, so it can be moved to a worker thread; it is not meant to be
/// shared between threads at the same time.
pub struct Engine {
    interp: Interpreter,
    policy: Policy,
    timeout: Option<Duration>,
    cancel: CancellationToken,
}

impl Engine {
    pub fn builder() -> EngineBuilder {
        EngineBuilder {
            policy: Policy::deny_all(),
            search_paths: Vec::new(),
            embedded: BTreeMap::new(),
            natives: Vec::new(),
            tracer: None,
            cancel: CancellationToken::new(),
            timeout: None,
            strict: false,
            args: Vec::new(),
        }
    }

    /// The token that cancels this engine's scripts. Clone it to another
    /// thread and call [`CancellationToken::cancel`] there.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Change the per-call wall-clock limit.
    pub fn set_timeout(&mut self, limit: Option<Duration>) {
        self.timeout = limit;
    }

    /// The policy scripts run under.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Run a script file: its top level executes, and its functions become
    /// callable with [`Engine::call`]. Local imports (`import .x`) resolve
    /// next to the file. Returns the value of the last top-level expression.
    pub fn run_file(&mut self, path: impl AsRef<Path>) -> Result<Value, EmbedError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|e| EmbedError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        self.run_with_path(path.to_path_buf(), &source)
    }

    /// Run a script given as text. `name` is its file name in error messages;
    /// it lives in the embedded module tree, so `import .x` finds embedded
    /// module `x.que`.
    pub fn run_source(&mut self, name: &str, source: &str) -> Result<Value, EmbedError> {
        self.run_with_path(Path::new(EMBEDDED_ROOT).join(name), source)
    }

    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    fn run_with_path(&mut self, path: PathBuf, source: &str) -> Result<Value, EmbedError> {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let module = crate::lexer::Lexer::new(source)
            .tokenize()
            .and_then(|tokens| crate::parser::Parser::new(tokens).parse_module())
            .map_err(|e| EmbedError::Script(ScriptError::from_que(e.with_file(file_name))))?;
        self.interp.set_script_path(path);
        self.guarded(|interp| interp.exec_module(&module))
    }

    /// Load a module by import path (`kit.license`, or a native module's
    /// name) and return its exports, without binding it in the script.
    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    pub fn import(&mut self, module_path: &str) -> Result<ModuleHandle, EmbedError> {
        let segments: Vec<String> = module_path.split('.').map(str::to_string).collect();
        if segments.iter().any(|s| s.is_empty()) {
            return Err(EmbedError::Config(format!(
                "'{}' is not a module path",
                module_path
            )));
        }
        let value = self.guarded(|interp| interp.import_module_value(&segments))?;
        let exports = match value {
            Value::Map(m) | Value::Module { entries: m, .. } => m,
            _ => BTreeMap::new(),
        };
        Ok(ModuleHandle {
            name: module_path.to_string(),
            exports,
        })
    }

    /// Call a function defined at the top level of the scripts run so far.
    pub fn call(&mut self, function: &str, args: Vec<Value>) -> Result<Value, EmbedError> {
        let callee = self
            .interp
            .env
            .get(function)
            .ok_or_else(|| EmbedError::FunctionNotFound(function.to_string()))?;
        self.call_named(function, callee, args)
    }

    /// Call a function exported by a module from [`Engine::import`].
    pub fn call_in(
        &mut self,
        module: &ModuleHandle,
        function: &str,
        args: Vec<Value>,
    ) -> Result<Value, EmbedError> {
        let qualified = format!("{}.{}", module.name, function);
        let callee = module
            .exports
            .get(function)
            .cloned()
            .ok_or(EmbedError::FunctionNotFound(qualified.clone()))?;
        self.call_named(&qualified, callee, args)
    }

    /// Call a function value (for example one a script returned).
    pub fn call_value(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, EmbedError> {
        self.call_named("<value>", callee.clone(), args)
    }

    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    fn call_named(
        &mut self,
        name: &str,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Value, EmbedError> {
        if !matches!(callee, Value::Function { .. } | Value::BuiltinFn(_)) {
            return Err(EmbedError::NotCallable {
                name: name.to_string(),
                type_name: callee.type_name().to_string(),
            });
        }
        self.guarded(|interp| interp.call_from_host(callee, args))
    }

    /// A top-level binding of the scripts run so far.
    pub fn global(&self, name: &str) -> Option<Value> {
        self.interp.env.get(name)
    }

    /// Take what scripts printed with `print`/`println` so far, one entry per
    /// line. Output is never written to the host's stdout.
    pub fn take_output(&mut self) -> Vec<String> {
        self.interp.flush_partial();
        std::mem::take(&mut self.interp.output)
    }

    /// Run `f` with the deadline armed, turning signals into results.
    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    fn guarded(
        &mut self,
        f: impl FnOnce(&mut Interpreter) -> Result<Value, Signal>,
    ) -> Result<Value, EmbedError> {
        self.interp.host.limits.deadline =
            self.timeout.map(|limit| (Instant::now() + limit, limit));
        // Checked up front so a token cancelled before the call stops it even
        // if the script never reaches a statement boundary.
        let result = self
            .interp
            .host
            .check_limits()
            .and_then(|()| f(&mut self.interp));
        self.interp.host.limits.deadline = None;
        match result {
            Ok(v) | Err(Signal::Return(v)) => Ok(v),
            Err(signal) => Err(EmbedError::from_signal(signal, self.timeout)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn an_engine_can_move_to_another_thread() {
        assert_send::<Engine>();
    }

    #[test]
    fn the_language_version_is_a_semantic_version() {
        let parts: Vec<&str> = LANGUAGE_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
        assert_eq!(language_version(), LANGUAGE_VERSION);
    }
}
