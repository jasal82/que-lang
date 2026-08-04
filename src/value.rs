/// Runtime values for the Que interpreter.

use crate::ast::{Block, Param, TypeExpr};
use crate::token::DurationUnit;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufReader, BufWriter};
use std::sync::{Arc, Mutex};

/// A field definition in a struct type.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub default: Option<Value>,
}

/// A resolved method (from an `impl` or `impl Trait for Type` block).
#[derive(Debug, Clone)]
pub struct MethodDef {
    pub name: String,
    /// True if this method has no `self` first parameter (static / constructor).
    pub is_static: bool,
    /// True for `fn m(mut self, ...)`: the receiver is rebound from the value
    /// `self` holds when the body finishes.
    pub mutates_self: bool,
    pub params: Vec<Param>,
    pub body: Block,
    pub closure_env: crate::environment::Environment,
}

impl PartialEq for MethodDef {
    fn eq(&self, other: &Self) -> bool { self.name == other.name }
}

/// A method slot inside a trait definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethodDef {
    pub name: String,
    pub params: Vec<Param>,
    pub default_body: Option<Block>,
}

/// Data for a task (build automation unit). Boxed inside Value to keep enum small.
#[derive(Debug, Clone)]
pub struct TaskData {
    pub name: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub params: Vec<Param>,
    pub depends_on: Vec<String>,
    pub inputs: Vec<crate::ast::Expr>,
    pub outputs: Vec<crate::ast::Expr>,
    pub env_keys: Vec<String>,
    pub body: Block,
    pub closure_env: crate::environment::Environment,
}

impl PartialEq for TaskData {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// The single spelling of a hidden secret.
///
/// One constant so a redaction added in one place is recognisable in every
/// other: the scrubber, the command renderer and `Display` all agree.
pub const REDACTED: &str = "<redacted>";

/// A Que runtime value.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Null,
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Set(Vec<Value>),
    Tuple(Vec<Value>),
    Path(String),
    Glob(String),
    Duration(f64, DurationUnit),
    Regex(String),
    Semver(String),
    Secret(String),

    // Function values
    Function {
        name: Option<String>,
        params: Vec<Param>,
        return_type: Option<Box<TypeExpr>>,
        body: Block,
        closure_env: crate::environment::Environment,
    },
    BuiltinFn(String),

    // Result wrappers (for ? operator and error handling)
    Ok(Box<Value>),
    Err(Box<Value>),

    // Command (unevaluated) — CmdModifiers is boxed to keep Value size small.
    Cmd(Vec<CmdPart>, Box<CmdModifiers>),

    // Process result (after command execution)
    ProcessResult {
        exit_code: i64,
        stdout: String,
        stderr: String,
    },

    // Task (build automation unit) — boxed to keep Value enum small
    Task(Box<TaskData>),

    // Stream (I/O-backed: lazy source + optional write sink)
    Stream(Stream),

    // Background process handle (from `spawn` keyword)
    ProcessHandle(ProcessHandle),

    // File handle (from `open()` builtin)
    FileHandle(FileHandle),

    // Struct instance
    Instance {
        type_name: String,
        fields: BTreeMap<String, Value>,
    },

    // User-defined enum variant (with optional associated data fields)
    Enum {
        enum_name: String,
        variant: String,
        fields: BTreeMap<String, Value>,
    },

    // A reference to a named type (bound when a struct is declared).
    // Used so `TypeName.method(args)` can dispatch to static methods.
    TypeRef(String),

    // Std module namespace (no built-in methods, only field access + call)
    Module {
        name: String,
        entries: BTreeMap<String, Value>,
    },
}

/// Where a stream reads its content from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamSource {
    /// In-memory text (already materialized).
    Buffer(String),
    /// Lazy file read — opened on demand.
    File(String),
    /// Read from parent stdin on demand.
    Stdin,
}

/// Where a stream writes its content to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamSink {
    /// No write target; content stays in buffer.
    None,
    /// Forward to parent stdout.
    Stdout,
    /// Forward to parent stderr.
    Stderr,
    /// Write to a file (overwrite or append).
    File { path: String, append: bool },
    /// Write through an open FileHandle.
    FileHandle(FileHandle),
}

/// A lazy transformation queued on a stream. Executed only when the stream
/// is consumed by a terminal op (collect, write, lines, ...).
///
/// Ops are classified as **per-line** (truly streaming, O(1) extra memory)
/// or **whole-buffer** (forces the pipeline to materialize at that point).
#[derive(Debug, Clone)]
pub enum StreamOp {
    // ── Per-line ops ────────────────────────────────────────────────
    ToUpper,
    ToLower,
    /// `from.replace(to)` applied per-line. Only used when `from` has no '\n'.
    ReplaceLine(String, String),
    Map(Value),
    Filter(Value),
    Grep(String),
    Head(usize),
    SkipEmpty,
    EnumerateLines,
    UniqueLines,

