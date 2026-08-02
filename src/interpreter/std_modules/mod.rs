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
mod tty;
mod watch;
mod yaml;

use crate::error::*;
use crate::value::Value;
use super::Interpreter;

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
        // One enforcement point for every std function. Checking here rather
        // than inside each module means a new function is covered the day it
        // is written, and `crate::permissions::std_effect` is the single
        // place to read to know what a script is allowed to do.
        if self.permissions.is_some() {
            let effects = [
                crate::permissions::std_effect(module, func),
                crate::permissions::std_extra_effect(module, func),
            ];
            for (cap, arg) in effects.into_iter().flatten() {
                let subject = match arg.and_then(|i| args.get(i)) {
                    Some(v) => v.display_string(),
                    None => name.to_string(),
                };
                if let Err(e) = self.check_permission(cap, &subject) {
                    return Some(Err(e));
                }
            }
        }
        match module {
            "fs"       => Some(self.call_fs(func, args)),
            "http"     => Some(self.call_http(func, args)),
            "json"     => Some(self.call_json(func, args)),
            "yaml"     => Some(self.call_yaml(func, args)),
            "toml"     => Some(self.call_toml(func, args)),
            "hash"     => Some(self.call_hash(func, args)),
            "csv"      => Some(self.call_csv(func, args)),
            "dotenv"   => Some(self.call_dotenv(func, args)),
            "log"      => Some(self.call_log(func, args)),
            "git"      => Some(self.call_git(func, args)),
            "archive"  => Some(self.call_archive(func, args)),
            "template" => Some(self.call_template(func, args)),
            "net"      => Some(self.call_net(func, args)),
            "time"     => Some(self.call_time(func, args)),
            "tty"      => Some(self.call_tty(func, args)),
            "prompt"   => Some(self.call_prompt(func, args)),
            "ssh"      => Some(self.call_ssh(func, args)),
            "container" => Some(self.call_container(func, args)),
            "watch"    => Some(self.call_watch(func, args)),
            "config"   => Some(self.call_config(func, args)),
            "stream"   => Some(self.call_stream(func, args)),
            "reflect"  => Some(self.call_reflect(func, args)),
            // os.exit is a global builtin, not a std module
            _          => None,
        }
    }
}
