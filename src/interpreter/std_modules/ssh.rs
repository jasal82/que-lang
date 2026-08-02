//! std.ssh module — Run commands and move files on remote hosts.
//!
//! Implemented by driving the `ssh` and `scp` binaries rather than by linking
//! an SSH library, for the same reason `std.git` drives the `git` binary
//! (see `src/manifest.rs`): the system client already knows how to use
//! ssh-agent, `~/.ssh/config`, `Match` blocks, `ProxyJump`, hardware keys,
//! GSSAPI, and whatever corporate `Include` file the security team ships.
//! A linked library starts from zero on every one of those, and the first
//! host someone cannot reach is the last time they use the feature.
//!
//! Everything here is built on top of `ssh.cmd`, which returns an ordinary
//! `Value::Cmd`. That means `.timeout()`, `.dir()`, `.silent()`, `.stdin()`,
//! pipelines and `--dry-run` all work on a remote command with no extra code,
//! and a remote command reads like a local one.

use crate::error::*;
use crate::value::{CmdModifiers, CmdPart, Value};
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "ssh",
        functions: &["cmd", "run", "out", "check", "upload", "download"],
    }
}

/// Everything that turns a host name into an `ssh` invocation.
struct SshOpts {
    user: Option<String>,
    port: Option<i64>,
    key: Option<String>,
    /// `-J`, for bastion hosts.
    jump: Option<String>,
    /// Seconds handed to `ConnectTimeout`.
    connect_timeout: Option<i64>,
    /// `-o BatchMode=yes` unless the caller asks for a prompt.
    interactive: bool,
    forward_agent: bool,
    strict_host_key_checking: bool,
    known_hosts: Option<String>,
    /// Raw `-o Key=Value` strings, for anything not modelled above.
    options: Vec<String>,
}

impl Default for SshOpts {
    fn default() -> Self {
        SshOpts {
            user: None,
            port: None,
            key: None,
            jump: None,
            connect_timeout: Some(10),
            interactive: false,
            forward_agent: false,
            // Host key checking stays on. Turning it off is a decision that
            // has to be written down in the script, not inherited from a
            // default nobody reads.
            strict_host_key_checking: true,
            known_hosts: None,
            options: Vec::new(),
        }
    }
}

