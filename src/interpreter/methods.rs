//! Method dispatch for the Que interpreter.

use super::helpers::*;
use super::Interpreter;
use crate::error::*;
use crate::token::DurationUnit;
use crate::value::{CmdModifiers, CmdPart, FileHandleInner, Stream, StreamOp, StreamSink, StreamSource, Value};
use std::sync::{Arc, Mutex};

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

impl Interpreter {
    // ── Method calls ─────────────────────────────────────────────────

    pub(crate) fn call_method(
        &mut self,
        obj: &Value,
        method: &str,
        args: Vec<Value>,
    ) -> IResult {
        // ── Universal methods available on all types ──
        if method == "inspect" {
            return Ok(Value::Map(obj.inspect_map()));
        }
        if method == "methods" {
            return Ok(Value::List(
                obj.available_methods()
                    .iter()
                    .map(|s| Value::String(s.to_string()))
                    .collect(),
            ));
        }
        if method == "type_name" {
            return Ok(Value::String(obj.type_name().to_string()));
        }
        if method == "is_type" {
            let type_name = match args.first() {
                Some(Value::String(s)) => s.clone(),
                _ => return Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    "is_type() requires a string argument",
                ))),
            };
            // Only the capitalised spelling. `type_name()` reports "Int", the
            // annotation syntax writes `Int`, so `is_type("int")` was a third
            // way of naming the same thing.
            let matches = match type_name.as_str() {
                "Function" | "Fn" => {
                    matches!(obj, Value::Function { .. } | Value::BuiltinFn(_))
                }
                _ => obj.type_name() == type_name,
            };
            return Ok(Value::Bool(matches));
        }

        match obj {
            // ── String methods ──
            Value::String(s) => self.string_method(s, method, &args),
            // ── List methods ──
            Value::List(items) => self.list_method(items, method, args),
            // ── Set methods ──
            Value::Set(items) => self.set_method(items, method, args),
            // ── Map methods ──
            Value::Map(map) => self.map_method(map, method, &args),
            // ── Path methods ──
            Value::Path(p) => self.path_method(p, method, &args),
            // ── Duration methods ──
            Value::Duration(val, unit) => self.duration_method(*val, *unit, method),
            // ── ProcessResult methods ──
            Value::ProcessResult {
                exit_code,
                stdout,
                stderr,
            } => self.process_result_method(*exit_code, stdout, stderr, method),
            // ── Result/Option methods ──
            Value::Ok(inner) => match method {
                "unwrap" => Ok(*inner.clone()),
                "is_ok" => Ok(Value::Bool(true)),
                "is_err" => Ok(Value::Bool(false)),
                "map" => {
                    if let Some(func) = args.first() {
                        let result = self.call_value(func.clone(), vec![*inner.clone()])?;
                        Ok(Value::Ok(Box::new(result)))
                    } else {
                        Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            "map requires a function argument",
                        )))
                    }
                }
                "unwrap_or" => Ok(*inner.clone()),
                "and_then" => {
                    if let Some(func) = args.first() {
                        self.call_value(func.clone(), vec![*inner.clone()])
                    } else {
                        Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            "and_then requires a function argument",
                        )))
                    }
                }
                "map_err" => Ok(Value::Ok(inner.clone())),
                "or_else" => Ok(Value::Ok(inner.clone())),
                // context() on Ok is a pass-through — used for error chain compatibility
                "context" => Ok(Value::Ok(inner.clone())),
                "unwrap_err" => Err(Signal::Error(QueError::runtime(
                    "called unwrap_err on Ok(_)",
                ))),
                "value" => Ok(*inner.clone()),
                "ok" | "inner" => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`.{}()` was removed; use `.value()` instead", method),
                ))),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("Ok has no method '{}'", method),
                ))),
            },
            Value::Err(inner) => match method {
                "unwrap" => Err(Signal::Error(QueError::runtime(format!(
                    "called unwrap on Err({})",
                    inner
                )))),
                "is_ok" => Ok(Value::Bool(false)),
                "is_err" => Ok(Value::Bool(true)),
                "map" => Ok(Value::Err(inner.clone())),
                "unwrap_or" => {
                    Ok(args.first().cloned().unwrap_or(Value::Null))
                }
                "and_then" => Ok(Value::Err(inner.clone())),
                "map_err" => {
                    if let Some(func) = args.first() {
                        let result = self.call_value(func.clone(), vec![*inner.clone()])?;
                        Ok(Value::Err(Box::new(result)))
                    } else {
                        Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            "map_err requires a function argument",
                        )))
                    }
                }
                "or_else" => {
                    if let Some(func) = args.first() {
                        self.call_value(func.clone(), vec![*inner.clone()])
                    } else {
                        Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            "or_else requires a function argument",
                        )))
                    }
                }
                // context(msg) wraps the error message with additional context
                "context" => {
                    let ctx = args.first().map(|v| v.display_string()).unwrap_or_default();
                    let wrapped = format!("{}: {}", ctx, inner.display_string());
                    Ok(Value::Err(Box::new(Value::String(wrapped))))
                }
                "unwrap_err" => Ok(*inner.clone()),
                "value" => Ok(*inner.clone()),
                "err" | "inner" => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`.{}()` was removed; use `.value()` instead", method),
                ))),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("Err has no method '{}'", method),
                ))),
            },
            // ── Semver methods ──
            Value::Semver(s) => self.semver_method(s, method, &args),
            // ── Secret methods ──
            Value::Secret(s) => self.secret_method(s, method),
            // ── Glob methods ──
            Value::Glob(g) => self.glob_method(g, method, &args),
            // ── Regex methods ──
            Value::Regex(r) => self.regex_method(r, method, &args),
            // ── Cmd methods ──
            Value::Cmd(parts, mods) => self.cmd_method(parts, &mods, method, &args),
            // ── Task methods ──
            Value::Task(t) => {
                self.task_method(&t, method, args)
            }
            // ── ProcessHandle methods ──
            Value::ProcessHandle(handle) => {
                let handle = handle.clone();
                match method {
                    "pid" => Ok(Value::Int(handle.pid as i64)),
                    "is_alive" => {
                        let mut child = handle.child.lock().map_err(|_| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, "process handle lock poisoned"))
                        })?;
                        // Try_wait returns None if process is still running
                        match child.try_wait() {
                            Ok(None) => Ok(Value::Bool(true)),
                            Ok(Some(_)) => Ok(Value::Bool(false)),
                            Err(_) => Ok(Value::Bool(false)),
                        }
                    }
                    "wait" => {
                        let mut child = handle.child.lock().map_err(|_| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, "process handle lock poisoned"))
                        })?;
                        let status = child.wait().map_err(|e| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, format!("wait failed: {}", e)))
                        })?;
                        Ok(Value::Int(status.code().unwrap_or(-1) as i64))
                    }
                    "kill" => {
                        let mut child = handle.child.lock().map_err(|_| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, "process handle lock poisoned"))
                        })?;
                        child.kill().map_err(|e| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, format!("kill failed: {}", e)))
                        })?;
                        Ok(Value::Null)
                    }
                    "kill_force" => {
                        // On Unix, kill_force sends SIGKILL; on other platforms same as kill
                        let mut child = handle.child.lock().map_err(|_| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, "process handle lock poisoned"))
                        })?;
                        child.kill().map_err(|e| {
                            Signal::Error(QueError::new(ErrorKind::Runtime, format!("kill_force failed: {}", e)))
                        })?;
                        Ok(Value::Null)
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("ProcessHandle has no method '{}'", method),
                    ))),
                }
            }
            // ── FileHandle methods ──
            Value::FileHandle(fh) => {
                let fh = fh.clone();
                match method {
                    "path" => {
                        let inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        Ok(Value::Path(inner.path.clone()))
                    }
                    "is_open" => {
                        let inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        Ok(Value::Bool(inner.open))
                    }
                    "close" => {
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if inner.open {
                            if let Some(mut w) = inner.writer.take() {
                                let _ = w.flush();
                            }
                            inner.reader.take();
                            inner.open = false;
                        }
                        Ok(Value::Null)
                    }
                    "flush" => {
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        if inner.discard {
                            return Ok(Value::Null);
                        }
                        match inner.writer.as_mut() {
                            Some(w) => match w.flush() {
                                Ok(_) => Ok(Value::Null),
                                Err(e) => Ok(Value::Err(Box::new(Value::String(format!("flush failed: {}", e))))),
                            },
                            None => Ok(Value::Err(Box::new(Value::String("file not open for writing".into())))),
                        }
                    }
                    "read" => {
                        use std::io::Read;
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        match inner.reader.as_mut() {
                            Some(r) => {
                                let mut buf = String::new();
                                match r.read_to_string(&mut buf) {
                                    Ok(_) => Ok(Value::Ok(Box::new(Value::String(buf)))),
                                    Err(e) => Ok(Value::Err(Box::new(Value::String(format!("read failed: {}", e))))),
                                }
                            }
                            None => Ok(Value::Err(Box::new(Value::String("file not open for reading".into())))),
                        }
                    }
                    "read_line" => {
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        match inner.reader.as_mut() {
                            Some(r) => {
                                let mut line = String::new();
                                match r.read_line(&mut line) {
                                    Ok(0) => Ok(Value::Null), // EOF
                                    Ok(_) => {
                                        let stripped = line.trim_end_matches('\n').trim_end_matches('\r').to_string();
                                        Ok(Value::String(stripped))
                                    }
                                    Err(e) => Ok(Value::Err(Box::new(Value::String(format!("read_line failed: {}", e))))),
                                }
                            }
                            None => Ok(Value::Err(Box::new(Value::String("file not open for reading".into())))),
                        }
                    }
                    "lines" => {
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        match inner.reader.as_mut() {
                            Some(r) => {
                                let mut result = Vec::new();
                                let mut line = String::new();
                                loop {
                                    line.clear();
                                    match r.read_line(&mut line) {
                                        Ok(0) => break,
                                        Ok(_) => {
                                            let stripped = line.trim_end_matches('\n').trim_end_matches('\r').to_string();
                                            result.push(Value::String(stripped));
                                        }
                                        Err(e) => return Ok(Value::Err(Box::new(Value::String(format!("lines failed: {}", e))))),
                                    }
                                }
                                Ok(Value::List(result))
                            }
                            None => Ok(Value::Err(Box::new(Value::String("file not open for reading".into())))),
                        }
                    }
                    "write" => {
                        let data = match args.first() {
                            Some(v) => v.display_string(),
                            None => return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch, "write() requires a string argument",
                            ))),
                        };
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        if inner.discard {
                            let path = inner.path.clone();
                            drop(inner);
                            self.dry_run_skip(format!(
                                "write {} ({} bytes)",
                                path,
                                data.len()
                            ));
                            return Ok(Value::Null);
                        }
                        match inner.writer.as_mut() {
                            Some(w) => match w.write_all(data.as_bytes()) {
                                Ok(_) => Ok(Value::Null),
                                Err(e) => Ok(Value::Err(Box::new(Value::String(format!("write failed: {}", e))))),
                            },
                            None => Ok(Value::Err(Box::new(Value::String("file not open for writing".into())))),
                        }
                    }
                    "writeln" => {
                        let data = match args.first() {
                            Some(v) => v.display_string(),
                            None => String::new(),
                        };
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        if inner.discard {
                            let path = inner.path.clone();
                            drop(inner);
                            // The newline is counted because it is written.
                            self.dry_run_skip(format!(
                                "write {} ({} bytes)",
                                path,
                                data.len() + 1
                            ));
                            return Ok(Value::Null);
                        }
                        match inner.writer.as_mut() {
                            Some(w) => {
                                let line = format!("{}\n", data);
                                match w.write_all(line.as_bytes()) {
                                    Ok(_) => Ok(Value::Null),
                                    Err(e) => Ok(Value::Err(Box::new(Value::String(format!("writeln failed: {}", e))))),
                                }
                            }
                            None => Ok(Value::Err(Box::new(Value::String("file not open for writing".into())))),
                        }
                    }
                    "seek" => {
                        use std::io::Seek;
                        let offset = match args.first() {
                            Some(Value::Int(n)) => *n,
                            Some(_) => return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch, "seek() offset must be an integer",
                            ))),
                            None => 0,
                        };
                        let origin = match args.get(1) {
                            Some(Value::String(s)) => s.as_str(),
                            None => "start",
                            _ => return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch, "seek() origin must be a string",
                            ))),
                        };
                        let seek_from = match origin {
                            "start" => std::io::SeekFrom::Start(offset as u64),
                            "current" => std::io::SeekFrom::Current(offset),
                            "end" => std::io::SeekFrom::End(offset),
                            _ => return Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                format!("seek() origin must be \"start\", \"current\", or \"end\", got \"{}\"", origin),
                            ))),
                        };
                        let mut inner = fh.inner.lock().map_err(|_| Signal::Error(QueError::new(
                            ErrorKind::Runtime, "FileHandle lock poisoned",
                        )))?;
                        if !inner.open {
                            return Ok(Value::Err(Box::new(Value::String("file is closed".into()))));
                        }
                        let result = if let Some(r) = inner.reader.as_mut() {
                            r.seek(seek_from)
                        } else if let Some(w) = inner.writer.as_mut() {
                            w.seek(seek_from)
                        } else {
                            return Ok(Value::Err(Box::new(Value::String("file has no reader or writer".into()))));
                        };
                        match result {
                            Ok(pos) => Ok(Value::Int(pos as i64)),
                            Err(e) => Ok(Value::Err(Box::new(Value::String(format!("seek failed: {}", e))))),
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("FileHandle has no method '{}'", method),
                    ))),
                }
            }
            // ── Tuple methods ──
            Value::Tuple(items) => self.tuple_method(items, method, &args),
            // ── BuiltinFn methods (e.g. path.home()) ──
            Value::BuiltinFn(name) if name == "path" => {
                match method {
                    "home" => {
                        let home = crate::interpreter::helpers::home_dir()
                            .unwrap_or_else(|| ".".to_string());
                        Ok(Value::Path(home))
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("path has no method '{}'", method),
                    ))),
                }
            }
            // ── `env` namespace methods (env.get(), env.int(), env.is_ci(), ...) ──
            Value::BuiltinFn(name) if name == "env" => {
                // The environment is where credentials live in CI, so it is
                // its own capability rather than being folded into `read`.
                // Purely informational methods are exempt: denying
                // `env.is_ci()` protects nothing and only breaks scripts.
                if self.permissions.is_some()
                    && !matches!(
                        method,
                        "is_ci" | "ci_name" | "platform" | "is_root" | "is_interactive"
                    )
                {
                    // Most env methods name one variable. `env.scope` and
                    // `env.set_all` take a map, and dumping the whole map
                    // into the denial would print its values -- which is
                    // exactly the material this capability exists to guard.
                    // Each key is checked on its own instead.
                    let keys: Vec<String> = match args.first() {
                        Some(Value::Map(m)) => m.keys().cloned().collect(),
                        Some(v) => vec![v.display_string()],
                        None => vec![format!("env.{}", method)],
                    };
                    for key in &keys {
                        self.check_permission(crate::permissions::Capability::Env, key)?;
                    }
                }
                match method {
                    "get" => {
                        // env.get(KEY) -> String | null
                        // env.get(KEY, default) -> String
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        if key.is_empty() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch,
                                "env.get() requires a key argument",
                            )));
                        }
                        match std::env::var(&key) {
                            Ok(v) => Ok(Value::String(v)),
                            Err(_) => Ok(args.get(1).cloned().unwrap_or(Value::Null)),
                        }
                    }
                    "has" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        Ok(Value::Bool(std::env::var(&key).is_ok()))
                    }
                    "unset" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        if key.is_empty() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch,
                                "env.unset() requires a key argument",
                            )));
                        }
                        std::env::remove_var(&key);
                        Ok(Value::Null)
                    }
                    "scope" => {
                        // Returns an EnvScope context manager:
                        //   with env.scope({ "DEBUG": "1" }) { ... }
                        let vars = match args.first() {
                            Some(Value::Map(m)) => m.clone(),
                            _ => return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch,
                                "env.scope() requires a map of variables",
                            ))),
                        };
                        let mut fields = BTreeMap::new();
                        fields.insert("vars".to_string(), Value::Map(vars));
                        Ok(Value::Instance {
                            type_name: "EnvScope".to_string(),
                            fields,
                        })
                    }
                    "bool" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        match std::env::var(&key) {
                            Ok(v) => {
                                let b = matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
                                Ok(Value::Bool(b))
                            }
                            Err(_) => Ok(Value::Null),
                        }
                    }
                    "int" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        match std::env::var(&key) {
                            Ok(v) => {
                                if let Ok(n) = v.parse::<i64>() {
                                    Ok(Value::Int(n))
                                } else {
                                    Ok(Value::Err(Box::new(Value::String(
                                        format!("cannot parse '{}' as int", v),
                                    ))))
                                }
                            }
                            Err(_) => Ok(Value::Null),
                        }
                    }
                    "list" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        let sep = args.get(1).map(|v| v.display_string()).unwrap_or_else(|| ",".to_string());
                        match std::env::var(&key) {
                            Ok(v) => {
                                let items: Vec<Value> = v.split(&sep)
                                    .map(|s| Value::String(s.trim().to_string()))
                                    .collect();
                                Ok(Value::List(items))
                            }
                            Err(_) => Ok(Value::Null),
                        }
                    }
                    "set" => {
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        let val = args.get(1).map(|v| v.display_string()).unwrap_or_default();
                        if key.is_empty() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch,
                                "env.set() requires a key argument",
                            )));
                        }
                        std::env::set_var(&key, &val);
                        Ok(Value::Null)
                    }
                    "loadFile" | "load_file" => {
                        let path = args.first().map(|v| v.display_string()).unwrap_or_default();
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                for line in content.lines() {
                                    let line = line.trim();
                                    if line.is_empty() || line.starts_with('#') {
                                        continue;
                                    }
                                    if let Some((key, val)) = line.split_once('=') {
                                        let key = key.trim();
                                        let val = val.trim().trim_matches('"').trim_matches('\'');
                                        std::env::set_var(key, val);
                                    }
                                }
                                Ok(Value::Ok(Box::new(Value::Null)))
                            }
                            Err(e) => Ok(Value::Err(Box::new(Value::String(
                                format!("failed to load env file: {}", e),
                            )))),
                        }
                    }
                    "all" => {
                        // Return all environment variables as a Map<String, String>
                        let mut map = BTreeMap::new();
                        for (key, val) in std::env::vars() {
                            map.insert(key, Value::String(val));
                        }
                        Ok(Value::Map(map))
                    }
                    "require" => {
                        // require(["VAR1", "VAR2"]) or require("VAR1") — fail if any var is missing
                        let keys: Vec<String> = match args.first() {
                            Some(Value::List(items)) => items
                                .iter()
                                .map(|v| v.display_string())
                                .collect(),
                            Some(v) => vec![v.display_string()],
                            None => return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch,
                                "env.require() requires at least one variable name",
                            ))),
                        };
                        let mut missing = Vec::new();
                        for key in &keys {
                            if std::env::var(key).is_err() {
                                missing.push(key.clone());
                            }
                        }
                        if !missing.is_empty() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                format!(
                                    "required environment variable{} not set: {}",
                                    if missing.len() == 1 { "" } else { "s" },
                                    missing.join(", ")
                                ),
                            )));
                        }
                        // Return a map of the required vars
                        let mut result = BTreeMap::new();
                        for key in &keys {
                            if let Ok(val) = std::env::var(key) {
                                result.insert(key.clone(), Value::String(val));
                            }
                        }
                        Ok(Value::Map(result))
                    }
                    "is_ci" => {
                        // Detect common CI environment indicators
                        let is_ci = std::env::var("CI").is_ok()
                            || std::env::var("GITHUB_ACTIONS").is_ok()
                            || std::env::var("JENKINS_URL").is_ok()
                            || std::env::var("GITLAB_CI").is_ok()
                            || std::env::var("CIRCLECI").is_ok()
                            || std::env::var("TRAVIS").is_ok()
                            || std::env::var("BUILDKITE").is_ok()
                            || std::env::var("TEAMCITY_VERSION").is_ok();
                        Ok(Value::Bool(is_ci))
                    }
                    "ci_name" => {
                        // Return the name of the CI provider or null
                        let name = if std::env::var("GITHUB_ACTIONS").is_ok() {
                            Some("github-actions")
                        } else if std::env::var("GITLAB_CI").is_ok() {
                            Some("gitlab-ci")
                        } else if std::env::var("JENKINS_URL").is_ok() {
                            Some("jenkins")
                        } else if std::env::var("CIRCLECI").is_ok() {
                            Some("circleci")
                        } else if std::env::var("TRAVIS").is_ok() {
                            Some("travis-ci")
                        } else if std::env::var("BUILDKITE").is_ok() {
                            Some("buildkite")
                        } else if std::env::var("TEAMCITY_VERSION").is_ok() {
                            Some("teamcity")
                        } else if std::env::var("CI").is_ok() {
                            Some("unknown-ci")
                        } else {
                            None
                        };
                        Ok(name.map_or(Value::Null, |s| Value::String(s.to_string())))
                    }
                    "secret" => {
                        // The environment is where secrets actually arrive in
                        // CI, and reading one through `env.get` first would
                        // create an unredacted String that the scrubber never
                        // learns about.
                        let key = args.first().map(|v| v.display_string()).unwrap_or_default();
                        if key.is_empty() {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::ArityMismatch,
                                "env.secret() requires a variable name",
                            )));
                        }
                        match std::env::var(&key) {
                            Ok(v) => {
                                self.register_secret(&v);
                                Ok(Value::Ok(Box::new(Value::Secret(v))))
                            }
                            // The name is safe to report; it is the value
                            // that is secret.
                            Err(_) => Ok(Value::Err(Box::new(Value::String(format!(
                                "environment variable '{}' is not set",
                                key
                            ))))),
                        }
                    }
                    "platform" => {
                        Ok(Value::String(std::env::consts::OS.to_string()))
                    }
                    "is_root" => Ok(Value::Bool(running_as_root())),
                    "is_interactive" => {
                        // Check if stdout is a TTY using stable std::io::IsTerminal (Rust 1.70+)
                        use std::io::IsTerminal;
                        Ok(Value::Bool(std::io::stdout().is_terminal()))
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("env has no method '{}'", method),
                    ))),
                }
            }
            // ── Int methods ──
            Value::Int(n) => match method {
                "to_float" => Ok(Value::Float(*n as f64)),
                "to_string" => Ok(Value::String(n.to_string())),
                "abs" => Ok(Value::Int(n.abs())),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("Int has no method '{}'", method),
                ))),
            },
            // ── Float methods ──
            Value::Float(f) => match method {
                "to_int" => Ok(Value::Int(*f as i64)),
                "to_string" => Ok(Value::String(f.to_string())),
                "abs" => Ok(Value::Float(f.abs())),
                "floor" => Ok(Value::Float(f.floor())),
                "ceil" => Ok(Value::Float(f.ceil())),
                "round" => Ok(Value::Float(f.round())),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("Float has no method '{}'", method),
                ))),
            },
            // ── Bool methods ──
            Value::Bool(b) => match method {
                "to_int" => Ok(Value::Int(if *b { 1 } else { 0 })),
                "to_string" => Ok(Value::String(b.to_string())),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("Bool has no method '{}'", method),
                ))),
            },
            // ── Stream methods ──
            Value::Stream(s) => self.stream_method(s.clone(), method, args),
            // ── DateTime instance methods ──
            Value::Instance { ref type_name, ref fields } if type_name == "DateTime" => {
                self.datetime_method(fields, method, &args)
            }
            // ── Logger instance methods ──
            Value::Instance { ref type_name, ref fields } if type_name == "Logger" => {
                let context = match fields.get("context") {
                    Some(Value::Map(m)) => m.clone(),
                    _ => BTreeMap::new(),
                };
                match method {
                    "debug" => self.log_emit_to_sinks("DEBUG", 0, &context, &args),
                    "info"  => self.log_emit_to_sinks("INFO",  1, &context, &args),
                    "warn"  => self.log_emit_to_sinks("WARN",  2, &context, &args),
                    "error" => self.log_emit_to_sinks("ERROR", 3, &context, &args),
                    "child" | "with" => {
                        let mut merged = context;
                        if let Some(Value::Map(m)) = args.first() {
                            for (k, v) in m {
                                merged.insert(k.clone(), v.clone());
                            }
                        }
                        let mut new_fields = BTreeMap::new();
                        new_fields.insert("context".to_string(), Value::Map(merged));
                        Ok(Value::Instance {
                            type_name: "Logger".to_string(),
                            fields: new_fields,
                        })
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("Logger has no method '{}'", method),
                    ))),
                }
            }
            // ── User-defined enum variant methods ──
            Value::Enum { enum_name, variant, fields } => {
                let enum_name = enum_name.clone();
                let variant = variant.clone();
                let fields = fields.clone();
                match method {
                    "variant" => Ok(Value::String(variant)),
                    "enum_name" => Ok(Value::String(enum_name)),
                    "is_variant" => {
                        let name = match args.first() {
                            Some(Value::String(s)) => s.clone(),
                            _ => return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch,
                                "is_variant() requires a string argument",
                            ))),
                        };
                        Ok(Value::Bool(variant == name))
                    }
                    "fields" => Ok(Value::Map(fields)),
                    _ => {
                        // Delegate to user-defined impl methods for the enum type
                        if let Some(m) = self.find_instance_method(&enum_name, method) {
                            return self.call_method_def(m, Some(obj.clone()), args);
                        }
                        Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            format!("{}.{} has no method '{}'", enum_name, variant, method),
                        )))
                    }
                }
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!(
                    "{} has no method '{}'",
                    obj.type_name(),
                    method
                ),
            ))),
        }
    }

    fn string_method(&self, s: &str, method: &str, args: &[Value]) -> IResult {
        match method {
            "len" => Ok(Value::Int(s.len() as i64)),
            "trim" => Ok(Value::String(s.trim().to_string())),
            "trim_start" => Ok(Value::String(s.trim_start().to_string())),
            "trim_end" => Ok(Value::String(s.trim_end().to_string())),
            "to_upper" => Ok(Value::String(s.to_uppercase())),
            "to_lower" => Ok(Value::String(s.to_lowercase())),
            "starts_with" => {
                let prefix = arg_str(args, 0, "starts_with")?;
                Ok(Value::Bool(s.starts_with(prefix)))
            }
            "ends_with" => {
                let suffix = arg_str(args, 0, "ends_with")?;
                Ok(Value::Bool(s.ends_with(suffix)))
            }
            "contains" => {
                let sub = arg_str(args, 0, "contains")?;
                Ok(Value::Bool(s.contains(sub)))
            }
            // `contains(a) && contains(b) && …` and its `||` twin, which is
            // what a filter over a list of hints spells out by hand.
            // Vacuously true / false on an empty list, matching `all` / `any`.
            "contains_all" => {
                let needles = substring_args(args, "contains_all")?;
                Ok(Value::Bool(needles.iter().all(|n| s.contains(n.as_str()))))
            }
            "contains_any" => {
                let needles = substring_args(args, "contains_any")?;
                Ok(Value::Bool(needles.iter().any(|n| s.contains(n.as_str()))))
            }
            "replace" => {
                let from = arg_str(args, 0, "replace")?;
                let to = arg_str(args, 1, "replace")?;
                Ok(Value::String(s.replace(from, to)))
            }
            "split" => {
                let sep = arg_str(args, 0, "split")?;
                Ok(Value::List(
                    s.split(sep)
                        .map(|p| Value::String(p.to_string()))
                        .collect(),
                ))
            }
            "chars" => Ok(Value::List(
                s.chars()
                    .map(|c| Value::String(c.to_string()))
                    .collect(),
            )),
            "lines" => Ok(Value::List(
                s.lines()
                    .map(|l| Value::String(l.to_string()))
                    .collect(),
            )),
            "repeat" => {
                let n = arg_int(args, 0, "repeat")? as usize;
                Ok(Value::String(s.repeat(n)))
            }
            "is_empty" => Ok(Value::Bool(s.is_empty())),
            "parse_int" => match s.parse::<i64>() {
                Ok(n) => Ok(Value::Ok(Box::new(Value::Int(n)))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
            },
            "parse_float" => match s.parse::<f64>() {
                Ok(f) => Ok(Value::Ok(Box::new(Value::Float(f)))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
            },
            "index_of" => {
                let sub = arg_str(args, 0, "index_of")?;
                match s.find(sub) {
                    Some(idx) => Ok(Value::Int(idx as i64)),
                    None => Ok(Value::Int(-1)),
                }
            }
            "substring" => {
                let start = arg_int(args, 0, "substring")? as usize;
                let end = if args.len() > 1 {
                    arg_int(args, 1, "substring")? as usize
                } else {
                    s.len()
                };
                let start = start.min(s.len());
                let end = end.min(s.len());
                Ok(Value::String(s[start..end].to_string()))
            }
            "pad_start" => {
                let width = arg_int(args, 0, "pad_start")? as usize;
                let fill = if args.len() > 1 {
                    arg_str(args, 1, "pad_start")?
                } else {
                    " "
                };
                if s.len() >= width {
                    Ok(Value::String(s.to_string()))
                } else {
                    let padding = fill.repeat((width - s.len()) / fill.len().max(1) + 1);
                    Ok(Value::String(format!("{}{}", &padding[..width - s.len()], s)))
                }
            }
            "pad_end" => {
                let width = arg_int(args, 0, "pad_end")? as usize;
                let fill = if args.len() > 1 {
                    arg_str(args, 1, "pad_end")?
                } else {
                    " "
                };
                if s.len() >= width {
                    Ok(Value::String(s.to_string()))
                } else {
                    let padding = fill.repeat((width - s.len()) / fill.len().max(1) + 1);
                    Ok(Value::String(format!("{}{}", s, &padding[..width - s.len()])))
                }
            }
            "reverse" => {
                Ok(Value::String(s.chars().rev().collect()))
            }
            "to_path" => Ok(Value::Path(s.to_string())),
            "matches" => {
                let pattern = arg_str(args, 0, "matches")?;
                let matched = simple_regex_test(pattern, s);
                Ok(Value::Bool(matched))
            }
            "bytes" => {
                Ok(Value::List(s.bytes().map(|b| Value::Int(b as i64)).collect()))
            }
            // Parsing a string can fail, so it belongs with the other
            // `parse_*` methods that say so in their return type. These two
            // handed back a bare Int or Float when they worked and an Err
            // when they did not, which no caller could match on.
            "to_int" | "to_float" => {
                let instead = if method == "to_int" { "parse_int" } else { "parse_float" };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`.{}()` was removed; use `.{}()` instead", method, instead),
                )))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("String has no method '{}'", method),
            ))),
        }
    }

    // ── Stream methods ───────────────────────────────────────────────
    //
    // Streams are lazy pipelines. Chainable methods (to_upper, map, grep, …)
    // only push a `StreamOp` onto the queue — no I/O. Terminal methods
    // (collect, lines, write, len, parse_*, …) execute the pipeline, reading
    // the source line-by-line and applying ops on the fly so large files
    // never need to fit in memory.

    fn stream_method(
        &mut self,
        stream: Stream,
        method: &str,
        args: Vec<Value>,
    ) -> IResult {
        match method {
            // ── Chainable / lazy: push an op, no I/O ──────────────────
            "to_upper" => Ok(Value::Stream(stream.pushed(StreamOp::ToUpper))),
            "to_lower" => Ok(Value::Stream(stream.pushed(StreamOp::ToLower))),
            "trim" => Ok(Value::Stream(stream.pushed(StreamOp::Trim))),
            "replace" => {
                let from = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.replace requires 2 string arguments",
                    ))),
                };
                let to = match args.get(1) {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.replace requires 2 string arguments",
                    ))),
                };
                // If the pattern can span lines, fall back to a buffering op.
                let op = if from.contains('\n') {
                    StreamOp::ReplaceBuf(from, to)
                } else {
                    StreamOp::ReplaceLine(from, to)
                };
                Ok(Value::Stream(stream.pushed(op)))
            }
            "prepend" => {
                let prefix = args.first().map(|v| v.display_string()).unwrap_or_default();
                Ok(Value::Stream(stream.pushed(StreamOp::Prepend(prefix))))
            }
            "append" => {
                let suffix = args.first().map(|v| v.display_string()).unwrap_or_default();
                Ok(Value::Stream(stream.pushed(StreamOp::Append(suffix))))
            }
            "map" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.map requires a function argument",
                    ))
                })?.clone();
                Ok(Value::Stream(stream.pushed(StreamOp::Map(func))))
            }
            "filter" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.filter requires a function argument",
                    ))
                })?.clone();
                Ok(Value::Stream(stream.pushed(StreamOp::Filter(func))))
            }
            "grep" => {
                let pattern = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Regex(r)) => r.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.grep requires a string or regex pattern",
                    ))),
                };
                Ok(Value::Stream(stream.pushed(StreamOp::Grep(pattern))))
            }
            "head" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n as usize,
                    None => 10,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "stream.head requires an integer argument",
                    ))),
                };
                Ok(Value::Stream(stream.pushed(StreamOp::Head(n))))
            }
            "tail" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n as usize,
                    None => 10,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "stream.tail requires an integer argument",
                    ))),
                };
                Ok(Value::Stream(stream.pushed(StreamOp::Tail(n))))
            }
            "skip_empty" => Ok(Value::Stream(stream.pushed(StreamOp::SkipEmpty))),
            "reverse_lines" => Ok(Value::Stream(stream.pushed(StreamOp::ReverseLines))),
            "sort_lines" => Ok(Value::Stream(stream.pushed(StreamOp::SortLines))),
            "unique_lines" => Ok(Value::Stream(stream.pushed(StreamOp::UniqueLines))),
            "enumerate_lines" => Ok(Value::Stream(stream.pushed(StreamOp::EnumerateLines))),
            "join_lines" => {
                let sep = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    None => " ".to_string(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "stream.join_lines requires a string separator",
                    ))),
                };
                Ok(Value::Stream(stream.pushed(StreamOp::JoinLines(sep))))
            }

            // ── Terminal: materialize to String ─────────────────────
            "collect" => {
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                Ok(Value::String(buf))
            }
            "lines" => {
                let mut lines: Vec<Value> = Vec::new();
                self.run_stream_pipeline(
                    &stream,
                    &mut StreamOutput::Lines(&mut lines),
                )?;
                Ok(Value::List(lines))
            }
            "split" => {
                let sep = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.split requires a string separator",
                    ))),
                };
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                let parts: Vec<Value> = buf.split(&*sep)
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                Ok(Value::List(parts))
            }

            // ── Terminal: write to a sink (true streaming) ──────────
            "write_to" => {
                let target = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.write requires a path or stream argument",
                    ))
                })?;
                match target {
                    Value::String(_) | Value::Path(_) => {
                        let p = path_arg(target, "stream.write_to")?;
                        match self.stream_to_file(&stream, &p, /*append=*/false) {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Stream(stream)))),
                            Err(Signal::Error(e)) => Ok(Value::Err(Box::new(Value::String(e.message)))),
                            Err(other) => Err(other),
                        }
                    }
                    Value::Stream(sink_stream) => {
                        let sink = sink_stream.get_sink();
                        match self.stream_to_sink(&stream, &sink) {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Stream(stream)))),
                            Err(Signal::Error(e)) => Ok(Value::Err(Box::new(Value::String(e.message)))),
                            Err(other) => Err(other),
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "stream.write requires a path, string, or stream argument",
                    ))),
                }
            }
            "append_to" => {
                let target = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.append_to requires a path or stream argument",
                    ))
                })?;
                match target {
                    Value::String(_) | Value::Path(_) => {
                        let p = path_arg(target, "stream.append_to")?;
                        match self.stream_to_file(&stream, &p, /*append=*/true) {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Stream(stream)))),
                            Err(Signal::Error(e)) => Ok(Value::Err(Box::new(Value::String(e.message)))),
                            Err(other) => Err(other),
                        }
                    }
                    Value::Stream(sink_stream) => {
                        let sink = match sink_stream.get_sink() {
                            StreamSink::File { path, .. } => StreamSink::File { path, append: true },
                            other => other,
                        };
                        match self.stream_to_sink(&stream, &sink) {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Stream(stream)))),
                            Err(Signal::Error(e)) => Ok(Value::Err(Box::new(Value::String(e.message)))),
                            Err(other) => Err(other),
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "stream.append_to requires a path, string, or stream argument",
                    ))),
                }
            }

            // ── Terminal: queries (materialize then measure) ────────
            "len" => {
                let mut counter: usize = 0;
                self.run_stream_pipeline(&stream, &mut StreamOutput::ByteCount(&mut counter))?;
                Ok(Value::Int(counter as i64))
            }
            "count_lines" => {
                let mut counter: usize = 0;
                self.run_stream_pipeline(&stream, &mut StreamOutput::LineCount(&mut counter))?;
                Ok(Value::Int(counter as i64))
            }
            "is_empty" => {
                let mut counter: usize = 0;
                self.run_stream_pipeline(&stream, &mut StreamOutput::ByteCount(&mut counter))?;
                Ok(Value::Bool(counter == 0))
            }
            "contains" => {
                let needle = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "stream.contains requires a string argument",
                    ))),
                };
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                Ok(Value::Bool(buf.contains(&*needle)))
            }

            // ── Terminal: config parsing ────────────────────────────
            "parse_json" => {
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                match crate::config::parse_json(&buf) {
                    Ok(v) => Ok(v),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "parse_yaml" => {
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                match crate::config::parse_yaml(&buf) {
                    Ok(v) => Ok(v),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "parse_toml" => {
                let mut buf = String::new();
                self.run_stream_pipeline(&stream, &mut StreamOutput::Buffer(&mut buf))?;
                match crate::config::parse_toml(&buf) {
                    Ok(v) => Ok(v),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }

            // Second spellings of the arms above, kept only to say so.
            // `.append()` is not among them: on a Stream it appends text to
            // the content, which is a different thing from `.append_to(path)`.
            "to_string" | "write" | "bytes" => {
                let instead = match method {
                    "to_string" => "collect",
                    "write" => "write_to",
                    _ => "len",
                };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`.{}()` was removed; use `.{}()` instead", method, instead),
                )))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Stream has no method '{}'", method),
            ))),
        }
    }

    /// Write a stream's output to a file, line-by-line through a `BufWriter`.
    fn stream_to_file(&mut self, stream: &Stream, path: &str, append: bool) -> Result<(), Signal> {
        // `stream.write(p)` and `stream.append_to(p)` both land here, as does
        // a `StreamSink::File`. It is the only place a stream reaches the
        // filesystem, so it is the only place that needs the check.
        self.check_permission(crate::permissions::Capability::Write, path)?;
        if self.dry_run {
            // Drain the pipeline first so the announced size is the real one
            // and any reads upstream still happen -- a dry run is meant to
            // show what would be written, not guess at it.
            let mut bytes = 0usize;
            self.run_stream_pipeline(stream, &mut StreamOutput::ByteCount(&mut bytes))?;
            let verb = if append { "append to" } else { "write" };
            self.dry_run_skip(format!("{} {} ({} bytes)", verb, path, bytes));
            return Ok(());
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(!append)
            .append(append)
            .truncate(!append)
            .open(path)
            .map_err(|e| Signal::Error(QueError::new(
                ErrorKind::IoError,
                format!("stream: cannot open '{}': {}", path, e),
            )))?;
        let mut w: Box<dyn Write> = Box::new(std::io::BufWriter::new(file));
        self.run_stream_pipeline(stream, &mut StreamOutput::Writer(&mut w))?;
        w.flush().map_err(|e| Signal::Error(QueError::new(
            ErrorKind::IoError, format!("stream: write failed: {}", e),
        )))
    }

    /// Write a stream's output to a `StreamSink`. Stdout/Stderr/File sinks
    /// stream directly; FileHandle sinks go through the wrapped writer.
    /// `StreamSink::None` materializes and discards (no-op write).
    fn stream_to_sink(&mut self, stream: &Stream, sink: &StreamSink) -> Result<(), Signal> {
        match sink {
            StreamSink::None => {
                // Drain the pipeline (in case of side effects) but discard output.
                let mut sink_buf = String::new();
                self.run_stream_pipeline(stream, &mut StreamOutput::Buffer(&mut sink_buf))
            }
            StreamSink::Stdout => {
                let mut w: Box<dyn Write> = Box::new(std::io::stdout().lock());
                self.run_stream_pipeline(stream, &mut StreamOutput::Writer(&mut w))?;
                w.flush().ok();
                Ok(())
            }
            StreamSink::Stderr => {
                let mut w: Box<dyn Write> = Box::new(std::io::stderr().lock());
                self.run_stream_pipeline(stream, &mut StreamOutput::Writer(&mut w))?;
                w.flush().ok();
                Ok(())
            }
            StreamSink::File { path, append } => self.stream_to_file(stream, path, *append),
            StreamSink::FileHandle(fh) => {
                // A handle opened during a dry run discards silently, so the
                // announcement has to happen here -- the writer itself has no
                // way back to the interpreter.
                let discarding = fh.inner.lock().map(|i| i.discard).unwrap_or(false);
                if discarding {
                    let path = fh.inner.lock().map(|i| i.path.clone()).unwrap_or_default();
                    let mut bytes = 0usize;
                    self.run_stream_pipeline(stream, &mut StreamOutput::ByteCount(&mut bytes))?;
                    self.dry_run_skip(format!("write {} ({} bytes)", path, bytes));
                    return Ok(());
                }
                let mut w: Box<dyn Write> = Box::new(FileHandleWriter(Arc::clone(&fh.inner)));
                self.run_stream_pipeline(stream, &mut StreamOutput::Writer(&mut w))?;
                w.flush().ok();
                Ok(())
            }
        }
    }

    /// Execute a stream's pipeline, emitting transformed lines into `output`.
    ///
    /// Algorithm: split the op list at every buffering op. Each segment is
    /// processed line-by-line. When a segment ends in a buffering op, its
    /// output is collected into a `String`, the buffering op is applied, and
    /// the result becomes the source for the next segment. The final segment
    /// streams directly into `output`.
    fn run_stream_pipeline(
        &mut self,
        stream: &Stream,
        output: &mut StreamOutput<'_>,
    ) -> Result<(), Signal> {
        let (source, ops) = {
            let inner = stream.inner.lock().unwrap();
            (inner.source.clone(), inner.ops.clone())
        };

        // Group ops into segments: each segment is (streaming_ops, optional buffering_op).
        // We always end with a streaming-only segment (possibly empty) that drives
        // the final emit into `output`.
        let mut segments: Vec<(Vec<StreamOp>, Option<StreamOp>)> = Vec::new();
        let mut current: Vec<StreamOp> = Vec::new();
        for op in ops {
            if op.is_buffering() {
                segments.push((std::mem::take(&mut current), Some(op)));
            } else {
                current.push(op);
            }
        }
        // Always push a final streaming-only segment so the pipeline terminates
        // in `output` (even when the last user-visible op was a buffering one).
        segments.push((current, None));

        let total = segments.len();
        let mut current_source = source;

        for (idx, (streaming, buffering)) in segments.into_iter().enumerate() {
            let is_final_segment = idx + 1 == total;
            debug_assert!(!is_final_segment || buffering.is_none());
            if is_final_segment {
                // Stream directly into the caller-provided output.
                self.process_streaming_segment(&current_source, &streaming, output)?;
            } else {
                // Collect this segment into a String, then apply the buffering op.
                let mut buf = String::new();
                {
                    let mut seg_out = StreamOutput::Buffer(&mut buf);
                    self.process_streaming_segment(&current_source, &streaming, &mut seg_out)?;
                }
                if let Some(bop) = buffering {
                    buf = apply_buffer_op_value(&bop, buf)
                        .map_err(|e| Signal::Error(QueError::new(ErrorKind::Runtime, e)))?;
                }
                current_source = StreamSource::Buffer(buf);
            }
        }
        Ok(())
    }

    /// Process one streaming segment: open `source`, iterate lines, apply
    /// each per-line op in order, and emit surviving lines to `output`.
    fn process_streaming_segment(
        &mut self,
        source: &StreamSource,
        ops: &[StreamOp],
        output: &mut StreamOutput<'_>,
    ) -> Result<(), Signal> {
        // Per-segment state for stateful ops.
        let mut head_remaining: Vec<Option<usize>> = ops
            .iter()
            .map(|o| if let StreamOp::Head(n) = o { Some(*n) } else { None })
            .collect();
        let mut enum_counters: Vec<usize> = ops.iter().map(|_| 0usize).collect();
        let mut unique_seen: Vec<std::collections::HashSet<String>> =
            ops.iter().map(|_| std::collections::HashSet::new()).collect();

        let mut emitted_any = false;
        let mut iter = open_source_lines(source)?;
        'outer: while let Some(line_res) = iter.next() {
            let mut line = line_res.map_err(|e| Signal::Error(QueError::new(
                ErrorKind::IoError, format!("stream: read error: {}", e),
            )))?;
            // Apply ops in order.
            for (i, op) in ops.iter().enumerate() {
                match op {
                    StreamOp::ToUpper => line = line.to_uppercase(),
                    StreamOp::ToLower => line = line.to_lowercase(),
                    StreamOp::ReplaceLine(from, to) => line = line.replace(from, to),
                    StreamOp::Grep(pat) => {
                        let m = if let Ok(re) = regex_lite::Regex::new(pat) {
                            re.is_match(&line)
                        } else {
                            line.contains(pat.as_str())
                        };
                        if !m { continue 'outer; }
                    }
                    StreamOp::Map(func) => {
                        let v = self.call_value(func.clone(), vec![Value::String(line.clone())])?;
                        line = v.display_string();
                    }
                    StreamOp::Filter(func) => {
                        let v = self.call_value(func.clone(), vec![Value::String(line.clone())])?;
                        if !v.is_truthy() { continue 'outer; }
                    }
                    StreamOp::SkipEmpty => {
                        if line.trim().is_empty() { continue 'outer; }
                    }
                    StreamOp::Head(_) => {
                        match &mut head_remaining[i] {
                            Some(0) => break 'outer,
                            Some(n) => *n -= 1,
                            None => {}
                        }
                    }
                    StreamOp::EnumerateLines => {
                        enum_counters[i] += 1;
                        line = format!("{}\t{}", enum_counters[i], line);
                    }
                    StreamOp::UniqueLines => {
                        if !unique_seen[i].insert(line.clone()) { continue 'outer; }
                    }
                    // Buffering ops never appear inside a streaming segment.
                    _ => {}
                }
            }
            if emitted_any {
                output.emit_separator()?;
            }
            output.emit_line(&line)?;
            emitted_any = true;
        }
        Ok(())
    }

    fn list_method(
        &mut self,
        items: &[Value],
        method: &str,
        args: Vec<Value>,
    ) -> IResult {
        match method {
            "len" => Ok(Value::Int(items.len() as i64)),
            "is_empty" => Ok(Value::Bool(items.is_empty())),
            "first" => Ok(items.first().cloned().unwrap_or(Value::Null)),
            "last" => Ok(items.last().cloned().unwrap_or(Value::Null)),
            // Lenient counterpart to `list[i]`, which raises when out of bounds.
            // Mirrors `map.get(key)` so both collections offer the same choice.
            "get" => {
                let idx = match args.first() {
                    Some(Value::Int(i)) => *i,
                    _ => {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            "get requires an integer index",
                        )))
                    }
                };
                let default = args.get(1).cloned().unwrap_or(Value::Null);
                let resolved = if idx < 0 { items.len() as i64 + idx } else { idx };
                if resolved < 0 {
                    return Ok(default);
                }
                Ok(items.get(resolved as usize).cloned().unwrap_or(default))
            }
            "reverse" => {
                let mut result = items.to_vec();
                result.reverse();
                Ok(Value::List(result))
            }
            "sort" => {
                let mut result = items.to_vec();
                result.sort_by(|a, b| value_cmp(a, b));
                Ok(Value::List(result))
            }
            "contains" => {
                let item = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "contains requires 1 argument",
                    ))
                })?;
                Ok(Value::Bool(items.contains(item)))
            }
            "push" => {
                let val = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "push requires 1 argument",
                    ))
                })?;
                let mut result = items.to_vec();
                result.push(val);
                Ok(Value::List(result))
            }
            "pop" => {
                let mut result = items.to_vec();
                let popped = result.pop().unwrap_or(Value::Null);
                Ok(Value::Tuple(vec![Value::List(result), popped]))
            }
            "join" => {
                let sep = args
                    .first()
                    .and_then(|v| {
                        if let Value::String(s) = v {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .unwrap_or("");
                let parts: Vec<String> =
                    items.iter().map(|v| v.display_string()).collect();
                Ok(Value::String(parts.join(sep)))
            }
            "map" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "map requires a function argument",
                    ))
                })?;
                let mut result = Vec::new();
                for item in items {
                    result.push(self.call_value(func.clone(), vec![item.clone()])?);
                }
                Ok(Value::List(result))
            }
            "filter" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "filter requires a function argument",
                    ))
                })?;
                let mut result = Vec::new();
                for item in items {
                    let keep = self.call_value(func.clone(), vec![item.clone()])?;
                    if keep.is_truthy() {
                        result.push(item.clone());
                    }
                }
                Ok(Value::List(result))
            }
            "fold" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fold requires 2 arguments (init, fn)",
                    )));
                }
                let mut acc = args[0].clone();
                let func = args[1].clone();
                for item in items {
                    acc = self.call_value(func.clone(), vec![acc, item.clone()])?;
                }
                Ok(acc)
            }
            "enumerate" => Ok(Value::List(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        Value::Tuple(vec![Value::Int(i as i64), v.clone()])
                    })
                    .collect(),
            )),
            "flat_map" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "flat_map requires a function argument",
                    ))
                })?;
                let mut result = Vec::new();
                for item in items {
                    let mapped = self.call_value(func.clone(), vec![item.clone()])?;
                    if let Value::List(inner) = mapped {
                        result.extend(inner);
                    } else {
                        result.push(mapped);
                    }
                }
                Ok(Value::List(result))
            }
            "find" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "find requires a function argument",
                    ))
                })?;
                for item in items {
                    let matches = self.call_value(func.clone(), vec![item.clone()])?;
                    if matches.is_truthy() {
                        return Ok(item.clone());
                    }
                }
                Ok(Value::Null)
            }
            "any" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "any requires a function argument",
                    ))
                })?;
                for item in items {
                    let matches = self.call_value(func.clone(), vec![item.clone()])?;
                    if matches.is_truthy() {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            "all" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "all requires a function argument",
                    ))
                })?;
                for item in items {
                    let matches = self.call_value(func.clone(), vec![item.clone()])?;
                    if !matches.is_truthy() {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            "take" => {
                let n = args.first().and_then(|v| if let Value::Int(n) = v { Some(*n as usize) } else { None })
                    .ok_or_else(|| Signal::Error(QueError::new(ErrorKind::ArityMismatch, "take requires an integer argument")))?;
                Ok(Value::List(items.iter().take(n).cloned().collect()))
            }
            "skip" => {
                let n = args.first().and_then(|v| if let Value::Int(n) = v { Some(*n as usize) } else { None })
                    .ok_or_else(|| Signal::Error(QueError::new(ErrorKind::ArityMismatch, "skip requires an integer argument")))?;
                Ok(Value::List(items.iter().skip(n).cloned().collect()))
            }
            "chunk" => {
                let n = args.first().and_then(|v| if let Value::Int(n) = v { Some(*n as usize) } else { None })
                    .ok_or_else(|| Signal::Error(QueError::new(ErrorKind::ArityMismatch, "chunk requires an integer argument")))?;
                if n == 0 {
                    return Err(Signal::Error(QueError::new(ErrorKind::Runtime, "chunk size must be > 0")));
                }
                let chunks: Vec<Value> = items.chunks(n)
                    .map(|c| Value::List(c.to_vec()))
                    .collect();
                Ok(Value::List(chunks))
            }
            "zip" => {
                let other = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "zip requires a list argument"))
                })?;
                if let Value::List(other_items) = other {
                    let zipped: Vec<Value> = items.iter()
                        .zip(other_items.iter())
                        .map(|(a, b)| Value::Tuple(vec![a.clone(), b.clone()]))
                        .collect();
                    Ok(Value::List(zipped))
                } else {
                    Err(Signal::Error(QueError::new(ErrorKind::TypeMismatch, "zip requires a list argument")))
                }
            }
            "partition" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "partition requires a function argument"))
                })?;
                let mut matching = Vec::new();
                let mut not_matching = Vec::new();
                for item in items {
                    let result = self.call_value(func.clone(), vec![item.clone()])?;
                    if result.is_truthy() {
                        matching.push(item.clone());
                    } else {
                        not_matching.push(item.clone());
                    }
                }
                Ok(Value::Tuple(vec![Value::List(matching), Value::List(not_matching)]))
            }
            "group_by" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "group_by requires a function argument"))
                })?;
                let mut groups: BTreeMap<String, Value> = BTreeMap::new();
                for item in items {
                    let key = self.call_value(func.clone(), vec![item.clone()])?;
                    let key_str = key.display_string();
                    if let Some(Value::List(ref mut list)) = groups.get_mut(&key_str) {
                        list.push(item.clone());
                    } else {
                        groups.insert(key_str, Value::List(vec![item.clone()]));
                    }
                }
                Ok(Value::Map(groups))
            }
            "sort_by" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "sort_by requires a function argument"))
                })?;
                let mut keyed: Vec<(Value, Value)> = Vec::new();
                for item in items {
                    let key = self.call_value(func.clone(), vec![item.clone()])?;
                    keyed.push((key, item.clone()));
                }
                keyed.sort_by(|(a, _), (b, _)| value_cmp(a, b));
                Ok(Value::List(keyed.into_iter().map(|(_, v)| v).collect()))
            }
            "each" => {
                let func = args.into_iter().next().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "each requires a function argument"))
                })?;
                for item in items {
                    self.call_value(func.clone(), vec![item.clone()])?;
                }
                Ok(Value::Null)
            }
            "unique" => {
                let mut seen = Vec::new();
                let mut result = Vec::new();
                for item in items {
                    if !seen.contains(item) {
                        seen.push(item.clone());
                        result.push(item.clone());
                    }
                }
                Ok(Value::List(result))
            }
            "flatten" => {
                let mut result = Vec::new();
                for item in items {
                    if let Value::List(inner) = item {
                        result.extend(inner.iter().cloned());
                    } else {
                        result.push(item.clone());
                    }
                }
                Ok(Value::List(result))
            }
            "index_of" => {
                let target = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "index_of requires 1 argument"))
                })?;
                let idx = items.iter().position(|v| v == target);
                Ok(idx.map(|i| Value::Int(i as i64)).unwrap_or(Value::Int(-1)))
            }
            "slice" => {
                let start = arg_int(&args, 0, "slice")? as usize;
                let end = if args.len() > 1 {
                    arg_int(&args, 1, "slice")? as usize
                } else {
                    items.len()
                };
                let start = start.min(items.len());
                let end = end.min(items.len());
                Ok(Value::List(items[start..end].to_vec()))
            }
            "window" => {
                let n = arg_int(&args, 0, "window")? as usize;
                if n == 0 || n > items.len() {
                    return Ok(Value::List(Vec::new()));
                }
                let result: Vec<Value> = items
                    .windows(n)
                    .map(|w| Value::List(w.to_vec()))
                    .collect();
                Ok(Value::List(result))
            }
            "to_tuple" => Ok(Value::Tuple(items.to_vec())),
            "to_set" => {
                let mut seen = Vec::new();
                for item in items {
                    if !self.set_contains(&seen, item)? {
                        seen.push(item.clone());
                    }
                }
                Ok(Value::Set(seen))
            }
            "to_map" => {
                let mut map = BTreeMap::new();
                for item in items {
                    match item {
                        Value::Tuple(pair) if pair.len() == 2 => {
                            let key = pair[0].display_string();
                            map.insert(key, pair[1].clone());
                        }
                        Value::List(pair) if pair.len() == 2 => {
                            let key = pair[0].display_string();
                            map.insert(key, pair[1].clone());
                        }
                        _ => return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            "to_map requires a list of key-value pairs (2-element tuples or lists)",
                        ))),
                    }
                }
                Ok(Value::Map(map))
            }
            "for_each" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`.for_each()` was removed; use `.each()` instead",
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("List has no method '{}'", method),
            ))),
        }
    }

    fn set_method(
        &mut self,
        items: &[Value],
        method: &str,
        args: Vec<Value>,
    ) -> IResult {
        match method {
            "len" => Ok(Value::Int(items.len() as i64)),
            "is_empty" => Ok(Value::Bool(items.is_empty())),
            "contains" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "contains requires 1 argument"))
                })?;
                Ok(Value::Bool(self.set_contains(items, val)?))
            }
            "add" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "add requires 1 argument"))
                })?.clone();
                let mut new_items: Vec<Value> = items.to_vec();
                if !self.set_contains(&new_items, &val)? {
                    new_items.push(val);
                }
                Ok(Value::Set(new_items))
            }
            "remove" => {
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "remove requires 1 argument"))
                })?.clone();
                let mut new_items = Vec::new();
                for item in items {
                    if !self.interpreter_eq(item, &val)? {
                        new_items.push(item.clone());
                    }
                }
                Ok(Value::Set(new_items))
            }
            "union" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "union requires a set or list argument",
                    ))),
                };
                let mut result = items.to_vec();
                for val in other {
                    if !self.set_contains(&result, &val)? {
                        result.push(val);
                    }
                }
                Ok(Value::Set(result))
            }
            "intersection" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "intersection requires a set or list argument",
                    ))),
                };
                let mut result = Vec::new();
                for item in items {
                    if self.set_contains(&other, item)? {
                        result.push(item.clone());
                    }
                }
                Ok(Value::Set(result))
            }
            "difference" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "difference requires a set or list argument",
                    ))),
                };
                let mut result = Vec::new();
                for item in items {
                    if !self.set_contains(&other, item)? {
                        result.push(item.clone());
                    }
                }
                Ok(Value::Set(result))
            }
            "symmetric_difference" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "symmetric_difference requires a set or list argument",
                    ))),
                };
                let mut result = Vec::new();
                for item in items {
                    if !self.set_contains(&other, item)? {
                        result.push(item.clone());
                    }
                }
                for val in &other {
                    if !self.set_contains(items, val)? && !self.set_contains(&result, val)? {
                        result.push(val.clone());
                    }
                }
                Ok(Value::Set(result))
            }
            "is_subset" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "is_subset requires a set or list argument",
                    ))),
                };
                let mut all_in = true;
                for item in items {
                    if !self.set_contains(&other, item)? {
                        all_in = false;
                        break;
                    }
                }
                Ok(Value::Bool(all_in))
            }
            "is_superset" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "is_superset requires a set or list argument",
                    ))),
                };
                let mut all_in = true;
                for item in &other {
                    if !self.set_contains(items, item)? {
                        all_in = false;
                        break;
                    }
                }
                Ok(Value::Bool(all_in))
            }
            "is_disjoint" => {
                let other = match args.first() {
                    Some(Value::Set(s)) => s.clone(),
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "is_disjoint requires a set or list argument",
                    ))),
                };
                let mut any_common = false;
                for item in items {
                    if self.set_contains(&other, item)? {
                        any_common = true;
                        break;
                    }
                }
                Ok(Value::Bool(!any_common))
            }
            "to_list" => Ok(Value::List(items.to_vec())),
            "to_tuple" => Ok(Value::Tuple(items.to_vec())),
            "map" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "map requires a function argument"))
                })?.clone();
                let mut result = Vec::new();
                for item in items {
                    let val = self.call_value(func.clone(), vec![item.clone()])?;
                    if !self.set_contains(&result, &val)? {
                        result.push(val);
                    }
                }
                Ok(Value::Set(result))
            }
            "filter" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "filter requires a function argument"))
                })?.clone();
                let mut result = Vec::new();
                for item in items {
                    let keep = self.call_value(func.clone(), vec![item.clone()])?;
                    if keep.is_truthy() {
                        result.push(item.clone());
                    }
                }
                Ok(Value::Set(result))
            }
            "each" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "each requires a function argument"))
                })?.clone();
                for item in items {
                    self.call_value(func.clone(), vec![item.clone()])?;
                }
                Ok(Value::Null)
            }
            "for_each" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`.for_each()` was removed; use `.each()` instead",
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Set has no method '{}'", method),
            ))),
        }
    }

    fn map_method(
        &mut self,
        map: &BTreeMap<String, Value>,
        method: &str,
        args: &[Value],
    ) -> IResult {
        match method {
            "len" => Ok(Value::Int(map.len() as i64)),
            "is_empty" => Ok(Value::Bool(map.is_empty())),
            "keys" => Ok(Value::List(
                map.keys().map(|k| Value::String(k.clone())).collect(),
            )),
            "values" => Ok(Value::List(map.values().cloned().collect())),
            "entries" => Ok(Value::List(
                map.iter()
                    .map(|(k, v)| {
                        Value::Tuple(vec![Value::String(k.clone()), v.clone()])
                    })
                    .collect(),
            )),
            "contains" => {
                let key = arg_str(args, 0, "contains")?;
                Ok(Value::Bool(map.contains_key(key)))
            }
            "get" => {
                let key = arg_str(args, 0, "get")?;
                let default = args.get(1).cloned().unwrap_or(Value::Null);
                Ok(map.get(key).cloned().unwrap_or(default))
            }
            "merge" => {
                let other = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "merge requires 1 argument",
                    ))
                })?;
                if let Value::Map(other_map) = other {
                    let mut result = map.clone();
                    for (k, v) in other_map {
                        result.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Map(result))
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "merge requires a map argument",
                    )))
                }
            }
            "remove" => {
                let key = arg_str(args, 0, "remove")?;
                let mut result = map.clone();
                result.remove(key);
                Ok(Value::Map(result))
            }
            "map_values" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "map_values requires a function argument"))
                })?;
                let mut result = BTreeMap::new();
                for (k, v) in map {
                    let new_v = self.call_value(func.clone(), vec![v.clone()])?;
                    result.insert(k.clone(), new_v);
                }
                Ok(Value::Map(result))
            }
            "filter_values" => {
                let func = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "filter_values requires a function argument"))
                })?;
                let mut result = BTreeMap::new();
                for (k, v) in map {
                    let keep = self.call_value(func.clone(), vec![v.clone()])?;
                    if keep.is_truthy() {
                        result.insert(k.clone(), v.clone());
                    }
                }
                Ok(Value::Map(result))
            }
            "deep_merge" => {
                let other = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "deep_merge requires 1 argument"))
                })?;
                if let Value::Map(other_map) = other {
                    fn deep_merge(base: &BTreeMap<String, Value>, overlay: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
                        let mut result = base.clone();
                        for (k, v) in overlay {
                            if let (Some(Value::Map(base_inner)), Value::Map(overlay_inner)) = (result.get(k), v) {
                                result.insert(k.clone(), Value::Map(deep_merge(base_inner, overlay_inner)));
                            } else {
                                result.insert(k.clone(), v.clone());
                            }
                        }
                        result
                    }
                    Ok(Value::Map(deep_merge(map, other_map)))
                } else {
                    Err(Signal::Error(QueError::new(ErrorKind::TypeMismatch, "deep_merge requires a map argument")))
                }
            }

            "to_list" => Ok(Value::List(
                map.iter()
                    .map(|(k, v)| {
                        Value::Tuple(vec![Value::String(k.clone()), v.clone()])
                    })
                    .collect(),
            )),

            // ── Config path operations ──
            "get_path" => {
                let path = arg_str(args, 0, "get_path")?;
                Ok(crate::config::config_get(&Value::Map(map.clone()), path))
            }
            "set_path" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "set_path requires 2 arguments (path, value)",
                    )));
                }
                let path = arg_str(args, 0, "set_path")?;
                let new_val = args[1].clone();
                Ok(crate::config::config_set(&Value::Map(map.clone()), path, new_val))
            }
            "delete_path" => {
                let path = arg_str(args, 0, "delete_path")?;
                Ok(crate::config::config_delete(&Value::Map(map.clone()), path))
            }
            "has_path" => {
                let path = arg_str(args, 0, "has_path")?;
                Ok(Value::Bool(crate::config::config_has(&Value::Map(map.clone()), path)))
            }
            "paths" => {
                let paths = crate::config::config_paths(&Value::Map(map.clone()));
                Ok(Value::List(paths.into_iter().map(Value::String).collect()))
            }

            // ── Config serialization methods ──
            "to_json" => {
                let indent = args.first().and_then(|v| {
                    if let Value::Int(n) = v { Some(*n as usize) } else { None }
                });
                match crate::config::to_json(&Value::Map(map.clone()), indent) {
                    Ok(s) => Ok(Value::String(s)),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
                }
            }
            "to_yaml" => {
                match crate::config::to_yaml(&Value::Map(map.clone())) {
                    Ok(s) => Ok(Value::String(s)),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
                }
            }
            "to_toml" => {
                match crate::config::to_toml(&Value::Map(map.clone())) {
                    Ok(s) => Ok(Value::String(s)),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
                }
            }
            // Convenience method for HTTP responses: parse the body field as JSON
            "json" => {
                let body = match map.get("body") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.display_string(),
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "json() called on a map without a 'body' field",
                    ))),
                };
                match crate::config::parse_json(&body) {
                    Ok(v) => Ok(v),
                    Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
                }
            }
            // Convenience method for HTTP responses: check ok field
            "ok" => {
                Ok(map.get("ok").cloned().unwrap_or(Value::Bool(false)))
            }

            _ => {
                // Not a built-in map method. Check if the map has a
                // callable value at this key (module-as-map pattern).
                if let Some(func_val) = map.get(method) {
                    match func_val {
                        Value::Function { .. } | Value::BuiltinFn(_) => {
                            self.call_value(func_val.clone(), args.to_vec())
                        }
                        _ => Err(Signal::Error(QueError::new(
                            ErrorKind::NotCallable,
                            format!("map key '{}' is not callable (got {})", method, func_val.type_name()),
                        ))),
                    }
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("Map has no method or key '{}'", method),
                    )))
                }
            }
        }
    }

    fn path_method(&mut self, p: &str, method: &str, _args: &[Value]) -> IResult {
        let path = std::path::Path::new(p);
        // Path methods are the other half of the filesystem surface (std.fs
        // is the first). Checking at dispatch keeps both halves enforced from
        // one table instead of from every implementation.
        if self.permissions.is_some() {
            if let Some(cap) = crate::permissions::path_effect(method) {
                // Two-path methods change the *argument*, so that is what
                // gets checked; `a.copy_to(b)` must not be authorised by
                // write access to `a`.
                let subject = match (cap, _args.first()) {
                    (crate::permissions::Capability::Write, Some(dest))
                        if matches!(
                            method,
                            "copy" | "copy_to" | "move_to" | "rename" | "rename_to"
                        ) =>
                    {
                        // The write lands where the copy actually goes, so
                        // resolve `into a directory` before asking.
                        path_arg(dest, method)
                            .map(|d| resolve_into_dir(path, d))
                            .unwrap_or_else(|_| dest.display_string())
                    }
                    _ => p.to_string(),
                };
                self.check_permission(cap, &subject)?;
            }
        }
        match method {
            "name" => Ok(path
                .file_name()
                .map(|n| Value::String(n.to_string_lossy().to_string()))
                .unwrap_or(Value::Null)),
            "parent" => {
                // Edge cases per spec: p"/" -> p"/", p"" -> p""
                if p == "/" || p.is_empty() {
                    Ok(Value::Path(p.to_string()))
                } else {
                    Ok(path
                        .parent()
                        .map(|n| {
                            let s = n.to_string_lossy().to_string();
                            // parent() of a root-relative single component returns ""
                            // which is correct (empty path = identity for /)
                            Value::Path(s)
                        })
                        .unwrap_or_else(|| Value::Path(p.to_string())))
                }
            }
            "extension" => Ok(path
                .extension()
                .map(|e| Value::String(e.to_string_lossy().to_string()))
                .unwrap_or(Value::Null)),
            // ext_with_dot returns the extension with leading dot, as specified
            "ext_dot" => Ok(path
                .extension()
                .map(|e| Value::String(format!(".{}", e.to_string_lossy())))
                .unwrap_or(Value::String(String::new()))),
            "stem" => Ok(path
                .file_stem()
                .map(|s| Value::String(s.to_string_lossy().to_string()))
                .unwrap_or(Value::Null)),
            "exists" => Ok(Value::Bool(path.exists())),
            "is_file" => Ok(Value::Bool(path.is_file())),
            "is_dir" => Ok(Value::Bool(path.is_dir())),
            "is_absolute" => Ok(Value::Bool(path.is_absolute())),
            "is_relative" => Ok(Value::Bool(!path.is_absolute())),
            "is_link" => {
                let is_symlink = std::fs::symlink_metadata(p)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false);
                Ok(Value::Bool(is_symlink))
            }
            "root" => {
                if path.is_absolute() {
                    Ok(Value::Path("/".to_string()))
                } else {
                    Ok(Value::Path(String::new()))
                }
            }
            "resolve" => {
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::env::current_dir()
                        .unwrap_or_default()
                        .join(path)
                };
                // Normalize: remove . and ..
                let mut components = Vec::new();
                for comp in absolute.components() {
                    match comp {
                        std::path::Component::CurDir => {}
                        std::path::Component::ParentDir => { components.pop(); }
                        _ => components.push(comp),
                    }
                }
                let result: std::path::PathBuf = components.iter().collect();
                Ok(Value::Path(result.to_string_lossy().to_string()))
            }
            "resolve_or" => {
                // Resolve to absolute path; if path is empty, return the fallback instead.
                let fallback = _args.first().cloned().unwrap_or(Value::Null);
                if p.is_empty() {
                    return Ok(fallback);
                }
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::env::current_dir().unwrap_or_default().join(path)
                };
                let mut components = Vec::new();
                for comp in absolute.components() {
                    match comp {
                        std::path::Component::CurDir => {}
                        std::path::Component::ParentDir => { components.pop(); }
                        _ => components.push(comp),
                    }
                }
                let result: std::path::PathBuf = components.iter().collect();
                Ok(Value::Path(result.to_string_lossy().to_string()))
            }
            "normalize" => {
                // Simple normalization: remove . and ..
                let mut components = Vec::new();
                for comp in path.components() {
                    match comp {
                        std::path::Component::CurDir => {}
                        std::path::Component::ParentDir => { components.pop(); }
                        _ => components.push(comp),
                    }
                }
                let result: std::path::PathBuf = components.iter().collect();
                Ok(Value::Path(result.to_string_lossy().to_string()))
            }
            "with_ext" => {
                let ext = arg_str(_args, 0, "with_ext")?;
                // Strip leading dot if provided (e.g. ".bak" → "bak")
                let ext = ext.trim_start_matches('.');
                let new_path = path.with_extension(ext);
                Ok(Value::Path(new_path.to_string_lossy().to_string()))
            }
            "with_name" => {
                let name = arg_str(_args, 0, "with_name")?;
                let new_path = path.with_file_name(name);
                Ok(Value::Path(new_path.to_string_lossy().to_string()))
            }
            "with_stem" => {
                let new_stem = arg_str(_args, 0, "with_stem")?;
                let new_name = if let Some(ext) = path.extension() {
                    format!("{}.{}", new_stem, ext.to_string_lossy())
                } else {
                    new_stem.to_string()
                };
                let new_path = path.with_file_name(new_name);
                Ok(Value::Path(new_path.to_string_lossy().to_string()))
            }
            "size" => {
                match std::fs::metadata(p) {
                    Ok(meta) => Ok(Value::Int(meta.len() as i64)),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "to_string" => Ok(Value::String(p.to_string())),
            "read" => match std::fs::read_to_string(p) {
                Ok(content) => Ok(Value::Ok(Box::new(Value::String(content)))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
            },
            "write_text" => {
                let content = arg_str(_args, 0, "write_text")?;
                if self.dry_run_skip(format!("write {} ({} bytes)", p, content.len())) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match std::fs::write(p, content) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "append_text" => {
                let content = arg_str(_args, 0, "append_text")?;
                if self.dry_run_skip(format!("append to {} ({} bytes)", p, content.len())) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                use std::io::Write;
                match std::fs::OpenOptions::new().create(true).append(true).open(p) {
                    Ok(mut f) => match f.write_all(content.as_bytes()) {
                        Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                        Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                    },
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "mkdir" => {
                // `clean` is "make this directory be exactly what I am about
                // to put in it": the `if exists { delete } / mkdir` pair that
                // every idempotent setup task writes out by hand.
                let clean = bool_opt(_args.first(), "clean", "mkdir")?;
                let action = if clean {
                    format!("mkdir {} (clean)", p)
                } else {
                    format!("mkdir {}", p)
                };
                if self.dry_run_skip(action) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                if clean {
                    if let Err(e) = remove_path(p, true) {
                        return Ok(Value::Err(Box::new(Value::String(e.to_string()))));
                    }
                }
                match std::fs::create_dir_all(p) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "delete" => {
                // Without `missing_ok` an absent path is still an error: a
                // delete that quietly does nothing is how a script deletes the
                // wrong thing for a week before anyone notices.
                let missing_ok = bool_opt(_args.first(), "missing_ok", "delete")?;
                if self.dry_run_skip(format!("remove {}", p)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match remove_path(p, missing_ok) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "symlink" => {
                let target = arg_path_str(_args, 0, "symlink")?;
                if self.dry_run_skip(format!("symlink {} -> {}", p, target)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                #[cfg(unix)]
                {
                    match std::os::unix::fs::symlink(&target, p) {
                        Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                        Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                    }
                }
                #[cfg(not(unix))]
                {
                    // Windows has symlinks, but creating one needs either
                    // Developer Mode or SeCreateSymbolicLinkPrivilege, and it
                    // needs to know up front whether the target is a
                    // directory. Both are worth surfacing rather than
                    // silently degrading to a copy, which would break the
                    // aliasing the script asked for.
                    #[cfg(windows)]
                    {
                        let result = if std::path::Path::new(&target).is_dir() {
                            std::os::windows::fs::symlink_dir(&target, p)
                        } else {
                            std::os::windows::fs::symlink_file(&target, p)
                        };
                        match result {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                            Err(e) => Ok(Value::Err(Box::new(Value::String(format!(
                                "{} (on Windows, creating a symlink requires Developer Mode \
                                 or the SeCreateSymbolicLinkPrivilege)",
                                e
                            ))))),
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = target;
                        Ok(Value::Err(Box::new(Value::String(
                            "symlink() is not supported on this platform".to_string(),
                        ))))
                    }
                }
            }
            "copy_to" => {
                let dest = resolve_into_dir(path, arg_path_str(_args, 0, "copy_to")?);
                if dest == p {
                    return Ok(Value::Err(Box::new(Value::String(format!(
                        "cannot copy '{}' onto itself",
                        p
                    )))));
                }
                let dest = dest.as_str();
                if self.dry_run_skip(format!("copy {} -> {}", p, dest)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                if path.is_dir() {
                    match copy_dir_recursive(path, std::path::Path::new(dest)) {
                        Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                        Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                    }
                } else {
                    match std::fs::copy(p, dest) {
                        Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                        Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                    }
                }
            }
            "move_to" => {
                let dest = resolve_into_dir(path, arg_path_str(_args, 0, "move_to")?);
                if dest == p {
                    return Ok(Value::Err(Box::new(Value::String(format!(
                        "cannot move '{}' onto itself",
                        p
                    )))));
                }
                let dest = dest.as_str();
                if self.dry_run_skip(format!("move {} -> {}", p, dest)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                // Try rename first (fast, same-filesystem).
                // Fall back to copy + delete for cross-filesystem moves.
                match std::fs::rename(p, dest) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(_) => {
                        let result = if path.is_dir() {
                            copy_dir_recursive(path, std::path::Path::new(dest))
                                .and_then(|_| std::fs::remove_dir_all(path))
                        } else {
                            std::fs::copy(p, dest)
                                .map(|_| ())
                                .and_then(|_| std::fs::remove_file(p))
                        };
                        match result {
                            Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                            Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                        }
                    }
                }
            }
            "ls" => {
                let pattern = _args.first().and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
                match std::fs::read_dir(p) {
                    Ok(entries) => {
                        let mut items = Vec::new();
                        for entry in entries.flatten() {
                            if let Some(pat) = &pattern {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if !glob_matches(pat, &name) {
                                    continue;
                                }
                            }
                            items.push(Value::Path(entry.path().to_string_lossy().to_string()));
                        }
                        Ok(Value::List(items))
                    }
                    Err(e) => Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!("Cannot list '{}': {}", p, e),
                    ))),
                }
            }
            "walk" => {
                let mut results = Vec::new();
                walk_dir_recursive(path, &mut results, WalkFilter::All)?;
                results.sort();
                Ok(Value::List(results.into_iter().map(Value::Path).collect()))
            }
            "files" => {
                let mut results = Vec::new();
                walk_dir_recursive(path, &mut results, WalkFilter::FilesOnly)?;
                results.sort();
                Ok(Value::List(results.into_iter().map(Value::Path).collect()))
            }
            "dirs" => {
                let mut results = Vec::new();
                walk_dir_recursive(path, &mut results, WalkFilter::DirsOnly)?;
                results.sort();
                Ok(Value::List(results.into_iter().map(Value::Path).collect()))
            }
            "glob" => {
                let pattern = arg_str(_args, 0, "glob")?;
                // Rooted at this directory, then expanded exactly as a
                // standalone `g"..."` would be — `{a,b}` and all.
                let full_pattern = format!("{}/{}", p.trim_end_matches('/'), pattern);
                let results = glob_expand(&full_pattern)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| Value::Path(entry.to_string_lossy().into_owned()))
                    .collect();
                Ok(Value::List(results))
            }
            "join" => {
                let other = arg_path_str(_args, 0, "join")?;
                let result = path.join(other);
                Ok(Value::Path(result.to_string_lossy().to_string()))
            }
            "modified" => {
                match std::fs::metadata(p) {
                    Ok(meta) => match meta.modified() {
                        Ok(time) => {
                            let millis = time
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as i64;
                            Ok(Value::Int(millis))
                        }
                        Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                    },
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "relative_to" => {
                let base = arg_path_str(_args, 0, "relative_to")?;
                let base_path = std::path::Path::new(&base);
                // Simple relative path calculation
                if let Ok(stripped) = path.strip_prefix(base_path) {
                    Ok(Value::Path(stripped.to_string_lossy().to_string()))
                } else {
                    Ok(Value::Path(p.to_string()))
                }
            }
            "components" => {
                let parts: Vec<Value> = path
                    .components()
                    .map(|c| Value::String(c.as_os_str().to_string_lossy().to_string()))
                    .collect();
                Ok(Value::List(parts))
            }
            "depth" => {
                let count = path.components().count();
                Ok(Value::Int(count as i64))
            }
            // Second spellings of the arms above, kept only to say so.
            "ext" | "is_abs" | "is_rel" | "read_text" | "remove" | "copy"
            | "children" | "str" | "parts" => {
                let instead = match method {
                    "ext" => "extension",
                    "is_abs" => "is_absolute",
                    "is_rel" => "is_relative",
                    "read_text" => "read",
                    "remove" => "delete",
                    "copy" => "copy_to",
                    "children" => "ls",
                    "str" => "to_string",
                    _ => "components",
                };
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("`.{}()` was removed; use `.{}()` instead", method, instead),
                )))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Path has no method '{}'", method),
            ))),
        }
    }

    fn duration_method(&self, val: f64, unit: DurationUnit, method: &str) -> IResult {
        let ms = duration_to_ms(val, unit);
        match method {
            "to_millis" => Ok(Value::Float(ms)),
            "to_seconds" => Ok(Value::Float(ms / 1000.0)),
            "to_minutes" => Ok(Value::Float(ms / 60_000.0)),
            "to_hours" => Ok(Value::Float(ms / 3_600_000.0)),
            // The bare spellings read like a field rather than a conversion,
            // so only the `to_` ones survive.
            "millis" | "seconds" | "minutes" | "hours" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("`.{}()` was removed; use `.to_{}()` instead", method, method),
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Duration has no method '{}'", method),
            ))),
        }
    }

    fn process_result_method(
        &self,
        exit_code: i64,
        stdout: &str,
        stderr: &str,
        method: &str,
    ) -> IResult {
        match method {
            "success" => Ok(Value::Bool(exit_code == 0)),
            "ok" => {
                if exit_code == 0 {
                    Ok(Value::Ok(Box::new(Value::String(
                        stdout.trim_end().to_string(),
                    ))))
                } else {
                    Ok(Value::Err(Box::new(Value::String(
                        stderr.trim_end().to_string(),
                    ))))
                }
            }
            "stdout" => Ok(Value::String(stdout.to_string())),
            "stderr" => Ok(Value::String(stderr.to_string())),
            "exit_code" => Ok(Value::Int(exit_code)),
            "trim" => Ok(Value::String(stdout.trim().to_string())),
            "lines" => Ok(Value::List(
                stdout
                    .lines()
                    .map(|l| Value::String(l.to_string()))
                    .collect(),
            )),
            "code" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`.code()` was removed; use `.exit_code()` instead",
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("ProcessResult has no method '{}'", method),
            ))),
        }
    }

    // ── Semver methods ───────────────────────────────────────────────

    fn semver_method(&self, s: &str, method: &str, _args: &[Value]) -> IResult {
        let parse_semver = |ver: &str| -> (u64, u64, u64, String) {
            // Parse "major.minor.patch" or "major.minor.patch-prerelease"
            let (version_part, pre) = if let Some(idx) = ver.find('-') {
                (&ver[..idx], ver[idx..].to_string())
            } else {
                (ver, String::new())
            };
            let parts: Vec<&str> = version_part.split('.').collect();
            let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
            let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
            let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
            (major, minor, patch, pre)
        };

        match method {
            "bump_major" => {
                let (major, _, _, _) = parse_semver(s);
                Ok(Value::Semver(format!("{}.0.0", major + 1)))
            }
            "bump_minor" => {
                let (major, minor, _, _) = parse_semver(s);
                Ok(Value::Semver(format!("{}.{}.0", major, minor + 1)))
            }
            "bump_patch" => {
                let (major, minor, patch, _) = parse_semver(s);
                Ok(Value::Semver(format!("{}.{}.{}", major, minor, patch + 1)))
            }
            "major" => {
                let (major, _, _, _) = parse_semver(s);
                Ok(Value::Int(major as i64))
            }
            "minor" => {
                let (_, minor, _, _) = parse_semver(s);
                Ok(Value::Int(minor as i64))
            }
            "patch" => {
                let (_, _, patch, _) = parse_semver(s);
                Ok(Value::Int(patch as i64))
            }
            "prerelease" => {
                let (_, _, _, pre) = parse_semver(s);
                if pre.is_empty() {
                    Ok(Value::Null)
                } else {
                    // Strip leading '-'
                    Ok(Value::String(pre.trim_start_matches('-').to_string()))
                }
            }
            "is_prerelease" => {
                let (_, _, _, pre) = parse_semver(s);
                Ok(Value::Bool(!pre.is_empty()))
            }
            "satisfied_by" => {
                // Simple constraint matching: ">=1.2.0, <2.0.0".satisfiedBy(v"1.3.0")
                let ver = match _args.first() {
                    Some(Value::Semver(v)) => v.as_str(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "satisfied_by requires a Semver argument",
                    ))),
                };
                let satisfied = check_semver_constraint(s, ver);
                Ok(Value::Bool(satisfied))
            }
            "to_string" => Ok(Value::String(s.to_string())),
            "pre_release" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`.pre_release()` was removed; use `.prerelease()` instead",
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Semver has no method '{}'", method),
            ))),
        }
    }

    // ── Secret methods ───────────────────────────────────────────────

    fn secret_method(&self, s: &str, method: &str) -> IResult {
        match method {
            "expose" => Ok(Value::String(s.to_string())),
            "len" => Ok(Value::Int(s.len() as i64)),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Secret has no method '{}'", method),
            ))),
        }
    }


    // ── Glob methods ─────────────────────────────────────────────────

    fn glob_method(&mut self, pattern: &str, method: &str, args: &[Value]) -> IResult {
        match method {
            "test" => {
                // Test if a path matches this glob pattern
                let path = match args.first() {
                    Some(Value::Path(p)) => p.as_str(),
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "test() requires a path or string argument",
                    ))),
                };
                let matches = glob_matches(pattern, path);
                Ok(Value::Bool(matches))
            }
            "expand" => {
                let results = glob_expand(pattern)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| Value::Path(entry.to_string_lossy().into_owned()))
                    .collect();
                Ok(Value::List(results))
            }
            "first" => match glob_expand(pattern).unwrap_or_default().into_iter().next() {
                Some(entry) => Ok(Value::Path(entry.to_string_lossy().into_owned())),
                None => Ok(Value::Null),
            },
            "count" => Ok(Value::Int(glob_expand(pattern).unwrap_or_default().len() as i64)),
            "any" => Ok(Value::Bool(!glob_expand(pattern).unwrap_or_default().is_empty())),
            "pattern" => Ok(Value::String(pattern.to_string())),
            "to_string" => Ok(Value::String(pattern.to_string())),
            "copy_to" | "move_to" => self.glob_transfer(pattern, method, args),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Glob has no method '{}'", method),
            ))),
        }
    }

    /// `copy_to` / `move_to` for a whole match set.
    ///
    /// Each match keeps its position below the pattern's fixed base, so
    /// `glob("src/**/*.txt").copy_to(dest)` reproduces the subdirectories at
    /// `dest` instead of flattening every file into one directory. Missing
    /// directories are created; a match that is itself a directory brings its
    /// contents along, as `copy_to` on a `Path` does.
    fn glob_transfer(&mut self, pattern: &str, method: &str, args: &[Value]) -> IResult {
        let dest = std::path::PathBuf::from(path_arg(
            args.first().ok_or_else(|| {
                Signal::Error(QueError::new(
                    ErrorKind::ArityMismatch,
                    format!("{}() requires a destination", method),
                ))
            })?,
            method,
        )?);
        let pattern = expand_tilde(pattern);
        // The base comes from the pattern before brace expansion, so that the
        // alternatives in `{a,b}/*.txt` stay apart at the destination.
        let base = std::path::PathBuf::from(glob_base(&pattern));
        let moving = method == "move_to";

        // Expand fully before touching anything: copying into a directory the
        // pattern also covers would otherwise keep re-matching what it wrote.
        let sources = match glob_expand(&pattern) {
            Ok(sources) => sources,
            Err(e) => return Ok(Value::Err(Box::new(Value::String(e)))),
        };

        let mut moved = Vec::new();
        for src in sources {
            // A directory moved earlier takes its children with it, so by the
            // time they come up there is nothing left to move.
            if moving && !src.exists() {
                continue;
            }
            let target = dest.join(src.strip_prefix(&base).unwrap_or(&src));
            if target == src {
                return Ok(Value::Err(Box::new(Value::String(format!(
                    "cannot {} '{}' onto itself",
                    if moving { "move" } else { "copy" },
                    src.display()
                )))));
            }
            if self.permissions.is_some() {
                self.check_permission(
                    crate::permissions::Capability::Write,
                    &target.to_string_lossy(),
                )?;
            }
            if self.dry_run_skip(format!(
                "{} {} -> {}",
                if moving { "move" } else { "copy" },
                src.display(),
                target.display()
            )) {
                moved.push(Value::Path(target.to_string_lossy().into_owned()));
                continue;
            }
            let parent = target.parent().unwrap_or(&dest);
            let result = std::fs::create_dir_all(parent).and_then(|_| {
                if moving {
                    // Rename first, then fall back for cross-filesystem moves.
                    std::fs::rename(&src, &target).or_else(|_| {
                        if src.is_dir() {
                            copy_dir_recursive(&src, &target)
                                .and_then(|_| std::fs::remove_dir_all(&src))
                        } else {
                            std::fs::copy(&src, &target)
                                .and_then(|_| std::fs::remove_file(&src))
                        }
                    })
                } else if src.is_dir() {
                    copy_dir_recursive(&src, &target)
                } else {
                    std::fs::copy(&src, &target).map(|_| ())
                }
            });
            if let Err(e) = result {
                return Ok(Value::Err(Box::new(Value::String(format!(
                    "{}: {}",
                    src.display(),
                    e
                )))));
            }
            moved.push(Value::Path(target.to_string_lossy().into_owned()));
        }
        Ok(Value::Ok(Box::new(Value::List(moved))))
    }

    // ── Regex methods ────────────────────────────────────────────────

    fn regex_method(&self, pattern: &str, method: &str, args: &[Value]) -> IResult {
        let input = match args.first() {
            Some(Value::String(s)) => s.as_str(),
            _ if method == "to_string" => {
                return Ok(Value::String(pattern.to_string()));
            }
            _ => return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!("{}() requires a string argument", method),
            ))),
        };

        match method {
            "test" => {
                let matched = simple_regex_test(pattern, input);
                Ok(Value::Bool(matched))
            }
            "find" => {
                // Return first match or None
                if let Some(m) = simple_regex_find(pattern, input) {
                    Ok(Value::String(m))
                } else {
                    Ok(Value::Null)
                }
            }
            "find_all" => {
                let matches = simple_regex_find_all(pattern, input);
                Ok(Value::List(matches.into_iter().map(Value::String).collect()))
            }
            "replace" => {
                let replacement = match args.get(1) {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "replace() requires a replacement string",
                    ))),
                };
                let result = simple_regex_replace(pattern, input, replacement);
                Ok(Value::String(result))
            }
            "replace_all" => {
                let replacement = match args.get(1) {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "replace_all() requires a replacement string",
                    ))),
                };
                let result = simple_regex_replace_all(pattern, input, replacement);
                Ok(Value::String(result))
            }
            "captures" => {
                let caps = simple_regex_captures(pattern, input);
                Ok(Value::List(caps.into_iter().map(Value::String).collect()))
            }
            "named_captures" => {
                let map = simple_regex_named_captures(pattern, input);
                Ok(Value::Map(
                    map.into_iter().map(|(k, v)| (k, Value::String(v))).collect(),
                ))
            }
            "split" => {
                let parts = simple_regex_split(pattern, input);
                Ok(Value::List(parts.into_iter().map(Value::String).collect()))
            }
            "match" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "`.match()` was removed; use `.find()` instead",
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Regex has no method '{}'", method),
            ))),
        }
    }

    // ── Cmd methods ──────────────────────────────────────────────────

    /// Run a command and raise if it exits non-zero.
    ///
    /// This is the default for bare `` `cmd` `` statements and for `.run()`;
    /// use `.try()` to get the `ProcessResult` without raising.
    pub(crate) fn run_cmd_checked(&mut self, parts: &[CmdPart], mods: &CmdModifiers) -> IResult {
        let result = self.run_cmd_parts(parts, mods)?;
        if let Value::ProcessResult { exit_code, stderr, .. } = &result {
            if *exit_code != 0 {
                let detail = stderr.trim();
                let suffix = if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", detail)
                };
                return Err(Signal::Error(
                    QueError::new(
                        ErrorKind::CommandFailed,
                        format!("command failed with exit code {}{}", exit_code, suffix),
                    )
                    // Forward the child's own exit code, the way `set -e` does,
                    // so a CI step gating on a specific code still sees it.
                    .with_exit_code(*exit_code as i32),
                ));
            }
        }
        Ok(result)
    }

    fn cmd_method(&mut self, parts: &[CmdPart], mods: &CmdModifiers, method: &str, args: &[Value]) -> IResult {
        match method {
            // `.run()` is the explicit spelling of what a bare `` `cmd` ``
            // statement does: stream output to the terminal, raise if it fails.
            "run" => {
                let mut mods = mods.clone();
                if !mods.silent {
                    mods.forward_stdout
                        .get_or_insert_with(|| Box::new(crate::value::StreamSink::Stdout));
                    mods.forward_stderr
                        .get_or_insert_with(|| Box::new(crate::value::StreamSink::Stderr));
                }
                self.run_cmd_checked(parts, &mods)
            }
            // `.out()` — run, raise on failure, return trimmed stdout.
            "out" => {
                let result = self.run_cmd_checked(parts, mods)?;
                if let Value::ProcessResult { stdout, .. } = result {
                    Ok(Value::String(stdout.trim().to_string()))
                } else {
                    Ok(result)
                }
            }
            // `.try()` — run, never raise; inspect the ProcessResult yourself.
            "try" => self.run_cmd_parts(parts, mods),
            "run_checked" | "capture" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!(
                    "`.{}()` was removed; commands now raise on failure — use `.run()`, `.out()` or `.try()`",
                    method
                ),
            ))),
            "silent" => {
                // Return a new Cmd with silent modifier
                let mut new_mods = mods.clone();
                new_mods.silent = true;
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            // `.attach()` — give the child the terminal itself, for programs
            // that need a TTY (an editor, a shell, `docker run -it`). The
            // streams belong to the terminal in that mode, so there is
            // nothing left to capture and only the exit code comes back.
            "attach" => {
                if mods.stdin_data.is_some() {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "`.attach()` and `.stdin()` both claim the child's stdin",
                    )));
                }
                if !mods.stdin_from.is_empty() {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "`.attach()` cannot be used in a pipeline: the previous stage owns stdin",
                    )));
                }
                if mods.forward_stdout.is_some() || mods.forward_stderr.is_some() {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "`.attach()` writes straight to the terminal, so its output cannot also be forwarded to a stream",
                    )));
                }
                let mut new_mods = mods.clone();
                new_mods.attach = true;
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "dir" => {
                let dir_str = arg_path_str(args, 0, "dir")?;
                let mut new_mods = mods.clone();
                new_mods.dir = Some(dir_str);
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "env" => {
                // .env("KEY", "VALUE") — set a single env var
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "env() requires 2 arguments: key, value",
                    )));
                }
                let key = args[0].display_string();
                let val = args[1].display_string();
                let mut new_mods = mods.clone();
                new_mods.env_vars.push((key, val));
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "env_map" => {
                // .env_map({ "KEY": "VALUE", ... })
                let map = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "env_map() requires 1 argument"))
                })?;
                if let Value::Map(m) = map {
                    let mut new_mods = mods.clone();
                    for (k, v) in m {
                        new_mods.env_vars.push((k.clone(), v.display_string()));
                    }
                    Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "env_map() requires a map",
                    )))
                }
            }
            "timeout" => {
                let dur = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "timeout() requires 1 argument"))
                })?;
                let ms = match dur {
                    Value::Int(ms) => *ms as u64,
                    Value::Duration(val, unit) => duration_to_ms(*val, *unit) as u64,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "timeout() requires a duration",
                    ))),
                };
                let mut new_mods = mods.clone();
                new_mods.timeout_ms = Some(ms);
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "stdin" => {
                let data = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "stdin() requires 1 argument"))
                })?;
                if mods.attach {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "`.stdin()` and `.attach()` both claim the child's stdin",
                    )));
                }
                let mut new_mods = mods.clone();
                new_mods.stdin_data = Some(data.display_string());
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "forward_stdout" => {
                let target = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "forward_stdout() requires 1 stream argument"))
                })?;
                let sink = match target {
                    Value::Stream(s) => s.get_sink(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "forward_stdout() requires a stream argument",
                    ))),
                };
                let mut new_mods = mods.clone();
                new_mods.forward_stdout = Some(Box::new(sink));
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "forward_stderr" => {
                let target = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "forward_stderr() requires 1 stream argument"))
                })?;
                let sink = match target {
                    Value::Stream(s) => s.get_sink(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "forward_stderr() requires a stream argument",
                    ))),
                };
                let mut new_mods = mods.clone();
                new_mods.forward_stderr = Some(Box::new(sink));
                Ok(Value::Cmd(parts.to_vec(), Box::new(new_mods)))
            }
            "arg" => {
                // .arg(value) — append a single shell-escaped argument to the command.
                let val = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "arg() requires 1 argument"))
                })?;
                let mut new_parts = parts.to_vec();
                new_parts.push(CmdPart::Literal(" ".to_string()));
                new_parts.push(cmd_part_for(val));
                Ok(Value::Cmd(new_parts, Box::new(mods.clone())))
            }
            "flag" => {
                // .flag("--name") — append a boolean flag (no value).
                // .flag("--name", value) — append --name VALUE (value is shell-escaped).
                let flag_name = match args.first() {
                    Some(v) => v.display_string(),
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "flag() requires at least 1 argument (flag name)",
                    ))),
                };
                let mut new_parts = parts.to_vec();
                if let Some(val) = args.get(1) {
                    // --name VALUE form
                    new_parts.push(CmdPart::Literal(format!(" {} ", flag_name)));
                    new_parts.push(cmd_part_for(val));
                } else {
                    // Boolean flag form
                    new_parts.push(CmdPart::Literal(format!(" {}", flag_name)));
                }
                Ok(Value::Cmd(new_parts, Box::new(mods.clone())))
            }
            // `.sudo()` / `.sudo(user)` / `.sudo({ user, preserve_env, non_interactive })`
            //
            // Prepends the escalation to the command text rather than storing
            // a flag, so `--dry-run`, `.to_string()` and every failure message
            // show exactly what will be run, elevation included.
            "sudo" => {
                let opts = sudo_opts(args)?;
                let prefix = match sudo_prefix(&opts, parts)? {
                    // Already root: adding `sudo` here would break the very
                    // common case of the same script running in a container
                    // that has no sudo binary installed.
                    None => return Ok(Value::Cmd(parts.to_vec(), Box::new(mods.clone()))),
                    Some(p) => p,
                };
                let mut new_parts = vec![CmdPart::Literal(prefix)];
                new_parts.extend(parts.iter().cloned());
                Ok(Value::Cmd(new_parts, Box::new(mods.clone())))
            }
            "to_string" => {
                // The redacting renderer: `.to_string()` exists to be shown,
                // so a secret must not survive the conversion into an
                // ordinary String that nothing tracks.
                let cmd_str: String = parts.iter().map(|p| match p {
                    CmdPart::Literal(s) | CmdPart::Interpolated(s) | CmdPart::Raw(s) => s.clone(),
                    CmdPart::Secret(_) => crate::value::REDACTED.to_string(),
                }).collect::<Vec<_>>().join("");
                Ok(Value::String(cmd_str))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Cmd has no method '{}'", method),
            ))),
        }
    }

    // ── Tuple methods ────────────────────────────────────────────────

    fn tuple_method(&mut self, items: &[Value], method: &str, _args: &[Value]) -> IResult {
        match method {
            "len" => Ok(Value::Int(items.len() as i64)),
            "to_list" => Ok(Value::List(items.to_vec())),
            "to_set" => {
                let mut seen = Vec::new();
                for item in items {
                    if !self.set_contains(&seen, item)? {
                        seen.push(item.clone());
                    }
                }
                Ok(Value::Set(seen))
            }
            "contains" => {
                let item = _args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "contains requires 1 argument"))
                })?;
                Ok(Value::Bool(items.contains(item)))
            }
            "first" => Ok(items.first().cloned().unwrap_or(Value::Null)),
            "last" => Ok(items.last().cloned().unwrap_or(Value::Null)),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Tuple has no method '{}'", method),
            ))),
        }
    }


    pub(crate) fn run_cmd_parts(&mut self, parts: &[CmdPart], mods: &CmdModifiers) -> IResult {
        if !mods.stdin_from.is_empty() {
            return self.run_pipeline(parts, mods);
        }
        let cmd_str = render_cmd(parts);

        // Every subprocess in the language funnels through here or through
        // `run_pipeline`/`eval_spawn`, which is why exec is the capability
        // that can be enforced completely.
        self.check_permission(
            crate::permissions::Capability::Exec,
            &render_cmd_display(parts),
        )?;

        if self.dry_run_skip(render_cmd_display(parts)) {
            return Ok(Value::ProcessResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        let mut cmd = crate::interpreter::helpers::shell_command(&cmd_str);

        if let Some(ref dir) = mods.dir {
            cmd.current_dir(dir);
        }
        for (key, val) in &mods.env_vars {
            cmd.env(key, val);
        }

        // `.attach()` — the child inherits the terminal, so it can read keys
        // and draw on the screen like a program started from the shell. There
        // is nothing to capture once the streams are the terminal's, and no
        // timeout either: a program the user is talking to ends when the user
        // ends it.
        if mods.attach {
            let status = cmd
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .map_err(|e| Signal::Error(QueError::new(
                    ErrorKind::CommandFailed,
                    format!("failed to execute command: {}", e),
                )))?;
            return Ok(Value::ProcessResult {
                exit_code: status.code().unwrap_or(-1) as i64,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        let has_stdin = mods.stdin_data.is_some();
        let has_fwd_stdout = mods.forward_stdout.is_some();
        let has_fwd_stderr = mods.forward_stderr.is_some();

        if has_stdin || has_fwd_stdout || has_fwd_stderr {
            cmd.stdin(if has_stdin { std::process::Stdio::piped() } else { std::process::Stdio::null() });
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| Signal::Error(QueError::new(
                ErrorKind::CommandFailed,
                format!("failed to execute command: {}", e),
            )))?;

            if let Some(ref stdin_data) = mods.stdin_data {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(stdin_data.as_bytes());
                    drop(stdin);
                }
            }

            let (exit_code, stdout, stderr) = capture_child(child, mods)?;
            Ok(Value::ProcessResult { exit_code, stdout, stderr })
        } else {
            // Simple path: no stdin data, no forwarding.
            cmd.stdin(std::process::Stdio::null());
            let output = cmd.output().map_err(|e| Signal::Error(QueError::new(
                ErrorKind::CommandFailed,
                format!("failed to execute command: {}", e),
            )))?;
            Ok(Value::ProcessResult {
                exit_code: output.status.code().unwrap_or(-1) as i64,
                stdout: if mods.silent { String::new() } else { String::from_utf8_lossy(&output.stdout).to_string() },
                stderr: if mods.silent { String::new() } else { String::from_utf8_lossy(&output.stderr).to_string() },
            })
        }
    }

    /// Run `a | b | c`: each stage's stdout becomes the next stage's stdin.
    ///
    /// Unlike a plain `sh` pipeline, this fails if *any* stage fails rather
    /// than only the last. A pipeline whose first command died but whose last
    /// one succeeded is the classic shell footgun, and Que already raises on a
    /// failing command, so `set -o pipefail` semantics is the only consistent
    /// choice here.
    fn run_pipeline(&mut self, last_parts: &[CmdPart], mods: &CmdModifiers) -> IResult {
        use std::process::Stdio;

        // Each stage is its own process, so each stage is its own check.
        for stage in &mods.stdin_from {
            self.check_permission(
                crate::permissions::Capability::Exec,
                &render_cmd_display(&stage.parts),
            )?;
        }
        self.check_permission(
            crate::permissions::Capability::Exec,
            &render_cmd_display(last_parts),
        )?;

        if self.dry_run {
            let mut text = String::new();
            for stage in &mods.stdin_from {
                text.push_str(&render_cmd_display(&stage.parts));
                text.push_str(" | ");
            }
            text.push_str(&render_cmd_display(last_parts));
            self.dry_run_skip(text);
            return Ok(Value::ProcessResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        // Upstream stages: stdout piped onward, stderr straight to the
        // terminal like a shell, since only the last stage's output is the
        // pipeline's value.
        let mut upstream: Vec<(String, std::process::Child)> = Vec::new();
        let mut previous: Option<std::process::ChildStdout> = None;

        for (i, stage) in mods.stdin_from.iter().enumerate() {
            let text = render_cmd(&stage.parts);
            let mut cmd = build_shell_command(&text, &stage.mods);            match previous.take() {
                Some(out) => {
                    cmd.stdin(Stdio::from(out));
                }
                None => {
                    cmd.stdin(if stage.mods.stdin_data.is_some() {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    });
                }
            }
            cmd.stdout(Stdio::piped());
            if stage.mods.silent {
                cmd.stderr(Stdio::null());
            }
            let mut child = cmd.spawn().map_err(|e| {
                Signal::Error(QueError::new(
                    ErrorKind::CommandFailed,
                    format!("failed to execute command: {}", e),
                ))
            })?;
            // Only the first stage can be fed from Que.
            if i == 0 {
                if let Some(ref data) = stage.mods.stdin_data {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(data.as_bytes());
                    }
                }
            }
            previous = child.stdout.take();
            // The display form: this text only ever reaches a failure
            // message, and a failure message is exactly where a leaked
            // token would end up in a CI log.
            upstream.push((render_cmd_display(&stage.parts), child));
        }

        let last_text = render_cmd(last_parts);
        let mut cmd = build_shell_command(&last_text, mods);
        cmd.stdin(previous.map(Stdio::from).unwrap_or_else(Stdio::null));
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let child = cmd.spawn().map_err(|e| {
            Signal::Error(QueError::new(
                ErrorKind::CommandFailed,
                format!("failed to execute command: {}", e),
            ))
        })?;
        let (exit_code, stdout, stderr) = capture_child(child, mods)?;

        // Report the leftmost failure: it is the one that caused the rest.
        for (text, mut child) in upstream {
            let status = child.wait().map_err(|e| {
                Signal::Error(QueError::new(
                    ErrorKind::CommandFailed,
                    format!("failed to wait for command: {}", e),
                ))
            })?;
            let code = status.code().unwrap_or(-1) as i64;
            if code != 0 {
                return Ok(Value::ProcessResult {
                    exit_code: code,
                    stdout,
                    stderr: format!("`{}` in pipeline failed with exit code {}", text.trim(), code),
                });
            }
        }

        Ok(Value::ProcessResult { exit_code, stdout, stderr })
    }
}

/// Turn a value appended to a command into the right `CmdPart`.
///
/// `.arg(tok)` and `.flag("--token", tok)` are the programmatic spelling of
/// interpolation and have to keep a secret secret for the same reason.
fn cmd_part_for(val: &Value) -> CmdPart {
    match val {
        Value::Secret(s) => CmdPart::Secret(s.clone()),
        other => CmdPart::Interpolated(other.display_string()),
    }
}

/// Read one boolean out of an optional `{ key: bool }` options map.
///
/// A non-map argument is refused rather than coerced: `mkdir(true)` is a
/// typo for `mkdir({ clean: true })`, and silently accepting it would make
/// the two spellings drift apart.
fn bool_opt(arg: Option<&Value>, key: &str, method: &str) -> Result<bool, Signal> {
    match arg {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Map(m)) => match m.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(other) => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "{}(): option '{}' must be a Bool, got {}",
                    method,
                    key,
                    other.type_name()
                ),
            ))),
        },
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{}() takes an options map like {{ {}: true }}, got {}",
                method,
                key,
                other.type_name()
            ),
        ))),
    }
}