    // ── Whole-buffer ops (force materialization at this step) ───────
    Trim,
    Tail(usize),
    ReverseLines,
    SortLines,
    JoinLines(String),
    Prepend(String),
    Append(String),
    /// Replace where `from` contains '\n', so we must operate on the full buffer.
    ReplaceBuf(String, String),
}

impl StreamOp {
    /// Whether this op forces the pipeline to materialize the entire buffer.
    pub fn is_buffering(&self) -> bool {
        matches!(
            self,
            StreamOp::Trim
                | StreamOp::Tail(_)
                | StreamOp::ReverseLines
                | StreamOp::SortLines
                | StreamOp::JoinLines(_)
                | StreamOp::Prepend(_)
                | StreamOp::Append(_)
                | StreamOp::ReplaceBuf(_, _)
        )
    }

    /// True if this op invokes a user-supplied Que function and therefore
    /// requires the interpreter to execute (cannot be evaluated by `value.rs`
    /// alone, e.g. from `config.rs`).
    pub fn needs_interpreter(&self) -> bool {
        matches!(self, StreamOp::Map(_) | StreamOp::Filter(_))
    }
}

pub struct StreamInner {
    pub source: StreamSource,
    pub ops: Vec<StreamOp>,
    pub sink: StreamSink,
}

impl fmt::Debug for StreamInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamInner")
            .field("source", &self.source)
            .field("ops", &self.ops.len())
            .field("sink", &self.sink)
            .finish()
    }
}

/// I/O-backed stream: lazy source + queued transformations + optional sink.
///
/// Transformations are appended cheaply (no I/O). The pipeline runs only
/// when a terminal op consumes the stream, processing line-by-line so that
/// large files never need to fit in memory.
#[derive(Debug, Clone)]
pub struct Stream {
    pub inner: Arc<Mutex<StreamInner>>,
}

impl Stream {
    pub fn from_string(s: String) -> Self {
        Self::new(StreamSource::Buffer(s), Vec::new(), StreamSink::None)
    }

    pub fn from_file(path: String) -> Self {
        Self::new(StreamSource::File(path), Vec::new(), StreamSink::None)
    }

    pub fn from_stdin() -> Self {
        Self::new(StreamSource::Stdin, Vec::new(), StreamSink::None)
    }

    pub fn with_sink(sink: StreamSink) -> Self {
        Self::new(StreamSource::Buffer(String::new()), Vec::new(), sink)
    }

    fn new(source: StreamSource, ops: Vec<StreamOp>, sink: StreamSink) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StreamInner { source, ops, sink })),
        }
    }

    /// Return a NEW Stream that is a copy of `self` with `op` appended to its
    /// pipeline. The original stream is unchanged (Arc not shared).
    pub fn pushed(&self, op: StreamOp) -> Self {
        let inner = self.inner.lock().unwrap();
        let mut ops = inner.ops.clone();
        ops.push(op);
        Self::new(inner.source.clone(), ops, inner.sink.clone())
    }

    pub fn get_sink(&self) -> StreamSink {
        self.inner.lock().unwrap().sink.clone()
    }

    /// Eagerly materialize the stream into a `String` *without* requiring the
    /// interpreter. Returns an error if any queued op needs a user closure
    /// (Map/Filter); in that case the caller must execute the pipeline via
    /// the interpreter first.
    pub fn materialize_eager(&self) -> Result<String, String> {
        let (source, ops) = {
            let inner = self.inner.lock().unwrap();
            (inner.source.clone(), inner.ops.clone())
        };
        if ops.iter().any(|o| o.needs_interpreter()) {
            return Err("stream contains user closures; materialize via interpreter".to_string());
        }
        let mut text = read_source_eager(&source)?;
        for op in &ops {
            text = apply_op_eager(op, text)?;
        }
        Ok(text)
    }

    /// True if this stream has no queued transformations (raw source only).
    pub fn is_raw(&self) -> bool {
        self.inner.lock().unwrap().ops.is_empty()
    }
}

/// Render one command's parts back into source form, for `Display`.
fn write_cmd_parts(f: &mut std::fmt::Formatter<'_>, parts: &[CmdPart]) -> std::fmt::Result {
    for p in parts {
        match p {
            CmdPart::Literal(s) => write!(f, "{}", s)?,
            CmdPart::Interpolated(s) => write!(f, "${{{}}}", s)?,
            CmdPart::Raw(s) => write!(f, "!{{{}}}", s)?,
            CmdPart::Secret(_) => write!(f, "${{{}}}", REDACTED)?,
        }
    }
    Ok(())
}