impl Interpreter {
    pub(crate) fn call_ssh(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "cmd" => {
                let (host, command, opts) = host_cmd_opts(args, "ssh.cmd")?;
                Ok(self.ssh_command(&host, &command, &opts))
            }
            "run" => {
                let (host, command, opts) = host_cmd_opts(args, "ssh.run")?;
                let cmd = self.ssh_command(&host, &command, &opts);
                let (parts, mods) = match cmd {
                    Value::Cmd(p, m) => (p, m),
                    _ => unreachable!("ssh_command always builds a Cmd"),
                };
                match self.run_cmd_parts(&parts, &mods) {
                    Ok(result) => Ok(Value::Ok(Box::new(result))),
                    Err(Signal::Error(e)) => {
                        Ok(Value::Err(Box::new(Value::String(e.message))))
                    }
                    Err(other) => Err(other),
                }
            }
            "out" => {
                let (host, command, opts) = host_cmd_opts(args, "ssh.out")?;
                let cmd = self.ssh_command(&host, &command, &opts);
                let (parts, mods) = match cmd {
                    Value::Cmd(p, m) => (p, m),
                    _ => unreachable!(),
                };
                match self.run_cmd_parts(&parts, &mods)? {
                    Value::ProcessResult { exit_code, stdout, stderr } => {
                        if exit_code == 0 {
                            Ok(Value::Ok(Box::new(Value::String(
                                stdout.trim_end().to_string(),
                            ))))
                        } else {
                            Ok(Value::Err(Box::new(Value::String(format!(
                                "{} on {} exited {}: {}",
                                command,
                                host,
                                exit_code,
                                stderr.trim()
                            )))))
                        }
                    }
                    other => Ok(Value::Ok(Box::new(other))),
                }
            }
            "check" => {
                // Reachability, not a command result: a host that is down and
                // a host that refuses the key are both "cannot use this
                // host", which is the only question `check` is answering.
                let host = string_arg(args.first(), "ssh.check", "host")?;
                let opts = ssh_opts(args.get(1), "ssh.check")?;
                let cmd = self.ssh_command(&host, "true", &opts);
                let (parts, mut mods) = match cmd {
                    Value::Cmd(p, m) => (p, m),
                    _ => unreachable!(),
                };
                mods.silent = true;
                match self.run_cmd_parts(&parts, &mods) {
                    Ok(Value::ProcessResult { exit_code, .. }) => {
                        Ok(Value::Bool(exit_code == 0))
                    }
                    Ok(_) => Ok(Value::Bool(false)),
                    Err(Signal::Error(_)) => Ok(Value::Bool(false)),
                    Err(other) => Err(other),
                }
            }
            "upload" => {
                let local = path_arg(args.first(), "ssh.upload", "local path")?;
                let host = string_arg(args.get(1), "ssh.upload", "host")?;
                let remote = path_arg(args.get(2), "ssh.upload", "remote path")?;
                let opts = ssh_opts(args.get(3), "ssh.upload")?;
                if !std::path::Path::new(&local).exists() {
                    return Ok(Value::Err(Box::new(Value::String(format!(
                        "local path '{}' does not exist",
                        local
                    )))));
                }
                self.scp(
                    &opts,
                    &crate::interpreter::helpers::shell_escape(&local),
                    &format!("{}:{}", target(&host, &opts), shell_quote(&remote)),
                    Value::Path(remote),
                )
            }
            "download" => {
                let host = string_arg(args.first(), "ssh.download", "host")?;
                let remote = path_arg(args.get(1), "ssh.download", "remote path")?;
                let local = path_arg(args.get(2), "ssh.download", "local path")?;
                let opts = ssh_opts(args.get(3), "ssh.download")?;
                // scp will not create the destination directory, and the
                // error it gives for a missing one is unhelpfully generic.
                if let Some(parent) = std::path::Path::new(&local).parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            return Ok(Value::Err(Box::new(Value::String(format!(
                                "could not create '{}': {}",
                                parent.display(),
                                e
                            )))));
                        }
                    }
                }
                self.scp(
                    &opts,
                    &format!("{}:{}", target(&host, &opts), shell_quote(&remote)),
                    &crate::interpreter::helpers::shell_escape(&local),
                    Value::Path(local),
                )
            }
            _ => Err(sig_err(format!("unknown function 'ssh.{}'", func))),
        }
    }

    /// Build `ssh <flags> <target> <command>` as an ordinary `Cmd`.
    ///
    /// The remote command is single-quoted as one argument, so the *remote*
    /// shell sees exactly what was written and the local shell does not get
    /// a second chance to interpret it.
    fn ssh_command(&mut self, host: &str, command: &str, opts: &SshOpts) -> Value {
        let mut text = String::from("ssh");
        for flag in common_flags(opts) {
            text.push(' ');
            text.push_str(&flag);
        }
        if opts.forward_agent {
            text.push_str(" -A");
        }
        if let Some(port) = opts.port {
            text.push_str(&format!(" -p {}", port));
        }
        text.push(' ');
        text.push_str(&crate::interpreter::helpers::shell_escape(&target(host, opts)));
        text.push(' ');
        text.push_str(&shell_quote(command));
        Value::Cmd(
            vec![CmdPart::Literal(text)],
            Box::new(CmdModifiers::default()),
        )
    }

    /// Run `scp` between two already-rendered endpoints.
    fn scp(&mut self, opts: &SshOpts, from: &str, to: &str, success: Value) -> IResult {
        let mut text = String::from("scp -r");
        for flag in common_flags(opts) {
            text.push(' ');
            text.push_str(&flag);
        }
        // scp spells the port `-P`, not `-p`. Getting this wrong silently
        // enables "preserve times" and connects to the wrong port.
        if let Some(port) = opts.port {
            text.push_str(&format!(" -P {}", port));
        }
        text.push_str(&format!(" {} {}", from, to));

        let parts = vec![CmdPart::Literal(text.clone())];
        let mods = CmdModifiers::default();
        if self.dry_run_skip(text) {
            return Ok(Value::Ok(Box::new(success)));
        }
        match self.run_cmd_parts(&parts, &mods)? {
            Value::ProcessResult { exit_code, stderr, .. } => {
                if exit_code == 0 {
                    Ok(Value::Ok(Box::new(success)))
                } else {
                    Ok(Value::Err(Box::new(Value::String(format!(
                        "scp exited {}: {}",
                        exit_code,
                        stderr.trim()
                    )))))
                }
            }
            other => Ok(Value::Ok(Box::new(other))),
        }
    }
}