/// Remove a file, directory or symlink.
///
/// `symlink_metadata` rather than `is_dir` so a symlink pointing at a
/// directory is unlinked instead of being handed to `remove_dir_all`, which
/// fails on it. `missing_ok` decides whether an absent path is success or
/// the `NotFound` the caller would otherwise see.
fn remove_path(p: &str, missing_ok: bool) -> std::io::Result<()> {
    match std::fs::symlink_metadata(p) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
        Err(e) if missing_ok && e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Collect the needles for `contains_all` / `contains_any`.
///
/// Both spellings are accepted because both read naturally at the call site:
/// `name.contains_all(hints)` when the needles already are a collection, and
/// `name.contains_all("x86", "gcc")` when they are written out. A single
/// collection argument is unwrapped rather than stringified, so the common
/// case never has to be spread.
fn substring_args(args: &[Value], method: &str) -> Result<Vec<String>, Signal> {
    let items: &[Value] = match args {
        [Value::List(items)] | [Value::Set(items)] | [Value::Tuple(items)] => items,
        rest => rest,
    };
    items
        .iter()
        .map(|v| match v {
            Value::String(s) => Ok(s.clone()),
            Value::Path(p) => Ok(p.clone()),
            other => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "{}() needles must be strings, got {}",
                    method,
                    other.type_name()
                ),
            ))),
        })
        .collect()
}