fn read_source_eager(source: &StreamSource) -> Result<String, String> {
    match source {
        StreamSource::Buffer(s) => Ok(s.clone()),
        StreamSource::File(path) => std::fs::read_to_string(path)
            .map_err(|e| format!("stream: cannot read '{}': {}", path, e)),
        StreamSource::Stdin => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| format!("stream: cannot read stdin: {}", e))?;
            Ok(s)
        }
    }
}

/// Apply a non-closure op to a fully-materialized buffer. Used by
/// `materialize_eager` and by the streaming pipeline when it hits a buffer
/// boundary.
fn apply_op_eager(op: &StreamOp, text: String) -> Result<String, String> {
    Ok(match op {
        StreamOp::ToUpper => text.to_uppercase(),
        StreamOp::ToLower => text.to_lowercase(),
        StreamOp::Trim => text.trim().to_string(),
        StreamOp::ReplaceLine(from, to) | StreamOp::ReplaceBuf(from, to) => text.replace(from, to),
        StreamOp::Prepend(p) => format!("{}{}", p, text),
        StreamOp::Append(s) => format!("{}{}", text, s),
        StreamOp::Grep(pat) => {
            if let Ok(re) = regex_lite::Regex::new(pat) {
                text.lines().filter(|l| re.is_match(l)).collect::<Vec<_>>().join("\n")
            } else {
                text.lines().filter(|l| l.contains(pat.as_str())).collect::<Vec<_>>().join("\n")
            }
        }
        StreamOp::Head(n) => text.lines().take(*n).collect::<Vec<_>>().join("\n"),
        StreamOp::Tail(n) => {
            let all: Vec<&str> = text.lines().collect();
            let start = all.len().saturating_sub(*n);
            all[start..].join("\n")
        }
        StreamOp::SkipEmpty => text.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n"),
        StreamOp::ReverseLines => {
            let mut v: Vec<&str> = text.lines().collect();
            v.reverse();
            v.join("\n")
        }
        StreamOp::SortLines => {
            let mut v: Vec<&str> = text.lines().collect();
            v.sort();
            v.join("\n")
        }
        StreamOp::UniqueLines => {
            let mut seen = std::collections::HashSet::new();
            text.lines().filter(|l| seen.insert(l.to_string())).collect::<Vec<_>>().join("\n")
        }
        StreamOp::EnumerateLines => text
            .lines()
            .enumerate()
            .map(|(i, l)| format!("{}\t{}", i + 1, l))
            .collect::<Vec<_>>()
            .join("\n"),
        StreamOp::JoinLines(sep) => text.lines().collect::<Vec<_>>().join(sep),
        StreamOp::Map(_) | StreamOp::Filter(_) => {
            return Err("Map/Filter require the interpreter to execute".to_string());
        }
    })
}

impl PartialEq for Stream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for Stream {}

impl std::hash::Hash for Stream {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.inner) as usize).hash(state);
    }
}

/// Inner state of an open file handle.
pub struct FileHandleInner {
    pub path: String,
    pub mode: String, // "r", "w", "a"
    pub reader: Option<BufReader<std::fs::File>>,
    pub writer: Option<BufWriter<std::fs::File>>,
    pub open: bool,
    /// A write handle produced by a dry run: it has no `writer`, and every
    /// write against it succeeds without touching the disk.
    ///
    /// A dry run cannot simply refuse to open the file, because the script
    /// would then take its error branch and stop showing what it would have
    /// done -- which is the whole point of the run.
    pub discard: bool,
}

impl fmt::Debug for FileHandleInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileHandleInner")
            .field("path", &self.path)
            .field("mode", &self.mode)
            .field("open", &self.open)
            .field("discard", &self.discard)
            .finish()
    }
}

/// A handle to an open file, created by `open()`.
/// Wrapped in Arc<Mutex<>> so it can be cloned and passed around.
#[derive(Debug, Clone)]
pub struct FileHandle {
    pub inner: Arc<Mutex<FileHandleInner>>,
}

impl Eq for FileHandle {}

impl PartialEq for FileHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

/// A handle to a background process started with `spawn`.
/// Wrapped in Arc<Mutex<>> so it can be cloned and passed around.
#[derive(Debug, Clone)]
pub struct ProcessHandle {
    pub pid: u32,
    /// The underlying child process, shared behind a mutex.
    pub child: Arc<Mutex<std::process::Child>>,
}

