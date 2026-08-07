/// Tree-walking interpreter for the Que language.
///
/// Evaluates an AST produced by the parser, executing statements and
/// computing expressions in an environment that supports lexical scoping.

use crate::ast::*;
use crate::environment::Environment;
use crate::error::*;
use crate::lexer::Lexer;
use crate::module_loader::ModuleLoader;
use crate::parser::Parser;
use crate::value::Value;

use std::collections::BTreeMap;


mod builtins;
mod expr;
pub(crate) mod helpers;
mod methods;
mod patterns;
pub(crate) mod std_modules;
pub(crate) mod task_cache;
mod tasks;

pub use helpers::home_dir;

// ── Assignment path segments ─────────────────────────────────────────

enum AssignSegment {
    Field(String),
    Index(Value),
}

/// Whether an expression is syntactically rooted in a backtick literal.
///
/// This is what separates a command *written* at the end of a block —
/// `` `ls` `` or `` `ls`.dir(d) `` — from one merely *produced* there, such as
/// a bound `Cmd` value being returned. Only the former is in statement
/// position and therefore runs.
fn is_cmd_rooted(expr: &Expr) -> bool {
    match expr {
        Expr::CmdLit(_) => true,
        Expr::MethodCall { object, .. } | Expr::OptionalAccess { object, .. } => {
            is_cmd_rooted(object)
        }
        _ => false,
    }
}

/// Turn an `Err(payload)` value into a raised error.
///
/// Que has one error *value* (`Err`) and one error *channel* (`Signal::Error`).
/// This is the single conversion point between them, used wherever an `Err`
/// stops being inspectable and becomes a failure: statement position, the end
/// of a `try` block, and the `?` operator. Keeping it in one place is what
/// makes the two spellings behave identically.
pub(crate) fn err_value_to_error(payload: &Value) -> QueError {
    match payload {
        Value::String(s) => QueError::runtime(s.clone()),
        other => QueError::runtime(other.to_string()),
    }
}

// ── Interpreter ──────────────────────────────────────────────────────

/// A `parallel` branch runs on its own thread with its own `Interpreter`,
/// made by cloning this one. The clone shares the caller's variable scopes
/// (they live behind `Arc<Mutex<…>>` in `Environment`) but gets its own copy
/// of everything else, so a branch cannot disturb its siblings' call stack,
/// deferred list or output buffer.
#[derive(Clone)]
pub struct Interpreter {
    pub env: Environment,
    /// Captured output from print/println (only populated when direct_output is false).
    pub output: Vec<String>,
    /// Buffer for the in-progress line being built up by `print()` calls
    /// in buffered mode. Flushed to `output` when a newline arrives via
    /// `println()` (or another `print` that contains '\n').
    pub(crate) partial_line: String,
    /// When true, print/println write directly to stdout instead of buffering in `output`.
    pub direct_output: bool,
    /// Deferred expressions to run when the current block exits.
    deferred: Vec<Expr>,
    /// True while deferred expressions are being run. Interrupt polling is
    /// suppressed here: the signal that triggered the unwind is still pending,
    /// and re-raising it would abort the cleanup it exists to perform.
    in_cleanup: bool,
    /// Task execution status tracking: task_name -> (status, result_value)
    /// Status: "succeeded", "failed", "skipped"
    task_status: std::collections::HashMap<String, (String, Value)>,
    /// Task cache: task_name -> hash of (params, env vars) from last successful run.
    /// Used to detect when params or tracked env vars change even if files haven't.
    task_cache: std::collections::HashMap<String, u64>,
    /// On-disk record of the contents each task last consumed and produced,
    /// used when file timestamps are not trustworthy. Loaded on first task.
    task_cache_file: task_cache::TaskCache,
    /// Directory the on-disk task cache anchors to, when it must not follow
    /// the script. A global Quefile lives in the user's home but its tasks
    /// read and write files in whatever directory `que` was invoked from,
    /// so one shared cache next to the script would describe every project
    /// at once.
    task_cache_dir_override: Option<std::path::PathBuf>,
    /// Module loader: resolves imports, caches loaded modules, detects cycles.
    pub(crate) module_loader: Option<ModuleLoader>,
    /// Path of the script/module being executed (for resolving local imports).
    script_path: Option<std::path::PathBuf>,
    /// Current source location (updated from each item/statement's span
    /// during execution). Used to attach file/line info to runtime errors.
    current_span: Option<crate::token::Span>,
    /// Filename of the current script (short name only, for error messages).
    current_file: Option<String>,
    /// Struct field definitions: type_name → ordered list of field defs.
    pub(crate) struct_defs: std::collections::HashMap<String, Vec<crate::value::FieldDef>>,
    /// Enum definitions: enum_name → [(variant_name, [field_names])].
    pub(crate) enum_defs: std::collections::HashMap<String, Vec<(String, Vec<String>)>>,
    /// Reverse mapping: variant_name → enum_name (for constructor disambiguation).
    pub(crate) enum_variant_to_enum: std::collections::HashMap<String, String>,
    /// User-defined methods: type_name → list of method defs.
    pub(crate) impl_methods: std::collections::HashMap<String, Vec<crate::value::MethodDef>>,
    /// Trait implementations: (type_name, trait_name) → method defs.
    pub(crate) trait_impls: std::collections::HashMap<(String, String), Vec<crate::value::MethodDef>>,
    /// Trait definitions: trait_name → method signatures (with optional defaults).
    pub(crate) trait_defs: std::collections::HashMap<String, Vec<crate::value::TraitMethodDef>>,
    /// Call stack for backtrace reporting. Each entry is pushed on function/task entry
    /// and popped on return.
    call_stack: Vec<crate::error::CallFrame>,
    /// When true, type annotations on function params/returns and struct fields are enforced.
    pub strict: bool,
    /// When true, operations that change the world outside the process are
    /// announced instead of performed. See `Interpreter::dry_run_skip`.
    pub dry_run: bool,
    /// When true, tasks run even when their inputs and outputs say they are up
    /// to date. The result is still recorded, so the next run without this can
    /// skip again.
    pub force_run: bool,
    /// Capability policy, or `None` for unrestricted. Opt-in: a script with
    /// no policy behaves exactly as before. See `crate::permissions`.
    pub permissions: Option<crate::permissions::Policy>,
    /// Plaintext of every secret this run has produced, scrubbed from all
    /// emitted output. See `Interpreter::register_secret`.
    secrets: Vec<String>,
    /// Minimum log level: 0=DEBUG, 1=INFO, 2=WARN, 3=ERROR (default 0).
    log_level: u8,
    /// Default output format for log lines (default Text).
    log_format: helpers::LogFormat,
    /// Registered log sinks (default: single console sink with no overrides).
    log_sinks: Vec<helpers::LogSink>,
    /// Command-line arguments passed to the running script (everything after
    /// `--` on the `que script.que -- ...` command line). Empty for the REPL
    /// and for the `que run` task subcommand (tasks consume their own args).
    pub(crate) script_args: Vec<String>,
    /// What a `mut self` method left in `self`, waiting to be written back
    /// over the expression it was called on.
    ///
    /// An instance is a value, not a reference, so the method mutates a copy;
    /// the copy only becomes the caller's if someone stores it. The call site
    /// is the only place that knows *where* to store it, so the value is
    /// parked here for the one step between the two.
    pub(crate) pending_self_writeback: Option<Value>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Every global function name the interpreter binds at startup.
///
/// Shared with the resolver so static analysis and the runtime cannot
/// disagree about what counts as a defined name.
pub const BUILTIN_NAMES: &[&str] = &[
        "print", "println", "typeof", "str", "int", "float", "bool", "abs",
        "min", "max", "range", "chr", "ord",
        "Ok", "Err", "assert", "compose",
        // Additional builtins
        "secret", "fail", "sleep", "input", "confirm",
        "quefile_dir", "script_dir", "dry_run",
        "path", "cd", "dir",
        "glob",
        // File handle
        "open",
        // Tool discovery
        "which",
        // Retry and timeout
        "retry", "timeout",
        // Semver parse
        "semver_parse",
        // Regex constructor
        "regex",
        // Debugging aids you reach for mid-edit or at a REPL prompt
        "dbg", "help",
        // CLI arguments
        "args",
        // Task runner
        "tasks", "run_task",
        // Strict mode
        "strict",
        // Removed, but still bound so that calling one reaches the arm in
        // `call_builtin` that says what to write instead. Dropping the
        // binding too would turn a sentence naming the replacement into
        // "undefined variable", which tells nobody anything.
        "Some", "None", "env", "error", "assert_eq", "to_path",
        "len", "push", "pop", "keys", "values", "contains", "split", "trim",
        "join", "replace", "chars", "filter", "map", "fold", "flat_map",
        "group_by", "sort_by", "any", "all", "find", "zip", "enumerate",
        "take", "skip", "chunk", "partition", "flatten", "each", "for_each",
        "config_get", "config_set", "config_delete", "config_has",
        "config_merge", "config_paths", "config_read", "config_write",
        "stream", "stream_of", "stdout", "stderr", "stdin",
        "inspect", "methods", "is_type", "type_info", "fields", "has_method",
        "vars", "var_info", "scope_depth", "modules",
        "now",
        // Internal context-manager builtins (not user-visible, used by TempDir/TempFile/EnvScope impls)
        "__ctx_tempdir_enter", "__ctx_tempdir_exit",
        "__ctx_tempfile_enter", "__ctx_tempfile_exit",
        "__ctx_envscope_enter", "__ctx_envscope_exit",
        "__ctx_dir_enter", "__ctx_dir_exit",
];

impl Interpreter {
    pub fn new() -> Self {
        #[cfg(test)]
        colored::control::set_override(false);

        let mut interp = Self {
            env: Environment::new(),
            output: Vec::new(),
            partial_line: String::new(),
            direct_output: false,
            deferred: Vec::new(),
            in_cleanup: false,
            task_status: std::collections::HashMap::new(),
            task_cache: std::collections::HashMap::new(),
            task_cache_file: task_cache::TaskCache::default(),
            task_cache_dir_override: None,
            dry_run: false,
            force_run: false,
            permissions: None,
            secrets: Vec::new(),
            module_loader: None,
            script_path: None,
            current_span: None,
            current_file: None,
            struct_defs: std::collections::HashMap::new(),
            enum_defs: std::collections::HashMap::new(),
            enum_variant_to_enum: std::collections::HashMap::new(),
            impl_methods: std::collections::HashMap::new(),
            trait_impls: std::collections::HashMap::new(),
            trait_defs: std::collections::HashMap::new(),
            call_stack: Vec::new(),
            strict: false,
            log_level: 0,
            log_format: helpers::LogFormat::Text,
            log_sinks: vec![helpers::default_console_sink()],
            script_args: Vec::new(),
            pending_self_writeback: None,
        };
        interp.register_builtins();
        interp
    }

    /// Set the script/module path for this interpreter.
    /// Also updates `current_file` (filename only) used in error messages.
    pub fn set_script_path(&mut self, path: std::path::PathBuf) {
        self.current_file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.script_path = Some(path);
    }

    /// Anchor the on-disk task cache to `dir` instead of the script's own
    /// directory. Used for the global Quefile, whose tasks operate on the
    /// directory the user is standing in.
    pub fn set_task_cache_dir(&mut self, dir: std::path::PathBuf) {
        self.task_cache_dir_override = Some(dir);
    }

    /// Set the script's command-line arguments (returned by the `args()` builtin).
    pub fn set_script_args(&mut self, args: Vec<String>) {
        self.script_args = args;
    }

    /// Initialize the module loader from the script path.
    /// Call this before executing a module that may contain imports.
    pub fn init_module_loader(&mut self) {
        if self.module_loader.is_none() {
            let root = if let Some(ref sp) = self.script_path {
                let dir = sp.parent().unwrap_or(std::path::Path::new("."));
                crate::module_loader::find_package_root(dir)
            } else {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                crate::module_loader::find_package_root(&cwd)
            };
            self.module_loader = Some(ModuleLoader::new(root));
        }
    }

    /// Set the module loader (used by module_loader to share cache).
    pub fn set_module_loader(&mut self, loader: ModuleLoader) {
        self.module_loader = Some(loader);
    }

    /// Take the module loader out (used by module_loader to retrieve updated cache).
    pub fn take_module_loader(&mut self) -> ModuleLoader {
        self.module_loader.take().unwrap_or_else(|| {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            ModuleLoader::new(cwd)
        })
    }