/// Options for `.sudo()`.
struct SudoOpts {
    user: Option<String>,
    preserve_env: bool,
    non_interactive: bool,
    binary: String,
}

impl Default for SudoOpts {
    fn default() -> Self {
        SudoOpts {
            user: None,
            preserve_env: false,
            non_interactive: false,
            binary: "sudo".to_string(),
        }
    }
}

fn sudo_opts(args: &[Value]) -> Result<SudoOpts, Signal> {
    let mut opts = SudoOpts::default();
    match args.first() {
        None | Some(Value::Null) => {}
        // `.sudo("postgres")` — the overwhelmingly common case gets the
        // short spelling.
        Some(Value::String(user)) => opts.user = Some(user.clone()),
        Some(Value::Map(m)) => {
            if let Some(v) = m.get("user") {
                if !matches!(v, Value::Null) {
                    opts.user = Some(v.display_string());
                }
            }
            if let Some(Value::Bool(b)) = m.get("preserve_env") {
                opts.preserve_env = *b;
            }
            if let Some(Value::Bool(b)) = m.get("non_interactive") {
                opts.non_interactive = *b;
            }
            // `doas` on OpenBSD, `run0` on newer systemd, or an absolute
            // path when sudo is not on PATH for the elevated environment.
            if let Some(v) = m.get("binary") {
                if !matches!(v, Value::Null) {
                    opts.binary = v.display_string();
                }
            }
        }
        Some(other) => {
            return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "sudo() takes a user name or an options map, got {}",
                    other.type_name()
                ),
            )))
        }
    }
    Ok(opts)
}