impl PartialEq for ProcessHandle {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

/// Modifiers for command execution (working dir, env vars, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CmdModifiers {
    pub dir: Option<String>,
    pub env_vars: Vec<(String, String)>,
    pub stdin_data: Option<String>,
    pub timeout_ms: Option<u64>,
    pub silent: bool,
    /// Run the child on the parent's own stdin/stdout/stderr instead of pipes,
    /// so a program that needs a terminal gets the real one. Nothing can be
    /// captured in that mode — the streams belong to the terminal.
    pub attach: bool,
    pub forward_stdout: Option<Box<StreamSink>>,
    pub forward_stderr: Option<Box<StreamSink>>,
    /// Commands whose stdout feeds this one, in order. `a | b | c` is stored
    /// as the command `c` with `stdin_from = [a, b]`, so every existing
    /// modifier and terminal method keeps working on the pipeline as a whole.
    pub stdin_from: Vec<CmdStage>,
}

/// One command in a pipeline, with the modifiers written on it. Each stage
/// carries its own working directory and environment, the way a shell gives
/// each side of a `|` its own process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CmdStage {
    pub parts: Vec<CmdPart>,
    pub mods: CmdModifiers,
}

/// Parts of a command after interpolation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CmdPart {
    Literal(String),
    Interpolated(String),
    Raw(String),
    /// A `Value::Secret` interpolated into a command.
    ///
    /// Kept apart from `Interpolated` so the two renderings can differ: the
    /// string handed to the shell is the real one, because a command that
    /// receives `<redacted>` as its token is a broken command, while every
    /// rendering meant for a human or a log shows `<redacted>` instead.
    Secret(String),
}

