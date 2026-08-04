//! Built-in function dispatch for the Que interpreter.

use super::helpers::*;
use super::Interpreter;
use crate::error::*;
use crate::value::{FileHandle, FileHandleInner, Value};

use colored::Colorize;
use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter};
use std::sync::{Arc, Mutex};

impl Interpreter {

    /// Enforce the capability policy for a global builtin.
    ///
    /// Split out of `call_builtin` because the two argument-dependent cases
    /// need real inspection of the arguments, and burying that in the middle
    /// of a 1700-line match is how it gets missed next time.
    fn check_global_builtin(&mut self, name: &str, args: &[Value]) -> Result<(), Signal> {
        use crate::permissions::{Capability, GlobalEffect};

        let effect = match crate::permissions::global_effect(name) {
            Some(e) => e,
            // Unclassified: deny. See `permissions::global_effect`.
            None => {
                return self.check_permission(Capability::Exec, name);
            }
        };
        let (cap, idx) = match effect {
            GlobalEffect::Pure => return Ok(()),
            GlobalEffect::Needs(cap, idx) => (cap, idx),
            GlobalEffect::OpenPath => {
                let mode = match args.get(1) {
                    Some(Value::String(s)) => s.as_str(),
                    _ => "r",
                };
                // Anything that is not a read is treated as a write,
                // including a mode string the interpreter will go on to
                // reject: the check must not be the lenient one.
                let cap = if mode == "r" { Capability::Read } else { Capability::Write };
                (cap, Some(0))
            }
            GlobalEffect::EnvVars(idx) => {
                let vars = match args.get(idx) {
                    Some(Value::Instance { fields, .. }) => fields.get("vars"),
                    other => other,
                };
                if let Some(Value::Map(m)) = vars {
                    for key in m.keys() {
                        self.check_permission(Capability::Env, key)?;
                    }
                }
                return Ok(());
            }
            GlobalEffect::TempBase => {
                // No explicit `dir` means the system temp directory, which is
                // where the write will actually land -- naming the builtin
                // instead would make `--allow write=/tmp` mysteriously fail.
                let base = match args.first() {
                    Some(Value::Instance { fields, .. }) => fields
                        .get("dir")
                        .and_then(|v| v.as_path())
                        .unwrap_or_else(|| std::env::temp_dir().display().to_string()),
                    _ => std::env::temp_dir().display().to_string(),
                };
                return self.check_permission(Capability::Write, &base);
            }
        };
        let subject = match idx.and_then(|i| args.get(i)) {
            Some(v) => v.display_string(),
            None => name.to_string(),
        };
        self.check_permission(cap, &subject)
    }