    /// Announce an effect that a dry run must not perform.
    ///
    /// Returns `true` when the caller should skip the real work. The single
    /// choke point exists so the printed line and the decision to skip can
    /// never disagree.
    ///
    /// A dry run only suppresses *writes*: running a command, and the
    /// filesystem and network operations that change something. Reads run for
    /// real, because a script that cannot read cannot reach the decisions the
    /// dry run is meant to show.
    pub(crate) fn dry_run_skip(&mut self, action: impl std::fmt::Display) -> bool {
        if !self.dry_run {
            return false;
        }
        self.emit(format!("[dry-run] {}", action));
        true
    }

    /// Enforce the capability policy, if one is configured.
    ///
    /// Deliberately adjacent to `dry_run_skip`: both are single choke points
    /// for "may this effect happen", and keeping them together makes it hard
    /// to add one without considering the other.
    pub(crate) fn check_permission(
        &self,
        cap: crate::permissions::Capability,
        subject: &str,
    ) -> Result<(), Signal> {
        let policy = match &self.permissions {
            None => return Ok(()),
            Some(p) => p,
        };
        policy.check(cap, subject).map_err(|denied| {
            Signal::Error(QueError::new(ErrorKind::Runtime, denied.message()))
        })
    }

    /// Emit a line of output: write directly to stdout when `direct_output` is set,
    /// otherwise buffer in `self.output` (for tests and library use). Any
    /// partial line accumulated by `emit_partial` is flushed first so that
    /// `print("a"); println("b")` produces the single entry `"ab"`.
    pub(crate) fn emit(&mut self, line: String) {
        let line = self.redact(line);
        if self.direct_output {
            println!("{}", line);
        } else {
            let mut full = std::mem::take(&mut self.partial_line);
            full.push_str(&line);
            self.output.push(full);
        }
    }

    /// Remember a secret's plaintext so it can be scrubbed from output.
    ///
    /// Redacting `Value::Secret` at its own print sites is not enough: the
    /// moment a script calls `.expose()`, or a subprocess echoes the token
    /// back, the plaintext is an ordinary `String` that no type can catch.
    /// A value-independent scrub of everything on its way out is the only
    /// thing that survives that.
    pub(crate) fn register_secret(&mut self, plaintext: &str) {
        // A one- or two-character secret would turn output into confetti,
        // and is not a secret worth protecting anyway.
        if plaintext.len() < 4 || self.secrets.iter().any(|s| s == plaintext) {
            return;
        }
        self.secrets.push(plaintext.to_string());
    }