/// Build the text to prepend, or `None` when no escalation is needed.
fn sudo_prefix(opts: &SudoOpts, parts: &[CmdPart]) -> Result<Option<String>, Signal> {
    if cfg!(windows) {
        return Err(Signal::Error(QueError::new(
            ErrorKind::Runtime,
            "sudo() is not available on Windows; run the script from an elevated prompt instead",
        )));
    }

    // Elevating to the user you already are is a no-op everywhere, and
    // pretending otherwise means requiring a sudo binary that a minimal
    // container image will not have.
    if running_as_root() && opts.user.as_deref().unwrap_or("root") == "root" {
        return Ok(None);
    }

    // A shell operator after the command would run *unelevated*, because
    // only the first word is passed to sudo. `sudo -- tee /etc/hosts` works;
    // `sudo -- echo x > /etc/hosts` writes as you, not as root, and the
    // permission error it produces points at the wrong thing. Refuse rather
    // than silently do the wrong half.
    if let Some(op) = leading_shell_operator(parts) {
        return Err(Signal::Error(QueError::new(
            ErrorKind::Runtime,
            format!(
                "sudo() cannot elevate a command containing `{}` \u{2014} only the first stage would run \
                 as root. Elevate each part separately, or use a command that does the whole job \
                 (for example `tee` instead of a redirect).",
                op
            ),
        )));
    }

    let mut prefix = opts.binary.clone();
    if opts.non_interactive {
        // Fail loudly instead of blocking on a password prompt nobody is
        // watching. This is what you want in CI.
        prefix.push_str(" -n");
    }
    if opts.preserve_env {
        prefix.push_str(" -E");
    }
    if let Some(user) = &opts.user {
        prefix.push_str(" -u ");
        prefix.push_str(&shell_escape(user));
    }
    // `--` so a command whose first word begins with `-` is not read as a
    // sudo option.
    prefix.push_str(" -- ");
    Ok(Some(prefix))
}