    pub(crate) fn call_builtin(&mut self, name: &str, args: Vec<Value>) -> IResult {
        // Try std module dispatch first (all std builtins use "module.func" format)
        if let Some(result) = self.call_std_builtin(name, &args) {
            return result;
        }

        if self.permissions.is_some() {
            self.check_global_builtin(name, &args)?;
        }

        match name {
            "print" => {
                let mut parts = Vec::with_capacity(args.len());
                for v in args {
                    parts.push(self.display_value(v)?);
                }
                let s = parts.join(" ");
                self.emit_partial(&s);
                Ok(Value::Null)
            }
            "println" => {
                let mut parts = Vec::with_capacity(args.len());
                for v in args {
                    parts.push(self.display_value(v)?);
                }
                self.emit(parts.join(" "));
                Ok(Value::Null)
            }
            "typeof" => {
                let val = args.first().unwrap_or(&Value::Null);
                Ok(Value::String(val.type_name().to_string()))
            }
            "str" => {
                let val = args.into_iter().next().unwrap_or(Value::Null);
                Ok(Value::String(self.display_value(val)?))
            }
            "int" => {
                let val = args.first().unwrap_or(&Value::Null);
                match val {
                    Value::Int(n) => Ok(Value::Int(*n)),
                    Value::Float(f) => Ok(Value::Int(*f as i64)),
                    Value::String(s) => s.parse::<i64>().map(Value::Int).map_err(|_| {
                        Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            format!("cannot convert '{}' to int", s),
                        ))
                    }),
                    Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("cannot convert {} to int", val.type_name()),
                    ))),
                }
            }
            "float" => {
                let val = args.first().unwrap_or(&Value::Null);
                match val {
                    Value::Float(f) => Ok(Value::Float(*f)),
                    Value::Int(n) => Ok(Value::Float(*n as f64)),
                    Value::String(s) => s.parse::<f64>().map(Value::Float).map_err(|_| {
                        Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            format!("cannot convert '{}' to float", s),
                        ))
                    }),
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("cannot convert {} to float", val.type_name()),
                    ))),
                }
            }
            "abs" => {
                let val = args.first().unwrap_or(&Value::Null);
                match val {
                    Value::Int(n) => Ok(Value::Int(n.abs())),
                    Value::Float(f) => Ok(Value::Float(f.abs())),
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("abs() not supported for {}", val.type_name()),
                    ))),
                }
            }
            "min" => {
                let (a, b) = two_args(&args, "min")?;
                self.eval_cmp(a, b, |o| o.is_lt())
                    .and_then(|v| {
                        if v == Value::Bool(true) {
                            Ok(a.clone())
                        } else {
                            Ok(b.clone())
                        }
                    })
            }
            "max" => {
                let (a, b) = two_args(&args, "max")?;
                self.eval_cmp(a, b, |o| o.is_gt())
                    .and_then(|v| {
                        if v == Value::Bool(true) {
                            Ok(a.clone())
                        } else {
                            Ok(b.clone())
                        }
                    })
            }
            "range" => {
                let (start, end) = two_args(&args, "range")?;
                match (start, end) {
                    (Value::Int(a), Value::Int(b)) => {
                        let items: Vec<Value> = (*a..*b).map(Value::Int).collect();
                        Ok(Value::List(items))
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "range() requires two integers",
                    ))),
                }
            }
            // ── Removed: the collection and string globals ────────────
            //
            // Every one of these was the matching method with the receiver
            // moved into the first argument slot — `filter(list, fn)` ran the
            // same loop as `list.filter(fn)`, `trim(s)` returned the same
            // string as `s.trim()`. Two ways to write one thing meant every
            // script picked one at random and no two read alike.
            //
            // The method is the form that survives: it chains without
            // nesting, it is what `help` and completion show, and it is the
            // only form for types the globals never covered (Stream.filter,
            // Map.map_values). `|>` keeps working — it is for your own
            // functions, and `xs |> filter(f)` was only ever `xs.filter(f)`
            // spelled longer.
            "len" | "push" | "pop" | "keys" | "values" | "contains" | "split"
            | "trim" | "join" | "replace" | "chars" | "filter" | "map"
            | "fold" | "flat_map" | "group_by" | "sort_by" | "any" | "all"
            | "find" | "zip" | "enumerate" | "take" | "skip" | "chunk"
            | "partition" | "flatten" | "each" | "for_each" => {
                // `for_each` and `each` were already one arm; both now point
                // at the one surviving method.
                let method = if name == "for_each" { "each" } else { name };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!(
                        "`{}(x, ...)` was removed; use the method `x.{}(...)`",
                        name, method
                    ),
                )))
            }
            "chr" => {
                match args.first() {
                    Some(Value::Int(n)) => {
                        match char::from_u32(*n as u32) {
                            Some(c) => Ok(Value::String(c.to_string())),
                            None => Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                &format!("chr(): {} is not a valid Unicode code point", n),
                            ))),
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "chr() requires an integer",
                    ))),
                }
            }
            "ord" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        let mut chars = s.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) => Ok(Value::Int(c as i64)),
                            _ => Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                "ord() requires a single-character string",
                            ))),
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "ord() requires a string",
                    ))),
                }
            }
            "Ok" => {
                let val = args.into_iter().next().unwrap_or(Value::Null);
                Ok(Value::Ok(Box::new(val)))
            }
            "Err" => {
                let val = args.into_iter().next().unwrap_or(Value::Null);
                Ok(Value::Err(Box::new(val)))
            }
            "Some" | "None" => {
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "`Option`/`Some`/`None` were removed; use `null`, `??` and `?.` instead",
                )))
            }
            "env" => {
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "`env(KEY)` was removed; `env` is a namespace — use `env.get(KEY)`",
                )))
            }
            // Contextual enter/exit for EnvScope (called by WithContext via trait impl)
            "__ctx_envscope_enter" => {
                let vars = match args.first() {
                    Some(Value::Instance { fields, .. }) => match fields.get("vars") {
                        Some(Value::Map(m)) => m.clone(),
                        _ => return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            "env.scope() requires a map of variables",
                        ))),
                    },
                    _ => BTreeMap::new(),
                };
                // Return the previous values so exit() can restore them.
                // A missing variable is recorded as Null.
                let mut saved = BTreeMap::new();
                for (key, val) in &vars {
                    saved.insert(
                        key.clone(),
                        std::env::var(key).map(Value::String).unwrap_or(Value::Null),
                    );
                    std::env::set_var(key, val.display_string());
                }
                Ok(Value::Map(saved))
            }
            "__ctx_envscope_exit" => {
                if let Some(Value::Map(saved)) = args.get(1) {
                    for (key, original) in saved {
                        match original {
                            Value::Null => std::env::remove_var(key),
                            other => std::env::set_var(key, other.display_string()),
                        }
                    }
                }
                Ok(Value::Null)
            }
            // Contextual enter/exit for TempDir (called by WithContext via trait impl)
            "__ctx_tempdir_enter" => {
                let (prefix, parent_dir) = if let Some(Value::Instance { fields, .. }) = args.first() {
                    let prefix = fields.get("prefix")
                        .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or_else(|| "que_".to_string());
                    let parent = fields.get("dir").and_then(|v| v.as_path());
                    (prefix, parent)
                } else {
                    ("que_".to_string(), None)
                };
                match super::std_modules::fs::create_temp_dir_in(&prefix, parent_dir.as_deref()) {
                    Ok(path) => Ok(Value::Path(path)),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::IoError, e))),
                }
            }
            "__ctx_tempdir_exit" => {
                if let Some(p) = args.get(1).and_then(|v| v.as_path()) {
                    let path = std::path::Path::new(&p);
                    if path.exists() {
                        let _ = std::fs::remove_dir_all(path);
                    }
                }
                Ok(Value::Null)
            }
            "__ctx_tempfile_enter" => {
                let (prefix, suffix, parent_dir) = if let Some(Value::Instance { fields, .. }) = args.first() {
                    let p = fields.get("prefix")
                        .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or_else(|| "que_".to_string());
                    let s = fields.get("suffix")
                        .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or_default();
                    let d = fields.get("dir").and_then(|v| v.as_path());
                    (p, s, d)
                } else {
                    ("que_".to_string(), "".to_string(), None)
                };
                match super::std_modules::fs::create_temp_file_in(&prefix, &suffix, parent_dir.as_deref()) {
                    Ok(path) => Ok(Value::Path(path)),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::IoError, e))),
                }
            }
            "__ctx_tempfile_exit" => {
                if let Some(p) = args.get(1).and_then(|v| v.as_path()) {
                    let path = std::path::Path::new(&p);
                    if path.exists() {
                        let _ = std::fs::remove_file(path);
                    }
                }
                Ok(Value::Null)
            }

            // `error(msg)` was `fail(msg)` without the optional exit code, so
            // scripts that started with `error` had to be rewritten the first
            // time they needed to say *which* failure it was.
            "error" => {
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "`error(msg)` was removed; use `fail(msg)`, which also takes an exit code",
                )))
            }
            // Reached only when `assert` is called through a value rather than
            // written as a call — `[1, 2].each(assert)`, say. The direct form is
            // intercepted in eval_expr so it can see the unevaluated condition.
            "assert" => {
                let val = args.first().unwrap_or(&Value::Null);
                if val.is_truthy() {
                    Ok(Value::Null)
                } else {
                    let msg = args
                        .get(1)
                        .map(|v| v.display_string())
                        .unwrap_or_else(|| "assertion failed".to_string());
                    Err(Signal::Error(QueError::runtime(msg)))
                }
            }
            "assert_eq" => {
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "`assert_eq(a, b)` was removed; `assert(a == b)` reports both values on its own",
                )))
            }
            // ── Functional combinator free functions ──────────────────
            "compose" => {
                // compose(f, g, h) returns a value that when called with x,
                // computes h(g(f(x))) — left-to-right composition (pipeline order)
                if args.is_empty() {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "compose requires at least 1 function argument",
                    )));
                }
                // Store as a Tuple with a sentinel first element
                let mut composed = vec![Value::String("__composed__".to_string())];
                composed.extend(args);
                Ok(Value::Tuple(composed))
            }

            // ── Additional builtins ──────────────────────────────────
            "secret" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "secret requires 1 argument"))
                })?;
                match val {
                    Value::String(s) => {
                        self.register_secret(s);
                        Ok(Value::Secret(s.clone()))
                    }
                    // Already a secret: wrapping twice is harmless and saves
                    // callers from having to know whether a value is wrapped.
                    Value::Secret(s) => Ok(Value::Secret(s.clone())),
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "secret() requires a string argument",
                    ))),
                }
            }
            "fail" => {
                let msg = args
                    .first()
                    .map(|v| v.display_string())
                    .unwrap_or_else(|| "fail".to_string());
                // `fail(msg, code)` pins the process exit code, so a script can
                // signal *which* kind of failure happened to whatever is
                // gating on it in CI.
                let err = match args.get(1) {
                    Some(Value::Int(code)) => {
                        QueError::runtime(msg).with_exit_code(*code as i32)
                    }
                    Some(other) => {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            format!(
                                "fail() exit code must be an Int, got {}",
                                other.type_name()
                            ),
                        )))
                    }
                    None => QueError::runtime(msg),
                };
                Err(Signal::Error(err))
            }
            "quefile_dir" | "script_dir" => {
                // Returns the absolute path of the directory containing the
                // currently-executing Quefile or .que script.
                // Falls back to the current working directory when the
                // interpreter was not started from a file (e.g. in tests or REPL).
                let dir = self.script_path.as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        std::env::current_dir()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                Ok(Value::Path(dir))
            }
            "dry_run" => {
                // A dry run cannot suppress an effect Que does not know about.
                // This is the hook a script uses to guard its own.
                Ok(Value::Bool(self.dry_run))
            }
            "sleep" => {
                let duration_ms = match args.first() {
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit),
                    Some(Value::Int(ms)) => *ms as f64,
                    Some(Value::Float(ms)) => *ms,
                    _ => {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            "sleep() requires a duration or number (milliseconds)",
                        )));
                    }
                };
                std::thread::sleep(std::time::Duration::from_millis(duration_ms as u64));
                Ok(Value::Null)
            }
            // Both halves of what `now()` was asked for live in `std.time`:
            // a number to subtract from another number, and a date.
            "now" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`now()` was removed; use `import std.time` and \
                 `time.timestamp()` for Unix milliseconds, or `time.now()` \
                 for a DateTime"
                    .to_string(),
            ))),
            "input" => {
                let prompt = args.first().map(|v| v.display_string()).unwrap_or_default();
                if !prompt.is_empty() {
                    eprint!("{}", prompt);
                }
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).ok();
                Ok(Value::String(line.trim_end_matches('\n').to_string()))
            }
            "confirm" => {
                let prompt = args.first().map(|v| v.display_string()).unwrap_or_else(|| "Confirm?".to_string());
                eprint!("{} [y/N] ", prompt);
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).ok();
                let answer = line.trim().to_lowercase();
                Ok(Value::Bool(answer == "y" || answer == "yes"))
            }
            "bool" => {
                let val = args.first().unwrap_or(&Value::Null);
                Ok(Value::Bool(val.is_truthy()))
            }
            "path" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "path() requires 1 argument"))
                })?;
                let s = match val {
                    Value::String(s) => s.clone(),
                    Value::Path(s) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "path() requires a string or path",
                    ))),
                };
                Ok(Value::Path(crate::interpreter::helpers::expand_tilde(&s)))
            }
            "cd" => {
                // Change the working directory of the whole `que` process and
                // hand back the one we left. Returning the old directory is
                // what lets a scoped form be written in Que rather than baked
                // into the language:
                //
                //     impl Contextual for Dir {
                //         fn enter(self) -> Path { cd(self.path) }
                //         fn exit(self, previous) { cd(previous) }
                //     }
                //
                // The capability policy is unaffected: its path grants were
                // resolved to absolute paths when the flags were parsed, so
                // `cd` moves the script, never the fence around it.
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "cd() requires 1 argument"))
                })?;
                let target = match val {
                    Value::String(s) | Value::Path(s) => {
                        crate::interpreter::helpers::expand_tilde(s)
                    }
                    other => {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            format!("cd() requires a string or path, got {}", other.type_name()),
                        )))
                    }
                };
                let previous = std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // A dry run still moves: the directory a later command would
                // have run in is part of the plan the run exists to show, and
                // every effect that directory feeds is suppressed on its own.
                std::env::set_current_dir(&target).map_err(|e| {
                    Signal::Error(QueError::new(
                        ErrorKind::IoError,
                        format!("cd '{}': {}", target, e),
                    ))
                })?;
                Ok(Value::Path(previous))
            }
            // The global spelling only; `"a/b".to_path()` is still a String
            // method, and that is the form worth keeping.
            "to_path" => {
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "`to_path(s)` was removed; use `path(s)` or `s.to_path()`",
                )))
            }
            "glob" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "glob() requires 1 argument"))
                })?;
                let s = match val {
                    Value::String(s) => s.clone(),
                    Value::Path(p) => p.clone(),
                    Value::Glob(g) => g.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "glob() requires a string, path, or glob argument",
                    ))),
                };
                Ok(Value::Glob(s))
            }
            "retry" => {
                // retry(max, fn) — calls fn up to max times until it succeeds
                // retry(max, delay_ms, fn) — with delay between retries
                if args.is_empty() {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "retry requires at least 2 arguments: max, fn",
                    )));
                }
                let max_retries = expect_int(&args, 0, "retry max")?;
                let (delay_ms, func) = if args.len() >= 3 {
                    // retry(max, delay, fn)
                    let delay = match &args[1] {
                        Value::Int(ms) => *ms as u64,
                        Value::Duration(val, unit) => duration_to_ms(*val, *unit) as u64,
                        _ => 0,
                    };
                    (delay, args[2].clone())
                } else {
                    (0, args[1].clone())
                };
                let mut last_err = None;
                for attempt in 0..max_retries {
                    match self.call_value(func.clone(), vec![Value::Int(attempt)]) {
                        Ok(v) => return Ok(v),
                        Err(Signal::Error(e)) => {
                            last_err = Some(e);
                            if delay_ms > 0 && attempt < max_retries - 1 {
                                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                            }
                        }
                        Err(other) => return Err(other),
                    }
                }
                Err(Signal::Error(last_err.unwrap_or_else(|| {
                    QueError::runtime("retry exhausted".to_string())
                })))
            }
            "timeout" => {
                // timeout(duration, fn) — runs fn, fails if it takes too long
                // Note: true timeout requires threads; we provide a simplified version
                // that checks after execution
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "timeout requires 2 arguments: duration, fn",
                    )));
                }
                let timeout_ms = match &args[0] {
                    Value::Int(ms) => *ms as u128,
                    Value::Duration(val, unit) => duration_to_ms(*val, *unit) as u128,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "timeout first arg must be a duration",
                    ))),
                };
                let func = args[1].clone();
                let start = std::time::Instant::now();
                let result = self.call_value(func, vec![]);
                let elapsed = start.elapsed().as_millis();
                if elapsed > timeout_ms {
                    return Err(Signal::Error(QueError::runtime(
                        format!("operation timed out after {}ms (limit: {}ms)", elapsed, timeout_ms),
                    )));
                }
                result
            }
            "semver_parse" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "semver_parse requires 1 argument"))
                })?;
                match val {
                    Value::String(s) => {
                        // Basic validation: must have at least X.Y.Z format
                        let base = s.split('-').next().unwrap_or(s);
                        let parts: Vec<&str> = base.split('.').collect();
                        if parts.len() >= 2 && parts.iter().all(|p| p.parse::<u64>().is_ok()) {
                            Ok(Value::Ok(Box::new(Value::Semver(s.clone()))))
                        } else {
                            Ok(Value::Err(Box::new(Value::String(
                                format!("invalid semver string: {}", s),
                            ))))
                        }
                    }
                    _ => Ok(Value::Err(Box::new(Value::String(
                        "semver_parse requires a string".to_string(),
                    )))),
                }
            }

            "regex" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "regex() requires 1 argument"))
                })?;
                match val {
                    Value::Regex(_) => Ok(val.clone()),
                    Value::String(s) => {
                        match regex_lite::Regex::new(s) {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Regex(s.clone())))),
                            Err(e) => Ok(Value::Err(Box::new(Value::String(
                                format!("invalid regex: {}", e),
                            )))),
                        }
                    }
                    _ => Ok(Value::Err(Box::new(Value::String(
                        "regex() requires a string".to_string(),
                    )))),
                }
            }

            // ── Inspection & Reflection builtins ─────────────────────

            "dbg" => {
                // dbg(value) → prints debug info to output AND returns the value
                let val = args.first().cloned().unwrap_or(Value::Null);
                let debug_line = format!("[dbg] {} = {}", val.type_name(), val.debug_string());
                self.emit(debug_line);
                Ok(val)
            }

            // Three of these are already methods on every value, and were
            // only ever a second way to write the same call. The rest moved
            // to `std.reflect`: they are tooling, not vocabulary, and a
            // script that never asks about itself should not carry them.
            // `help` and `dbg` stay global — you type them at a prompt or
            // drop them into a line mid-edit, where an import is friction.
            "inspect" | "methods" | "is_type" | "type_info" | "fields"
            | "has_method" | "vars" | "var_info" | "scope_depth"
            | "modules" => {
                let msg = match name {
                    "inspect" => "`inspect(x)` was removed; use the method `x.inspect()`".to_string(),
                    "methods" => "`methods(x)` was removed; use the method `x.methods()`".to_string(),
                    "is_type" => "`is_type(x, name)` was removed; use the method `x.is_type(name)`".to_string(),
                    other => format!(
                        "`{}(...)` was removed; use `import std.reflect` and `reflect.{}(...)`",
                        other, other
                    ),
                };
                Err(Signal::Error(QueError::new(ErrorKind::Runtime, msg)))
            }
            "help" => {
                // help() → overview; help(value | "name") → details
                let text = self.format_help(args.first());
                self.emit(text);
                Ok(Value::Null)
            }
            "args" => {
                // args() → List<String> — command-line arguments passed to
                // the script (everything after `--` on the `que ... -- ...`
                // command line). Empty in the REPL.
                let items: Vec<Value> = self
                    .script_args
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect();
                Ok(Value::List(items))
            }

            // ── Strict mode ──
            "strict" => {
                // Reporting the mode is fine; changing it half-way through a
                // run is not. A function that type-checks its arguments on
                // one call and not the next cannot be reasoned about, by a
                // reader or by the linter, so the switch lives in the source
                // text (`#!strict`) and on the command line (`--strict`).
                if args.is_empty() {
                    return Ok(Value::Bool(self.strict));
                }
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "strict() no longer takes an argument; put `#!strict` at the top of the file or pass --strict",
                )))
            }

            // ── Task builtins ──
            "tasks" => {
                // tasks() → returns a map of task_name -> task_value for all registered tasks
                let all_vars = self.env.list_vars();
                let mut task_map = BTreeMap::new();
                for (name, val, _) in all_vars {
                    if matches!(&val, Value::Task(_)) {
                        task_map.insert(name, val);
                    }
                }
                Ok(Value::Map(task_map))
            }
            "run_task" => {
                // run_task("name", ...args) → run a task by name with dependency resolution
                let task_name = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "run_task requires a string task name as first argument",
                    ))),
                };
                let task_args: Vec<(Option<String>, Value)> =
                    args.into_iter().skip(1).map(|v| (None, v)).collect();
                let task = self.env.get(&task_name).ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::UndefinedVariable,
                        format!("no task named '{}'", task_name),
                    ))
                })?;
                match task {
                    Value::Task(t) => {
                        self.execute_task(&t, task_args)
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("'{}' is not a task", task_name),
                    ))),
                }
            }

            // ── Stream constructors ──────────────────────────────────
            // Moved to `std.stream`, and `stream()` split in two on the way:
            // it used to pick between "read this file" and "wrap this text"
            // by looking at the argument's type, which hid a disk read behind
            // a name that often did not do one.
            "stream" | "stream_of" | "stdout" | "stderr" | "stdin" => {
                let instead = match name {
                    "stream" => "stream.file(path) for a file, stream.of(x) for text, a list or a handle",
                    "stream_of" => "stream.of(x)",
                    other => return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!(
                            "`{}()` was removed; use `import std.stream` and `stream.{}()`",
                            other, other
                        ),
                    ))),
                };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!(
                        "`{}()` was removed; use `import std.stream` and {}",
                        name, instead
                    ),
                )))
            }

            // Reading and writing config files moved to `std.config`; the
            // rest of what you do to a config is a Map operation and lives
            // on Map.
            "config_read" | "config_write" => {
                let func = name.trim_start_matches("config_");
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!(
                        "`{}(...)` was removed; use `import std.config` and `config.{}(...)`",
                        name, func
                    ),
                )))
            }
            // These six were a second front door onto the same
            // `crate::config::*` functions the Map methods below already call.
            // A config is a Map, so the method form reads better and is one
            // fewer name to learn.
            "config_get" | "config_set" | "config_delete" | "config_has"
            | "config_merge" | "config_paths" => {
                let instead = match name {
                    "config_get" => "map.get_path(path)",
                    "config_set" => "map.set_path(path, value)",
                    "config_delete" => "map.delete_path(path)",
                    "config_has" => "map.has_path(path)",
                    "config_merge" => "base.deep_merge(overlay)",
                    _ => "map.paths()",
                };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`{}()` was removed; use `{}` instead", name, instead),
                )))
            }


            // ── Tool discovery ────────────────────────────────────────
            "which" => {
                let cmd_name = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => v.display_string(),
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "which() requires a command name",
                    ))),
                };
                Ok(which_command(&cmd_name))
            }

            // ── Process control ───────────────────────────────────────
            "os.exit" => {
                let code = match args.first() {
                    Some(Value::Int(n)) => *n as i32,
                    Some(Value::Null) | None => 0,
                    Some(other) => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("os.exit() expects an Int exit code, got {}", other.type_name()),
                    ))),
                };
                Err(Signal::Exit(code))
            }

            // ── File handle ───────────────────────────────────────────
            "open" => {
                let path = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Path(s)) => s.clone(),
                    Some(v) => v.display_string(),
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "open() requires a path argument",
                    ))),
                };
                let mode = match args.get(1) {
                    Some(Value::String(s)) => s.clone(),
                    None => "r".to_string(),
                    Some(v) => v.display_string(),
                };
                match mode.as_str() {
                    "r" => {
                        match std::fs::File::open(&path) {
                            Ok(file) => {
                                let inner = FileHandleInner {
                                    path,
                                    mode: "r".into(),
                                    reader: Some(BufReader::new(file)),
                                    writer: None,
                                    open: true,
                                    discard: false,
                                };
                                Ok(Value::FileHandle(FileHandle {
                                    inner: Arc::new(Mutex::new(inner)),
                                }))
                            }
                            Err(e) => Ok(Value::Err(Box::new(Value::String(
                                format!("open failed: {}", e),
                            )))),
                        }
                    }
                    "w" => {
                        // Creating the file is itself the destructive act: it
                        // truncates. So the dry run stops here rather than at
                        // the first `write`.
                        if self.dry_run_skip(format!("open {} for writing (truncate)", path)) {
                            return Ok(discarding_handle(path, "w"));
                        }
                        match std::fs::File::create(&path) {
                            Ok(file) => {
                                let inner = FileHandleInner {
                                    path,
                                    mode: "w".into(),
                                    reader: None,
                                    writer: Some(BufWriter::new(file)),
                                    open: true,
                                    discard: false,
                                };
                                Ok(Value::FileHandle(FileHandle {
                                    inner: Arc::new(Mutex::new(inner)),
                                }))
                            }
                            Err(e) => Ok(Value::Err(Box::new(Value::String(
                                format!("open failed: {}", e),
                            )))),
                        }
                    }
                    "a" => {
                        if self.dry_run_skip(format!("open {} for appending", path)) {
                            return Ok(discarding_handle(path, "a"));
                        }
                        match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
                            Ok(file) => {
                                let inner = FileHandleInner {
                                    path,
                                    mode: "a".into(),
                                    reader: None,
                                    writer: Some(BufWriter::new(file)),
                                    open: true,
                                    discard: false,
                                };
                                Ok(Value::FileHandle(FileHandle {
                                    inner: Arc::new(Mutex::new(inner)),
                                }))
                            }
                            Err(e) => Ok(Value::Err(Box::new(Value::String(
                                format!("open failed: {}", e),
                            )))),
                        }
                    }
                    other => Ok(Value::Err(Box::new(Value::String(
                        format!("invalid file mode '{}'; use \"r\", \"w\", or \"a\"", other),
                    )))),
                }
            }





            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown builtin function '{}'", name),
            ))),
        }
    }
}

