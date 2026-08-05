//! Task execution engine for the Que interpreter.

use super::Interpreter;
use crate::error::*;
use crate::value::Value;

use colored::Colorize;

impl Interpreter {
    // ── Task methods & execution engine ──────────────────────────────

    pub(crate) fn task_method(
        &mut self,
        task: &crate::value::TaskData,
        method: &str,
        args: Vec<Value>,
    ) -> IResult {
        match method {
            "run" => {
                let named_args: Vec<(Option<String>, Value)> =
                    args.into_iter().map(|v| (None, v)).collect();
                self.execute_task(task, named_args)
            }
            "deps" => {
                Ok(Value::List(
                    task.depends_on.iter().map(|d| Value::String(d.clone())).collect(),
                ))
            }
            "inputs" => {
                // Evaluate input expressions and return as list.
                // Plain strings are promoted to Path values since inputs are always file paths.
                let mut result = Vec::new();
                for expr in &task.inputs {
                    let val = self.eval_expr(expr)?;
                    match val {
                        Value::String(s) => result.push(Value::Path(s)),
                        other => result.push(other),
                    }
                }
                Ok(Value::List(result))
            }
            "outputs" => {
                // Evaluate output expressions and return as list.
                // Plain strings are promoted to Path values since outputs are always file paths.
                let mut result = Vec::new();
                for expr in &task.outputs {
                    let val = self.eval_expr(expr)?;
                    match val {
                        Value::String(s) => result.push(Value::Path(s)),
                        other => result.push(other),
                    }
                }
                Ok(Value::List(result))
            }
            "env" => {
                // Return the list of tracked env var names
                Ok(Value::List(
                    task.env_keys.iter().map(|k| Value::String(k.clone())).collect(),
                ))
            }
            "status" => {
                // Return the status from last execution
                match self.task_status.get(&task.name) {
                    Some((status, _)) => Ok(Value::String(status.clone())),
                    None => Ok(Value::String("pending".to_string())),
                }
            }
            "result" => self.task_result(&task.name),
            "description" => {
                Ok(task.description.as_ref()
                    .map(|d| Value::String(d.clone()))
                    .unwrap_or(Value::Null))
            }
            "aliases" => {
                Ok(Value::List(
                    task.aliases.iter().map(|a| Value::String(a.clone())).collect(),
                ))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Task has no method '{}'", method),
            ))),
        }
    }

    /// The value a task's body evaluated to on its last run.
    ///
    /// A dependency that produces something — a temporary directory, a version
    /// string, a list of files — has nowhere to put it: `@deps` runs it and
    /// throws the value away. Without this, the two tasks have to agree on a
    /// path out of band, or the dependent task re-runs its dependency just to
    /// see what it returns. The value is already recorded next to the status,
    /// so handing it back costs nothing.
    pub(crate) fn task_result(&self, name: &str) -> IResult {
        match self.task_status.get(name) {
            Some((status, value)) if status == "succeeded" => Ok(value.clone()),
            // A skipped task's outputs were already on disk and its body never
            // ran, so there is no value from this run to hand over. The files
            // it declared as outputs are what it produced in that case.
            Some((status, _)) if status == "skipped" => Ok(Value::Null),
            Some((status, _)) if status == "failed" => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("task '{}' failed, so it has no result", name),
            ))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!(
                    "task '{}' has not run yet, so it has no result; name it in @deps or call run_task(\"{}\") first",
                    name, name
                ),
            ))),
        }
    }

    /// Execute a task with full dependency resolution and optional skip detection.
    ///
    /// `args` is a list of `(name, value)` pairs where `name` is `None` for positional
    /// arguments and `Some(name)` for named arguments (e.g. from `key=value` CLI syntax or
    /// `param: value` in-language syntax). Named arguments take precedence over positional
    /// binding; any unmatched named args are ignored.
    pub fn execute_task(
        &mut self,
        task: &crate::value::TaskData,
        args: Vec<(Option<String>, Value)>,
    ) -> IResult {
        let name = &task.name;
        let params = &task.params;
        let depends_on = &task.depends_on;
        let body = &task.body;
        let closure_env = &task.closure_env;

        // 1. Resolve and execute dependencies first
        for dep_name in depends_on {
            // Skip already-succeeded dependencies in this execution
            if let Some(("succeeded", _)) = self.task_status.get(dep_name.as_str()).map(|(s, v)| (s.as_str(), v)) {
                continue;
            }

            let dep_task = self.env.get(dep_name).ok_or_else(|| {
                Signal::Error(QueError::new(
                    ErrorKind::UndefinedVariable,
                    format!("dependency task '{}' not found (required by '{}')", dep_name, name),
                ))
            })?;

            match dep_task {
                Value::Task(t) => {
                    self.execute_task(&t, vec![])?;
                }
                _ => {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        format!("dependency '{}' is not a task", dep_name),
                    )));
                }
            }
        }

        // Split into positional and named lists for binding
        let mut positional: Vec<Value> = Vec::new();
        let mut named: Vec<(String, Value)> = Vec::new();
        for (name_opt, val) in args.iter() {
            match name_opt {
                Some(n) => named.push((n.clone(), val.clone())),
                None => positional.push(val.clone()),
            }
        }

        // Flat values for hashing (order: positional then named values)
        let hash_args: Vec<Value> = positional.iter().chain(named.iter().map(|(_, v)| v)).cloned().collect();

        // 2. Check if we can skip (inputs/outputs freshness + param/env hash check)
        if self.should_skip_task(task, &hash_args)? {
            self.task_status.insert(name.to_string(), ("skipped".to_string(), Value::Null));
            self.emit(format!("{}", format!("[SKIP] {}", name).yellow()));
            return Ok(Value::Null);
        }

        // 3. Execute the task body
        self.emit(format!("{}", format!("[RUN]  {}", name).cyan()));

        // Save call-site span/file so that after the task returns, current_span
        // reflects the call site rather than a stale line inside the task body.
        let saved_span = self.current_span;
        let saved_file = self.current_file.clone();
        self.call_stack.push(crate::error::CallFrame {
            name: name.clone(),
            call_file: self.current_file.clone(),
            call_span: self.current_span,
        });

        let saved_env = std::mem::replace(&mut self.env, closure_env.clone());
        self.env.push_scope();

        // Bind parameters: named args first (by name), then positional in order, then defaults
        let mut pos_idx = 0;
        for param in params.iter() {
            let val = if let Some(idx) = named.iter().position(|(n, _)| n == &param.name) {
                named.remove(idx).1
            } else if pos_idx < positional.len() {
                let v = positional[pos_idx].clone();
                pos_idx += 1;
                v
            } else if let Some(default) = &param.default {
                self.eval_expr(default)?
            } else {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::ArityMismatch,
                    format!(
                        "task '{}': required argument '{}' not provided",
                        name, param.name
                    ),
                )));
            };
            self.env.define(&param.name, val, false);
        }

        let result = self.eval_block(body);
        self.env.pop_scope();
        self.env = saved_env;

        // Restore call-site span/file and pop the call stack frame.
        self.current_span = saved_span;
        self.current_file = saved_file;
        self.call_stack.pop();

        // 4. On success, store the param/env hash for future skip checks
        match result {
            Ok(v) | Err(Signal::Return(v)) => {
                let hash = self.compute_task_hash(task, &hash_args);
                self.task_cache.insert(name.to_string(), hash);
                self.record_task_result(task, &hash_args);
                self.task_status.insert(name.to_string(), ("succeeded".to_string(), v.clone()));
                self.emit(format!("{}", format!("[DONE] {}", name).green()));
                Ok(v)
            }
            Err(Signal::Error(e)) => {
                self.task_status.insert(name.to_string(), ("failed".to_string(), Value::String(e.to_string())));
                self.emit(format!("{}", format!("[FAIL] {}", name).red()));
                Err(Signal::Error(e))
            }
            Err(other) => Err(other),
        }
    }

    /// Compute a hash over the task's arguments and tracked env vars.
    /// This captures non-file inputs that affect the task's output.
    fn compute_task_hash(&self, task: &crate::value::TaskData, args: &[Value]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        // Hash each argument's display representation
        for arg in args {
            arg.display_string().hash(&mut hasher);
        }
        // Hash tracked environment variables (name + current value)
        for key in &task.env_keys {
            key.hash(&mut hasher);
            match std::env::var(key) {
                Ok(val) => { true.hash(&mut hasher); val.hash(&mut hasher); }
                Err(_)  => { false.hash(&mut hasher); }
            }
        }
        hasher.finish()
    }

    /// Check whether a string looks like a glob pattern (contains `*`, `?`, or `[`).
    /// Used to auto-detect globs in task `inputs:` / `outputs:` string literals.
    fn is_glob_pattern(s: &str) -> bool {
        s.contains('*') || s.contains('?') || s.contains('[')
    }

    /// Reject a wildcard in `outputs:`.
    ///
    /// A pattern there cannot be matched — the files it describes do not exist
    /// until the task has run — so it is kept as written, and a literal `*`
    /// never names a file. The freshness check would see an output that is
    /// always missing and rerun the task forever, which looks like the cache
    /// not working rather than like a mistake in the declaration.
    ///
    /// Only `*` and `?` are refused. A `[` is a glob metacharacter too, but it
    /// is also a legal character in a filename, and an output that names a real
    /// file with a bracket in it works today.
    fn reject_output_pattern(s: &str) -> Result<(), Signal> {
        if !s.contains('*') && !s.contains('?') {
            return Ok(());
        }
        Err(Signal::Error(QueError::new(
            ErrorKind::Runtime,
            format!(
                "task output '{}' is a pattern, but @outputs must name concrete \
                 paths: the files do not exist yet when the check runs, so a \
                 pattern would never match and the task could never be skipped. \
                 Name the directory, or a stamp file the task writes last.",
                s
            ),
        )))
    }

    /// Flatten one `inputs:` / `outputs:` value into file paths.
    ///
    /// `expand_globs` is off for outputs: a pattern there describes files the
    /// task is about to create, so matching it against what exists now would
    /// silently declare no outputs at all on the first run. A pattern written
    /// there anyway is an error rather than a path that can never match.
    fn collect_task_paths(
        value: Value,
        expand_globs: bool,
        out: &mut Vec<String>,
    ) -> Result<(), Signal> {
        let expand = |pattern: &str, out: &mut Vec<String>| {
            if let Ok(entries) = glob::glob(pattern) {
                for entry in entries.flatten() {
                    out.push(entry.to_string_lossy().to_string());
                }
            }
        };
        match value {
            // A Path carrying a pattern is treated like a String one. Rooting a
            // pattern at the project — `quefile_dir() / "src/*.txt"` — is the
            // natural way to write it, and leaving it unexpanded would leave the
            // task with an input path that never exists, so nothing ever looks
            // stale and it skips forever.
            Value::Path(p) | Value::String(p) => {
                if Self::is_glob_pattern(&p) {
                    if expand_globs {
                        expand(&p, out);
                        return Ok(());
                    }
                    Self::reject_output_pattern(&p)?;
                }
                out.push(p);
            }
            // An explicit `g"..."` says "pattern" out loud, so it is refused in
            // outputs whatever characters it happens to contain.
            Value::Glob(pattern) => {
                if expand_globs {
                    expand(&pattern, out);
                } else {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        format!(
                            "task output '{}' is a glob, but @outputs must name \
                             concrete paths: the files do not exist yet when the \
                             check runs, so a pattern would never match and the \
                             task could never be skipped. Name the directory, or \
                             a stamp file the task writes last.",
                            pattern
                        ),
                    )));
                }
            }
            Value::List(items) => {
                for item in items {
                    Self::collect_task_paths(item, expand_globs, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Evaluate a list of path expressions to concrete paths.
    fn eval_task_paths(
        &mut self,
        exprs: &[crate::ast::Expr],
        expand_globs: bool,
    ) -> Result<Vec<String>, Signal> {
        let mut paths = Vec::new();
        for expr in exprs {
            let val = self.eval_expr(expr)?;
            Self::collect_task_paths(val, expand_globs, &mut paths)?;
        }
        Ok(paths)
    }

    /// Directory holding the cache file: next to the script being run, unless
    /// a caller pinned it elsewhere with `set_task_cache_dir`.
    ///
    /// `None` for source evaluated from memory (the REPL, embedding, tests).
    /// There is no project directory to anchor a cache to, and writing one into
    /// whatever the current directory happens to be would be a surprise.
    fn task_cache_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(dir) = &self.task_cache_dir_override {
            return Some(dir.clone());
        }
        self.script_path
            .as_ref()?
            .parent()
            .map(|p| p.to_path_buf())
    }

    /// Content-hash a set of paths, dropping the ones that cannot be read.
    fn hash_paths(paths: &[String]) -> std::collections::BTreeMap<String, String> {
        paths
            .iter()
            .filter_map(|p| {
                super::task_cache::hash_file(std::path::Path::new(p)).map(|h| (p.clone(), h))
            })
            .collect()
    }

    /// Store what this task consumed and produced, so the next run can tell
    /// whether the files really changed or only their timestamps did.
    fn record_task_result(&mut self, task: &crate::value::TaskData, args: &[Value]) {
        // A dry run did not produce the outputs, so recording them would make
        // the next real run skip work it never did.
        if task.outputs.is_empty() || self.dry_run {
            return;
        }
        let Some(dir) = self.task_cache_dir() else {
            return;
        };
        self.task_cache_file.load_from(&dir);

        let inputs = self.eval_task_paths(&task.inputs.clone(), true).unwrap_or_default();
        let outputs = self.eval_task_paths(&task.outputs.clone(), false).unwrap_or_default();
        let entry = super::task_cache::TaskEntry {
            args_hash: self.compute_task_hash(task, args),
            inputs: Self::hash_paths(&inputs),
            outputs: Self::hash_paths(&outputs),
        };
        self.task_cache_file.record(&task.name, entry);
    }

    /// Determine if a task can be skipped.
    ///
    /// Three questions, cheapest first:
    ///
    /// 1. Do all declared outputs exist, and do the arguments and tracked
    ///    environment variables match the last successful run?
    /// 2. Is every input older than every output? If so nothing changed and
    ///    no file is read.
    /// 3. Otherwise, do the inputs and outputs still hash to what the last
    ///    successful run recorded? A timestamp says when a file was written,
    ///    not whether the bytes differ, and a checkout, a restored CI cache or
    ///    a `touch` rewrites the former without touching the latter.
    fn should_skip_task(&mut self, task: &crate::value::TaskData, args: &[Value]) -> Result<bool, Signal> {
        // No outputs declared → always run (side-effect-only task)
        if task.outputs.is_empty() {
            return Ok(false);
        }

        let dir = self.task_cache_dir();
        if let Some(dir) = &dir {
            self.task_cache_file.load_from(dir);
        }

        let output_paths = self.eval_task_paths(&task.outputs.clone(), false)?;

        // Asked for explicitly, so no evidence on disk gets to argue.
        //
        // The outputs are still evaluated first: a malformed declaration is an
        // error under `--force` exactly as it is without one, since a check
        // that a flag can switch off is a check nobody finds out they failed.
        //
        // Dependencies see the same flag, since they run through this function
        // too — forcing a task forces what it is built on.
        if self.force_run {
            return Ok(false);
        }

        // If any output doesn't exist, must run.
        let mut oldest_output = std::time::SystemTime::UNIX_EPOCH;
        let mut first = true;
        for path in &output_paths {
            match std::fs::metadata(path) {
                Ok(meta) => {
                    if let Ok(modified) = meta.modified() {
                        if first || modified < oldest_output {
                            oldest_output = modified;
                            first = false;
                        }
                    }
                }
                Err(_) => return Ok(false), // Output missing → must run
            }
        }

        // Arguments and tracked env vars. The recorded hash comes from the
        // cache file when there is one, so a task with parameters can still be
        // skipped across two separate invocations of `que` — which is every
        // invocation in CI.
        let current_hash = self.compute_task_hash(task, args);
        let recorded_hash = self
            .task_cache
            .get(&task.name)
            .copied()
            .or_else(|| self.task_cache_file.get(&task.name).map(|e| e.args_hash));
        match recorded_hash {
            Some(hash) if hash != current_hash => return Ok(false),
            None if !task.params.is_empty() || !task.env_keys.is_empty() => return Ok(false),
            _ => {}
        }

        // No file inputs declared but outputs exist and hash matches → skip
        if task.inputs.is_empty() {
            return Ok(true);
        }

        let input_paths = self.eval_task_paths(&task.inputs.clone(), true)?;

        // Fast path: every input strictly older than every output means
        // nothing has changed, and we get there without reading a single byte.
        //
        // Equal timestamps count as suspect. Filesystem timestamps are coarse
        // — often one second — so an edit made moments after a build lands on
        // the same tick as the artifact it invalidates. Timestamps cannot tell
        // those apart; contents can.
        let mut suspect = false;
        for path in &input_paths {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(modified) = meta.modified() {
                    if modified >= oldest_output {
                        suspect = true;
                        break;
                    }
                }
            }
            // If input doesn't exist, that's not our problem here (the task body will fail)
        }
        if !suspect {
            return Ok(true);
        }

        // A timestamp says the task may be stale. Ask the contents instead.
        let Some(entry) = self.task_cache_file.get(&task.name) else {
            return Ok(false); // Nothing recorded to compare against → run.
        };
        if entry.inputs != Self::hash_paths(&input_paths) {
            return Ok(false);
        }
        // Outputs are compared too. Getting here means the timestamps are
        // ambiguous, and an artifact that is no longer the one this task
        // produced has to be rebuilt even if every input matches.
        if entry.outputs != Self::hash_paths(&output_paths) {
            return Ok(false);
        }

        Ok(true)
    }

}
