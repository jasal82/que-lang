//! std.container module — Build, run and inspect OCI containers.
//!
//! Drives the `docker` or `podman` CLI rather than talking to a daemon
//! socket. The CLI is where credential helpers, `DOCKER_HOST`, contexts,
//! rootless configuration, buildx builders and registry auth already live; a
//! direct socket client reimplements all of it and works in fewer places.
//!
//! Named `container` rather than `docker` because podman is a drop-in for
//! everything here and pretending otherwise would force every podman user to
//! write `docker` in their scripts.

use crate::error::*;
use crate::value::{CmdModifiers, CmdPart, Value};
use super::super::Interpreter;
use super::StdModule;
use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "container",
        functions: &[
            "engine",
            "build",
            "run",
            "exec",
            "stop",
            "remove",
            "logs",
            "exists",
            "is_running",
            "wait_healthy",
            "pull",
            "push",
            "login",
        ],
    }
}

/// The container CLI to use.
///
/// `$QUE_CONTAINER_ENGINE` wins, then docker, then podman, then nerdctl. The
/// PATH probe is cached because it cannot change mid-run and every call
/// consults it; the environment variable is not, so a script can override the
/// choice with `env.set` before the first container call.
fn detect_engine() -> Option<&'static str> {
    if let Ok(name) = std::env::var("QUE_CONTAINER_ENGINE") {
        return match name.as_str() {
            "docker" => Some("docker"),
            "podman" => Some("podman"),
            "nerdctl" => Some("nerdctl"),
            _ => None,
        };
    }
    static PROBED: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *PROBED.get_or_init(|| {
        ["docker", "podman", "nerdctl"]
            .into_iter()
            .find(|c| which_exists(c))
    })
}

fn which_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
        })
        .unwrap_or(false)
}

fn engine_or_err() -> Result<&'static str, Signal> {
    detect_engine().ok_or_else(|| {
        sig_err(
            "no container engine found: install docker or podman, or set \
             $QUE_CONTAINER_ENGINE",
        )
    })
}