impl Value {
    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::String(_) => "String",
            Value::Null => "Null",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Tuple(_) => "Tuple",
            Value::Path(_) => "Path",
            Value::Glob(_) => "Glob",
            Value::Duration(..) => "Duration",
            Value::Regex(_) => "Regex",
            Value::Semver(_) => "Semver",
            Value::Secret(_) => "Secret",
            Value::Function { .. } => "Function",
            Value::BuiltinFn(_) => "Function",
            Value::Ok(_) => "Ok",
            Value::Err(_) => "Err",
            Value::Cmd(_, _) => "Cmd",
            Value::ProcessResult { .. } => "ProcessResult",
            Value::Task(_) => "Task",
            Value::Stream(_) => "Stream",
            Value::ProcessHandle(_) => "ProcessHandle",
            Value::FileHandle(_) => "FileHandle",
            Value::Instance { type_name, .. } => type_name.as_str(),
            Value::Enum { enum_name, .. } => enum_name.as_str(),
            Value::TypeRef(_) => "Type",
            Value::Module { .. } => "Module",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Null => false,
            Value::List(l) => !l.is_empty(),
            Value::Set(s) => !s.is_empty(),
            Value::Err(_) => false,
            Value::Stream(s) => {
                let inner = s.inner.lock().unwrap();
                let buf_non_empty = matches!(&inner.source, StreamSource::Buffer(b) if !b.is_empty());
                buf_non_empty
                    || !matches!(inner.source, StreamSource::Buffer(_))
                    || !inner.ops.is_empty()
                    || inner.sink != StreamSink::None
            }
            Value::Instance { .. } => true,
            Value::Enum { .. } => true,
            Value::TypeRef(_) => true,
            _ => true,
        }
    }

    /// Convert to a display string (used by print, string interpolation, etc.).
    pub fn display_string(&self) -> String {
        match self {
            Value::Secret(_) => REDACTED.to_string(),
            other => format!("{}", other),
        }
    }

    /// Coerce a value standing for a filesystem location into a path string,
    /// or `None` if it does not name one.
    ///
    /// This is the single definition of "a String is accepted wherever a Path
    /// is". A `Path` has `~` expanded when it is built, so a bare String is
    /// expanded here too; otherwise `f.relative_to(p"~")` and
    /// `f.relative_to("~")` would disagree.
    ///
    /// The conversion is one-way and argument-local: the two stay distinct as
    /// values, so `p"/a" == "/a"` is still false and they hash differently.
    pub fn as_path(&self) -> Option<String> {
        match self {
            Value::Path(s) => Some(s.clone()),
            Value::String(s) => Some(crate::interpreter::helpers::expand_tilde(s)),
            _ => None,
        }
    }

    /// Return a debug-oriented string (more detail than Display).
    pub fn debug_string(&self) -> String {
        match self {
            Value::String(s) => format!("\"{}\"", s),
            Value::Secret(_) => format!("Secret({})", REDACTED),
            Value::Function { name, params, .. } => {
                let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                format!(
                    "Function({}, params: [{}])",
                    name.as_deref().unwrap_or("anonymous"),
                    param_names.join(", ")
                )
            }
            Value::BuiltinFn(name) => format!("BuiltinFn({})", name),
            Value::Instance { type_name, fields } => {
                format!("{}(fields: [{}])", type_name, fields.keys().cloned().collect::<Vec<_>>().join(", "))
            }
            Value::Enum { enum_name, variant, fields } => {
                if fields.is_empty() {
                    format!("{}.{}", enum_name, variant)
                } else {
                    format!("{}.{}(fields: [{}])", enum_name, variant, fields.keys().cloned().collect::<Vec<_>>().join(", "))
                }
            }
            Value::TypeRef(name) => format!("Type({})", name),
            Value::Task(t) => {
                if t.depends_on.is_empty() {
                    format!("Task({})", t.name)
                } else {
                    format!("Task({}, deps: [{}])", t.name, t.depends_on.join(", "))
                }
            }
            Value::Stream(s) => {
                let inner = s.inner.lock().unwrap();
                let suffix = if inner.ops.is_empty() {
                    String::new()
                } else {
                    format!(", ops={}", inner.ops.len())
                };
                match &inner.source {
                    StreamSource::File(p) => format!("Stream(file:{}{})", p, suffix),
                    StreamSource::Stdin => format!("Stream(stdin{})", suffix),
                    StreamSource::Buffer(b) => format!("Stream(len={}{})", b.len(), suffix),
                }
            }
            Value::List(items) => format!("List(len={})", items.len()),
            Value::Set(items) => format!("Set(len={})", items.len()),
            Value::Map(map) => {
                let keys: Vec<&String> = map.keys().collect();
                format!("Map(len={}, keys={:?})", map.len(), keys)
            }
            Value::Tuple(items) => format!("Tuple(len={})", items.len()),
            Value::Module { name, entries } => {
                let keys: Vec<&String> = entries.keys().collect();
                format!("Module({}, keys={:?})", name, keys)
            }
            other => format!("{}", other),
        }
    }

    /// Produce a rich introspection map for this value.
    pub fn inspect_map(&self) -> BTreeMap<String, Value> {
        let mut m = BTreeMap::new();
        m.insert("type".into(), Value::String(self.type_name().into()));
        m.insert("value".into(), Value::String(self.display_string()));
        m.insert("debug".into(), Value::String(self.debug_string()));
        m.insert("truthy".into(), Value::Bool(self.is_truthy()));

        match self {
            Value::List(items) => {
                m.insert("length".into(), Value::Int(items.len() as i64));
                if !items.is_empty() {
                    let first_type = items[0].type_name().to_string();
                    let homogeneous = items.iter().all(|v| v.type_name() == first_type);
                    m.insert("homogeneous".into(), Value::Bool(homogeneous));
                    if homogeneous {
                        m.insert("element_type".into(), Value::String(first_type));
                    }
                }
                m.insert("empty".into(), Value::Bool(items.is_empty()));
            }
            Value::Set(items) => {
                m.insert("length".into(), Value::Int(items.len() as i64));
                m.insert("empty".into(), Value::Bool(items.is_empty()));
            }
            Value::Map(map) => {
                m.insert("length".into(), Value::Int(map.len() as i64));
                m.insert(
                    "keys".into(),
                    Value::List(map.keys().map(|k| Value::String(k.clone())).collect()),
                );
                m.insert("empty".into(), Value::Bool(map.is_empty()));
            }
            Value::Tuple(items) => {
                m.insert("length".into(), Value::Int(items.len() as i64));
                m.insert(
                    "element_types".into(),
                    Value::List(
                        items.iter().map(|v| Value::String(v.type_name().into())).collect(),
                    ),
                );
            }
            Value::String(s) => {
                m.insert("length".into(), Value::Int(s.len() as i64));
                m.insert("empty".into(), Value::Bool(s.is_empty()));
            }
            Value::Function { name, params, .. } => {
                m.insert(
                    "name".into(),
                    name.as_ref()
                        .map(|n| Value::String(n.clone()))
                        .unwrap_or(Value::Null),
                );
                m.insert("arity".into(), Value::Int(params.len() as i64));
                m.insert(
                    "params".into(),
                    Value::List(
                        params.iter().map(|p| Value::String(p.name.clone())).collect(),
                    ),
                );
                m.insert(
                    "has_defaults".into(),
                    Value::Bool(params.iter().any(|p| p.default.is_some())),
                );
            }
            Value::BuiltinFn(name) => {
                m.insert("name".into(), Value::String(name.clone()));
                m.insert("builtin".into(), Value::Bool(true));
            }
            Value::Ok(inner) => {
                m.insert("variant".into(), Value::String("Ok".into()));
                m.insert("inner_type".into(), Value::String(inner.type_name().into()));
            }
            Value::Err(inner) => {
                m.insert("variant".into(), Value::String("Err".into()));
                m.insert("inner_type".into(), Value::String(inner.type_name().into()));
            }
            Value::ProcessResult { exit_code, .. } => {
                m.insert("exit_code".into(), Value::Int(*exit_code));
                m.insert("success".into(), Value::Bool(*exit_code == 0));
            }
            Value::Task(t) => {
                m.insert("name".into(), Value::String(t.name.clone()));
                m.insert(
                    "description".into(),
                    t.description.as_ref()
                        .map(|d| Value::String(d.clone()))
                        .unwrap_or(Value::Null),
                );
                m.insert("arity".into(), Value::Int(t.params.len() as i64));
                m.insert(
                    "params".into(),
                    Value::List(
                        t.params.iter().map(|p| Value::String(p.name.clone())).collect(),
                    ),
                );
                m.insert(
                    "depends_on".into(),
                    Value::List(
                        t.depends_on.iter().map(|d| Value::String(d.clone())).collect(),
                    ),
                );
                m.insert(
                    "aliases".into(),
                    Value::List(
                        t.aliases.iter().map(|a| Value::String(a.clone())).collect(),
                    ),
                );
            }
            Value::Stream(s) => {
                let inner = s.inner.lock().unwrap();
                if let StreamSource::Buffer(b) = &inner.source {
                    if inner.ops.is_empty() {
                        m.insert("length".into(), Value::Int(b.len() as i64));
                        m.insert("lines".into(), Value::Int(b.lines().count() as i64));
                        m.insert("empty".into(), Value::Bool(b.is_empty()));
                    }
                }
                m.insert("pending_ops".into(), Value::Int(inner.ops.len() as i64));
                match &inner.source {
                    StreamSource::File(p) => { m.insert("source".into(), Value::Path(p.clone())); }
                    StreamSource::Stdin => { m.insert("source".into(), Value::String("stdin".into())); }
                    StreamSource::Buffer(_) => {}
                }
                match &inner.sink {
                    StreamSink::None => {}
                    StreamSink::Stdout => { m.insert("sink".into(), Value::String("stdout".into())); }
                    StreamSink::Stderr => { m.insert("sink".into(), Value::String("stderr".into())); }
                    StreamSink::File { path, .. } => { m.insert("sink".into(), Value::Path(path.clone())); }
                    StreamSink::FileHandle(_) => { m.insert("sink".into(), Value::String("file_handle".into())); }
                }
            }
            Value::Instance { type_name, fields } => {
                m.insert("type_name".into(), Value::String(type_name.clone()));
                m.insert("fields".into(), Value::List(
                    fields.keys().map(|k| Value::String(k.clone())).collect()
                ));
                m.insert("field_count".into(), Value::Int(fields.len() as i64));
            }
            Value::Enum { enum_name, variant, fields } => {
                m.insert("enum_name".into(), Value::String(enum_name.clone()));
                m.insert("variant".into(), Value::String(variant.clone()));
                m.insert("fields".into(), Value::List(
                    fields.keys().map(|k| Value::String(k.clone())).collect()
                ));
                m.insert("field_count".into(), Value::Int(fields.len() as i64));
            }
            Value::TypeRef(name) => {
                m.insert("type_name".into(), Value::String(name.clone()));
            }
            Value::FileHandle(fh) => {
                if let Some(inner) = fh.inner.lock().ok() {
                    m.insert("path".into(), Value::Path(inner.path.clone()));
                    m.insert("mode".into(), Value::String(inner.mode.clone()));
                    m.insert("open".into(), Value::Bool(inner.open));
                }
            }
            Value::Module { name, entries } => {
                m.insert("name".into(), Value::String(name.clone()));
                m.insert("length".into(), Value::Int(entries.len() as i64));
                m.insert(
                    "keys".into(),
                    Value::List(entries.keys().map(|k| Value::String(k.clone())).collect()),
                );
            }
            Value::Duration(val, unit) => {
                m.insert("amount".into(), Value::Float(*val));
                m.insert("unit".into(), Value::String(format!("{}", unit)));
            }
            Value::Semver(s) => {
                let base = s.split('-').next().unwrap_or(s);
                let parts: Vec<&str> = base.split('.').collect();
                if let Some(Ok(major)) = parts.first().map(|p| p.parse::<i64>()) {
                    m.insert("major".into(), Value::Int(major));
                }
                if let Some(Ok(minor)) = parts.get(1).map(|p| p.parse::<i64>()) {
                    m.insert("minor".into(), Value::Int(minor));
                }
                if let Some(Ok(patch)) = parts.get(2).map(|p| p.parse::<i64>()) {
                    m.insert("patch".into(), Value::Int(patch));
                }
            }
            _ => {}
        }
        m
    }

    /// Return the list of method names available on this value.
    ///
    /// The per-type tables live in [`crate::docs::methods_for_type`], which is
    /// also what `help`, hover and completion read. This used to be a second
    /// hand-written copy, and the two drifted: `has_method` was promising
    /// `Duration.as_secs`, `List.reduce` and `Map.contains_key`, none of which
    /// the dispatcher had ever heard of.
    pub fn available_methods(&self) -> Vec<&'static str> {
        let key = match self {
            // Ok and Err share one method set, so they share one table entry.
            Value::Ok(_) | Value::Err(_) => "Result",
            // `type_name` reports the user's enum name here, but the built-in
            // methods are the same whatever the enum is called.
            Value::Enum { .. } => "Enum",
            // DateTime and Logger are instances with a built-in method set; a
            // user-defined struct gets its methods from `impl` blocks instead
            // and so is not in the table at all.
            Value::Instance { type_name, .. } => type_name.as_str(),
            other => other.type_name(),
        };
        let mut names: Vec<&'static str> = crate::docs::methods_for_type(key)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        // Answered by `call_method` before it reaches any per-type dispatch.
        names.extend(["inspect", "methods", "type_name", "is_type"]);
        names
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::Bool(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "{}", s),
            Value::Null => write!(f, "null"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    // Strings display with quotes in list context
                    match item {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, "]")
            }
            Value::Set(items) => {
                write!(f, "#{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match item {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    // Strings display with quotes in map value context
                    match v {
                        Value::String(s) => write!(f, "\"{}\": \"{}\"", k, s)?,
                        _ => write!(f, "\"{}\": {}", k, v)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Tuple(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    // Strings display with quotes in tuple context
                    match item {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, ")")
            }
            Value::Path(p) => write!(f, "{}", p),
            Value::Glob(g) => write!(f, "{}", g),
            Value::Duration(val, unit) => write!(f, "{}{}", val, unit),
            Value::Regex(r) => write!(f, "re\"{}\"", r),
            Value::Semver(v) => write!(f, "{}", v),
            Value::Secret(_) => write!(f, "{}", REDACTED),
            Value::Function { name, .. } => {
                write!(f, "<fn {}>", name.as_deref().unwrap_or("anonymous"))
            }
            Value::BuiltinFn(name) => write!(f, "<builtin {}>", name),
            Value::Ok(v) => match v.as_ref() {
                Value::String(s) => write!(f, "Ok(\"{}\")", s),
                _ => write!(f, "Ok({})", v),
            },
            Value::Err(v) => match v.as_ref() {
                Value::String(s) => write!(f, "Err(\"{}\")", s),
                _ => write!(f, "Err({})", v),
            },
            Value::Task(t) => {
                if let Some(ref desc) = t.description {
                    write!(f, "<task {} \u{2014} {}>", t.name, desc)
                } else {
                    write!(f, "<task {}>", t.name)
                }
            }
            Value::Cmd(parts, mods) => {
                write!(f, "`")?;
                for stage in &mods.stdin_from {
                    write_cmd_parts(f, &stage.parts)?;
                    write!(f, " | ")?;
                }
                write_cmd_parts(f, parts)?;
                write!(f, "`")
            }
            Value::Stream(s) => write!(f, "{}", s.materialize_eager().unwrap_or_default()),
            Value::ProcessResult {
                exit_code,
                stdout,
                stderr,
            } => {
                write!(
                    f,
                    "ProcessResult(exit: {}, stdout: {:?}, stderr: {:?})",
                    exit_code, stdout, stderr
                )
            }
            Value::ProcessHandle(h) => write!(f, "<ProcessHandle pid={}>", h.pid),
            Value::FileHandle(fh) => {
                if let Some(inner) = fh.inner.lock().ok() {
                    write!(f, "<FileHandle path={} mode={} open={}>", inner.path, inner.mode, inner.open)
                } else {
                    write!(f, "<FileHandle>")
                }
            }
            Value::Instance { type_name, fields } if type_name == "DateTime" => {
                // Display DateTime as ISO 8601
                match crate::interpreter::std_modules::time::format_iso(fields) {
                    Ok(iso) => write!(f, "{}", iso),
                    Err(_) => write!(f, "<DateTime invalid>"),
                }
            }
            Value::Instance { type_name, fields } => {
                write!(f, "{} {{", type_name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Enum { enum_name, variant, fields } => {
                if fields.is_empty() {
                    write!(f, "{}.{}", enum_name, variant)
                } else {
                    write!(f, "{}.{} {{", enum_name, variant)?;
                    for (i, (k, v)) in fields.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}: {}", k, v)?;
                    }
                    write!(f, "}}")
                }
            }
            Value::TypeRef(name) => write!(f, "<type {}>", name),
            Value::Module { name, .. } => write!(f, "<module {}>", name),
        }
    }
}