/// The first shell operator found in the *literal* (unescaped) parts of a
/// command. Interpolated and secret parts are escaped before they reach the
/// shell, so an operator inside one is inert.
fn leading_shell_operator(parts: &[CmdPart]) -> Option<&'static str> {
    for part in parts {
        let text = match part {
            CmdPart::Literal(s) | CmdPart::Raw(s) => s,
            _ => continue,
        };
        // Skip anything inside quotes: `grep "a|b" f` is one command.
        let mut quote: Option<char> = None;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match (quote, c) {
                (Some(q), _) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '\'') | (None, '"') => quote = Some(c),
                (None, '\\') => {
                    chars.next();
                }
                (None, '|') => return Some("|"),
                (None, ';') => return Some(";"),
                (None, '&') => return Some("&"),
                (None, '>') => return Some(">"),
                (None, '<') => return Some("<"),
                _ => {}
            }
        }
    }
    None
}

/// Whether this process is already running with an effective uid of 0.
///
/// Asked once and cached: it cannot change during a run, and it is consulted
/// on every `.sudo()`.
pub(crate) fn running_as_root() -> bool {
    static IS_ROOT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IS_ROOT.get_or_init(|| {
        if cfg!(windows) {
            return false;
        }
        // `id -u` rather than libc::geteuid, to avoid taking a C dependency
        // for one integer. It runs at most once per process.
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|s| s.trim() == "0")
            .unwrap_or(false)
    })
}