/// Flags shared by `ssh` and `scp`.
fn common_flags(opts: &SshOpts) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(key) = &opts.key {
        flags.push("-i".to_string());
        flags.push(crate::interpreter::helpers::shell_escape(key));
        // An explicit key means an explicit key. Without this, ssh still
        // offers every identity the agent holds first, and a host with
        // MaxAuthTries=3 rejects the connection before reaching the one
        // that was actually asked for.
        flags.push("-o".to_string());
        flags.push("IdentitiesOnly=yes".to_string());
    }
    if let Some(jump) = &opts.jump {
        flags.push("-J".to_string());
        flags.push(crate::interpreter::helpers::shell_escape(jump));
    }
    if !opts.interactive {
        // Fail rather than block forever on a password prompt that no CI job
        // will ever answer.
        flags.push("-o".to_string());
        flags.push("BatchMode=yes".to_string());
    }
    if let Some(secs) = opts.connect_timeout {
        flags.push("-o".to_string());
        flags.push(format!("ConnectTimeout={}", secs));
    }
    if !opts.strict_host_key_checking {
        flags.push("-o".to_string());
        flags.push("StrictHostKeyChecking=no".to_string());
        flags.push("-o".to_string());
        flags.push("UserKnownHostsFile=/dev/null".to_string());
    }
    if let Some(kh) = &opts.known_hosts {
        flags.push("-o".to_string());
        flags.push(format!(
            "UserKnownHostsFile={}",
            crate::interpreter::helpers::shell_escape(kh)
        ));
    }
    for raw in &opts.options {
        flags.push("-o".to_string());
        flags.push(crate::interpreter::helpers::shell_escape(raw));
    }
    flags
}

/// `user@host`, unless the host already carries a user.
fn target(host: &str, opts: &SshOpts) -> String {
    match &opts.user {
        Some(u) if !host.contains('@') => format!("{}@{}", u, host),
        _ => host.to_string(),
    }
}

/// Wrap in single quotes for the *local* shell so the remote command arrives
/// at the remote shell intact.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn host_cmd_opts(args: &[Value], who: &str) -> Result<(String, String, SshOpts), Signal> {
    let host = string_arg(args.first(), who, "host")?;
    let command = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        // A `Cmd` is accepted so a command can be written in backticks and
        // then sent somewhere else.
        Some(Value::Cmd(parts, _)) => crate::interpreter::methods::render_cmd(parts),
        Some(other) => {
            return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "{}: command must be a String or a Cmd, got {}",
                    who,
                    other.type_name()
                ),
            )))
        }
        None => {
            return Err(Signal::Error(QueError::new(
                ErrorKind::ArityMismatch,
                format!("{} requires a command", who),
            )))
        }
    };
    let opts = ssh_opts(args.get(2), who)?;
    Ok((host, command, opts))
}

fn string_arg(v: Option<&Value>, who: &str, what: &str) -> Result<String, Signal> {
    match v {
        Some(Value::String(s)) => Ok(s.clone()),
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

fn path_arg(v: Option<&Value>, who: &str, what: &str) -> Result<String, Signal> {
    match v {
        Some(val) => crate::interpreter::helpers::path_arg(val, &format!("{}: {}", who, what)),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{} requires a {}", who, what),
        ))),
    }
}

