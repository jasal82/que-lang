//! Modular std library dispatch for the Que interpreter.
//!
//! Each std module (fs, http, json, …) lives in its own file and exports:
//! - A `module()` function returning a `StdModule` descriptor
//! - A `call_X(&mut self, func, args)` method on `Interpreter`

mod archive;
mod config;
mod container;
mod csv;
mod dotenv;
pub(crate) mod fs;
mod git;
mod hash;
mod http;
mod json;
mod log;
mod net;
mod prompt;
mod reflect;
mod ssh;
mod stream;
mod template;
pub(crate) mod time;
mod toml;
pub(crate) mod tty;
mod watch;
mod yaml;

use super::Interpreter;
use crate::error::*;
use crate::value::Value;

/// Descriptor for a std module: its name and the bare function names it provides.
/// The fully-qualified name is constructed as `"{name}.{fn_name}"`.
pub(crate) struct StdModule {
    pub name: &'static str,
    pub functions: &'static [&'static str],
}

/// Return descriptors for all std modules.
pub(crate) fn all_modules() -> Vec<StdModule> {
    vec![
        fs::module(),
        http::module(),
        json::module(),
        yaml::module(),
        toml::module(),
        hash::module(),
        csv::module(),
        dotenv::module(),
        log::module(),
        git::module(),
        archive::module(),
        template::module(),
        net::module(),
        time::module(),
        tty::module(),
        prompt::module(),
        ssh::module(),
        container::module(),
        watch::module(),
        config::module(),
        stream::module(),
        reflect::module(),
    ]
}

impl Interpreter {
    /// Try dispatching a builtin call as a std module function.
    ///
    /// `name` is the fully-qualified name stored in `Value::BuiltinFn`
    /// (e.g. `"hash.sha256"`). Returns `None` if `name` is not a std module
    /// function, allowing the caller to fall through to global builtins.
    pub(crate) fn call_std_builtin(&mut self, name: &str, args: &[Value]) -> Option<IResult> {
        let (module, func) = name.split_once('.')?;
        if !is_std_module(module) {
            return None;
        }
        let effects = std_effects(module, func, args);
        // One enforcement point for every std function. Checking here rather
        // than inside each module means a new function is covered the day it
        // is written, and `crate::permissions::std_effect` is the single
        // place to read to know what a script is allowed to do.
        if self.permissions.is_some() {
            for &(cap, arg) in &effects {
                for subject in subjects(arg, args, name) {
                    if let Err(e) = self.check_permission(cap, &subject) {
                        return Some(Err(e));
                    }
                }
            }
        }
        if let Err(e) = self.refuse_global_std(module, func) {
            return Some(Err(e));
        }
        let result = self.dispatch_std(module, func, args);
        // Traced from the same classification that was just enforced, so the
        // record of what a script touched can't drift from what it was
        // allowed to touch.
        if self.host.tracer.is_some() {
            let outcome = outcome_of(&result);
            for &(cap, arg) in &effects {
                let Some(kind) = trace_kind(module, func, cap) else {
                    continue;
                };
                // Without a path argument there is nothing on disk to name.
                if arg.is_none() && kind != crate::embed::AccessKind::Exec {
                    continue;
                }
                for subject in subjects(arg, args, name) {
                    self.trace(kind, name, &subject, outcome.clone());
                }
            }
        }
        Some(result)
    }

    /// Std functions that change process-global state or read the terminal
    /// are unavailable in embedded mode.
    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    fn refuse_global_std(&self, module: &str, func: &str) -> Result<(), Signal> {
        match (module, func) {
            ("dotenv", "load" | "load_overwrite") => self.host.refuse_global(&format!(
                "dotenv.{}() (setting environment variables)",
                func
            )),
            ("prompt", _) => self
                .host
                .refuse_global(&format!("prompt.{}() (reading the terminal)", func)),
            _ => Ok(()),
        }
    }