/// A write handle for a dry run: open, correct about its path and mode, and
/// backed by nothing.
///
/// Returning an error from `open` instead would push the script down its
/// failure path, and a dry run that stops at the first write shows nothing
/// about what the rest of the script would have done.
fn discarding_handle(path: String, mode: &str) -> Value {
    Value::FileHandle(FileHandle {
        inner: Arc::new(Mutex::new(FileHandleInner {
            path,
            mode: mode.into(),
            reader: None,
            writer: None,
            open: true,
            discard: true,
        })),
    })
}

/// Search the PATH for an executable named `cmd`. Returns a `Value::Path` with
/// the full path on success, or `Value::Null` when the command is not found.
fn which_command(cmd: &str) -> Value {
    // If the command already contains a path separator, test it directly.
    let p = std::path::Path::new(cmd);
    if p.components().count() > 1 {
        if is_executable(p) {
            return Value::Path(p.to_string_lossy().into_owned());
        }
        return Value::Null;
    }

    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(cmd);
        if is_executable(&candidate) {
            return Value::Path(candidate.to_string_lossy().into_owned());
        }

        // On Windows, also try appending PATHEXT extensions (.EXE, .CMD, …).
        #[cfg(windows)]
        {
            let pathext = std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
            for ext in pathext.split(';') {
                let ext = ext.trim_start_matches('.');
                if ext.is_empty() { continue; }
                let mut with_ext = candidate.clone();
                with_ext.set_extension(ext);
                if with_ext.is_file() {
                    return Value::Path(with_ext.to_string_lossy().into_owned());
                }
            }
        }
    }
    Value::Null
}