impl Interpreter {
    pub(crate) fn call_container(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "engine" => Ok(match detect_engine() {
                Some(e) => Value::String(e.to_string()),
                None => Value::Null,
            }),

            "build" => {
                let opts = map_arg(args.first(), "container.build")?;
                let tag = match opts.get("tag") {
                    Some(v) if !matches!(v, Value::Null) => v.display_string(),
                    _ => return Err(sig_err("container.build requires a `tag`")),
                };
                let mut b = CmdBuilder::new(engine_or_err()?, "build");
                b.flag_value("-t", &tag);
                if let Some(v) = opts.get("file") {
                    if !matches!(v, Value::Null) {
                        b.flag_value("-f", &v.display_string());
                    }
                }
                for (k, v) in map_of(opts.get("build_args")) {
                    b.flag_value("--build-arg", &format!("{}={}", k, v.display_string()));
                }
                for p in list_of(opts.get("platform")) {
                    b.flag_value("--platform", &p.display_string());
                }
                for t in list_of(opts.get("targets")) {
                    b.flag_value("--target", &t.display_string());
                }
                if opts.get("cache") == Some(&Value::Bool(false)) {
                    b.flag("--no-cache");
                }
                if opts.get("pull") == Some(&Value::Bool(true)) {
                    b.flag("--pull");
                }
                b.extra(opts.get("args"));
                // The context is positional and must come last.
                let context = opts
                    .get("context")
                    .filter(|v| !matches!(v, Value::Null))
                    .map(|v| v.display_string())
                    .unwrap_or_else(|| ".".to_string());
                b.value(&context);
                self.finish(b, Value::String(tag))
            }

            "run" => {
                let opts = map_arg(args.first(), "container.run")?;
                let image = match opts.get("image") {
                    Some(v) if !matches!(v, Value::Null) => v.display_string(),
                    _ => return Err(sig_err("container.run requires an `image`")),
                };
                let detach = opts.get("detach") != Some(&Value::Bool(false));
                // `tty` hands the terminal to the container, which only means
                // anything while the caller is waiting on it. Detaching would
                // silently throw away the thing that was asked for, so say so.
                let tty = opts.get("tty") == Some(&Value::Bool(true));
                if tty && opts.get("detach") == Some(&Value::Bool(true)) {
                    return Err(sig_err(
                        "container.run: `tty` needs a foreground container, so it cannot be combined with `detach: true`",
                    ));
                }
                let detach = detach && !tty;
                let mut b = CmdBuilder::new(engine_or_err()?, "run");
                if detach {
                    b.flag("-d");
                }
                if tty {
                    b.flag("-it");
                }
                // `--rm` defaults on. A script that leaves stopped containers
                // behind fills a CI runner's disk over a few hundred builds,
                // and nothing in the script points at the cause.
                if opts.get("remove") != Some(&Value::Bool(false)) {
                    b.flag("--rm");
                }
                if let Some(v) = opts.get("name") {
                    if !matches!(v, Value::Null) {
                        b.flag_value("--name", &v.display_string());
                    }
                }
                for (host, guest) in map_of(opts.get("ports")) {
                    b.flag_value("-p", &format!("{}:{}", host, guest.display_string()));
                }
                for (local, mount) in map_of(opts.get("volumes")) {
                    b.flag_value("-v", &format!("{}:{}", local, mount.display_string()));
                }
                for (k, v) in map_of(opts.get("env")) {
                    match v {
                        // A secret must not reach argv, where any user on the
                        // host can read it out of `ps`. Passing only the name
                        // and setting the value in the child's environment
                        // gets it to the container without that exposure.
                        Value::Secret(plain) => {
                            b.flag_value("-e", &k);
                            b.child_env(k.clone(), plain.clone());
                        }
                        other => {
                            b.flag_value("-e", &format!("{}={}", k, other.display_string()))
                        }
                    }
                }
                if let Some(v) = opts.get("workdir") {
                    if !matches!(v, Value::Null) {
                        b.flag_value("-w", &v.display_string());
                    }
                }
                if let Some(v) = opts.get("user") {
                    if !matches!(v, Value::Null) {
                        b.flag_value("-u", &v.display_string());
                    }
                }
                if opts.get("network") .is_some_and(|v| !matches!(v, Value::Null)) {
                    b.flag_value("--network", &opts["network"].display_string());
                }
                b.extra(opts.get("args"));
                b.value(&image);
                if let Some(v) = opts.get("command") {
                    match v {
                        Value::Null => {}
                        Value::List(items) => {
                            for item in items {
                                b.value(&item.display_string());
                            }
                        }
                        Value::Cmd(parts, _) => {
                            b.raw(&crate::interpreter::methods::render_cmd(parts))
                        }
                        other => b.raw(&other.display_string()),
                    }
                }

                // A dry run starts nothing, and a `tty` run has already handed
                // its output to the terminal, so in neither case is there an
                // id to report. The name is then the only handle a following
                // `container.stop(...)` could use; without one there is
                // nothing to name at all.
                let named = opts
                    .get("name")
                    .filter(|v| !matches!(v, Value::Null))
                    .map(|v| v.display_string());
                match self.run_builder_attached(b, tty)? {
                    Outcome::DryRun => Ok(Value::Ok(Box::new(Value::String(
                        named.unwrap_or_else(|| "<dry-run>".to_string()),
                    )))),
                    Outcome::Failed(msg) => Ok(Value::Err(Box::new(Value::String(msg)))),
                    // Detached: stdout is the container id, which is the
                    // handle every other function in this module accepts.
                    // Attached: stdout is whatever the container printed,
                    // which is what the caller wanted in that case.
                    Outcome::Ok(_) if tty => {
                        Ok(Value::Ok(Box::new(Value::String(named.unwrap_or_default()))))
                    }
                    Outcome::Ok(stdout) => {
                        Ok(Value::Ok(Box::new(Value::String(stdout.trim().to_string()))))
                    }
                }
            }

            "exec" => {
                let name = string_arg(args.first(), "container.exec", "container name")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "exec");
                let opts = args.get(2).and_then(as_map);
                let tty = opts
                    .as_ref()
                    .is_some_and(|m| m.get("tty") == Some(&Value::Bool(true)));
                if tty {
                    b.flag("-it");
                }
                if let Some(m) = &opts {
                    if let Some(v) = m.get("user") {
                        if !matches!(v, Value::Null) {
                            b.flag_value("-u", &v.display_string());
                        }
                    }
                    if let Some(v) = m.get("workdir") {
                        if !matches!(v, Value::Null) {
                            b.flag_value("-w", &v.display_string());
                        }
                    }
                }
                b.value(&name);
                match args.get(1) {
                    Some(Value::List(items)) => {
                        for item in items {
                            b.value(&item.display_string());
                        }
                    }
                    // A string or a backtick command is shell syntax, so it
                    // needs a shell inside the container to interpret it.
                    Some(Value::String(s)) => {
                        b.value("sh");
                        b.value("-c");
                        b.value(s);
                    }
                    Some(Value::Cmd(parts, _)) => {
                        b.value("sh");
                        b.value("-c");
                        b.value(&crate::interpreter::methods::render_cmd(parts));
                    }
                    _ => return Err(sig_err("container.exec requires a command")),
                }
                let (parts, mut mods) = b.build();
                mods.attach = tty;
                match self.run_cmd_parts(&parts, &mods)? {
                    result @ Value::ProcessResult { .. } => Ok(Value::Ok(Box::new(result))),
                    other => Ok(Value::Ok(Box::new(other))),
                }
            }