fn ssh_opts(arg: Option<&Value>, who: &str) -> Result<SshOpts, Signal> {
    let mut opts = SshOpts::default();
    let map = match arg {
        Some(Value::Map(m)) => m,
        None | Some(Value::Null) => return Ok(opts),
        Some(other) => {
            return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!("{}: options must be a Map, got {}", who, other.type_name()),
            )))
        }
    };
    if let Some(v) = map.get("user") {
        if !matches!(v, Value::Null) {
            opts.user = Some(v.display_string());
        }
    }
    if let Some(Value::Int(p)) = map.get("port") {
        opts.port = Some(*p);
    }
    if let Some(v) = map.get("key") {
        if !matches!(v, Value::Null) {
            opts.key = Some(v.display_string());
        }
    }
    if let Some(v) = map.get("jump") {
        if !matches!(v, Value::Null) {
            opts.jump = Some(v.display_string());
        }
    }
    match map.get("connect_timeout") {
        Some(Value::Null) => opts.connect_timeout = None,
        Some(Value::Int(n)) => opts.connect_timeout = Some(*n),
        Some(Value::Duration(val, unit)) => {
            let ms = crate::interpreter::helpers::duration_to_ms(*val, *unit);
            opts.connect_timeout = Some((ms / 1000.0).ceil().max(1.0) as i64);
        }
        _ => {}
    }
    if let Some(Value::Bool(b)) = map.get("interactive") {
        opts.interactive = *b;
    }
    if let Some(Value::Bool(b)) = map.get("forward_agent") {
        opts.forward_agent = *b;
    }
    if let Some(Value::Bool(b)) = map.get("strict_host_key_checking") {
        opts.strict_host_key_checking = *b;
    }
    if let Some(v) = map.get("known_hosts") {
        if !matches!(v, Value::Null) {
            opts.known_hosts = Some(v.display_string());
        }
    }
    if let Some(Value::List(items)) = map.get("options") {
        opts.options = items.iter().map(|v| v.display_string()).collect();
    }
    Ok(opts)
}

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_option_is_only_applied_when_the_host_lacks_one() {
        let mut opts = SshOpts::default();
        opts.user = Some("deploy".into());
        assert_eq!(target("web-1", &opts), "deploy@web-1");
        // An explicit `root@` in the host wins; silently rewriting it would
        // be the kind of surprise that gets discovered in production.
        assert_eq!(target("root@web-1", &opts), "root@web-1");
    }

    #[test]
    fn batch_mode_and_a_connect_timeout_are_on_by_default() {
        let flags = common_flags(&SshOpts::default()).join(" ");
        assert!(flags.contains("BatchMode=yes"), "{}", flags);
        assert!(flags.contains("ConnectTimeout=10"), "{}", flags);
    }

    #[test]
    fn an_explicit_key_pins_identities_only() {
        let mut opts = SshOpts::default();
        opts.key = Some("/home/me/.ssh/deploy".into());
        let flags = common_flags(&opts).join(" ");
        assert!(flags.contains("IdentitiesOnly=yes"), "{}", flags);
    }

    #[test]
    fn disabling_host_key_checking_also_detaches_known_hosts() {
        // Leaving UserKnownHostsFile alone would poison the real file with
        // an unverified key that every later connection then trusts.
        let mut opts = SshOpts::default();
        opts.strict_host_key_checking = false;
        let flags = common_flags(&opts).join(" ");
        assert!(flags.contains("StrictHostKeyChecking=no"), "{}", flags);
        assert!(flags.contains("UserKnownHostsFile=/dev/null"), "{}", flags);
    }

    #[test]
    fn a_remote_command_is_quoted_for_the_local_shell() {
        assert_eq!(shell_quote("echo hi"), "'echo hi'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