/// Returns `true` if `path` is a regular file and is executable.
fn is_executable(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true // On non-Unix just check the file exists (PATHEXT handled above)
    }
}

// ── help() formatting ────────────────────────────────────────────────

impl Interpreter {
    /// Render help text for `help()` (overview) or `help(value | "name")`.
    pub(crate) fn format_help(&self, target: Option<&Value>) -> String {
        match target {
            None => self.help_overview(),
            Some(Value::String(s)) => self.help_for_name(s),
            Some(Value::TypeRef(name)) => self.help_for_type_name(name),
            Some(Value::BuiltinFn(name)) => help_for_builtin(name)
                .unwrap_or_else(|| format!("Builtin function `{}` (no documentation)", name)),
            Some(Value::Module { name, entries }) => help_for_module(name, entries),
            Some(Value::Instance { type_name, fields }) => self.help_for_instance(type_name, fields),
            Some(Value::Enum { enum_name, variant, fields }) => {
                self.help_for_enum_value(enum_name, variant, fields)
            }
            Some(Value::Function { name, params, .. }) => {
                help_for_function(name.as_deref(), params)
            }
            Some(v) => help_for_value(v),
        }
    }

    fn help_for_name(&self, name: &str) -> String {
        // Try a Que keyword first.
        if let Some(doc) = crate::docs::keyword_doc(name) {
            return format!("{}\n\n{}", heading("Keyword", name), strip_md(doc));
        }
        // Then a registered builtin function.
        if let Some(doc) = help_for_builtin(name) {
            return doc;
        }
        // Then a bundled std module name (e.g. "fs", "http").
        if let Some(m) = crate::interpreter::std_modules::all_modules()
            .into_iter()
            .find(|m| m.name == name)
        {
            return help_for_std_module(&m);
        }
        // Then a method by name (any type).
        if let Some(doc) = crate::docs::method_doc(name) {
            return format!(
                "{}\n\n{}",
                heading("Method", &format!(".{}()", name)),
                strip_md(&doc)
            );
        }
        // Then a user-defined type currently in scope.
        if self.struct_defs.contains_key(name)
            || self.enum_defs.contains_key(name)
            || self.trait_defs.contains_key(name)
        {
            return self.help_for_type_name(name);
        }
        // Then a value bound in the current environment.
        if let Some(val) = self.env.get(name) {
            return self.format_help(Some(&val));
        }
        // Finally a built-in type name (`Int`, `List`, ...). This comes last
        // so a user binding named `Int` still wins.
        let methods = crate::docs::methods_for_type(name);
        if !methods.is_empty() {
            return help_for_primitive_type(name, &methods);
        }
        format!(
            "{}\n{}",
            hint(&format!("No help found for `{}`.", name)),
            hint("Try `help()` for an overview, or `methods(value)` to list available methods on a value.")
        )
    }