// Partial equality for testing and pattern matching.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => {
                a.len() == b.len() && a.iter().all(|item| b.contains(item))
            }
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::Path(a), Value::Path(b)) => a == b,
            (Value::Glob(a), Value::Glob(b)) => a == b,
            (Value::Duration(a, au), Value::Duration(b, bu)) => a == b && au == bu,
            (Value::Regex(a), Value::Regex(b)) => a == b,
            (Value::Semver(a), Value::Semver(b)) => a == b,
            (Value::Stream(a), Value::Stream(b)) => Arc::ptr_eq(&a.inner, &b.inner),
            (Value::Task(a), Value::Task(b)) => a.name == b.name,
            (Value::Instance { type_name: ta, fields: fa }, Value::Instance { type_name: tb, fields: fb }) => {
                ta == tb && fa == fb
            }
            (
                Value::Enum { enum_name: ea, variant: va, fields: fa },
                Value::Enum { enum_name: eb, variant: vb, fields: fb },
            ) => ea == eb && va == vb && fa == fb,
            (Value::TypeRef(a), Value::TypeRef(b)) => a == b,
            (Value::Ok(a), Value::Ok(b)) => a == b,
            (Value::Err(a), Value::Err(b)) => a == b,
            (Value::FileHandle(a), Value::FileHandle(b)) => a == b,
            (Value::Module { name: na, entries: ea }, Value::Module { name: nb, entries: eb }) => {
                na == nb && ea == eb
            }
            _ => false,
        }
    }
}