/// Flatten a command's parts into the text handed to `sh -c`. Only
/// interpolated values are escaped; literal text is shell syntax by design,
/// which is what makes `` `grep x f | wc -l` `` and `` `cmd > out.txt` `` work.
pub(crate) fn render_cmd(parts: &[CmdPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            CmdPart::Literal(s) => s.clone(),
            CmdPart::Interpolated(s) => shell_escape(s),
            CmdPart::Raw(s) => s.clone(),
            CmdPart::Secret(s) => shell_escape(s),
        })
        .collect()
}

/// Flatten a command's parts for a human: identical to [`render_cmd`] except
/// that interpolated secrets appear as `<redacted>`.
///
/// Every place that shows a command without running it -- the `--dry-run`
/// echo, a pipeline's failure message -- must use this. Keeping it beside
/// `render_cmd` is deliberate: the two are meant to be read together, so a
/// new `CmdPart` cannot be added to one without confronting the other.
pub(crate) fn render_cmd_display(parts: &[CmdPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            CmdPart::Literal(s) => s.clone(),
            CmdPart::Interpolated(s) => shell_escape(s),
            CmdPart::Raw(s) => s.clone(),
            CmdPart::Secret(_) => crate::value::REDACTED.to_string(),
        })
        .collect()
}

/// Build `sh -c <text>` with a stage's working directory and environment
/// applied. Stdio is left to the caller.
fn build_shell_command(text: &str, mods: &CmdModifiers) -> std::process::Command {
    let mut cmd = crate::interpreter::helpers::shell_command(text);
    if let Some(ref dir) = mods.dir {
        cmd.current_dir(dir);
    }
    for (key, val) in &mods.env_vars {
        cmd.env(key, val);
    }
    cmd
}