    /// Replace every registered secret in `text` with `<redacted>`.
    ///
    /// This deliberately does *not* touch values a script computes with:
    /// only text on its way to a human. Scrubbing captured stdout would
    /// corrupt a script that legitimately reads a token out of a command.
    pub(crate) fn redact(&self, text: String) -> String {
        if self.secrets.is_empty() {
            return text;
        }
        let mut out = text;
        for s in &self.secrets {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), crate::value::REDACTED);
            }
        }
        out
    }

    /// Emit text without an implicit trailing newline (the `print()` builtin).
    /// In direct mode this writes to stdout and flushes; in buffered mode the
    /// text is appended to `partial_line`. Any embedded `\n` in `s` splits
    /// the buffer at that point: each completed segment becomes its own
    /// `output` entry and the trailing fragment stays in `partial_line`.
    pub(crate) fn emit_partial(&mut self, s: &str) {
        let s = self.redact(s.to_string());
        let s = s.as_str();
        if self.direct_output {
            use std::io::Write;
            print!("{}", s);
            let _ = std::io::stdout().flush();
            return;
        }
        // Buffered mode: split on '\n' so embedded newlines flush lines.
        let mut iter = s.split('\n');
        let first = iter.next().unwrap_or("");
        self.partial_line.push_str(first);
        for chunk in iter {
            // Newline encountered: push completed line, start a new partial.
            let completed = std::mem::take(&mut self.partial_line);
            self.output.push(completed);
            self.partial_line.push_str(chunk);
        }
    }

    /// Flush any in-progress partial line to `output`. Called at the end
    /// of script execution so trailing `print()` output isn't lost.
    pub fn flush_partial(&mut self) {
        if !self.direct_output && !self.partial_line.is_empty() {
            let line = std::mem::take(&mut self.partial_line);
            self.output.push(line);
        }
    }

    fn register_builtins(&mut self) {
        // NOTE: Format-specific parsing/serialization (json.parse, json.stringify,
        // yaml.parse, etc.), file I/O (fs.read, fs.write, fs.exists), and HTTP
        // functions (http.get, http.post, etc.) are available only via
        // `import std.*`. See exec_std_import().
        for name in BUILTIN_NAMES {
            self.env
                .define(name, Value::BuiltinFn(name.to_string()), false);
        }

        // ── OS object with platform info ──
        let mut os_fields = BTreeMap::new();
        os_fields.insert("path_separator".into(), Value::String(
            if cfg!(windows) { ";".into() } else { ":".into() }
        ));
        os_fields.insert("dir_separator".into(), Value::String(
            std::path::MAIN_SEPARATOR.to_string(),
        ));
        os_fields.insert("family".into(), Value::String(std::env::consts::FAMILY.into()));
        os_fields.insert("name".into(), Value::String(std::env::consts::OS.into()));
        os_fields.insert("arch".into(), Value::String(std::env::consts::ARCH.into()));
        // os.exit(code) — exits the process with the given exit code
        os_fields.insert("exit".into(), Value::BuiltinFn("os.exit".into()));
        self.env.define("os", Value::Instance { type_name: "OsInfo".to_string(), fields: os_fields }, false);

        // ── Built-in Display trait ──
        // Any struct implementing `to_string(self) -> String` gets automatic
        // string coercion in println(), str(), and string interpolation.
        self.trait_defs.insert("Display".to_string(), vec![
            crate::value::TraitMethodDef {
                name: "to_string".to_string(),
                params: vec![Param { name: "self".to_string(), type_ann: None, default: None, rest: false }],
                default_body: None,
            },
        ]);

        // ── Built-in Eq trait ──
        // Implementing `equals(self, other) -> Bool` enables == and != dispatch
        // for user-defined types.
        self.trait_defs.insert("Eq".to_string(), vec![
            crate::value::TraitMethodDef {
                name: "equals".to_string(),
                params: vec![
                    Param { name: "self".to_string(), type_ann: None, default: None, rest: false },
                    Param { name: "other".to_string(), type_ann: None, default: None, rest: false },
                ],
                default_body: None,
            },
        ]);

        // ── Built-in Ord trait ──
        // Implementing `compare(self, other) -> Int` (returning -1, 0, or 1)
        // enables <, >, <=, >= dispatch for user-defined types.
        self.trait_defs.insert("Ord".to_string(), vec![
            crate::value::TraitMethodDef {
                name: "compare".to_string(),
                params: vec![
                    Param { name: "self".to_string(), type_ann: None, default: None, rest: false },
                    Param { name: "other".to_string(), type_ann: None, default: None, rest: false },
                ],
                default_body: None,
            },
        ]);

        // ── Built-in Hash trait ──
        // Implementing `hash(self) -> Int` is required for using a user-defined
        // type as a Map key or Set element.
        self.trait_defs.insert("Hash".to_string(), vec![
            crate::value::TraitMethodDef {
                name: "hash".to_string(),
                params: vec![Param { name: "self".to_string(), type_ann: None, default: None, rest: false }],
                default_body: None,
            },
        ]);

        // ── Built-in Contextual trait ──
        // Any struct implementing `enter(self) -> Any` and `exit(self, resource)`
        // can be used in `with expr as name { }` blocks.
        self.trait_defs.insert("Contextual".to_string(), vec![
            crate::value::TraitMethodDef {
                name: "enter".to_string(),
                params: vec![Param { name: "self".to_string(), type_ann: None, default: None, rest: false }],
                default_body: None,
            },
            crate::value::TraitMethodDef {
                name: "exit".to_string(),
                params: vec![
                    Param { name: "self".to_string(), type_ann: None, default: None, rest: false },
                    Param { name: "resource".to_string(), type_ann: None, default: None, rest: false },
                ],
                default_body: None,
            },
        ]);

        // ── Register stdlib context manager types ──
        // TempDir and TempFile are defined as Que structs with Contextual impls.
        self.register_stdlib_context_managers();
    }

    fn register_stdlib_context_managers(&mut self) {
        // ── TempDir ──
        let temp_dir_fields = vec![
            crate::value::FieldDef {
                name: "prefix".to_string(),
                default: Some(Value::String("que_".to_string())),
            },
            crate::value::FieldDef {
                name: "dir".to_string(),
                default: Some(Value::Null),  // None → use OS temp dir
            },
        ];
        self.struct_defs.insert("TempDir".to_string(), temp_dir_fields);
        self.env.define("TempDir", Value::TypeRef("TempDir".to_string()), false);

        // TempDir.new(prefix = "que_", dir = null) static method
        let td_new_src = r#"
fn new(prefix, dir) -> TempDir { TempDir { prefix, dir } }
"#;
        if let Ok(m) = Self::parse_single_method(td_new_src) {
            self.impl_methods.entry("TempDir".to_string()).or_default().push(m);
        }

        // Contextual impl for TempDir — enter and exit are Rust builtins via BuiltinFn
        self.register_builtin_contextual("TempDir", "__ctx_tempdir_enter", "__ctx_tempdir_exit");

        // ── TempFile ──
        let temp_file_fields = vec![
            crate::value::FieldDef {
                name: "prefix".to_string(),
                default: Some(Value::String("que_".to_string())),
            },
            crate::value::FieldDef {
                name: "suffix".to_string(),
                default: Some(Value::String("".to_string())),
            },
            crate::value::FieldDef {
                name: "dir".to_string(),
                default: Some(Value::Null),  // None → use OS temp dir
            },
        ];
        self.struct_defs.insert("TempFile".to_string(), temp_file_fields);
        self.env.define("TempFile", Value::TypeRef("TempFile".to_string()), false);

        self.register_builtin_contextual("TempFile", "__ctx_tempfile_enter", "__ctx_tempfile_exit");

        // ── EnvScope ── produced by `env.scope(map)`, used as `with env.scope({...}) { ... }`
        let env_scope_fields = vec![crate::value::FieldDef {
            name: "vars".to_string(),
            default: None,
        }];
        self.struct_defs.insert("EnvScope".to_string(), env_scope_fields);

        self.register_builtin_contextual("EnvScope", "__ctx_envscope_enter", "__ctx_envscope_exit");

        // ── Dir ── produced by `dir(path)`, used as `with dir(p"sub") { ... }`
        //
        // The scoped spelling of `cd`, built in so that the restore is not
        // something a script has to remember. `cd` stays: it is the primitive
        // this is made of, and a Quefile that wants every task rooted at the
        // project has to move once at load time, which no block can express.
        let dir_fields = vec![crate::value::FieldDef {
            name: "path".to_string(),
            default: None,
        }];
        self.struct_defs.insert("Dir".to_string(), dir_fields);

        self.register_builtin_contextual("Dir", "__ctx_dir_enter", "__ctx_dir_exit");
    }

    /// Parse a single `fn` declaration from Que source for use as a MethodDef.
    fn parse_single_method(src: &str) -> Result<crate::value::MethodDef, ()> {
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().map_err(|_| ())?;
        let mut parser = Parser::new(tokens);
        // Skip newlines then expect `fn`
        // Just parse the whole module and extract the first FnDecl
        let module = parser.parse_module().map_err(|_| ())?;
        for (_, item) in module.items {
            if let crate::ast::Item::FnDecl(decl) = item {
                let is_static = decl.params.first().map(|p| p.name != "self").unwrap_or(true);
                return Ok(crate::value::MethodDef {
                    name: decl.name,
                    is_static,
                    mutates_self: decl.mutates_self,
                    params: decl.params,
                    body: decl.body,
                    closure_env: crate::environment::Environment::new(),
                });
            }
        }
        Err(())
    }

    /// Register a pair of builtin functions as the Contextual impl for a type.
    /// The enter/exit builtins receive the instance as their first argument.
    fn register_builtin_contextual(&mut self, type_name: &str, enter_fn: &str, exit_fn: &str) {
        // We create MethodDef wrappers that call the builtins via the interpreter.
        // The builtin names must be flat (no dots) so they work as function calls.
        // We capture self.env as closure_env so the internal builtins are in scope.
        let enter_src = format!(r#"fn enter(self) {{ {}(self) }}"#, enter_fn);
        let exit_src = format!(r#"fn exit(self, resource) {{ {}(self, resource) }}"#, exit_fn);
        let closure_env = self.env.clone();
        for src in [enter_src, exit_src] {
            if let Ok(mut m) = Self::parse_single_method(&src) {
                m.closure_env = closure_env.clone();
                let key = (type_name.to_string(), "Contextual".to_string());
                self.trait_impls.entry(key).or_default().push(m);
            }
        }
    }

    // ── OOP helpers ──────────────────────────────────────────────────

    /// Compile a list of FnDecl into MethodDef values, capturing the current env.
    pub(crate) fn compile_methods(&self, decls: &[FnDecl]) -> Vec<crate::value::MethodDef> {
        decls.iter().map(|d| {
            let is_static = d.params.first().map(|p| p.name != "self").unwrap_or(true);
            crate::value::MethodDef {
                name: d.name.clone(),
                is_static,
                mutates_self: d.mutates_self,
                params: d.params.clone(),
                body: d.body.clone(),
                closure_env: self.env.clone(),
            }
        }).collect()
    }

    /// Look up an instance method for `type_name` named `method_name`.
    /// Searches impl_methods first, then all trait_impls for this type.
    pub(crate) fn find_instance_method(&self, type_name: &str, method_name: &str)
        -> Option<crate::value::MethodDef>
    {
        // Search impl_methods
        if let Some(methods) = self.impl_methods.get(type_name) {
            if let Some(m) = methods.iter().find(|m| m.name == method_name && !m.is_static) {
                return Some(m.clone());
            }
        }
        // Search trait_impls
        for ((tname, _trait_name), methods) in &self.trait_impls {
            if tname == type_name {
                if let Some(m) = methods.iter().find(|m| m.name == method_name && !m.is_static) {
                    return Some(m.clone());
                }
            }
        }
        None
    }

    /// Look up a static method for `type_name` named `method_name`.
    pub(crate) fn find_static_method(&self, type_name: &str, method_name: &str)
        -> Option<crate::value::MethodDef>
    {
        if let Some(methods) = self.impl_methods.get(type_name) {
            if let Some(m) = methods.iter().find(|m| m.name == method_name && m.is_static) {
                return Some(m.clone());
            }
        }
        for ((tname, _), methods) in &self.trait_impls {
            if tname == type_name {
                if let Some(m) = methods.iter().find(|m| m.name == method_name && m.is_static) {
                    return Some(m.clone());
                }
            }
        }
        None
    }

    /// Check whether `type_name` implements `trait_name`.
    pub(crate) fn implements_trait(&self, type_name: &str, trait_name: &str) -> bool {
        self.trait_impls.contains_key(&(type_name.to_string(), trait_name.to_string()))
    }

    /// Convert `val` to a display string, calling `to_string()` if the value
    /// is an Instance that implements the Display trait.
    pub(crate) fn display_value(&mut self, val: Value) -> Result<String, crate::error::Signal> {
        if let Value::Instance { ref type_name, .. } = val {
            let type_name = type_name.clone();
            if let Some(m) = self.find_instance_method(&type_name, "to_string") {
                let result = self.call_method_def(m, Some(val), vec![])?;
                return Ok(result.display_string());
            }
        }
        Ok(val.display_string())
    }

    /// If both values are Instances of the same type with an `Ord` impl,
    /// call `compare(self, other)` and convert the -1/0/1 result to an
    /// `Ordering`. Returns `None` when the type has no `compare` method.
    pub(crate) fn try_instance_compare(
        &mut self,
        left: &Value,
        right: &Value,
    ) -> Result<Option<std::cmp::Ordering>, crate::error::Signal> {
        if let (
            Value::Instance { type_name: ta, .. },
            Value::Instance { type_name: tb, .. },
        ) = (left, right)
        {
            if ta == tb {
                let ta = ta.clone();
                if let Some(m) = self.find_instance_method(&ta, "compare") {
                    let result = self.call_method_def(m, Some(left.clone()), vec![right.clone()])?;
                    let ord = match result {
                        Value::Int(n) if n < 0 => std::cmp::Ordering::Less,
                        Value::Int(n) if n > 0 => std::cmp::Ordering::Greater,
                        Value::Int(_) => std::cmp::Ordering::Equal,
                        other => return Err(crate::error::Signal::Error(crate::error::QueError::new(
                            crate::error::ErrorKind::TypeMismatch,
                            format!("Ord.compare() must return -1, 0, or 1, got {}", other.type_name()),
                        ))),
                    };
                    return Ok(Some(ord));
                }
            }
        }
        Ok(None)
    }

    /// Check equality between two values, dispatching to the language-level
    /// `equals()` method for Instance types that implement the `Eq` trait.
    /// For all other types falls back to Rust structural equality.
    pub(crate) fn interpreter_eq(&mut self, a: &Value, b: &Value) -> Result<bool, crate::error::Signal> {
        if let (Value::Instance { type_name: ta, .. }, Value::Instance { type_name: tb, .. }) = (a, b) {
            if ta == tb {
                let ta = ta.clone();
                if let Some(m) = self.find_instance_method(&ta, "equals") {
                    let result = self.call_method_def(m, Some(a.clone()), vec![b.clone()])?;
                    return Ok(result.is_truthy());
                }
            }
        }
        Ok(a == b)
    }

    /// Test whether `val` is in `items`, using language-level equality for
    /// Instance types.  Enforces that any Instance value implements the `Hash`
    /// trait — the contract guarantees a consistent hash alongside equality.
    pub(crate) fn set_contains(&mut self, items: &[Value], val: &Value) -> Result<bool, crate::error::Signal> {
        if let Value::Instance { type_name, .. } = val {
            if !self.implements_trait(type_name, "Hash") {
                return Err(crate::error::Signal::Error(crate::error::QueError::new(
                    crate::error::ErrorKind::TypeMismatch,
                    format!(
                        "type '{}' must implement the Hash trait to be used as a Set element or Map key",
                        type_name
                    ),
                )));
            }
        }
        for item in items {
            if self.interpreter_eq(item, val)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ── Core evaluation ──────────────────────────────────────────────

    /// Execute a complete module.
    ///
    /// The module body is a scope like any other, so top-level `defer`
    /// statements run when it exits — on success, on error, and on an
    /// interrupt.
    pub fn exec_module(&mut self, module: &Module) -> IResult {
        // `#!strict` turns type enforcement on and never off again. An import
        // may tighten the checking its caller runs under; it may not loosen it.
        if module.strict {
            self.strict = true;
        }
        let deferred_len = self.deferred.len();
        let mut last = Value::Null;
        for (span, item) in &module.items {
            self.current_span = Some(*span);
            if let Err(signal) = self.check_interrupt() {
                self.run_deferred(deferred_len);
                return Err(signal);
            }
            match self.exec_item(item) {
                Ok(v) => last = v,
                Err(Signal::Error(mut e)) => {
                    if e.span.is_none() {
                        if let Some(span) = self.current_span {
                            e.span = Some(span);
                        }
                    }
                    if e.file.is_none() {
                        e.file = self.current_file.clone();
                    }
                    if e.backtrace.is_empty() && !self.call_stack.is_empty() {
                        e.backtrace = self.call_stack.clone();
                    }
                    self.run_deferred(deferred_len);
                    return Err(Signal::Error(e));
                }
                Err(signal) => {
                    self.run_deferred(deferred_len);
                    return Err(signal);
                }
            }
        }
        self.run_deferred(deferred_len);
        Ok(last)
    }

    fn exec_item(&mut self, item: &Item) -> IResult {
        match item {
            Item::Stmt(stmt) => self.exec_stmt(stmt),
            Item::FnDecl(decl) => {
                // Create function with current environment (which may be updated after definition).
                // For recursion to work, we need the function to capture an environment
                // that includes itself. We achieve this by:
                // 1. Creating a function value with current env
                // 2. Defining it in the environment
                // 3. Creating a new function with the updated env that includes itself
                // This is a bit of a workaround but ensures recursive calls work.
                let func = Value::Function {
                    name: Some(decl.name.clone()),
                    params: decl.params.clone(),
                    return_type: decl.return_type.clone().map(Box::new),
                    body: decl.body.clone(),
                    closure_env: self.env.clone(),
                };
                self.env.define(&decl.name, func.clone(), false);

                // Now update with a function that has the updated environment (post-definition).
                // We need to capture the function name in its own closure.
                let func_v2 = Value::Function {
                    name: Some(decl.name.clone()),
                    params: decl.params.clone(),
                    return_type: decl.return_type.clone().map(Box::new),
                    body: decl.body.clone(),
                    closure_env: self.env.clone(),
                };
                self.env.set(&decl.name, func_v2).ok();
                Ok(Value::Null)
            }
            Item::TaskDecl(decl) => {
                // Tasks are stored as first-class Task values with metadata.
                let dep_names: Vec<String> = decl.depends_on.iter().filter_map(|e| {
                    if let Expr::Ident(name) = e { Some(name.clone()) } else { None }
                }).collect();
                let task = Value::Task(Box::new(crate::value::TaskData {
                    name: decl.name.clone(),
                    description: decl.description.clone(),
                    aliases: decl.aliases.clone(),
                    params: decl.params.clone(),
                    depends_on: dep_names,
                    inputs: decl.inputs.clone(),
                    outputs: decl.outputs.clone(),
                    env_keys: decl.env_keys.clone(),
                    body: decl.body.clone(),
                    closure_env: self.env.clone(),
                }));
                self.env.define(&decl.name, task.clone(), false);
                // Register each alias as an additional binding to the same Task value.
                for alias in &decl.aliases {
                    self.env.define(alias, task.clone(), false);
                }
                Ok(Value::Null)
            }
            Item::TypeDecl(_) => {
                // Type alias declarations are compile-time only; no runtime effect.
                Ok(Value::Null)
            }
            Item::EnumDecl(decl) => {
                // Register enum definition and bind the type name + unit variants in the environment.
                let mut variants: Vec<(String, Vec<String>)> = Vec::new();
                for v in &decl.variants {
                    let field_names: Vec<String> = v.fields.iter().map(|(n, _)| n.clone()).collect();
                    variants.push((v.name.clone(), field_names.clone()));
                    // Register reverse mapping: variant_name → enum_name
                    self.enum_variant_to_enum.insert(v.name.clone(), decl.name.clone());
                    // Bind unit variants (no fields) directly as values in the environment
                    if field_names.is_empty() {
                        self.env.define(
                            &v.name,
                            Value::Enum {
                                enum_name: decl.name.clone(),
                                variant: v.name.clone(),
                                fields: BTreeMap::new(),
                            },
                            false,
                        );
                    }
                }
                self.enum_defs.insert(decl.name.clone(), variants);
                // Bind the enum type name as a TypeRef so Enum.Variant(...) dispatch works
                self.env.define(&decl.name, Value::TypeRef(decl.name.clone()), false);
                Ok(Value::Null)
            }
            Item::StructDecl(decl) => {
                // Register field definitions with evaluated defaults.
                let mut field_defs = Vec::new();
                for f in &decl.fields {
                    let default = if let Some(expr) = &f.default {
                        Some(self.eval_expr(expr)?)
                    } else {
                        None
                    };
                    field_defs.push(crate::value::FieldDef {
                        name: f.name.clone(),
                        default,
                    });
                }
                self.struct_defs.insert(decl.name.clone(), field_defs);
                // Bind the type name in the environment so `TypeName.method()` works.
                self.env.define(&decl.name, Value::TypeRef(decl.name.clone()), false);
                Ok(Value::Null)
            }
            Item::ImplDecl(decl) => {
                let methods = self.compile_methods(&decl.methods);
                self.impl_methods
                    .entry(decl.type_name.clone())
                    .or_default()
                    .extend(methods);
                Ok(Value::Null)
            }
            Item::TraitDecl(decl) => {
                let trait_method_defs: Vec<crate::value::TraitMethodDef> = decl.methods.iter().map(|m| {
                    crate::value::TraitMethodDef {
                        name: m.name.clone(),
                        params: m.params.clone(),
                        default_body: m.default_body.clone(),
                    }
                }).collect();
                self.trait_defs.insert(decl.name.clone(), trait_method_defs);
                Ok(Value::Null)
            }
            Item::TraitImplDecl(decl) => {
                // Validate: check required methods are all provided (those without default).
                if let Some(trait_def) = self.trait_defs.get(&decl.trait_name).cloned() {
                    for required in &trait_def {
                        if required.default_body.is_none() {
                            let provided = decl.methods.iter().any(|m| m.name == required.name);
                            if !provided {
                                return Err(Signal::Error(QueError::new(
                                    ErrorKind::Runtime,
                                    format!(
                                        "impl {} for {}: missing required method '{}'",
                                        decl.trait_name, decl.type_name, required.name
                                    ),
                                )));
                            }
                        }
                    }
                    // Also register default methods that weren't overridden.
                    let mut defaults_to_add: Vec<crate::value::MethodDef> = Vec::new();
                    for default_method in &trait_def {
                        if let Some(body) = &default_method.default_body {
                            let overridden = decl.methods.iter().any(|m| m.name == default_method.name);
                            if !overridden {
                                let is_static = default_method.params.first()
                                    .map(|p| p.name != "self")
                                    .unwrap_or(true);
                                defaults_to_add.push(crate::value::MethodDef {
                                    name: default_method.name.clone(),
                                    is_static,
                                    // A trait's default body is shared by every
                                    // implementor, so it cannot claim the
                                    // receiver of any one of them.
                                    mutates_self: false,
                                    params: default_method.params.clone(),
                                    body: body.clone(),
                                    closure_env: self.env.clone(),
                                });
                            }
                        }
                    }
                    let key = (decl.type_name.clone(), decl.trait_name.clone());
                    self.trait_impls.entry(key.clone()).or_default().extend(defaults_to_add);
                }
                let methods = self.compile_methods(&decl.methods);
                let key = (decl.type_name.clone(), decl.trait_name.clone());
                self.trait_impls.entry(key).or_default().extend(methods);
                Ok(Value::Null)
            }
            Item::PubLet { pattern, value, .. } => {
                let val = self.eval_expr(value)?;
                self.bind_pattern(pattern, val, false)?;
                Ok(Value::Null)
            }
            Item::Import(decl) => {
                self.exec_import(decl)
            }
        }
    }

    /// Execute an import declaration: resolve, load, and bind names.
    fn exec_import(&mut self, decl: &ImportDecl) -> IResult {
        // Ensure the module loader is initialized
        self.init_module_loader();

        // Handle `std` imports as a special case — std modules are built-in
        // functions in v0.1, not actual files. We build a synthetic module map.
        if !decl.is_local && !decl.path.is_empty() && decl.path[0] == "std" {
            return self.exec_std_import(decl);
        }

        // Dot-prefixed imports are resolved relative to the directory of the
        // file containing the import statement, not the project root. This lets
        // a package's mod.que use `import .sibling` to reach a sibling file in
        // the same directory, regardless of where the package is installed.
        let caller_dir: std::path::PathBuf = if let Some(ref sp) = self.script_path {
            sp.parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        } else {
            // REPL or no script path: fall back to package root
            self.module_loader
                .as_ref()
                .map(|l| l.package_root().to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
        };

        // Use the module loader to resolve and load
        let mut loader = self.module_loader.take().unwrap();
        loader.direct_output = self.direct_output;
        let result = loader.load_import(decl, &caller_dir);
        // Drain any output generated by module loading (println in sub-modules).
        // When direct_output is true the sub-interpreter already printed, so this is empty.
        self.output.append(&mut loader.pending_output);
        // Merge struct metadata from exported types into this interpreter
        for (name, fields) in loader.pending_struct_defs.drain() {
            self.struct_defs.insert(name, fields);
        }
        for (name, variants) in loader.pending_enum_defs.drain() {
            self.enum_defs.insert(name, variants);
        }
        for (variant_name, enum_name) in loader.pending_enum_variant_to_enum.drain() {
            self.enum_variant_to_enum.insert(variant_name, enum_name);
        }
        for (name, methods) in loader.pending_impl_methods.drain() {
            self.impl_methods.entry(name).or_default().extend(methods);
        }
        for (key, methods) in loader.pending_trait_impls.drain() {
            self.trait_impls.entry(key).or_default().extend(methods);
        }
        self.module_loader = Some(loader);

        let loaded = result.map_err(|e| Signal::Error(e))?;

        // Bind the loaded modules/names into the current environment
        if loaded.len() == 1 && loaded[0].0 == "__selective__" {
            // Selective imports: `import mod { fn1, fn2 }` or wildcard `import mod { * }`
            let module_map = &loaded[0].1;
            if let Value::Map(ref map) = module_map {
                if let Some(ref items) = decl.items {
                    if items.len() == 1 && items[0] == "*" {
                        // Wildcard: bind every export directly into scope
                        for (key, val) in map {
                            self.env.define(key, val.clone(), false);
                        }
                    } else {
                        for item_name in items {
                            if let Some(val) = map.get(item_name) {
                                self.env.define(item_name, val.clone(), false);
                            } else {
                                return Err(Signal::Error(QueError::new(
                                    ErrorKind::KeyNotFound,
                                    format!(
                                        "'{}' is not exported by module '{}'",
                                        item_name,
                                        decl.path.join(".")
                                    ),
                                )));
                            }
                        }
                    }
                }
            }
        } else {
            // Module binding: each (name, value) pair gets defined
            for (name, value) in loaded {
                self.env.define(&name, value, false);
            }
        }

        Ok(Value::Null)
    }

    /// Handle `import std.*` — build synthetic module maps from built-in functions.
    /// Std modules provide namespaced access to builtins. Some builtins (fs, json,
    /// yaml, toml, http) are *only* available via std imports to keep the global
    /// namespace clean. Others (path, env, math) are also global for convenience.
    fn exec_std_import(&mut self, decl: &ImportDecl) -> IResult {
        let modules = std_modules::all_modules();

        // Helper: build a module map from a StdModule descriptor.
        // Each bare function name gets a fully-qualified BuiltinFn value:
        // e.g. StdModule { name: "hash", functions: ["sha256", ...] }
        //  → { "sha256": BuiltinFn("hash.sha256"), ... }
        let build_mod_map = |module: &std_modules::StdModule| -> BTreeMap<String, Value> {
            module.functions.iter().map(|&fn_name| {
                let fq_name = format!("{}.{}", module.name, fn_name);
                (fn_name.to_string(), Value::BuiltinFn(fq_name))
            }).collect()
        };

        let find_module = |name: &str| -> Option<&std_modules::StdModule> {
            modules.iter().find(|m| m.name == name)
        };

        // `import std.{fs, path}` — multi-module shorthand (path=["std"], items=["fs","path"])
        if let Some(ref items) = decl.items {
            if decl.path.len() == 1 {
                // items are sub-module names under std
                for item_name in items {
                    if let Some(module) = find_module(item_name) {
                        let mod_map = build_mod_map(module);
                        self.env.define(item_name, Value::Module { name: item_name.clone(), entries: mod_map }, false);
                    } else {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::KeyNotFound,
                            format!("no std module named '{}'", item_name),
                        )));
                    }
                }
                return Ok(Value::Null);
            }

            // `import std.fs { readText }` or `import std.fs { * }` — selective from a std sub-module
            if decl.path.len() >= 2 {
                let mod_name = &decl.path[1];
                if let Some(module) = find_module(mod_name) {
                    let mod_map = build_mod_map(module);
                    if items.len() == 1 && items[0] == "*" {
                        // Wildcard: bind every function in the std sub-module
                        for (key, val) in &mod_map {
                            self.env.define(key, val.clone(), false);
                        }
                    } else {
                        for item_name in items {
                            if let Some(val) = mod_map.get(item_name) {
                                self.env.define(item_name, val.clone(), false);
                            } else {
                                return Err(Signal::Error(QueError::new(
                                    ErrorKind::KeyNotFound,
                                    format!("'{}' is not in std.{}", item_name, mod_name),
                                )));
                            }
                        }
                    }
                    return Ok(Value::Null);
                } else {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::KeyNotFound,
                        format!("no std module named '{}'", mod_name),
                    )));
                }
            }
        }

        // `import std` → import everything as a "std" namespace
        if decl.path.len() == 1 {
            let mut std_entries = BTreeMap::new();
            for module in &modules {
                let mod_map = build_mod_map(module);
                std_entries.insert(module.name.to_string(), Value::Module { name: module.name.to_string(), entries: mod_map });
            }
            let name = decl.alias.as_deref().unwrap_or("std");
            self.env.define(name, Value::Module { name: name.to_string(), entries: std_entries }, false);
            return Ok(Value::Null);
        }

        // `import std.fs` or `import std.fs as io` — single std sub-module
        if decl.path.len() >= 2 {
            let mod_name = &decl.path[1];
            if let Some(module) = find_module(mod_name) {
                let mod_map = build_mod_map(module);
                let bind_name = decl.alias.as_deref().unwrap_or(mod_name);
                let module_val = Value::Module { name: bind_name.to_string(), entries: mod_map };
                self.env.define(bind_name, module_val, false);
                return Ok(Value::Null);
            } else {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::KeyNotFound,
                    format!("no std module named '{}'", mod_name),
                )));
            }
        }

        Ok(Value::Null)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> IResult {
        match stmt {
            Stmt::Let {
                pattern,
                value,
                ..
            } => {
                let val = self.eval_expr(value)?;
                self.bind_pattern(pattern, val, false)?;
                Ok(Value::Null)
            }
            Stmt::Mut { name, value, .. } => {
                let val = self.eval_expr(value)?;
                self.env.define(name, val, true);
                Ok(Value::Null)
            }
            Stmt::Expr(expr) => {
                let val = self.eval_expr(expr)?;
                match val {
                    // A command in statement position runs, and raises if it fails.
                    // Binding it (`let c = `cmd``) keeps it lazy so builder methods
                    // like `.dir()` and `.env()` still work.
                    Value::Cmd(parts, mods) => self.run_cmd_in_stmt_position(&parts, *mods),
                    // An `Err` that nobody bound, matched on or unwrapped is an
                    // unhandled error. Dropping it silently is how a script gets
                    // to step 10 after step 3 failed.
                    Value::Err(payload) => Err(Signal::Error(err_value_to_error(&payload))),
                    other => Ok(other),
                }
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Null,
                };
                Err(Signal::Return(val))
            }
            Stmt::Break(expr) => {
                let val = expr.as_ref().map(|e| self.eval_expr(e)).transpose()?;
                Err(Signal::Break(val))
            }
            Stmt::Continue => Err(Signal::Continue),
            Stmt::For {
                pattern,
                iterable,
                body,
            } => {
                let iter_val = self.eval_expr(iterable)?;
                let items = self.value_to_iterable(iter_val)?;
                for item in items {
                    self.check_interrupt()?;
                    self.env.push_scope();
                    self.bind_pattern(pattern, item, false)?;
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Signal::Break(_)) => {
                            self.env.pop_scope();
                            break;
                        }
                        Err(Signal::Continue) => {
                            self.env.pop_scope();
                            continue;
                        }
                        Err(signal) => {
                            self.env.pop_scope();
                            return Err(signal);
                        }
                    }
                    self.env.pop_scope();
                }
                Ok(Value::Null)
            }
            Stmt::While { condition, body } => {
                loop {
                    self.check_interrupt()?;
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_truthy() {
                        break;
                    }
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Signal::Break(_)) => break,
                        Err(Signal::Continue) => continue,
                        Err(signal) => return Err(signal),
                    }
                }
                Ok(Value::Null)
            }
            Stmt::Loop { body } => {
                let result = loop {
                    self.check_interrupt()?;
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Signal::Break(val)) => break val.unwrap_or(Value::Null),
                        Err(Signal::Continue) => continue,
                        Err(signal) => return Err(signal),
                    }
                };
                Ok(result)
            }

            Stmt::Defer(expr) => {
                self.deferred.push(expr.clone());
                Ok(Value::Null)
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                let result = self.eval_block_scoped(try_body);
                // `Err` is an ordinary value, but a `try` block is the boundary
                // where it re-enters the raising channel. Without this, the two
                // error channels never meet and `try { might_fail() }` falls
                // straight through every `catch`.
                let result = match result {
                    Ok(Value::Err(payload)) => {
                        Err(Signal::Error(err_value_to_error(&payload)))
                    }
                    other => other,
                };
                let final_result = match result {
                    Ok(v) => Ok(v),
                    Err(Signal::Error(err)) => {
                        let mut caught = false;
                        let mut catch_result = Ok(Value::Null);
                        for clause in catches {
                            // In v0.1, error_type is checked as substring match on error kind name.
                            // If no error_type specified, catch all.
                            let matches = match &clause.error_type {
                                None => true,
                                Some(ty) => {
                                    let kind_str = format!("{:?}", err.kind);
                                    kind_str.contains(ty.as_str())
                                        || err.message.contains(ty.as_str())
                                }
                            };
                            if matches {
                                self.env.push_scope();
                                if let Some(ref binding) = clause.binding {
                                    self.env.define(
                                        binding,
                                        Value::String(err.message.clone()),
                                        false,
                                    );
                                }
                                catch_result = self.eval_block(&clause.body);
                                self.env.pop_scope();
                                caught = true;
                                break;
                            }
                        }
                        if caught {
                            catch_result
                        } else {
                            Err(Signal::Error(err))
                        }
                    }
                    Err(other) => Err(other),
                };
                if let Some(finally) = finally_body {
                    let _ = self.eval_block_scoped(finally);
                }
                final_result
            }
            Stmt::Assign { target, value } => {
                let val = self.eval_expr(value)?;
                self.assign_target(target, val)?;
                Ok(Value::Null)
            }
            Stmt::CompoundAssign { target, op, value } => {
                let current = self.eval_expr(target)?;
                let rhs = self.eval_expr(value)?;
                let new_val = self.eval_binary_op(*op, &current, &rhs)?;
                self.assign_target(target, new_val)?;
                Ok(Value::Null)
            }
        }
    }

    pub(crate) fn assign_target(&mut self, target: &Expr, value: Value) -> Result<(), Signal> {
        match target {
            Expr::Ident(name) => self.env.set(name, value).map_err(|msg| {
                Signal::Error(QueError::new(ErrorKind::ImmutableVariable, msg))
            }),
            Expr::Index { .. } | Expr::FieldAccess { .. } => {
                let (root_name, segments) = self.extract_assign_path(target)?;
                let root_val = self.env.get(&root_name).ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::UndefinedVariable,
                        format!("undefined variable '{}'", root_name),
                    ))
                })?;
                let new_root = Self::set_nested(&root_val, &segments, value)?;
                self.env.set(&root_name, new_root).map_err(|msg| {
                    Signal::Error(QueError::new(ErrorKind::ImmutableVariable, msg))
                })
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::InvalidAssignmentTarget,
                "invalid assignment target",
            ))),
        }
    }

    /// Walk a FieldAccess / Index chain to extract the root variable name
    /// and a list of path segments (field names or evaluated index values).
    fn extract_assign_path(&mut self, expr: &Expr) -> Result<(String, Vec<AssignSegment>), Signal> {
        let mut segments = Vec::new();
        let mut current = expr;
        loop {
            match current {
                Expr::FieldAccess { object, field } => {
                    segments.push(AssignSegment::Field(field.clone()));
                    current = object;
                }
                Expr::Index { object, index } => {
                    let idx = self.eval_expr(index)?;
                    segments.push(AssignSegment::Index(idx));
                    current = object;
                }
                Expr::Ident(name) => {
                    segments.reverse();
                    return Ok((name.clone(), segments));
                }
                _ => {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::InvalidAssignmentTarget,
                        "invalid assignment target",
                    )));
                }
            }
        }
    }

    /// Recursively clone-and-modify a value at the given path of segments.
    fn set_nested(root: &Value, segments: &[AssignSegment], new_val: Value) -> Result<Value, Signal> {
        if segments.is_empty() {
            return Ok(new_val);
        }
        let (head, tail) = segments.split_first().unwrap();
        match head {
            AssignSegment::Field(key) => match root {
                Value::Map(map) => {
                    let mut new_map = map.clone();
                    let child = map.get(key).cloned().unwrap_or_else(|| {
                        if tail.is_empty() { Value::Null } else { Value::Map(BTreeMap::new()) }
                    });
                    new_map.insert(key.clone(), Self::set_nested(&child, tail, new_val)?);
                    Ok(Value::Map(new_map))
                }
                Value::Instance { type_name, fields } => {
                    let mut new_fields = fields.clone();
                    let child = fields.get(key).cloned().unwrap_or_else(|| {
                        if tail.is_empty() { Value::Null } else { Value::Map(BTreeMap::new()) }
                    });
                    new_fields.insert(key.clone(), Self::set_nested(&child, tail, new_val)?);
                    Ok(Value::Instance { type_name: type_name.clone(), fields: new_fields })
                }
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!("cannot set field '{}' on {}", key, root.type_name()),
                ))),
            },
            AssignSegment::Index(idx) => match (root, idx) {
                (Value::List(list), Value::Int(i)) => {
                    let i = *i as usize;
                    if i >= list.len() {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::IndexOutOfBounds,
                            format!("index {} out of bounds (len {})", i, list.len()),
                        )));
                    }
                    let mut new_list = list.clone();
                    new_list[i] = Self::set_nested(&list[i], tail, new_val)?;
                    Ok(Value::List(new_list))
                }
                (Value::Map(map), Value::String(k)) => {
                    let mut new_map = map.clone();
                    let child = map.get(k).cloned().unwrap_or_else(|| {
                        if tail.is_empty() { Value::Null } else { Value::Map(BTreeMap::new()) }
                    });
                    new_map.insert(k.clone(), Self::set_nested(&child, tail, new_val)?);
                    Ok(Value::Map(new_map))
                }
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    "invalid index assignment target",
                ))),
            },
        }
    }


    // ── Block evaluation ─────────────────────────────────────────────

    /// Evaluate a block **without** creating a new scope (caller manages scope).
    pub(crate) fn eval_block(&mut self, block: &Block) -> IResult {
        let deferred_len = self.deferred.len();
        let mut last = Value::Null;
        for (span, stmt) in &block.stmts {
            self.current_span = Some(*span);
            if let Err(signal) = self.check_interrupt() {
                self.run_deferred(deferred_len);
                return Err(signal);
            }
            match self.exec_stmt(stmt) {
                Ok(v) => last = v,
                Err(Signal::Error(mut e)) => {
                    // Attach file + line to errors that don't already carry location info.
                    if e.span.is_none() {
                        if let Some(span) = self.current_span {
                            e.span = Some(span);
                        }
                    }
                    if e.file.is_none() {
                        e.file = self.current_file.clone();
                    }
                    if e.backtrace.is_empty() && !self.call_stack.is_empty() {
                        e.backtrace = self.call_stack.clone();
                    }
                    self.run_deferred(deferred_len);
                    return Err(Signal::Error(e));
                }
                Err(signal) => {
                    self.run_deferred(deferred_len);
                    return Err(signal);
                }
            }
        }
        // Update span for the trailing expression so errors point to the right location.
        // current_span was already set by the last statement in stmts (if any).
        let result = if let Some(expr) = &block.expr {
            let evaluated = self.eval_expr(expr);
            // The parser turns a block's last statement into its trailing
            // expression, but a backtick literal written on its own is still a
            // command in statement position: it has to run. Only syntactically
            // command-rooted expressions run, so returning a bound `Cmd` value
            // (`let c = `cmd`` … `c`) stays lazy.
            let evaluated = match evaluated {
                Ok(Value::Cmd(parts, mods)) if is_cmd_rooted(expr) => {
                    self.run_cmd_in_stmt_position(&parts, *mods)
                }
                other => other,
            };
            match evaluated {
                Ok(v) => v,
                Err(Signal::Error(mut e)) => {
                    if e.span.is_none() {
                        if let Some(span) = self.current_span {
                            e.span = Some(span);
                        }
                    }
                    if e.file.is_none() {
                        e.file = self.current_file.clone();
                    }
                    if e.backtrace.is_empty() && !self.call_stack.is_empty() {
                        e.backtrace = self.call_stack.clone();
                    }
                    self.run_deferred(deferred_len);
                    return Err(Signal::Error(e));
                }
                Err(signal) => {
                    self.run_deferred(deferred_len);
                    return Err(signal);
                }
            }
        } else {
            last
        };
        self.run_deferred(deferred_len);
        Ok(result)
    }

    /// Run a command that reached statement position: nothing consumes its
    /// output, so stream it to the terminal and raise if it fails.
    fn run_cmd_in_stmt_position(
        &mut self,
        parts: &[crate::value::CmdPart],
        mut mods: crate::value::CmdModifiers,
    ) -> IResult {
        // Stream to the terminal instead of capturing into a dropped value.
        // An attached command already writes to the terminal itself, so
        // wrapping it in sinks would only get in the way.
        if !mods.silent && !mods.attach {
            mods.forward_stdout
                .get_or_insert_with(|| Box::new(crate::value::StreamSink::Stdout));
            mods.forward_stderr
                .get_or_insert_with(|| Box::new(crate::value::StreamSink::Stderr));
        }
        self.run_cmd_checked(parts, &mods)
    }

    /// Evaluate a block in a new scope.
    pub(crate) fn eval_block_scoped(&mut self, block: &Block) -> IResult {
        self.env.push_scope();
        let result = self.eval_block(block);
        self.env.pop_scope();
        result
    }

    fn run_deferred(&mut self, start: usize) {
        // Cleanup must complete even when a signal is what triggered it.
        let was_in_cleanup = self.in_cleanup;
        self.in_cleanup = true;
        while self.deferred.len() > start {
            if let Some(expr) = self.deferred.pop() {
                // `defer `cmd`` is a command in statement position too — the
                // whole point of deferring it is to have it run.
                match self.eval_expr(&expr) {
                    Ok(Value::Cmd(parts, mods)) if is_cmd_rooted(&expr) => {
                        let _ = self.run_cmd_in_stmt_position(&parts, *mods);
                    }
                    _ => {}
                }
            }
        }
        self.in_cleanup = was_in_cleanup;
    }

    /// Poll for a pending SIGINT/SIGTERM.
    ///
    /// Called at statement boundaries and once per loop iteration. Returning
    /// `Signal::Interrupted` unwinds through `eval_block`, which runs every
    /// `defer` on the way out.
    pub(crate) fn check_interrupt(&self) -> Result<(), Signal> {
        if self.in_cleanup {
            return Ok(());
        }
        match crate::interrupt::pending_signal() {
            Some(sig) => Err(Signal::Interrupted(sig)),
            None => Ok(()),
        }
    }


    // ── Helpers ──────────────────────────────────────────────────────

    fn value_to_iterable(&self, val: Value) -> Result<Vec<Value>, Signal> {
        match val {
            Value::List(items) => Ok(items),
            Value::Set(items) => Ok(items),
            Value::Map(map) => Ok(map
                .into_iter()
                .map(|(k, v)| {
                    Value::Tuple(vec![Value::String(k), v])
                })
                .collect()),
            Value::String(s) => Ok(s
                .chars()
                .map(|c| Value::String(c.to_string()))
                .collect()),
            Value::Tuple(items) => Ok(items),
            Value::Glob(pattern) => {
                // Expand glob pattern against filesystem
                Ok(helpers::glob_expand(&pattern)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| Value::Path(entry.to_string_lossy().into_owned()))
                    .collect())
            }
            Value::Path(p) => {
                let path = std::path::Path::new(&p);
                if path.is_dir() {
                    match std::fs::read_dir(&p) {
                        Ok(entries) => {
                            let mut items = Vec::new();
                            for entry in entries.flatten() {
                                items.push(Value::Path(
                                    entry.path().to_string_lossy().to_string(),
                                ));
                            }
                            Ok(items)
                        }
                        Err(e) => Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            format!("Cannot iterate path '{}': {}", p, e),
                        ))),
                    }
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::NotIterable,
                        format!("Path '{}' is not a directory", p),
                    )))
                }
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::NotIterable,
                format!("{} is not iterable", val.type_name()),
            ))),
        }
    }
}