impl Eq for Value {}

impl std::hash::Hash for FileHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.inner) as usize).hash(state);
    }
}

impl std::hash::Hash for ProcessHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pid.hash(state);
    }
}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the discriminant first so different variants with the same
        // inner data don't collide.
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Int(n) => n.hash(state),
            // f64 has no Hash in std; use the bit representation.
            // This is consistent with PartialEq (NaN != NaN, but two NaN
            // bit patterns that compare equal as bits hash the same).
            Value::Float(f) => f.to_bits().hash(state),
            Value::Bool(b) => b.hash(state),
            Value::String(s) | Value::Path(s) | Value::Glob(s)
            | Value::Regex(s) | Value::Semver(s) | Value::TypeRef(s) => s.hash(state),
            Value::Stream(s) => s.hash(state),
            // Secret: hash the underlying value so equality is consistent,
            // but callers should not use secrets as set/map keys in practice.
            Value::Secret(s) => s.hash(state),
            Value::Null => {}
            Value::List(items) | Value::Tuple(items) => items.hash(state),
            Value::Set(items) => {
                // Sets are order-independent: XOR the hash of each element.
                items.len().hash(state);
                let mut xor: u64 = 0;
                for item in items {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    item.hash(&mut h);
                    xor ^= std::hash::Hasher::finish(&h);
                }
                xor.hash(state);
            }
            Value::Map(m) => {
                // BTreeMap iterates in sorted key order, so this is stable.
                m.len().hash(state);
                for (k, v) in m {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::Duration(f, u) => {
                f.to_bits().hash(state);
                u.hash(state);
            }
            Value::Function { name, .. } => name.hash(state),
            Value::BuiltinFn(s) => s.hash(state),
            Value::Ok(v) | Value::Err(v) => v.hash(state),
            Value::Cmd(parts, mods) => {
                parts.hash(state);
                mods.hash(state);
            }
            Value::ProcessResult { exit_code, stdout, stderr } => {
                exit_code.hash(state);
                stdout.hash(state);
                stderr.hash(state);
            }
            Value::ProcessHandle(h) => h.hash(state),
            Value::FileHandle(fh) => fh.hash(state),
            Value::Task(t) => t.name.hash(state),
            Value::Instance { type_name, fields } => {
                // Structural hash — fields are in BTreeMap order (stable).
                type_name.hash(state);
                for (k, v) in fields {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::Enum { enum_name, variant, fields } => {
                enum_name.hash(state);
                variant.hash(state);
                for (k, v) in fields {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Value::Module { name, entries } => {
                name.hash(state);
                for (k, v) in entries {
                    k.hash(state);
                    v.hash(state);
                }
            }
        }
    }
}