/// Drain a spawned child's stdout and stderr concurrently, forwarding each to
/// its sink if one is configured and buffering it otherwise. Reading on
/// threads is what keeps a chatty child from filling a pipe and deadlocking.
fn capture_child(
    mut child: std::process::Child,
    mods: &CmdModifiers,
) -> Result<(i64, String, String), Signal> {
    use std::sync::{Arc, Mutex};

    let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    let stdout_thread = child.stdout.take().map(|out| {
        drain(out, Arc::clone(&stdout_buf), mods.forward_stdout.as_deref().cloned(), mods.silent)
    });
    let stderr_thread = child.stderr.take().map(|err| {
        drain(err, Arc::clone(&stderr_buf), mods.forward_stderr.as_deref().cloned(), mods.silent)
    });

    let status = child.wait().map_err(|e| {
        Signal::Error(QueError::new(
            ErrorKind::CommandFailed,
            format!("failed to wait for command: {}", e),
        ))
    })?;
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }

    let stdout = if mods.silent || mods.forward_stdout.is_some() {
        String::new()
    } else {
        String::from_utf8_lossy(&stdout_buf.lock().unwrap()).to_string()
    };
    let stderr = if mods.silent || mods.forward_stderr.is_some() {
        String::new()
    } else {
        String::from_utf8_lossy(&stderr_buf.lock().unwrap()).to_string()
    };

    Ok((status.code().unwrap_or(-1) as i64, stdout, stderr))
}