    fn help_for_type_name(&self, name: &str) -> String {
        let mut out = String::new();
        if let Some(fields) = self.struct_defs.get(name) {
            out.push_str(&format!("{} {} {{\n", keyword("struct"), type_label(name)));
            for f in fields {
                if let Some(d) = &f.default {
                    out.push_str(&format!("    {} = {},\n", f.name, d.display_string()));
                } else {
                    out.push_str(&format!("    {},\n", f.name));
                }
            }
            out.push_str("}\n");
        } else if let Some(variants) = self.enum_defs.get(name) {
            out.push_str(&format!("{} {} {{\n", keyword("enum"), type_label(name)));
            for (variant, vfields) in variants {
                if vfields.is_empty() {
                    out.push_str(&format!("    {},\n", variant));
                } else {
                    out.push_str(&format!("    {}({}),\n", variant, vfields.join(", ")));
                }
            }
            out.push_str("}\n");
        } else if let Some(methods) = self.trait_defs.get(name) {
            out.push_str(&format!("{} {} {{\n", keyword("trait"), type_label(name)));
            for m in methods {
                out.push_str(&format!(
                    "    {} {}({})\n",
                    keyword("fn"),
                    ident(&m.name),
                    format_params(&m.params)
                ));
            }
            out.push_str("}\n");
        } else {
            // Probably a built-in primitive type name (Int, String, List, ...).
            return help_for_primitive_type(name, &crate::docs::methods_for_type(name));
        }
        // Append impl methods and trait impls.
        if let Some(methods) = self.impl_methods.get(name) {
            if !methods.is_empty() {
                out.push_str(&format!("\n{} {} {{\n", keyword("impl"), type_label(name)));
                for m in methods {
                    let kw = if m.is_static { "fn (static)" } else { "fn" };
                    out.push_str(&format!(
                        "    {} {}({})\n",
                        keyword(kw),
                        ident(&m.name),
                        format_params(&m.params)
                    ));
                }
                out.push_str("}\n");
            }
        }
        let trait_keys: Vec<_> = self
            .trait_impls
            .keys()
            .filter(|(ty, _)| ty == name)
            .collect();
        for (_, trait_name) in trait_keys {
            if let Some(methods) = self.trait_impls.get(&(name.to_string(), trait_name.clone())) {
                out.push_str(&format!(
                    "\n{} {} {} {} {{\n",
                    keyword("impl"),
                    type_label(trait_name),
                    keyword("for"),
                    type_label(name)
                ));
                for m in methods {
                    out.push_str(&format!(
                        "    {} {}({})\n",
                        keyword("fn"),
                        ident(&m.name),
                        format_params(&m.params)
                    ));
                }
                out.push_str("}\n");
            }
        }
        out
    }