            "stop" => {
                let name = string_arg(args.first(), "container.stop", "container name")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "stop");
                if let Some(Value::Int(secs)) = args.get(1) {
                    b.flag_value("-t", &secs.to_string());
                }
                b.value(&name);
                self.finish(b, Value::Null)
            }

            "remove" => {
                let name = string_arg(args.first(), "container.remove", "container name")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "rm");
                b.flag("-f");
                b.value(&name);
                self.finish(b, Value::Null)
            }

            "logs" => {
                let name = string_arg(args.first(), "container.logs", "container name")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "logs");
                if let Some(Value::Int(n)) = args.get(1) {
                    b.flag_value("--tail", &n.to_string());
                }
                b.value(&name);
                let (parts, mods) = b.build();
                match self.run_cmd_parts(&parts, &mods)? {
                    Value::ProcessResult { exit_code, stdout, stderr } => {
                        if exit_code == 0 {
                            // Container logs arrive on both streams; a caller
                            // asking for "the logs" wants both.
                            Ok(Value::Ok(Box::new(Value::String(format!(
                                "{}{}", stdout, stderr
                            )))))
                        } else {
                            Ok(Value::Err(Box::new(Value::String(stderr.trim().to_string()))))
                        }
                    }
                    other => Ok(Value::Ok(Box::new(other))),
                }
            }

            "exists" => {
                let name = string_arg(args.first(), "container.exists", "container name")?;
                Ok(Value::Bool(self.inspect(&name, "{{.Id}}")?.is_some()))
            }

            "is_running" => {
                let name = string_arg(args.first(), "container.is_running", "container name")?;
                Ok(Value::Bool(
                    self.inspect(&name, "{{.State.Running}}")?.as_deref() == Some("true"),
                ))
            }

            "wait_healthy" => {
                let name = string_arg(args.first(), "container.wait_healthy", "container name")?;
                let timeout_ms = match args.get(1) {
                    Some(Value::Duration(v, u)) => {
                        crate::interpreter::helpers::duration_to_ms(*v, *u) as u64
                    }
                    Some(Value::Int(ms)) => *ms as u64,
                    _ => 30_000,
                };
                self.wait_healthy(&name, timeout_ms)
            }

            "pull" => {
                let image = string_arg(args.first(), "container.pull", "image")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "pull");
                b.value(&image);
                self.finish(b, Value::String(image.clone()))
            }

            "push" => {
                let image = string_arg(args.first(), "container.push", "image")?;
                let mut b = CmdBuilder::new(engine_or_err()?, "push");
                b.value(&image);
                self.finish(b, Value::String(image.clone()))
            }

            "login" => {
                let registry = string_arg(args.first(), "container.login", "registry")?;
                let user = string_arg(args.get(1), "container.login", "username")?;
                let password = match args.get(2) {
                    Some(Value::Secret(s)) => s.clone(),
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_err("container.login requires a password or Secret")),
                };
                let mut b = CmdBuilder::new(engine_or_err()?, "login");
                b.flag_value("-u", &user);
                // --password-stdin, never --password. A password on argv is
                // readable by every user on the host through `ps`, and both
                // docker and podman print a warning saying so.
                b.flag("--password-stdin");
                b.value(&registry);
                let (parts, mut mods) = b.build();
                mods.stdin_data = Some(password);
                if self.dry_run_skip(format!(
                    "{} login -u {} --password-stdin {}",
                    engine_or_err()?,
                    user,
                    registry
                )) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match self.run_cmd_parts(&parts, &mods)? {
                    Value::ProcessResult { exit_code, stderr, .. } if exit_code != 0 => {
                        Ok(Value::Err(Box::new(Value::String(stderr.trim().to_string()))))
                    }
                    _ => Ok(Value::Ok(Box::new(Value::Null))),
                }
            }

            _ => Err(sig_err(format!("unknown function 'container.{}'", func))),
        }
    }

    /// `docker inspect -f <format> <name>`, or `None` if there is no such
    /// container.
    fn inspect(&mut self, name: &str, format: &str) -> Result<Option<String>, Signal> {
        let engine = engine_or_err()?;
        let mut b = CmdBuilder::new(engine, "inspect");
        b.flag_value("-f", format);
        b.value(name);
        let (parts, mods) = b.build();
        // Inspection is read-only, so it must still answer during a dry run
        // or every `if container.exists(...)` would take the wrong branch.
        let was_dry = std::mem::replace(&mut self.dry_run, false);
        let result = self.run_cmd_parts(&parts, &mods);
        self.dry_run = was_dry;
        match result? {
            Value::ProcessResult { exit_code, stdout, .. } if exit_code == 0 => {
                Ok(Some(stdout.trim().to_string()))
            }
            _ => Ok(None),
        }
    }

    fn wait_healthy(&mut self, name: &str, timeout_ms: u64) -> IResult {
        if self.dry_run {
            return Ok(Value::Ok(Box::new(Value::Null)));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            self.check_interrupt()?;
            match self.inspect(name, "{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}")? {
                None => {
                    return Ok(Value::Err(Box::new(Value::String(format!(
                        "container '{}' does not exist",
                        name
                    )))))
                }
                Some(status) => match status.as_str() {
                    "healthy" => return Ok(Value::Ok(Box::new(Value::Null))),
                    // An image with no HEALTHCHECK reports nothing at all. A
                    // naive loop would then block until the timeout on a
                    // container that started perfectly, so fall back to
                    // "is it running" — the strongest claim available.
                    "none" => {
                        if self.inspect(name, "{{.State.Running}}")?.as_deref() == Some("true") {
                            return Ok(Value::Ok(Box::new(Value::Null)));
                        }
                    }
                    // Docker gives up after the configured retries; waiting
                    // out our own timeout after that tells us nothing new.
                    "unhealthy" => {
                        return Ok(Value::Err(Box::new(Value::String(format!(
                            "container '{}' reported unhealthy",
                            name
                        )))))
                    }
                    _ => {}
                },
            }
            if std::time::Instant::now() >= deadline {
                return Ok(Value::Err(Box::new(Value::String(format!(
                    "container '{}' was not healthy within {}ms",
                    name, timeout_ms
                )))));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    /// Run a builder and map the outcome onto `Ok(success)` / `Err(stderr)`.
    fn finish(&mut self, b: CmdBuilder, success: Value) -> IResult {
        match self.run_builder(b)? {
            Outcome::DryRun | Outcome::Ok(_) => Ok(Value::Ok(Box::new(success))),
            Outcome::Failed(msg) => Ok(Value::Err(Box::new(Value::String(msg)))),
        }
    }

    fn run_builder(&mut self, b: CmdBuilder) -> Result<Outcome, Signal> {
        self.run_builder_attached(b, false)
    }

    /// `attach` gives the container the terminal, for `-it`. Nothing is
    /// captured then, so `Outcome::Ok` carries an empty string and a failure
    /// can only report the exit code.
    fn run_builder_attached(&mut self, b: CmdBuilder, attach: bool) -> Result<Outcome, Signal> {
        let text = b.display();
        let (parts, mut mods) = b.build();
        mods.attach = attach;
        // Deliberately not `mods.silent`: that discards the captured streams
        // rather than merely suppressing the echo, and every caller here
        // needs stdout (the container id) or stderr (the failure message).
        // Nothing is echoed anyway unless a forward sink is set, and none is.
        if self.dry_run_skip(text) {
            return Ok(Outcome::DryRun);
        }
        match self.run_cmd_parts(&parts, &mods)? {
            Value::ProcessResult { exit_code, stdout, stderr } => {
                if exit_code == 0 {
                    Ok(Outcome::Ok(stdout))
                } else {
                    let msg = stderr.trim();
                    Ok(Outcome::Failed(if msg.is_empty() {
                        format!("exited {}", exit_code)
                    } else {
                        msg.to_string()
                    }))
                }
            }
            _ => Ok(Outcome::Ok(String::new())),
        }
    }
}

enum Outcome {
    DryRun,
    Ok(String),
    Failed(String),
}

/// Assembles an engine invocation out of escaped pieces.
///
/// Values go in as `Interpolated` so they are shell-escaped: an image tag or
/// a volume path with a space in it is a normal thing, not an injection.
struct CmdBuilder {
    parts: Vec<CmdPart>,
    env: Vec<(String, String)>,
}

impl CmdBuilder {
    fn new(engine: &str, subcommand: &str) -> Self {
        CmdBuilder {
            parts: vec![CmdPart::Literal(format!("{} {}", engine, subcommand))],
            env: Vec::new(),
        }
    }
    fn flag(&mut self, f: &str) {
        self.parts.push(CmdPart::Literal(format!(" {}", f)));
    }
    fn flag_value(&mut self, f: &str, v: &str) {
        self.parts.push(CmdPart::Literal(format!(" {} ", f)));
        self.parts.push(CmdPart::Interpolated(v.to_string()));
    }
    fn value(&mut self, v: &str) {
        self.parts.push(CmdPart::Literal(" ".to_string()));
        self.parts.push(CmdPart::Interpolated(v.to_string()));
    }
    /// Text that is deliberately shell syntax (a container's command line).
    fn raw(&mut self, v: &str) {
        self.parts.push(CmdPart::Literal(format!(" {}", v)));
    }
    fn extra(&mut self, v: Option<&Value>) {
        for item in list_of(v) {
            self.value(&item.display_string());
        }
    }
    fn child_env(&mut self, k: String, v: String) {
        self.env.push((k, v));
    }
    fn display(&self) -> String {
        crate::interpreter::methods::render_cmd_display(&self.parts)
    }
    fn build(self) -> (Vec<CmdPart>, CmdModifiers) {
        let mods = CmdModifiers {
            env_vars: self.env,
            ..CmdModifiers::default()
        };
        (self.parts, mods)
    }
}

fn as_map(v: &Value) -> Option<BTreeMap<String, Value>> {
    match v {
        Value::Map(m) => Some(m.clone()),
        _ => None,
    }
}

fn map_arg(v: Option<&Value>, who: &str) -> Result<BTreeMap<String, Value>, Signal> {
    match v {
        Some(Value::Map(m)) => Ok(m.clone()),
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{} takes an options Map, got {}", who, other.type_name()),
        ))),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{} requires an options Map", who),
        ))),
    }
}