/// Read one child stream to completion on its own thread.
fn drain<R: std::io::Read + Send + 'static>(
    mut stream: R,
    buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    sink: Option<StreamSink>,
    silent: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut tmp = [0u8; 4096];
        let mut writer = sink.as_ref().and_then(sink_to_writer);
        loop {
            match stream.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if !silent {
                        if let Some(ref mut w) = writer {
                            let _ = w.write_all(&tmp[..n]);
                        } else {
                            buf.lock().unwrap().extend_from_slice(&tmp[..n]);
                        }
                    }
                }
            }
        }
        if let Some(ref mut w) = writer {
            let _ = w.flush();
        }
    })
}

// ── Stream I/O helpers ───────────────────────────────────────────────

/// Produce a `Box<dyn Write + Send>` from a `StreamSink` for live forwarding.
fn sink_to_writer(sink: &StreamSink) -> Option<Box<dyn std::io::Write + Send>> {
    match sink {
        StreamSink::Stdout => Some(Box::new(std::io::stdout())),
        StreamSink::Stderr => Some(Box::new(std::io::stderr())),
        StreamSink::File { path, append } => {
            std::fs::OpenOptions::new()
                .create(true)
                .write(!append)
                .append(*append)
                .truncate(!append)
                .open(path)
                .ok()
                .map(|f| Box::new(f) as Box<dyn std::io::Write + Send>)
        }
        StreamSink::FileHandle(fh) => {
            Some(Box::new(FileHandleWriter(Arc::clone(&fh.inner))))
        }
        StreamSink::None => None,
    }
}

/// Wraps a `FileHandle`'s inner `BufWriter` so it can be used as a `Write` target.
struct FileHandleWriter(Arc<Mutex<FileHandleInner>>);

impl std::io::Write for FileHandleWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.0.lock().unwrap();
        if inner.discard {
            return Ok(buf.len());
        }
        inner.writer.as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "not open for writing"))
            .and_then(|w| w.write(buf))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let mut inner = self.0.lock().unwrap();
        if let Some(ref mut w) = inner.writer { w.flush() } else { Ok(()) }
    }
}

// ── Streaming pipeline helpers ───────────────────────────────────────

/// Sink for emitted lines from a stream pipeline.
pub(crate) enum StreamOutput<'a> {
    /// Append lines to an in-memory String (joined with '\n').
    Buffer(&'a mut String),
    /// Push each line as a `Value::String` into a list.
    Lines(&'a mut Vec<Value>),
    /// Write lines directly to a `Write` target (joined with '\n').
    Writer(&'a mut Box<dyn std::io::Write>),
    /// Count UTF-8 bytes of emitted lines (plus separators).
    ByteCount(&'a mut usize),
    /// Count emitted lines.
    LineCount(&'a mut usize),
}

impl<'a> StreamOutput<'a> {
    fn emit_line(&mut self, line: &str) -> Result<(), Signal> {
        match self {
            StreamOutput::Buffer(b) => b.push_str(line),
            StreamOutput::Lines(v) => v.push(Value::String(line.to_string())),
            StreamOutput::Writer(w) => w.write_all(line.as_bytes()).map_err(io_err)?,
            StreamOutput::ByteCount(n) => **n += line.len(),
            StreamOutput::LineCount(n) => **n += 1,
        }
        Ok(())
    }

    fn emit_separator(&mut self) -> Result<(), Signal> {
        match self {
            StreamOutput::Buffer(b) => b.push('\n'),
            // Lines/LineCount: '\n' is the implicit separator between list items.
            StreamOutput::Lines(_) | StreamOutput::LineCount(_) => {}
            StreamOutput::Writer(w) => w.write_all(b"\n").map_err(io_err)?,
            StreamOutput::ByteCount(n) => **n += 1,
        }
        Ok(())
    }
}

fn io_err(e: std::io::Error) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::IoError,
        format!("stream: write error: {}", e),
    ))
}

/// Open a `StreamSource` as a line iterator. Yields lines without trailing
/// '\n'. For `Buffer` sources the iterator owns the data; for `File` and
/// `Stdin` it streams from the underlying handle through a `BufReader`.
pub(crate) fn open_source_lines(
    source: &StreamSource,
) -> Result<Box<dyn Iterator<Item = std::io::Result<String>> + Send>, Signal> {
    match source {
        StreamSource::Buffer(s) => {
            // Match `String::lines` semantics: split on '\n', strip optional '\r'.
            // Materializes the line vec, but the source string was already in memory.
            let owned = s.clone();
            let lines: Vec<String> = owned.lines().map(String::from).collect();
            Ok(Box::new(lines.into_iter().map(Ok)))
        }
        StreamSource::File(path) => {
            let file = std::fs::File::open(path).map_err(|e| Signal::Error(QueError::new(
                ErrorKind::IoError,
                format!("stream: cannot open '{}': {}", path, e),
            )))?;
            let reader = std::io::BufReader::new(file);
            Ok(Box::new(reader.lines()))
        }
        StreamSource::Stdin => {
            let stdin = std::io::stdin();
            // `BufReader` over a locked stdin can't outlive the lock — use the
            // built-in line iterator on a fresh BufReader instead.
            let reader = std::io::BufReader::new(stdin);
            Ok(Box::new(reader.lines()))
        }
    }
}

/// Apply a buffering op (Trim, SortLines, Prepend, …) to a fully-materialized
/// buffer. Mirrors `value::apply_op_eager` but lives next to the executor.
fn apply_buffer_op_value(op: &StreamOp, text: String) -> Result<String, String> {
    Ok(match op {
        StreamOp::Trim => text.trim().to_string(),
        StreamOp::Tail(n) => {
            let all: Vec<&str> = text.lines().collect();
            let start = all.len().saturating_sub(*n);
            all[start..].join("\n")
        }
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
        StreamOp::JoinLines(sep) => text.lines().collect::<Vec<_>>().join(sep),
        StreamOp::Prepend(p) => format!("{}{}", p, text),
        StreamOp::Append(s) => format!("{}{}", text, s),
        StreamOp::ReplaceBuf(from, to) => text.replace(from, to),
        // Streaming ops never appear here.
        _ => text,
    })
}

// ── Path walk helpers ────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum WalkFilter {
    All,
    FilesOnly,
    DirsOnly,
}

fn walk_dir_recursive(
    dir: &std::path::Path,
    results: &mut Vec<String>,
    filter: WalkFilter,
) -> Result<(), Signal> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        Signal::Error(QueError::new(
            ErrorKind::Runtime,
            format!("Cannot walk '{}': {}", dir.display(), e),
        ))
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = path.is_dir();
        match filter {
            WalkFilter::All => results.push(path.to_string_lossy().into_owned()),
            WalkFilter::FilesOnly => {
                if !is_dir {
                    results.push(path.to_string_lossy().into_owned());
                }
            }
            WalkFilter::DirsOnly => {
                if is_dir {
                    results.push(path.to_string_lossy().into_owned());
                }
            }
        }
        if is_dir {
            walk_dir_recursive(&path, results, filter)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::DurationUnit;

    /// Methods that do something to the machine rather than to the value.
    /// They are dispatched the same way as the rest, so leaving them out of
    /// the sweep below costs nothing and keeps the test from writing files
    /// or spawning processes.
    const DO_NOT_ACTUALLY_RUN: &[&str] = &[
        // Filesystem mutation
        "delete", "remove", "mkdir", "copy", "copy_to", "move_to", "symlink",
        "write", "write_text", "append", "append_text", "write_to", "append_to",
        // Process execution
        "run", "out", "try", "capture", "run_checked", "attach", "sudo",
        "kill", "kill_force", "wait", "close", "flush",
    ];

    /// A value of every type that carries a built-in method table, paired
    /// with the table's key.
    fn sample_values() -> Vec<Value> {
        vec![
            Value::String("abc".into()),
            Value::List(vec![Value::Int(1)]),
            Value::Map(BTreeMap::from([("a".to_string(), Value::Int(1))])),
            Value::Set(vec![Value::Int(1)]),
            Value::Tuple(vec![Value::Int(1), Value::Int(2)]),
            // A path under a directory that does not exist, so that a read
            // fails rather than reporting on somebody's real files.
            Value::Path("/nonexistent-que-method-sweep/f.txt".into()),
            Value::Glob("*.txt".into()),
            Value::Duration(1.0, DurationUnit::Seconds),
            Value::Regex("a.c".into()),
            Value::Semver("1.2.3".into()),
            Value::Secret("hunter2".into()),
            Value::Int(1),
            Value::Float(1.5),
            Value::Bool(true),
            Value::Ok(Box::new(Value::Int(1))),
            Value::Err(Box::new(Value::String("boom".into()))),
            Value::Cmd(
                vec![CmdPart::Literal("true".into())],
                Box::new(CmdModifiers::default()),
            ),
            Value::ProcessResult {
                exit_code: 0,
                stdout: "out".into(),
                stderr: String::new(),
            },
            Value::Enum {
                enum_name: "Color".into(),
                variant: "Red".into(),
                fields: BTreeMap::new(),
            },
        ]
    }

    /// `available_methods` reads one table and `call_method` implements
    /// another; nothing but a test keeps them honest. Before this existed the
    /// two had drifted far enough that `?90s` advertised `as_secs`, `has_method`
    /// agreed it was there, and calling it failed.
    #[test]
    fn every_advertised_method_is_one_the_dispatcher_answers_to() {
        let mut interp = Interpreter::new();
        let mut missing: Vec<String> = Vec::new();

        for value in sample_values() {
            for method in value.available_methods() {
                if DO_NOT_ACTUALLY_RUN.contains(&method) {
                    continue;
                }
                // Called with no arguments: a method that wants one answers
                // with an arity or type complaint, which is not what we are
                // looking for. Only "no such method" means the tables lied.
                if let Err(Signal::Error(e)) = interp.call_method(&value, method, vec![]) {
                    let msg = e.to_string();
                    if msg.contains("has no method") || msg.contains("has no method or key") {
                        missing.push(format!("{}.{}", value.type_name(), method));
                    }
                }
            }
        }

        assert!(
            missing.is_empty(),
            "these methods are advertised but not implemented: {:?}",
            missing
        );
    }

    /// The other direction is not fully checkable — the dispatcher's arms are
    /// not enumerable at runtime — but the types that carry methods should at
    /// least all have a table.
    #[test]
    fn every_sample_type_has_a_method_table() {
        for value in sample_values() {
            let names = value.available_methods();
            // Four universal methods come free; a real table adds to them.
            assert!(
                names.len() > 4,
                "{} has no methods in docs::methods_for_type",
                value.type_name()
            );
        }
    }
}