    fn help_for_instance(&self, type_name: &str, fields: &std::collections::BTreeMap<String, Value>) -> String {
        let mut out = format!(
            "{}\n\n{}\n",
            heading("Instance of", type_name),
            section("Fields:")
        );
        for (k, v) in fields {
            out.push_str(&format!(
                "  {}: {} = {}\n",
                ident(k),
                type_label(v.type_name()),
                v.display_string()
            ));
        }
        if let Some(methods) = self.impl_methods.get(type_name) {
            if !methods.is_empty() {
                out.push_str(&format!("\n{}\n", section("Methods:")));
                for m in methods.iter().filter(|m| !m.is_static) {
                    out.push_str(&format!("  {}\n", ident(&format!(".{}({})", m.name, format_params(&m.params)))));
                }
            }
        }
        out
    }

    fn help_for_enum_value(
        &self,
        enum_name: &str,
        variant: &str,
        fields: &std::collections::BTreeMap<String, Value>,
    ) -> String {
        let mut out = format!(
            "{}\n",
            heading("Enum value", &format!("{}.{}", enum_name, variant))
        );
        if !fields.is_empty() {
            out.push_str(&format!("\n{}\n", section("Fields:")));
            for (k, v) in fields {
                out.push_str(&format!(
                    "  {}: {} = {}\n",
                    ident(k),
                    type_label(v.type_name()),
                    v.display_string()
                ));
            }
        }
        out.push_str(&format!(
            "\n{}\n",
            hint(&format!("Use `help(\"{}\")` to see all variants.", enum_name))
        ));
        out
    }