    #[allow(clippy::result_large_err)] // `Signal` is the interpreter's error type (see `IResult`)
    fn dispatch_std(&mut self, module: &str, func: &str, args: &[Value]) -> IResult {
        match module {
            "fs" => self.call_fs(func, args),
            "http" => self.call_http(func, args),
            "json" => self.call_json(func, args),
            "yaml" => self.call_yaml(func, args),
            "toml" => self.call_toml(func, args),
            "hash" => self.call_hash(func, args),
            "csv" => self.call_csv(func, args),
            "dotenv" => self.call_dotenv(func, args),
            "log" => self.call_log(func, args),
            "git" => self.call_git(func, args),
            "archive" => self.call_archive(func, args),
            "template" => self.call_template(func, args),
            "net" => self.call_net(func, args),
            "time" => self.call_time(func, args),
            "tty" => self.call_tty(func, args),
            "prompt" => self.call_prompt(func, args),
            "ssh" => self.call_ssh(func, args),
            "container" => self.call_container(func, args),
            "watch" => self.call_watch(func, args),
            "config" => self.call_config(func, args),
            "stream" => self.call_stream(func, args),
            "reflect" => self.call_reflect(func, args),
            // Unreachable: `is_std_module` was checked by the caller.
            _ => Err(Signal::Error(QueError::runtime(format!(
                "no std module '{}'",
                module
            )))),
        }
    }
}

fn is_std_module(module: &str) -> bool {
    all_modules().iter().any(|m| m.name == module)
}

/// Every capability `<module>.<func>(args)` needs, with the argument that
/// names its subject.
fn std_effects(module: &str, func: &str, args: &[Value]) -> Vec<crate::permissions::Effect> {
    [
        crate::permissions::std_effect(module, func),
        crate::permissions::std_extra_effect(module, func),
        crate::permissions::std_dynamic_effect(module, func, args),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The subjects an effect is checked (and traced) against: the argument's
/// text, each element of a list argument, or the function's own name when
/// no argument names the subject.
fn subjects(arg: Option<usize>, args: &[Value], name: &str) -> Vec<String> {
    match arg.and_then(|i| args.get(i)) {
        Some(Value::List(items)) => items.iter().map(Value::display_string).collect(),
        Some(v) => vec![v.display_string()],
        None => vec![name.to_string()],
    }
}

/// The access kind to trace for one effect of a std function.
fn trace_kind(
    module: &str,
    func: &str,
    cap: crate::permissions::Capability,
) -> Option<crate::embed::AccessKind> {
    use crate::embed::AccessKind;
    use crate::permissions::Capability;
    Some(match (cap, module, func) {
        (Capability::Read, "fs", "exists" | "is_file" | "is_dir") => AccessKind::Exists,
        (Capability::Read, "fs", "list" | "list_dir") => AccessKind::ListDir,
        (Capability::Read, "fs", "glob") => AccessKind::Glob,
        (Capability::Read, "fs", "find" | "walk" | "copy_dir") => AccessKind::Walk,
        (Capability::Read, "watch", _) => AccessKind::Walk,
        (Capability::Read, _, _) => AccessKind::Read,
        // A temporary file's argument is a name prefix, not its path.
        (Capability::Write, "fs", "temp_file" | "temp_dir") => return None,
        (Capability::Write, "fs", "remove" | "remove_all" | "remove_dir" | "rm") => {
            AccessKind::Remove
        }
        (Capability::Write, "fs", "mkdir" | "mkdir_all") => AccessKind::CreateDir,
        (Capability::Write, _, _) => AccessKind::Write,
        (Capability::Exec, _, _) => AccessKind::Exec,
        (Capability::Net | Capability::Env, _, _) => return None,
    })
}

/// Success or failure of a call, as recorded in an access event. A function
/// that returns `Err(...)` failed as much as one that raised.
pub(crate) fn outcome_of(result: &IResult) -> Result<(), String> {
    match result {
        Ok(Value::Err(payload)) => Err(payload.display_string()),
        Ok(_) | Err(Signal::Return(_)) => Ok(()),
        Err(Signal::Error(e)) => Err(e.message.clone()),
        Err(other) => Err(format!("{:?}", other)),
    }
}