fn map_of(v: Option<&Value>) -> Vec<(String, Value)> {
    match v {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Vec::new(),
    }
}

fn list_of(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::List(items)) => items.clone(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.clone()],
    }
}

fn string_arg(v: Option<&Value>, who: &str, what: &str) -> Result<String, Signal> {
    match v {
        Some(Value::String(s)) | Some(Value::Path(s)) => Ok(s.clone()),
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{}: {} must be a String, got {}", who, what, other.type_name()),
        ))),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{} requires a {}", who, what),
        ))),
    }
}

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(b: CmdBuilder) -> String {
        crate::interpreter::methods::render_cmd(&b.build().0)
    }

    #[test]
    fn values_are_escaped_but_flags_are_not() {
        let mut b = CmdBuilder::new("docker", "run");
        b.flag("--rm");
        b.flag_value("-v", "/my dir:/data");
        assert_eq!(rendered(b), "docker run --rm -v '/my dir:/data'");
    }

    #[test]
    fn a_secret_env_value_never_reaches_argv() {
        // `ps` is readable by every user on the host.
        let mut b = CmdBuilder::new("docker", "run");
        b.flag_value("-e", "TOKEN");
        b.child_env("TOKEN".into(), "hunter2".into());
        let (parts, mods) = b.build();
        let text = crate::interpreter::methods::render_cmd(&parts);
        assert!(!text.contains("hunter2"), "{}", text);
        assert_eq!(mods.env_vars, vec![("TOKEN".to_string(), "hunter2".to_string())]);
    }

    #[test]
    fn the_engine_env_var_overrides_detection() {
        std::env::set_var("QUE_CONTAINER_ENGINE", "podman");
        assert_eq!(detect_engine(), Some("podman"));
        std::env::remove_var("QUE_CONTAINER_ENGINE");
    }
}