    fn help_overview(&self) -> String {
        let mut s = String::new();
        s.push_str(&title("Que REPL help"));
        s.push('\n');
        s.push_str("─────────────\n");
        s.push_str(&format!("\n{}\n", section("Usage:")));
        s.push_str(&entry("help()", "show this overview"));
        s.push_str(&entry("help(value)", "show type, fields, and methods of a value"));
        s.push_str(&entry("help(MyType)", "show definition of a user-defined struct/enum/trait"));
        s.push_str(&entry("help(\"println\")", "look up a builtin, keyword, method, or module by name"));
        s.push_str(&entry("modules()", "map of std module names → exported function names"));
        s.push_str(&entry("methods(value)", "list method names available on a value"));
        s.push_str(&entry("inspect(value)", "detailed map of the value's contents"));
        s.push_str(&entry("vars()", "map of all user-defined variables in scope"));
        s.push_str(&entry("typeof(value)", "type name as a string"));
        s.push_str(&format!(
            "\n{}\n",
            section("REPL meta-commands (no parentheses needed):")
        ));
        s.push_str(&entry(":h, :help", "show this overview"));
        s.push_str(&entry(":h <expr>, ?<expr>", "help on an expression's value"));
        s.push_str(&entry(":t <expr>", "typeof an expression"));
        s.push_str(&entry(":m <expr>", "methods of an expression's value"));
        s.push_str(&entry(":i <expr>", "inspect an expression's value"));
        s.push_str(&entry(":v, :vars", "list user-defined variables"));
        s.push_str(&entry(":load <file.que>", "evaluate a Que file in the current session"));
        s.push_str(&entry(":reset", "reset the interpreter (clears all bindings)"));
        s.push_str(&entry(":q, :exit", "exit the REPL (or press Ctrl-D)"));
        s.push_str(&format!("\n{}\n", section("Builtin categories:")));
        for (cat, names) in &builtin_groups() {
            s.push_str(&format!(
                "  {}:\n    {}\n",
                cat.bold(),
                ident(&names.join(", "))
            ));
        }
        s.push_str(&format!(
            "  {}\n",
            hint("Use `help(\"<name>\")` for details on any builtin.")
        ));
        s.push_str(&format!("\n{}\n", section("Standard library modules:")));
        let mods: Vec<&'static str> = crate::interpreter::std_modules::all_modules()
            .into_iter()
            .map(|m| m.name)
            .collect();
        s.push_str("  ");
        s.push_str(&ident(&mods.join(", ")));
        s.push('\n');
        s.push_str(&format!(
            "  {}\n",
            hint("Import with `import std.<name>`; use `help(\"<name>\")` to list its functions.")
        ));

        // List user packages from <package_root>/que_packages/ if any.
        if let Some(loader) = self.module_loader.as_ref() {
            let pkg_dir = loader.package_root().join("que_packages");
            if let Ok(rd) = std::fs::read_dir(&pkg_dir) {
                let mut names: Vec<String> = rd
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                if !names.is_empty() {
                    s.push_str(&format!(
                        "\n{}\n  {}\n",
                        section(&format!("Local packages in {}:", pkg_dir.display())),
                        ident(&names.join(", "))
                    ));
                }
            }
        }

        s.push_str(&format!(
            "\n{}\n",
            hint("Tutorial: see `tutorial.md` for the full language guide.")
        ));
        s
    }
}

fn builtin_groups() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("I/O",            vec!["print", "println", "input", "confirm"]),
        ("Types",          vec!["typeof", "str", "int", "float", "bool"]),
        ("Numbers",        vec!["abs", "min", "max", "range"]),
        ("Strings",        vec!["chr", "ord"]),
        ("Paths & FS",     vec!["path", "glob", "open", "cd", "quefile_dir", "script_dir"]),
        ("Errors/Results", vec!["Ok", "Err", "fail", "assert"]),
        ("Process",        vec!["which", "retry", "timeout", "sleep", "dry_run"]),
        ("Tasks",          vec!["tasks", "run_task"]),
        ("Debugging",      vec!["help", "dbg"]),
        ("Misc",           vec!["env", "args", "secret", "regex", "semver_parse", "compose",
                                "strict"]),
    ]
}

fn help_for_module(name: &str, entries: &std::collections::BTreeMap<String, Value>) -> String {
    let mut s = format!(
        "{}\n\n{}\n",
        heading("Module", name),
        section(&format!("Exports ({}):", entries.len()))
    );
    for (k, v) in entries {
        s.push_str(&format!("  {}: {}\n", ident(k), type_label(v.type_name())));
    }
    s.push_str(&format!(
        "\n{}\n",
        hint(&format!(
            "Access with `{}.<name>`, or import: `import {} {{ name }}`.",
            name, name
        ))
    ));
    s
}

fn help_for_std_module(m: &crate::interpreter::std_modules::StdModule) -> String {
    let mut s = format!(
        "{}\n\n{}\n  {}\n  {}\n\n",
        heading("Standard library module", m.name),
        section("Import with:"),
        ident(&format!("import std.{}", m.name)),
        ident(&format!(
            "import std.{} {{ {} }}",
            m.name,
            m.functions.first().copied().unwrap_or("…")
        ))
    );
    s.push_str(&format!("{}\n", section(&format!("Functions ({}):", m.functions.len()))));
    let qualified: Vec<String> = m
        .functions
        .iter()
        .map(|f| format!("{}.{}", m.name, f))
        .collect();
    let refs: Vec<&str> = qualified.iter().map(|s| s.as_str()).collect();
    // Wrap first, colour afterwards: the wrapper measures raw widths.
    s.push_str(&paint_lines(&wrap_list(&refs, 76, "  ")));
    s.push('\n');
    s.push_str(&format!(
        "\n{}\n",
        hint(&format!(
            "Use `help(\"{}.<func>\")` for details on a function.",
            m.name
        ))
    ));
    s
}