// ── Helper to run source code end-to-end ─────────────────────────────

/// Convenience: lex → parse → interpret a source string.
pub fn run(source: &str) -> Result<(Vec<String>, Value), QueError> {
    // Disable ANSI colors when used as a library (tests, embedding, etc.)
    colored::control::set_override(false);

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module()?;
    let mut interp = Interpreter::new();
    let result = interp.exec_module(&module);
    interp.flush_partial();
    match result {
        Ok(val) => Ok((interp.output, val)),
        Err(Signal::Error(e)) => Err(e),
        Err(Signal::Return(v)) => Ok((interp.output, v)),
        Err(Signal::Break(_)) => Err(QueError::runtime("break outside of loop")),
        Err(Signal::Continue) => Err(QueError::runtime("continue outside of loop")),
        // In the library/test context, represent exit as an error so callers can observe it.
        Err(Signal::Exit(code)) => Err(QueError::runtime(format!("exit({})", code))),
        Err(Signal::Interrupted(sig)) => Err(QueError::runtime(format!(
            "interrupted by signal {}", sig
        ))
        .with_exit_code(crate::interrupt::exit_code_for(sig))),
    }
}

/// Convenience function: execute source with strict type checking enabled.
pub fn run_strict(source: &str) -> Result<(Vec<String>, Value), QueError> {
    colored::control::set_override(false);

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module()?;
    let mut interp = Interpreter::new();
    interp.strict = true;
    let result = interp.exec_module(&module);
    interp.flush_partial();
    match result {
        Ok(val) => Ok((interp.output, val)),
        Err(Signal::Error(e)) => Err(e),
        Err(Signal::Return(v)) => Ok((interp.output, v)),
        Err(Signal::Break(_)) => Err(QueError::runtime("break outside of loop")),
        Err(Signal::Continue) => Err(QueError::runtime("continue outside of loop")),
        Err(Signal::Exit(code)) => Err(QueError::runtime(format!("exit({})", code))),
        Err(Signal::Interrupted(sig)) => Err(QueError::runtime(format!(
            "interrupted by signal {}", sig
        ))
        .with_exit_code(crate::interrupt::exit_code_for(sig))),
    }
}