/// Help page for a built-in type name (`Int`, `List`, ...).
fn help_for_primitive_type(name: &str, methods: &[(&'static str, &'static str)]) -> String {
    format!(
        "{}\n\n{}\n{}",
        heading("Type", name),
        section("Methods:"),
        format_method_list(methods)
    )
}

fn help_for_value(v: &Value) -> String {
    let ty = v.type_name();
    let methods = v.available_methods();
    let mut s = format!("{}\n  = {}\n", heading("Value of type", ty), v.debug_string());
    if !methods.is_empty() {
        s.push_str(&format!("\n{}\n", section(&format!("Methods ({}):", methods.len()))));
        s.push_str(&paint_lines(&wrap_list(&methods, 76, "  ")));
        s.push('\n');
        s.push_str(&format!(
            "\n{}\n",
            hint("Use `help(\"<method>\")` for documentation on any of these.")
        ));
    }
    s
}

fn help_for_function(name: Option<&str>, params: &[crate::ast::Param]) -> String {
    let n = name.unwrap_or("<anonymous>");
    format!(
        "{}\n",
        heading("Function", &format!("{}({})", n, format_params(params)))
    )
}

fn help_for_builtin(name: &str) -> Option<String> {
    crate::docs::builtin_functions()
        .into_iter()
        .find(|b| b.name == name)
        .map(|b| {
            format!(
                "{}\n\n  {}\n\n{}",
                heading("Builtin", b.name),
                b.signature.green(),
                strip_md(b.documentation)
            )
        })
}

fn format_params(params: &[crate::ast::Param]) -> String {
    params
        .iter()
        .map(|p| match &p.type_ann {
            Some(t) => format!("{}: {}", p.name, t),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_method_list(methods: &[(&'static str, &'static str)]) -> String {
    if methods.is_empty() {
        return "  (none)\n".to_string();
    }
    let mut s = String::new();
    for (name, sig) in methods {
        s.push_str(&format!("  {}{}\n", format!(".{}", name).green(), sig));
    }
    s
}

// ── help() colouring ─────────────────────────────────────────────────
//
// `colored` disables itself automatically when stdout is not a terminal or
// NO_COLOR is set, so these helpers degrade to plain text off the REPL.

/// A top-level heading with no subject, e.g. `Que REPL help`.
fn title(s: &str) -> String {
    s.cyan().bold().to_string()
}

/// A help page heading: what kind of thing it is, plus its name.
///
/// The name is coloured rather than wrapped in backticks, so headings match
/// the way identifiers are rendered everywhere else in the page.
fn heading(kind: &str, subject: &str) -> String {
    format!("{} {}", kind.cyan().bold(), subject.green().bold())
}

/// A section label inside a help page, e.g. `Functions (12):`.
fn section(s: &str) -> String {
    s.yellow().bold().to_string()
}

/// A prose line pointing at something to try next. Backticked spans in the
/// text are coloured like every other identifier.
fn hint(s: &str) -> String {
    render_inline_md(s)
}

/// An identifier: function, field, module, or method name.
fn ident(s: &str) -> String {
    s.green().to_string()
}

/// A type name.
fn type_label(s: &str) -> String {
    s.magenta().to_string()
}

/// A Que keyword shown in a rendered definition.
fn keyword(s: &str) -> String {
    s.blue().bold().to_string()
}

/// One `  <command>   <description>` row of the overview table.
///
/// The command is padded before being coloured: padding a `ColoredString`
/// would count the escape bytes and break the alignment.
fn entry(cmd: &str, desc: &str) -> String {
    format!("  {} {}\n", format!("{:<22}", cmd).green(), desc)
}

/// Colour an already-wrapped block line by line, leaving the layout intact.
fn paint_lines(block: &str) -> String {
    block
        .lines()
        .map(|l| l.green().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_list(items: &[&str], width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line = String::from(indent);
    for (i, item) in items.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        if !line.is_empty() && line.len() + sep.len() + item.len() > width && line != indent {
            out.push_str(line.trim_end_matches(", "));
            out.push('\n');
            line = String::from(indent);
            line.push_str(item);
        } else {
            line.push_str(sep);
            line.push_str(item);
        }
    }
    if !line.trim().is_empty() {
        out.push_str(&line);
    }
    out
}

/// Render Markdown documentation as coloured plain text for the REPL.
///
/// Code fences become green blocks, inline `` `code` `` becomes green and
/// `**bold**` becomes bold; the fence and emphasis markers themselves are
/// dropped so the text reads cleanly in a terminal.
fn strip_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_fence = false;
    for line in s.lines() {
        let trimmed = line.trim_start();
        // Drop language-marker fence lines like ```que and closing ```.
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push_str(&line.green().to_string());
        } else {
            out.push_str(&render_inline_md(line));
        }
        out.push('\n');
    }
    out
}

/// Colour inline `` `code` `` and `**bold**` spans within a single line.
fn render_inline_md(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while !rest.is_empty() {
        // Pick whichever marker opens first.
        let (is_code, at) = match (rest.find('`'), rest.find("**")) {
            (Some(c), Some(b)) => (c < b, c.min(b)),
            (Some(c), None) => (true, c),
            (None, Some(b)) => (false, b),
            (None, None) => {
                out.push_str(rest);
                break;
            }
        };
        out.push_str(&rest[..at]);
        let marker = if is_code { "`" } else { "**" };
        let after = &rest[at + marker.len()..];
        match after.find(marker) {
            Some(end) => {
                let inner = &after[..end];
                out.push_str(&if is_code {
                    inner.green().to_string()
                } else {
                    inner.bold().to_string()
                });
                rest = &after[end + marker.len()..];
            }
            // Unbalanced marker: emit the remainder verbatim.
            None => {
                out.push_str(&rest[at..]);
                break;
            }
        }
    }
    out
}