// ── Unit tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::token::DurationUnit;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Helper: run source, return output lines + result.
    fn eval(source: &str) -> (Vec<String>, Value) {
        run(source).expect("execution failed")
    }

    /// Helper: run source, return only the result value.
    fn eval_val(source: &str) -> Value {
        eval(source).1
    }

    /// Helper: run source, return only the output lines.
    fn eval_out(source: &str) -> Vec<String> {
        eval(source).0
    }

    /// Helper: run source that is expected to fail, return the message.
    fn eval_err(source: &str) -> String {
        match run(source) {
            Ok(_) => panic!("expected an error from: {}", source),
            Err(e) => e.to_string(),
        }
    }

    fn eval_file(path: &Path) -> Value {
        let source = fs::read_to_string(path).expect("read script");
        let mut lexer = Lexer::new(&source);
        let tokens = lexer.tokenize().expect("lexer failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parser failed");

        let mut interp = Interpreter::new();
        interp.set_script_path(path.to_path_buf());
        interp.init_module_loader();
        interp.exec_module(&module).expect("execution failed")
    }

    // ── Literals & Arithmetic ──

    #[test]
    fn int_literal() {
        assert_eq!(eval_val("42"), Value::Int(42));
    }

    #[test]
    fn float_literal() {
        assert_eq!(eval_val("3.14"), Value::Float(3.14));
    }

    #[test]
    fn bool_literal() {
        assert_eq!(eval_val("true"), Value::Bool(true));
        assert_eq!(eval_val("false"), Value::Bool(false));
    }

    #[test]
    fn null_literal() {
        assert_eq!(eval_val("null"), Value::Null);
    }

    #[test]
    fn string_literal() {
        assert_eq!(eval_val("\"hello\""), Value::String("hello".into()));
    }

    #[test]
    fn arithmetic_add() {
        assert_eq!(eval_val("2 + 3"), Value::Int(5));
        assert_eq!(eval_val("1.5 + 2.5"), Value::Float(4.0));
    }

    #[test]
    fn arithmetic_sub() {
        assert_eq!(eval_val("10 - 3"), Value::Int(7));
    }

    #[test]
    fn arithmetic_mul() {
        assert_eq!(eval_val("4 * 5"), Value::Int(20));
    }

    #[test]
    fn arithmetic_div() {
        assert_eq!(eval_val("10 / 3"), Value::Int(3));
        assert_eq!(eval_val("10.0 / 3.0"), Value::Float(10.0 / 3.0));
    }

    #[test]
    fn arithmetic_mod() {
        assert_eq!(eval_val("10 % 3"), Value::Int(1));
    }

    #[test]
    fn arithmetic_pow() {
        assert_eq!(eval_val("2 ** 10"), Value::Int(1024));
    }

    #[test]
    fn division_by_zero() {
        assert!(run("1 / 0").is_err());
    }

    #[test]
    fn comparison_operators() {
        assert_eq!(eval_val("1 < 2"), Value::Bool(true));
        assert_eq!(eval_val("2 > 3"), Value::Bool(false));
        assert_eq!(eval_val("1 <= 1"), Value::Bool(true));
        assert_eq!(eval_val("2 >= 3"), Value::Bool(false));
        assert_eq!(eval_val("1 == 1"), Value::Bool(true));
        assert_eq!(eval_val("1 != 2"), Value::Bool(true));
    }

    #[test]
    fn logical_operators() {
        assert_eq!(eval_val("true && false"), Value::Bool(false));
        assert_eq!(eval_val("true || false"), Value::Bool(true));
        assert_eq!(eval_val("!true"), Value::Bool(false));
    }

    #[test]
    fn short_circuit_and() {
        // false && (side effect) — should not evaluate right side.
        let out = eval_out("false && { println(\"no\"); true }");
        assert!(out.is_empty());
    }

    #[test]
    fn short_circuit_or() {
        // true || (side effect) — should not evaluate right side.
        let out = eval_out("true || { println(\"no\"); false }");
        assert!(out.is_empty());
    }

    #[test]
    fn unary_neg() {
        assert_eq!(eval_val("-5"), Value::Int(-5));
        assert_eq!(eval_val("-3.0"), Value::Float(-3.0));
    }

    #[test]
    fn string_concat() {
        assert_eq!(
            eval_val("\"hello\" + \" \" + \"world\""),
            Value::String("hello world".into())
        );
    }

    // ── Let / Mut bindings ──

    #[test]
    fn let_binding() {
        assert_eq!(eval_val("let x = 42\nx"), Value::Int(42));
    }

    #[test]
    fn let_immutable() {
        assert!(run("let x = 1\nx = 2").is_err());
    }

    #[test]
    fn mut_binding() {
        assert_eq!(eval_val("mut x = 1\nx = 2\nx"), Value::Int(2));
    }

    #[test]
    fn compound_assign() {
        assert_eq!(eval_val("mut x = 10\nx += 5\nx"), Value::Int(15));
        assert_eq!(eval_val("mut x = 10\nx -= 3\nx"), Value::Int(7));
        assert_eq!(eval_val("mut x = 10\nx *= 2\nx"), Value::Int(20));
    }

    // ── Lists ──

    #[test]
    fn list_literal() {
        assert_eq!(
            eval_val("[1, 2, 3]"),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn list_index() {
        assert_eq!(eval_val("let a = [10, 20, 30]\na[1]"), Value::Int(20));
    }

    #[test]
    fn list_negative_index() {
        assert_eq!(eval_val("let a = [10, 20, 30]\na[-1]"), Value::Int(30));
    }

    #[test]
    fn list_concat() {
        assert_eq!(
            eval_val("[1, 2] + [3, 4]"),
            Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ])
        );
    }

    #[test]
    fn list_map_method() {
        assert_eq!(
            eval_val("[1, 2, 3].map(|x| x * 2)"),
            Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
        );
    }

    #[test]
    fn list_filter_method() {
        assert_eq!(
            eval_val("[1, 2, 3, 4, 5].filter(|x| x > 2)"),
            Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)])
        );
    }

    #[test]
    fn list_fold_method() {
        assert_eq!(
            eval_val("[1, 2, 3, 4].fold(0, |acc, x| acc + x)"),
            Value::Int(10)
        );
    }

    #[test]
    fn list_join_method() {
        assert_eq!(
            eval_val("[\"a\", \"b\", \"c\"].join(\", \")"),
            Value::String("a, b, c".into())
        );
    }

    #[test]
    fn list_contains_method() {
        assert_eq!(eval_val("[1, 2, 3].contains(2)"), Value::Bool(true));
        assert_eq!(eval_val("[1, 2, 3].contains(5)"), Value::Bool(false));
    }

    #[test]
    fn list_enumerate_method() {
        assert_eq!(
            eval_val("[\"a\", \"b\"].enumerate()"),
            Value::List(vec![
                Value::Tuple(vec![Value::Int(0), Value::String("a".into())]),
                Value::Tuple(vec![Value::Int(1), Value::String("b".into())]),
            ])
        );
    }

    // ── Maps ──

    #[test]
    fn map_literal() {
        let val = eval_val("{ \"a\": 1, \"b\": 2 }");
        if let Value::Map(map) = val {
            assert_eq!(map.get("a"), Some(&Value::Int(1)));
            assert_eq!(map.get("b"), Some(&Value::Int(2)));
        } else {
            panic!("expected map, got {:?}", val);
        }
    }

    #[test]
    fn map_field_access() {
        assert_eq!(
            eval_val("let m = { \"name\": \"alice\" }\nm.name"),
            Value::String("alice".into())
        );
    }

    #[test]
    fn map_index_access() {
        assert_eq!(
            eval_val("let m = { \"x\": 42 }\nm[\"x\"]"),
            Value::Int(42)
        );
    }

    #[test]
    fn map_keys_method() {
        assert_eq!(
            eval_val("{ \"a\": 1, \"b\": 2 }.keys()"),
            Value::List(vec![
                Value::String("a".into()),
                Value::String("b".into())
            ])
        );
    }

    // ── Functions ──

    #[test]
    fn fn_declaration_and_call() {
        let source = r#"
fn add(a, b) {
    a + b
}
add(3, 4)
"#;
        assert_eq!(eval_val(source), Value::Int(7));
    }

    #[test]
    fn fn_with_return() {
        let source = r#"
fn abs_val(x) {
    if x < 0 {
        return -x
    }
    x
}
abs_val(-5)
"#;
        assert_eq!(eval_val(source), Value::Int(5));
    }

    #[test]
    fn fn_default_params() {
        let source = r#"
fn greet(name, greeting = "Hello") {
    greeting + " " + name
}
greet("world")
"#;
        assert_eq!(eval_val(source), Value::String("Hello world".into()));
    }

    #[test]
    fn fn_recursive() {
        let source = r#"
fn fib(n) {
    if n <= 1 { n }
    else { fib(n - 1) + fib(n - 2) }
}
fib(10)
"#;
        assert_eq!(eval_val(source), Value::Int(55));
    }

    // ── Closures ──

    #[test]
    fn closure_basic() {
        assert_eq!(eval_val("let f = |x| x + 1\nf(5)"), Value::Int(6));
    }

    #[test]
    fn closure_captures_env() {
        let source = r#"
let offset = 10
let add_offset = |x| x + offset
add_offset(5)
"#;
        assert_eq!(eval_val(source), Value::Int(15));
    }

    #[test]
    fn higher_order_function() {
        let source = r#"
fn apply(f, x) {
    f(x)
}
apply(|x| x * x, 5)
"#;
        assert_eq!(eval_val(source), Value::Int(25));
    }

    // ── If / Else ──

    #[test]
    fn if_expression() {
        assert_eq!(
            eval_val("if true { 1 } else { 2 }"),
            Value::Int(1)
        );
        assert_eq!(
            eval_val("if false { 1 } else { 2 }"),
            Value::Int(2)
        );
    }

    #[test]
    fn if_else_if() {
        let source = r#"
let x = 15
if x > 20 { "big" } else if x > 10 { "medium" } else { "small" }
"#;
        assert_eq!(eval_val(source), Value::String("medium".into()));
    }

    #[test]
    fn if_without_else() {
        assert_eq!(eval_val("if false { 1 }"), Value::Null);
    }

    // ── For loops ──

    #[test]
    fn for_loop_list() {
        let source = r#"
mut sum = 0
for x in [1, 2, 3, 4, 5] {
    sum += x
}
sum
"#;
        assert_eq!(eval_val(source), Value::Int(15));
    }

    #[test]
    fn for_loop_range() {
        let source = r#"
mut sum = 0
for i in 0..5 {
    sum += i
}
sum
"#;
        assert_eq!(eval_val(source), Value::Int(10));
    }

    #[test]
    fn for_loop_break() {
        let source = r#"
mut sum = 0
for i in 0..100 {
    if i >= 5 { break }
    sum += i
}
sum
"#;
        assert_eq!(eval_val(source), Value::Int(10));
    }

    #[test]
    fn for_loop_continue() {
        let source = r#"
mut sum = 0
for i in 0..10 {
    if i % 2 == 0 { continue }
    sum += i
}
sum
"#;
        assert_eq!(eval_val(source), Value::Int(25));
    }

    // ── While loops ──

    #[test]
    fn while_loop() {
        let source = r#"
mut i = 0
mut sum = 0
while i < 5 {
    sum += i
    i += 1
}
sum
"#;
        assert_eq!(eval_val(source), Value::Int(10));
    }

    // ── Loop (infinite) ──

    #[test]
    fn loop_with_break() {
        let source = r#"
mut i = 0
loop {
    i += 1
    if i == 5 { break }
}
i
"#;
        assert_eq!(eval_val(source), Value::Int(5));
    }

    // ── Match ──

    #[test]
    fn match_int() {
        let source = r#"
let x = 2
match x {
    1 => "one",
    2 => "two",
    _ => "other",
}
"#;
        assert_eq!(eval_val(source), Value::String("two".into()));
    }

    #[test]
    fn match_with_guard() {
        let source = r#"
let x = 15
match x {
    n if n > 100 => "huge",
    n if n > 10 => "big",
    _ => "small",
}
"#;
        assert_eq!(eval_val(source), Value::String("big".into()));
    }

    #[test]
    fn match_list_destructure() {
        let source = r#"
let items = [1, 2, 3]
match items {
    [a, b, c] => a + b + c,
    _ => 0,
}
"#;
        assert_eq!(eval_val(source), Value::Int(6));
    }

    #[test]
    fn match_enum_ok_err() {
        let source = r#"
let val = Ok(42)
match val {
    Ok(n) => n * 2,
    Err(e) => 0,
}
"#;
        assert_eq!(eval_val(source), Value::Int(84));
    }

    #[test]
    fn match_unit_enum_variant() {
        let source = r#"
enum Direction { North, South }

let dir = North
match dir {
    North => 1,
    South => 2,
}
"#;
        assert_eq!(eval_val(source), Value::Int(1));
    }

    #[test]
    fn match_null_variant() {
        let source = r#"
let value = [1, 2, 3].find(|x| x > 10)
if value == null { 0 } else { value }
"#;
        assert_eq!(eval_val(source), Value::Int(0));
    }

    #[test]
    fn optional_chaining_short_circuits_on_null() {
        assert_eq!(eval_val("let value = null\nvalue?.to_upper()"), Value::Null);
    }

    #[test]
    fn imported_enum_type_ref_resolves_qualified_unit_variants() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("que-import-enum-{}-{}", std::process::id(), unique));
        fs::create_dir_all(&temp_root).expect("create temp dir");

        let colors_path = temp_root.join("colors.que");
        let main_path = temp_root.join("main.que");

        fs::write(
            &colors_path,
            "pub enum Color {\n    Red\n    Blue\n}\n",
        )
        .expect("write colors module");
        fs::write(
            &main_path,
            "import .colors { Color }\nColor.Red\n",
        )
        .expect("write main module");

        let result = eval_file(&main_path);

        let _ = fs::remove_dir_all(&temp_root);

        assert_eq!(
            result,
            Value::Enum {
                enum_name: "Color".into(),
                variant: "Red".into(),
                fields: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn match_qualified_lowercase_unit_variant() {
        let source = r#"
enum Status { ok, err { code: Int } }

let status = Status.ok
match status {
    Status.ok => 1,
    Status.err { code } => code,
}
"#;
        assert_eq!(eval_val(source), Value::Int(1));
    }

    #[test]
    fn match_qualified_lowercase_named_variant() {
        let source = r#"
enum Msg { write { text: String }, quit }

let msg = Msg.write(text: "hello")
match msg {
    Msg.write { text } => text,
    Msg.quit => "quit",
}
"#;
        assert_eq!(eval_val(source), Value::String("hello".into()));
    }

    #[test]
    fn match_or_pattern() {
        let source = r#"
let x = 2
match x {
    1 | 2 | 3 => "small",
    _ => "big",
}
"#;
        assert_eq!(eval_val(source), Value::String("small".into()));
    }

    // ── Pipe operator ──

    #[test]
    fn pipe_basic() {
        let source = r#"
fn double(x) { x * 2 }
fn add_one(x) { x + 1 }
5 |> double |> add_one
"#;
        assert_eq!(eval_val(source), Value::Int(11));
    }

    #[test]
    fn pipe_with_lambda() {
        assert_eq!(
            eval_val("10 |> |x| x * 3"),
            Value::Int(30)
        );
    }

    #[test]
    fn pipe_with_call_args() {
        let source = r#"
fn add(a, b) { a + b }
5 |> add(3)
"#;
        assert_eq!(eval_val(source), Value::Int(8));
    }

    // ── Try (?) operator ──

    #[test]
    fn try_ok() {
        let source = r#"
fn safe_div(a, b) {
    if b == 0 { Err("division by zero") }
    else { Ok(a / b) }
}
fn compute() {
    let x = safe_div(10, 2)?
    x + 1
}
compute()
"#;
        assert_eq!(eval_val(source), Value::Int(6));
    }

    #[test]
    fn try_err_propagation() {
        let source = r#"
fn may_fail() {
    Err("oops")
}
fn caller() {
    let x = may_fail()?
    x
}
caller()
"#;
        assert!(run(source).is_err());
    }

    // ── Null coalescing ──

    #[test]
    fn null_coalesce() {
        assert_eq!(eval_val("null ?? 42"), Value::Int(42));
        assert_eq!(eval_val("10 ?? 42"), Value::Int(10));
    }

    // ── String interpolation ──

    #[test]
    fn string_interpolation() {
        let source = r#"
let name = "world"
"hello ${name}"
"#;
        assert_eq!(eval_val(source), Value::String("hello world".into()));
    }

    #[test]
    fn string_interpolation_expr() {
        assert_eq!(
            eval_val("let x = 5\n\"x = ${x + 1}\""),
            Value::String("x = 6".into())
        );
    }

    // ── Builtin functions ──

    #[test]
    fn builtin_println() {
        let out = eval_out("println(\"hello\")");
        assert_eq!(out, vec!["hello"]);
    }

    #[test]
    fn the_removed_collection_globals_name_the_method() {
        // The whole family shares one arm, so one probe per shape is enough.
        let err = eval_err("len(\"abc\")");
        assert!(err.contains("`x.len("), "got: {}", err);
        let err = eval_err("filter([1, 2], |x| x > 1)");
        assert!(err.contains("`x.filter("), "got: {}", err);
        let err = eval_err("for_each([1], |x| x)");
        assert!(err.contains("`x.each("), "got: {}", err);
    }

    #[test]
    fn builtin_type() {
        assert_eq!(
            eval_val("typeof(42)"),
            Value::String("Int".into())
        );
        assert_eq!(
            eval_val("typeof(\"hello\")"),
            Value::String("String".into())
        );
    }

    #[test]
    fn builtin_range() {
        assert_eq!(
            eval_val("range(0, 5)"),
            Value::List(vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ])
        );
    }

    #[test]
    fn builtin_str() {
        assert_eq!(
            eval_val("str(42)"),
            Value::String("42".into())
        );
    }

    #[test]
    fn builtin_int() {
        assert_eq!(eval_val("int(\"42\")"), Value::Int(42));
        assert_eq!(eval_val("int(3.7)"), Value::Int(3));
    }

    #[test]
    fn builtin_assert_pass() {
        eval("assert(1 + 1 == 2)");
    }

    #[test]
    fn builtin_assert_fail() {
        assert!(run("assert(1 == 2)").is_err());
    }

    #[test]
    fn a_failed_assert_reports_the_expression_and_its_values() {
        // `assert` used to see a bare `false`, so the only honest thing it
        // could say was "assertion failed" — which is why the language grew
        // an `assert_eq`. It now receives the expression instead.
        let err = run("let n = 2\nassert(n >= 5)").unwrap_err();
        assert!(err.to_string().contains("n >= 5"), "{}", err);
        assert!(err.to_string().contains("(2 >= 5)"), "{}", err);
    }

    #[test]
    fn assert_eq_is_gone() {
        let err = run("assert_eq(1, 2)").unwrap_err();
        assert!(err.to_string().contains("assert(a == b)"), "{}", err);
    }

    // ── String methods ──

    #[test]
    fn string_trim() {
        assert_eq!(
            eval_val("\"  hello  \".trim()"),
            Value::String("hello".into())
        );
    }

    #[test]
    fn string_split() {
        assert_eq!(
            eval_val("\"a,b,c\".split(\",\")"),
            Value::List(vec![
                Value::String("a".into()),
                Value::String("b".into()),
                Value::String("c".into())
            ])
        );
    }

    #[test]
    fn string_to_upper_lower() {
        assert_eq!(
            eval_val("\"Hello\".to_upper()"),
            Value::String("HELLO".into())
        );
        assert_eq!(
            eval_val("\"Hello\".to_lower()"),
            Value::String("hello".into())
        );
    }

    #[test]
    fn string_starts_ends_with() {
        assert_eq!(
            eval_val("\"hello world\".starts_with(\"hello\")"),
            Value::Bool(true)
        );
        assert_eq!(
            eval_val("\"hello world\".ends_with(\"world\")"),
            Value::Bool(true)
        );
    }

    #[test]
    fn string_replace() {
        assert_eq!(
            eval_val("\"hello world\".replace(\"world\", \"que\")"),
            Value::String("hello que".into())
        );
    }


    // ── Blocks as expressions ──

    #[test]
    fn block_expression() {
        let source = r#"
let x = {
    let a = 1
    let b = 2
    a + b
}
x
"#;
        assert_eq!(eval_val(source), Value::Int(3));
    }

    // ── Path operations ──

    #[test]
    fn path_literal() {
        assert_eq!(
            eval_val("path(\"./src\")"),
            Value::Path("./src".into())
        );
    }

    #[test]
    fn path_join_div() {
        assert_eq!(
            eval_val("path(\"./src\") / \"main.rs\""),
            Value::Path("./src/main.rs".into())
        );
    }

    #[test]
    fn path_name_method() {
        assert_eq!(
            eval_val("path(\"./src/main.rs\").name()"),
            Value::String("main.rs".into())
        );
    }

    #[test]
    fn path_extension_method() {
        assert_eq!(
            eval_val("path(\"./src/main.rs\").extension()"),
            Value::String("rs".into())
        );
    }

    #[test]
    fn path_parent_method() {
        assert_eq!(
            eval_val("path(\"./src/main.rs\").parent()"),
            Value::Path("./src".into())
        );
    }

    #[test]
    fn path_components_method() {
        assert_eq!(
            eval_val("path(\"src/main.rs\").components()"),
            Value::List(vec![
                Value::String("src".into()),
                Value::String("main.rs".into()),
            ])
        );
    }

    #[test]
    fn path_components_absolute() {
        let val = eval_val("path(\"/home/user/file.txt\").components()");
        if let Value::List(parts) = val {
            assert_eq!(parts.len(), 4);
            assert_eq!(parts[0], Value::String("/".into()));
            assert_eq!(parts[1], Value::String("home".into()));
            assert_eq!(parts[2], Value::String("user".into()));
            assert_eq!(parts[3], Value::String("file.txt".into()));
        } else {
            panic!("expected list, got {:?}", val);
        }
    }

    #[test]
    fn the_removed_path_spellings_say_what_to_use_instead() {
        // `parts` was a second name for `components`; every such pair now
        // leaves behind an arm that points at the survivor rather than the
        // bare "Path has no method" that would send someone reading the docs.
        let err = eval_err("path(\"a/b/c\").parts()");
        assert!(err.contains("`.components()`"), "got: {}", err);
        let err = eval_err("path(\"a/b/c\").str()");
        assert!(err.contains("`.to_string()`"), "got: {}", err);
    }

    #[test]
    fn the_other_removed_spellings_say_what_to_use_instead() {
        // Same idea as above, spot-checked across the remaining types so that
        // a future rewrite of one of these arms cannot quietly drop the hint.
        let err = eval_err("\"12\".to_int()");
        assert!(err.contains("`.parse_int()`"), "got: {}", err);
        let err = eval_err("\"1.5\".to_float()");
        assert!(err.contains("`.parse_float()`"), "got: {}", err);
        let err = eval_err("5s.seconds()");
        assert!(err.contains("`.to_seconds()`"), "got: {}", err);
        let err = eval_err("import std.stream\nstream.of(\"hi\").to_string()");
        assert!(err.contains("`.collect()`"), "got: {}", err);
        let err = eval_err("[1, 2].for_each(|x| x)");
        assert!(err.contains("`.each()`"), "got: {}", err);
    }

    #[test]
    fn path_depth_method() {
        assert_eq!(eval_val("path(\"src/main.rs\").depth()"), Value::Int(2));
        assert_eq!(eval_val("path(\"/home/user/file.txt\").depth()"), Value::Int(4));
    }

    // ── Duration ──

    #[test]
    fn duration_literal() {
        assert_eq!(
            eval_val("5s"),
            Value::Duration(5.0, DurationUnit::Seconds)
        );
    }

    #[test]
    fn duration_add() {
        // 1s + 500ms = 1500ms
        let val = eval_val("1s + 500ms");
        if let Value::Duration(v, DurationUnit::Milliseconds) = val {
            assert!((v - 1500.0).abs() < f64::EPSILON);
        } else {
            panic!("expected duration, got {:?}", val);
        }
    }

    #[test]
    fn duration_method_to_seconds() {
        let val = eval_val("5s.to_seconds()");
        assert_eq!(val, Value::Float(5.0));
    }

    // ── Pattern matching in let ──

    #[test]
    fn let_destructure_list() {
        let source = r#"
let [a, b, c] = [1, 2, 3]
a + b + c
"#;
        assert_eq!(eval_val(source), Value::Int(6));
    }

    #[test]
    fn let_destructure_tuple() {
        let source = r#"
let (x, y) = (10, 20)
x + y
"#;
        assert_eq!(eval_val(source), Value::Int(30));
    }

    // ── Complex programs ──

    #[test]
    fn fibonacci_with_loop() {
        let source = r#"
fn fib(n) {
    mut a = 0
    mut b = 1
    for _ in 0..n {
        let temp = b
        b = a + b
        a = temp
    }
    a
}
fib(10)
"#;
        assert_eq!(eval_val(source), Value::Int(55));
    }

    #[test]
    fn map_filter_fold_chain() {
        let source = r#"
[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    .filter(|x| x % 2 == 0)
    .map(|x| x * x)
    .fold(0, |acc, x| acc + x)
"#;
        assert_eq!(eval_val(source), Value::Int(220)); // 4+16+36+64+100
    }

    #[test]
    fn pipe_chain() {
        let source = r#"
fn double(x) { x * 2 }
fn to_str(x) { str(x) }
5 |> double |> double |> to_str
"#;
        assert_eq!(eval_val(source), Value::String("20".into()));
    }

    #[test]
    fn nested_functions() {
        let source = r#"
fn make_adder(n) {
    |x| x + n
}
let add5 = make_adder(5)
let add10 = make_adder(10)
add5(3) + add10(3)
"#;
        assert_eq!(eval_val(source), Value::Int(21));
    }

    #[test]
    fn result_methods() {
        assert_eq!(eval_val("Ok(42).unwrap()"), Value::Int(42));
        assert_eq!(eval_val("Ok(42).is_ok()"), Value::Bool(true));
        assert_eq!(eval_val("Err(\"oops\").is_err()"), Value::Bool(true));
    }

    #[test]
    fn optional_chaining_on_present_value() {
        assert_eq!(eval_val("let x = \"hi\"\nx?.to_upper()"), Value::String("HI".into()));
    }

    #[test]
    fn null_coalesce_with_missing_find() {
        // None should also trigger null coalesce
        let source = r#"
let x = [1, 2, 3].find(|x| x > 10)
x ?? 0
"#;
        assert_eq!(eval_val(source), Value::Int(0));
    }

    #[test]
    fn list_any_all() {
        assert_eq!(
            eval_val("[1, 2, 3].any(|x| x > 2)"),
            Value::Bool(true)
        );
        assert_eq!(
            eval_val("[1, 2, 3].all(|x| x > 0)"),
            Value::Bool(true)
        );
        assert_eq!(
            eval_val("[1, 2, 3].all(|x| x > 2)"),
            Value::Bool(false)
        );
    }

    #[test]
    fn semver_literal() {
        assert_eq!(
            eval_val("v\"1.2.3\""),
            Value::Semver("1.2.3".into())
        );
    }

    #[test]
    fn semver_comparison() {
        assert_eq!(eval_val("v\"1.2.3\" < v\"2.0.0\""), Value::Bool(true));
        assert_eq!(eval_val("v\"1.2.3\" > v\"1.2.2\""), Value::Bool(true));
    }

    #[test]
    fn bitwise_ops() {
        assert_eq!(eval_val("5 & 3"), Value::Int(1));
        assert_eq!(eval_val("5 | 3"), Value::Int(7));
        assert_eq!(eval_val("5 ^ 3"), Value::Int(6));
        assert_eq!(eval_val("1 << 3"), Value::Int(8));
        assert_eq!(eval_val("8 >> 2"), Value::Int(2));
        assert_eq!(eval_val("~0"), Value::Int(-1));
    }

    #[test]
    fn for_loop_map_iteration() {
        let source = r#"
mut result = []
for (k, v) in { "a": 1, "b": 2 } {
    result = result.push(k)
}
result
"#;
        let val = eval_val(source);
        if let Value::List(items) = val {
            assert_eq!(items.len(), 2);
            assert!(items.contains(&Value::String("a".into())));
            assert!(items.contains(&Value::String("b".into())));
        } else {
            panic!("expected list, got {:?}", val);
        }
    }

    #[test]
    fn index_assign() {
        let source = r#"
mut list = [1, 2, 3]
list[1] = 20
list
"#;
        assert_eq!(
            eval_val(source),
            Value::List(vec![Value::Int(1), Value::Int(20), Value::Int(3)])
        );
    }

    #[test]
    fn map_assign() {
        let source = r#"
mut m = { "a": 1 }
m["b"] = 2
m
"#;
        let val = eval_val(source);
        if let Value::Map(map) = val {
            assert_eq!(map.get("a"), Some(&Value::Int(1)));
            assert_eq!(map.get("b"), Some(&Value::Int(2)));
        } else {
            panic!("expected map, got {:?}", val);
        }
    }

    #[test]
    fn regex_literal() {
        assert_eq!(
            eval_val("re\"\\d+\""),
            Value::Regex("\\d+".into())
        );
    }

    #[test]
    fn glob_literal() {
        assert_eq!(
            eval_val("glob(\"*.rs\")"),
            Value::Glob("*.rs".into())
        );
    }

    #[test]
    fn range_inclusive() {
        assert_eq!(
            eval_val("1..=3"),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn range_exclusive() {
        assert_eq!(
            eval_val("0..4"),
            Value::List(vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3)
            ])
        );
    }

    #[test]
    fn process_result_methods() {
        // We won't actually run a command in unit tests; instead, test
        // ProcessResult methods using a constructed value via the interpreter.
        let source = r#"
fn make_result() {
    // This is a workaround: we create a value that looks like a process result
    // In real usage this would be: `echo hello`
    Ok("hello")
}
make_result().is_ok()
"#;
        assert_eq!(eval_val(source), Value::Bool(true));
    }

    #[test]
    fn print_multiple_args() {
        let out = eval_out("println(\"a\", \"b\", \"c\")");
        assert_eq!(out, vec!["a b c"]);
    }

    #[test]
    fn map_merge_method() {
        let source = r#"
let m1 = { "a": 1 }
let m2 = { "b": 2, "c": 3 }
let merged = m1.merge(m2)
merged.keys()
"#;
        let val = eval_val(source);
        if let Value::List(keys) = val {
            assert_eq!(keys.len(), 3);
        } else {
            panic!("expected list of keys");
        }
    }

    #[test]
    fn env_get_missing_var() {
        assert_eq!(
            eval_val("env.get(\"WISP_UNLIKELY_ENV_VAR_12345\")"),
            Value::Null
        );
    }

    #[test]
    fn env_get_missing_var_with_default() {
        assert_eq!(
            eval_val("env.get(\"WISP_UNLIKELY_ENV_VAR_12345\", \"fallback\")"),
            Value::String("fallback".into())
        );
    }

    #[test]
    fn list_sort_method() {
        assert_eq!(
            eval_val("[3, 1, 4, 1, 5].sort()"),
            Value::List(vec![
                Value::Int(1),
                Value::Int(1),
                Value::Int(3),
                Value::Int(4),
                Value::Int(5)
            ])
        );
    }

    #[test]
    fn list_reverse_method() {
        assert_eq!(
            eval_val("[1, 2, 3].reverse()"),
            Value::List(vec![Value::Int(3), Value::Int(2), Value::Int(1)])
        );
    }

    #[test]
    fn string_len_method() {
        assert_eq!(eval_val("\"hello\".len()"), Value::Int(5));
    }

    #[test]
    fn map_len_method() {
        assert_eq!(
            eval_val("{ \"a\": 1, \"b\": 2 }.len()"),
            Value::Int(2)
        );
    }

    #[test]
    fn list_flat_map() {
        assert_eq!(
            eval_val("[1, 2, 3].flat_map(|x| [x, x * 10])"),
            Value::List(vec![
                Value::Int(1),
                Value::Int(10),
                Value::Int(2),
                Value::Int(20),
                Value::Int(3),
                Value::Int(30),
            ])
        );
    }

    #[test]
    fn spread_in_list() {
        let source = r#"
let a = [1, 2]
let b = [3, 4]
[...a, ...b, 5]
"#;
        assert_eq!(
            eval_val(source),
            Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
                Value::Int(5),
            ])
        );
    }

    #[test]
    fn multi_line_output() {
        let source = r#"
for i in 0..3 {
    println(i)
}
"#;
        let out = eval_out(source);
        assert_eq!(out, vec!["0", "1", "2"]);
    }

    // ── Tasks ──

    #[test]
    fn task_declaration_creates_task_value() {
        let source = r#"
@description("Build the project")
task build {
    42
}
typeof(build)
"#;
        assert_eq!(eval_val(source), Value::String("Task".into()));
    }

    #[test]
    fn task_call_executes_body() {
        let source = r#"
task greet {
    println("hello from task")
}
greet()
"#;
        let out = eval_out(source);
        assert!(out.contains(&"[RUN]  greet".to_string()));
        assert!(out.contains(&"hello from task".to_string()));
        assert!(out.contains(&"[DONE] greet".to_string()));
    }

    #[test]
    fn task_with_params() {
        let source = r#"
task greet(name) {
    println("hello ${name}")
}
greet("world")
"#;
        let out = eval_out(source);
        assert!(out.contains(&"hello world".to_string()));
    }

    #[test]
    fn task_returns_value() {
        let source = r#"
task compute {
    21 * 2
}
compute()
"#;
        assert_eq!(eval_val(source), Value::Int(42));
    }

    #[test]
    fn task_dependencies_run_first() {
        let source = r#"
task a {
    println("running a")
}
@deps([a])
task b {
    println("running b")
}
b()
"#;
        let out = eval_out(source);
        let a_pos = out.iter().position(|s| s == "running a").unwrap();
        let b_pos = out.iter().position(|s| s == "running b").unwrap();
        assert!(a_pos < b_pos, "dependency 'a' should run before 'b'");
    }

    #[test]
    fn task_deps_run_only_once() {
        let source = r#"
task base {
    println("base ran")
}
@deps([base])
task left {
    println("left ran")
}
@deps([base])
task right {
    println("right ran")
}
@deps([left, right])
task top {
    println("top ran")
}
top()
"#;
        let out = eval_out(source);
        let base_count = out.iter().filter(|s| *s == "base ran").count();
        assert_eq!(base_count, 1, "shared dependency should run exactly once");
    }

    #[test]
    fn task_diamond_dependency() {
        let source = r#"
task a { println("a") }
@deps([a])
task b { println("b") }
@deps([a])
task c { println("c") }
@deps([b, c])
task d { println("d") }
d()
"#;
        let out = eval_out(source);
        let a_count = out.iter().filter(|s| *s == "a").count();
        assert_eq!(a_count, 1, "diamond dep 'a' should run once");
        let a_pos = out.iter().position(|s| s == "a").unwrap();
        let d_pos = out.iter().position(|s| s == "d").unwrap();
        assert!(a_pos < d_pos);
    }

    #[test]
    fn task_field_access() {
        let source = r#"
@description("Build it")
task build {
    42
}
build.name
"#;
        assert_eq!(eval_val(source), Value::String("build".into()));
    }

    #[test]
    fn task_description_field() {
        let source = r#"
@description("Build it")
task build {
    42
}
build.description
"#;
        assert_eq!(eval_val(source), Value::String("Build it".into()));
    }

    #[test]
    fn task_deps_field() {
        let source = r#"
task a { null }
@deps([a])
task b { null }
b.deps
"#;
        assert_eq!(
            eval_val(source),
            Value::List(vec![Value::String("a".into())])
        );
    }

    #[test]
    fn task_status_before_run() {
        let source = r#"
task build { 42 }
build.status
"#;
        assert_eq!(eval_val(source), Value::String("pending".into()));
    }

    #[test]
    fn task_status_after_run() {
        let source = r#"
task build { 42 }
build()
build.status
"#;
        assert_eq!(eval_val(source), Value::String("succeeded".into()));
    }

    #[test]
    fn tasks_builtin_lists_tasks() {
        let source = r#"
task build { null }
task test { null }
fn helper() { null }
let t = tasks()
t.len()
"#;
        assert_eq!(eval_val(source), Value::Int(2));
    }

    #[test]
    fn run_task_by_name() {
        let source = r#"
task greet {
    println("hi")
}
run_task("greet")
"#;
        let out = eval_out(source);
        assert!(out.contains(&"hi".to_string()));
    }

    #[test]
    fn task_display() {
        let source = r#"
@description("Build it")
task build {
    null
}
str(build)
"#;
        assert_eq!(eval_val(source), Value::String("<task build — Build it>".into()));
    }

    #[test]
    fn task_is_type_check() {
        let source = r#"
task build { null }
build.is_type("Task")
"#;
        assert_eq!(eval_val(source), Value::Bool(true));
    }

    #[test]
    fn task_methods_list() {
        let source = r#"
task build { null }
build.methods()
"#;
        if let Value::List(methods) = eval_val(source) {
            assert!(methods.contains(&Value::String("run".into())));
            assert!(methods.contains(&Value::String("deps".into())));
            assert!(methods.contains(&Value::String("status".into())));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn task_inspect() {
        let source = r#"
@description("Build it")
task build {
    null
}
let info = build.inspect()
info.name
"#;
        assert_eq!(eval_val(source), Value::String("build".into()));
    }

    #[test]
    fn task_with_default_params() {
        let source = r#"
task greet(name = "world") {
    println("hello ${name}")
}
greet()
"#;
        let out = eval_out(source);
        assert!(out.contains(&"hello world".to_string()));
    }
}
