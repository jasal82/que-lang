//! An opt-in capability model for scripts.
//!
//! The problem this solves is third-party code. `que install` fetches
//! somebody else's Quefile and runs it with the full authority of the user
//! who typed the command: it can read `~/.ssh`, POST it somewhere, and
//! `rm -rf` on the way out. Nothing in the language previously let a caller
//! say "this task builds a binary, it has no business touching the network".
//!
//! Two properties shape the design.
//!
//! **It is opt-in.** With no policy configured, nothing is checked and no
//! existing script changes behaviour. The moment *any* permission is
//! specified, the model flips to deny-by-default: everything not granted is
//! refused. A sandbox that defaults to "mostly allowed" gives a guarantee
//! nobody can reason about.
//!
//! **It is enforced at dispatch, not at the call sites.** Every std-module
//! function, every global builtin and every `Path` method is classified in
//! the tables below, and the check happens once, in the places where those
//! are dispatched. The alternative — sprinkling checks through 126
//! filesystem calls — guarantees that a new function eventually ships
//! without one, and a sandbox with a hole is worse than no sandbox because
//! it advertises a promise it does not keep. Adding an unclassified function
//! here is visible: it is denied under any policy until someone classifies
//! it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The five things a script can do to the world outside itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    Read,
    Write,
    Exec,
    Net,
    Env,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Write => "write",
            Capability::Exec => "exec",
            Capability::Net => "net",
            Capability::Env => "env",
        }
    }

    pub fn parse(s: &str) -> Option<Capability> {
        match s {
            "read" => Some(Capability::Read),
            "write" => Some(Capability::Write),
            "exec" => Some(Capability::Exec),
            "net" => Some(Capability::Net),
            "env" => Some(Capability::Env),
            _ => None,
        }
    }

    pub const ALL: [Capability; 5] = [
        Capability::Read,
        Capability::Write,
        Capability::Exec,
        Capability::Net,
        Capability::Env,
    ];
}

/// What a capability was granted for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Grant {
    /// Unscoped.
    All,
    /// Only these prefixes (paths) or suffixes (hosts).
    Scoped(BTreeSet<String>),
}

/// A set of grants. `None` anywhere in the interpreter means "unrestricted".
///
/// Built either from `--allow`/`--deny` flags ([`Policy::allow`],
/// [`Policy::deny`]) or programmatically by an embedder
/// ([`Policy::deny_all`] and the `allow_*` builders). Both kinds are checked
/// by the same code; they differ only in how a denial is worded, because an
/// embedder's user cannot pass `--allow` flags.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    grants: std::collections::BTreeMap<Capability, Grant>,
    /// Absolute program paths a command may start with. Only consulted for
    /// commands and [`Policy::check_exec_program`]; see
    /// [`Policy::allow_exec_program`].
    exec_programs: BTreeSet<PathBuf>,
    /// True for policies built by an embedder: denials then explain the
    /// grant instead of suggesting CLI flags.
    embedder: bool,
}

/// The outcome of a check, so the caller can name what was refused.
#[derive(Debug)]
pub struct Denied {
    pub capability: Capability,
    pub subject: String,
    /// Why the subject was refused, worded for an embedder's user. `None`
    /// for CLI policies, whose message suggests the `--allow` flag instead.
    pub reason: Option<String>,
}

impl Denied {
    pub fn message(&self) -> String {
        if let Some(reason) = &self.reason {
            return format!(
                "permission denied: {} '{}': {}",
                self.capability.as_str(),
                self.subject,
                reason
            );
        }
        format!(
            "permission denied: {} '{}' \u{2014} grant it with --allow {}={} \
             (or --allow {} for all)",
            self.capability.as_str(),
            self.subject,
            self.capability.as_str(),
            self.subject,
            self.capability.as_str()
        )
    }
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Denied {}

/// Characters that make a command's text more than "a program and its
/// arguments" to the shell: sequencing, pipes, redirection, substitution.
/// A command containing one of them can start programs other than its first
/// word, so it can't be matched against a program allow-list.
const SHELL_META: &[char] = &[';', '&', '|', '<', '>', '$', '`', '(', ')', '\n', '\r'];

/// What a command would start, as far as the exec allow-list is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandShape {
    /// The first word of the command, with interpolations substituted.
    pub program: Option<String>,
    /// The first shell metacharacter in the command's literal text, if any.
    pub shell_meta: Option<char>,
}

impl CommandShape {
    /// Analyse a command made of literal shell text and interpolated values.
    ///
    /// `parts` yields `(text, is_literal)`. Interpolated values are escaped
    /// by the interpreter, so they can't contain shell syntax and only
    /// contribute to the program word.
    pub fn analyse<'a>(parts: impl IntoIterator<Item = (&'a str, bool)>) -> CommandShape {
        let mut program = String::new();
        let mut program_done = false;
        let mut shell_meta = None;
        for (text, literal) in parts {
            if literal {
                if shell_meta.is_none() {
                    shell_meta = text.chars().find(|c| SHELL_META.contains(c));
                }
                for c in text.chars() {
                    if program_done {
                        break;
                    }
                    if c.is_whitespace() {
                        program_done = !program.is_empty();
                    } else {
                        program.push(c);
                    }
                }
            } else if !program_done {
                program.push_str(text);
            }
        }
        CommandShape {
            program: if program.is_empty() {
                None
            } else {
                Some(program)
            },
            shell_meta,
        }
    }
}

impl Policy {
    /// A policy that grants nothing, for embedders to extend with the
    /// `allow_*` builders.
    ///
    /// Equivalent to `Policy::default()` in what it permits, but denials are
    /// worded for an embedder ("outside the granted read prefixes") rather
    /// than suggesting `--allow` flags.
    pub fn deny_all() -> Self {
        Policy {
            embedder: true,
            ..Policy::default()
        }
    }

    /// Allow reading below `prefix` (a file or directory).
    ///
    /// Relative prefixes are resolved against the process's working
    /// directory now, so pass absolute paths.
    pub fn allow_read(mut self, prefix: impl AsRef<Path>) -> Self {
        self.add_scoped(Capability::Read, &prefix.as_ref().to_string_lossy());
        self
    }

    /// Allow writing (creating, modifying, removing) below `prefix`.
    pub fn allow_write(mut self, prefix: impl AsRef<Path>) -> Self {
        self.add_scoped(Capability::Write, &prefix.as_ref().to_string_lossy());
        self
    }

    /// Allow reading the environment variable `name`.
    pub fn allow_env(mut self, name: impl Into<String>) -> Self {
        self.add_scoped(Capability::Env, &name.into());
        self
    }

    /// Allow network access to `host` and its subdomains.
    pub fn allow_net_host(mut self, host: impl Into<String>) -> Self {
        self.add_scoped(Capability::Net, &host.into());
        self
    }

    /// Allow running the program at the absolute path `program`.
    ///
    /// A command is allowed when its first word is exactly this path and its
    /// text contains no shell metacharacters (`;`, `|`, `$`, …), which could
    /// start other programs. Relative paths are refused: `PATH` lookup would
    /// make the grant depend on the environment.
    pub fn allow_exec_program(mut self, program: impl AsRef<Path>) -> Result<Self, String> {
        let program = program.as_ref();
        if !program.is_absolute() {
            return Err(format!(
                "exec allow-list entries must be absolute paths, got '{}'",
                program.display()
            ));
        }
        self.exec_programs.insert(lexical_clean(program));
        Ok(self)
    }

    /// Grant `cap` without scope.
    pub fn allow_all(mut self, cap: Capability) -> Self {
        self.grants.insert(cap, Grant::All);
        self
    }

    fn add_scoped(&mut self, cap: Capability, item: &str) {
        let item = normalize(cap, item);
        match self.grants.get_mut(&cap) {
            Some(Grant::All) => {}
            Some(Grant::Scoped(existing)) => {
                existing.insert(item);
            }
            None => {
                self.grants
                    .insert(cap, Grant::Scoped(BTreeSet::from([item])));
            }
        }
    }

    /// The scoped grants for `cap`, in the form they are compared in
    /// (absolute paths, lowercase hosts). Empty for an unscoped grant.
    pub fn scoped_grants(&self, cap: Capability) -> Vec<String> {
        match self.grants.get(&cap) {
            Some(Grant::Scoped(items)) => items.iter().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The exec allow-list, as given to [`Policy::allow_exec_program`].
    pub fn exec_programs(&self) -> impl Iterator<Item = &Path> {
        self.exec_programs.iter().map(PathBuf::as_path)
    }

    /// Check a command about to be handed to the shell.
    ///
    /// `display` is the command as shown to humans (secrets redacted). A
    /// grant that matches it as a whole (`--allow exec`, `--allow exec=<cmd>`)
    /// wins. Otherwise the command must be a simple invocation of a program
    /// on the exec allow-list.
    pub fn check_command(&self, display: &str, shape: &CommandShape) -> Result<(), Denied> {
        let whole = self.check(Capability::Exec, display);
        if whole.is_ok() || self.exec_programs.is_empty() {
            return whole;
        }
        let deny = |reason: String| {
            Err(Denied {
                capability: Capability::Exec,
                subject: display.to_string(),
                reason: Some(reason),
            })
        };
        if let Some(c) = shape.shell_meta {
            return deny(format!(
                "the command uses shell syntax ('{}'), so it can't be matched against the \
                 exec allow-list; run the program without shell operators",
                c.escape_default()
            ));
        }
        match &shape.program {
            Some(program) if self.program_allowed(Path::new(program)) => Ok(()),
            Some(program) => deny(format!(
                "program '{}' is not in the exec allow-list ({})",
                program,
                self.describe_programs()
            )),
            None => deny("the command is empty".to_string()),
        }
    }

    /// Check that the program at `program` may be started. For embedders
    /// that spawn processes themselves (without a shell).
    pub fn check_exec_program(&self, program: &Path) -> Result<(), Denied> {
        if matches!(self.grants.get(&Capability::Exec), Some(Grant::All))
            || self.program_allowed(program)
        {
            return Ok(());
        }
        Err(Denied {
            capability: Capability::Exec,
            subject: program.display().to_string(),
            reason: Some(format!(
                "program is not in the exec allow-list ({})",
                self.describe_programs()
            )),
        })
    }

    fn program_allowed(&self, program: &Path) -> bool {
        program.is_absolute() && self.exec_programs.contains(&lexical_clean(program))
    }

    fn describe_programs(&self) -> String {
        if self.exec_programs.is_empty() {
            return "it is empty".to_string();
        }
        self.exec_programs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The embedder-facing explanation for a refused subject.
    fn reason(&self, cap: Capability) -> Option<String> {
        if !self.embedder {
            return None;
        }
        let scoped = self.scoped_grants(cap);
        Some(match cap {
            Capability::Read | Capability::Write if !scoped.is_empty() => format!(
                "the path is outside the granted {} prefixes ({})",
                cap.as_str(),
                scoped.join(", ")
            ),
            Capability::Env if !scoped.is_empty() => format!(
                "the variable is not in the env allow-list ({})",
                scoped.join(", ")
            ),
            Capability::Net if !scoped.is_empty() => format!(
                "the host is not in the network allow-list ({})",
                scoped.join(", ")
            ),
            Capability::Exec if !self.exec_programs.is_empty() => format!(
                "only programs on the exec allow-list may run ({})",
                self.describe_programs()
            ),
            Capability::Net => "network access is disabled".to_string(),
            _ => format!("no {} access was granted", cap.as_str()),
        })
    }

    /// Parse one `--allow` value: `read`, or `read=a,b`.
    ///
    /// Returns `Err` with the offending token rather than ignoring it. A
    /// typo'd capability that silently grants nothing would leave a script
    /// failing for a reason nobody can find.
    pub fn allow(&mut self, spec: &str) -> Result<(), String> {
        let (name, scope) = match spec.split_once('=') {
            Some((n, s)) => (n.trim(), Some(s)),
            None => (spec.trim(), None),
        };
        let cap = Capability::parse(name).ok_or_else(|| {
            format!(
                "unknown capability '{}': expected one of read, write, exec, net, env",
                name
            )
        })?;
        match scope {
            None => {
                self.grants.insert(cap, Grant::All);
            }
            Some(list) => {
                let items: BTreeSet<String> = list
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| normalize(cap, s))
                    .collect();
                if items.is_empty() {
                    return Err(format!("--allow {}= needs at least one value", name));
                }
                match self.grants.get_mut(&cap) {
                    // A later unscoped grant already won; widening then
                    // narrowing would be surprising.
                    Some(Grant::All) => {}
                    Some(Grant::Scoped(existing)) => existing.extend(items),
                    None => {
                        self.grants.insert(cap, Grant::Scoped(items));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty() && self.exec_programs.is_empty()
    }

    /// Grant everything except the named capabilities. Backs `--deny`.
    pub fn deny(&mut self, spec: &str) -> Result<(), String> {
        let cap = Capability::parse(spec.trim()).ok_or_else(|| {
            format!(
                "unknown capability '{}': expected one of read, write, exec, net, env",
                spec.trim()
            )
        })?;
        for other in Capability::ALL {
            if other != cap {
                self.grants.entry(other).or_insert(Grant::All);
            }
        }
        self.grants.remove(&cap);
        // Mark the denial explicitly so a policy built only from `--deny`
        // is not mistaken for an empty one.
        self.grants
            .entry(cap)
            .or_insert(Grant::Scoped(BTreeSet::new()));
        Ok(())
    }

    pub fn check(&self, cap: Capability, subject: &str) -> Result<(), Denied> {
        let deny = || {
            Err(Denied {
                capability: cap,
                subject: subject.to_string(),
                reason: self.reason(cap),
            })
        };
        match self.grants.get(&cap) {
            None => deny(),
            Some(Grant::All) => Ok(()),
            Some(Grant::Scoped(items)) => {
                let subject = normalize(cap, subject);
                let ok = items.iter().any(|item| match cap {
                    // Paths match by prefix, on the resolved form, so
                    // `--allow write=./build` cannot be escaped with
                    // `./build/../../etc/passwd`.
                    Capability::Read | Capability::Write => Path::new(&subject).starts_with(item),
                    // Hosts match themselves or any subdomain, so
                    // `--allow net=example.com` covers `api.example.com`
                    // but not `notexample.com`.
                    Capability::Net => subject == *item || subject.ends_with(&format!(".{}", item)),
                    _ => subject == *item,
                });
                if ok {
                    Ok(())
                } else {
                    deny()
                }
            }
        }
    }
}

/// Put a subject into the form `check` compares against.
///
/// Paths are made absolute and lexically cleaned; `..` is resolved here
/// rather than by the filesystem, because the target may not exist yet and
/// `canonicalize` would fail on exactly the writes worth policing.
fn normalize(cap: Capability, subject: &str) -> String {
    match cap {
        Capability::Read | Capability::Write => {
            let raw = PathBuf::from(subject);
            let absolute = if raw.is_absolute() {
                raw
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(raw)
            };
            lexical_clean(&absolute).to_string_lossy().into_owned()
        }
        Capability::Net => {
            // Accept a full URL or a bare host.
            let s = subject
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(subject);
            let s = s.split('/').next().unwrap_or(s);
            let s = s.rsplit('@').next().unwrap_or(s);
            let host = s.split(':').next().unwrap_or(s);
            host.to_ascii_lowercase()
        }
        _ => subject.to_string(),
    }
}

/// Resolve `.` and `..` without touching the filesystem.
fn lexical_clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in p.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What a std-module function does, and which argument names the subject.
///
/// `None` for the argument index means the capability is unscoped (there is
/// no single path or host to point at).
pub type Effect = (Capability, Option<usize>);

/// Classification for `<module>.<function>`.
///
/// A function missing from this table is denied under any policy. That is
/// deliberate: forgetting to classify a new function should fail closed and
/// loudly, not silently punch a hole.
pub fn std_effect(module: &str, func: &str) -> Option<Effect> {
    classify_std(module, func).unwrap_or(Some((Capability::Exec, None)))
}

/// Whether `<module>.<function>` has an explicit entry in the table, as
/// opposed to falling through to the fail-closed default.
pub fn std_is_classified(module: &str, func: &str) -> bool {
    classify_std(module, func).is_some()
}

/// The table behind [`std_effect`]: `None` means "not classified",
/// `Some(None)` means "touches nothing".
#[rustfmt::skip]
fn classify_std(module: &str, func: &str) -> Option<Option<Effect>> {
    use Capability::*;
    Some(Some(match (module, func) {
        // ── fs ──
        ("fs", "read") | ("fs", "read_text") | ("fs", "read_bytes")
        | ("fs", "read_secret") | ("fs", "read_lines") | ("fs", "exists")
        | ("fs", "is_file") | ("fs", "is_dir") | ("fs", "list")
        | ("fs", "list_dir") | ("fs", "walk") | ("fs", "find")
        | ("fs", "glob") | ("fs", "size") | ("fs", "metadata")
        | ("fs", "lines") => (Read, Some(0)),
        ("fs", "write") | ("fs", "write_text") | ("fs", "write_bytes")
        | ("fs", "write_lines") | ("fs", "atomic_write") | ("fs", "transform")
        | ("fs", "append") | ("fs", "mkdir") | ("fs", "mkdir_all")
        | ("fs", "remove") | ("fs", "remove_all") | ("fs", "remove_dir")
        | ("fs", "rm") | ("fs", "touch") | ("fs", "chmod") | ("fs", "symlink")
        | ("fs", "temp_file") | ("fs", "temp_dir") => (Write, Some(0)),
        // Two-path operations are checked on the destination, which is the
        // one that changes; the source is a second effect.
        ("fs", "copy") | ("fs", "move") | ("fs", "rename")
        | ("fs", "copy_dir") => (Write, Some(1)),

        // ── config files ──
        ("config", "read") => (Read, Some(0)),
        ("config", "write") => (Write, Some(0)),

        // ── streams ──
        // `stream.file` is the one that opens a file; `stream.of` wraps text,
        // a list, or a handle that `open()` already cleared. That is the
        // whole reason the two are separate names.
        ("stream", "file") => (Read, Some(0)),
        ("stream", _) => return Some(None),

        // Reflection reads the interpreter's own state, not the machine's.
        ("reflect", _) => return Some(None),

        // ── parsers that read or rewrite a file ──
        ("json", "read") | ("yaml", "read") | ("toml", "read")
        | ("csv", "read") | ("dotenv", "read") | ("dotenv", "parse")
        | ("dotenv", "load") | ("dotenv", "load_overwrite")
        | ("template", "render_file") | ("hash", "file")
        | ("hash", "sha256_file") | ("hash", "md5_file") | ("hash", "verify")
        | ("hash", "verify_checksums") => (Read, Some(0)),
        // `edit` reads the file, hands it to a closure and writes it back.
        ("json", "write") | ("yaml", "write") | ("toml", "write")
        | ("json", "edit") | ("yaml", "edit") | ("toml", "edit") => (Write, Some(0)),
        ("csv", "write") | ("dotenv", "write") | ("hash", "write_checksums") => {
            (Write, Some(1))
        }
        // A file sink appends every later log line to a file.
        ("log", "add_file_sink") => (Write, Some(0)),

        // Pure transformations of in-memory data touch nothing. `hash.sha256`
        // and `csv.parse` read a file when handed one; see
        // [`std_dynamic_effect`].
        ("json", _) | ("yaml", _) | ("toml", _) | ("csv", _)
        | ("template", _) | ("hash", _) | ("time", _) | ("tty", _)
        | ("log", _) => return Some(None),

        // ── network ──
        ("http", _) => (Net, Some(0)),
        ("net", _) => (Net, Some(0)),
        ("ssh", _) => (Net, Some(0)),

        // ── subprocesses ──
        ("git", _) => (Exec, None),
        ("container", "engine") => return Some(None),
        ("container", _) => (Exec, None),

        // ── mixed ──
        ("archive", "extract") | ("archive", "unzip") | ("archive", "untar") => {
            (Write, Some(1))
        }
        ("archive", "list") => (Read, Some(0)),
        ("archive", _) => (Write, Some(0)),
        ("watch", _) => (Read, Some(0)),
        ("prompt", _) => return Some(None),

        _ => return None,
    }))
}

/// The *second* effect of a std function, for the handful that have two.
///
/// Kept as a separate table rather than making `std_effect` return a list:
/// dual-effect functions are rare, and a list would invite classifying
/// everything vaguely instead of picking the one effect that matters.
#[rustfmt::skip]
pub fn std_extra_effect(module: &str, func: &str) -> Option<Effect> {
    use Capability::*;
    Some(match (module, func) {
        // Downloads land on disk, so they need write access to the
        // destination as well as network access to the host.
        ("http", "download") => (Write, Some(1)),
        // A clone writes a whole working tree at its destination. When the
        // destination is left implicit there is no argument to name, so a
        // scoped write grant refuses it: `git.clone(url, dest)` is the
        // spelling a sandboxed script has to use.
        ("git", "clone") => (Write, Some(1)),
        // Building an archive reads the files it is packing (a list).
        ("archive", "create") | ("archive", "zip") | ("archive", "tar")
        | ("archive", "targz") | ("archive", "tar_gz") => (Read, Some(1)),
        // Extraction reads the archive it is unpacking.
        ("archive", "extract") | ("archive", "unzip") | ("archive", "untar") => {
            (Read, Some(0))
        }
        // Copying reads the source tree; rewriting a file reads it first.
        // Without these, a read-scoped policy could be escaped by copying a
        // file into a writable directory.
        ("fs", "copy") | ("fs", "move") | ("fs", "rename") | ("fs", "copy_dir")
        | ("fs", "transform") | ("json", "edit") | ("yaml", "edit")
        | ("toml", "edit") => (Read, Some(0)),
        // Checksums are computed over a list of files.
        ("hash", "write_checksums") => (Read, Some(0)),
        // Loading a dotenv file sets process environment variables.
        ("dotenv", "load") | ("dotenv", "load_overwrite") => (Env, None),
        // scp moves a local file in either direction.
        ("ssh", "upload") => (Read, Some(1)),
        ("ssh", "download") => (Write, Some(2)),
        _ => return None,
    })
}

/// An effect that depends on an argument's type, for functions that read a
/// file only when handed a path.
///
/// `hash.sha256(p"f")` hashes the file's contents while `hash.sha256("f")`
/// hashes the string, and `csv.parse` accepts either a file name or the CSV
/// text itself. Classifying them by name alone would either deny the pure
/// form or let the file form read anything.
pub fn std_dynamic_effect(
    module: &str,
    func: &str,
    args: &[crate::value::Value],
) -> Option<Effect> {
    use crate::value::Value;
    match (module, func, args.first()) {
        ("hash", "sha256" | "sha512" | "md5", Some(Value::Path(_))) => {
            Some((Capability::Read, Some(0)))
        }
        ("csv", "parse", Some(Value::Path(_))) => Some((Capability::Read, Some(0))),
        // The same test `csv.parse` makes before reading.
        ("csv", "parse", Some(Value::String(s)))
            if !s.contains('\n')
                && Path::new(&crate::interpreter::helpers::expand_tilde(s)).exists() =>
        {
            Some((Capability::Read, Some(0)))
        }
        _ => None,
    }
}

/// Classification for a `Path` method.
///
/// Methods absent from both lists are pure (`name`, `parent`, `with_ext`, …)
/// and need no capability.
pub fn path_effect(method: &str) -> Option<Capability> {
    #[rustfmt::skip]
    const READS: &[&str] = &[
        "read_text", "read", "read_bytes", "read_lines", "lines", "exists",
        "is_file", "is_dir", "is_symlink", "is_link", "size", "modified",
        "created", "list", "list_dir", "ls", "walk", "files", "dirs", "glob",
        "hash", "sha256", "md5", "read_json", "read_yaml", "read_toml",
        "read_csv", "metadata", "mode", "is_executable", "canonicalize",
        "resolve", "real_path",
    ];
    #[rustfmt::skip]
    const WRITES: &[&str] = &[
        "write_text", "write", "write_bytes", "append", "append_text",
        "mkdir", "mkdir_all", "create_dir", "create_dir_all", "remove",
        "remove_all", "rm", "delete", "touch", "chmod", "set_mode",
        "copy", "copy_to", "move_to", "rename", "rename_to", "symlink",
        "hard_link", "write_json", "write_yaml", "write_toml", "write_csv",
        "truncate",
    ];
    if WRITES.contains(&method) {
        Some(Capability::Write)
    } else if READS.contains(&method) {
        Some(Capability::Read)
    } else {
        None
    }
}

/// Make a path subject absolute and lexically clean, the form grants are
/// compared in. Relative paths resolve against the process's working
/// directory, as the filesystem call itself will.
pub fn absolute_path(subject: &str) -> PathBuf {
    PathBuf::from(normalize(Capability::Read, subject))
}

/// What a *global* builtin does.
///
/// Global builtins are the ones with no `module.` prefix — `open`, `glob`,
/// `stream`, the `config_*` family — and they were the hole in the first cut
/// of this model: `open(p, "w")` wrote a file with no check at all because
/// only `std.fs` and `Path` methods were classified.
///
/// Two of them cannot be described by a fixed capability, because what they
/// touch depends on an argument. Rather than approximate — which would mean
/// either denying `open(p, "r")` under a read grant or permitting
/// `open(p, "w")` under one — they get their own variants, and the dispatcher
/// is forced by the compiler to handle them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalEffect {
    /// Touches nothing outside the interpreter.
    Pure,
    /// One capability, scoped to the argument at this index when there is one.
    Needs(Capability, Option<usize>),
    /// `open(path, mode)`: `"r"` (the default) reads, `"w"`/`"a"` write.
    OpenPath,
    /// A `with env.scope({...})` hook: one `Env` check per variable name, so
    /// a scoped `--allow env=PATH` still works and so the denial names the
    /// variable rather than printing the map's values.
    EnvVars(usize),
    /// A `with TempDir {}` / `with TempFile {}` hook: a `Write` scoped to the
    /// base directory the temporary will be created in.
    TempBase,
    /// A `with dir(...)` hook: a `Read` scoped to the `path` field of the
    /// context object. The instance is the argument, so the path has to be
    /// dug out of it — checking the argument itself would test a grant
    /// against the words `Dir { path: ... }`.
    DirTarget,
}

/// Classification for a global builtin.
///
/// `None` means the name is not classified, which is denied under any policy
/// — the same fail-closed rule as [`std_effect`]. A builtin added without a
/// line here stops working the first time anyone runs it in a sandbox, which
/// is the failure everyone wants over the silent one.
pub fn global_effect(name: &str) -> Option<GlobalEffect> {
    use Capability::*;
    use GlobalEffect::*;
    Some(match name {
        // ── the argument decides ──
        "open" => OpenPath,

        // ── filesystem ──
        "glob" => Needs(Read, Some(0)),
        "which" => Needs(Read, None),
        // Moving the process into a directory is a read of it: the directory
        // has to exist, and every relative path afterwards resolves through
        // it. The grants themselves were made absolute when the flags were
        // parsed, so `cd` moves the script and not the fence around it.
        "cd" => Needs(Read, Some(0)),
        // `dir(path)` only builds the context object; the move happens when the
        // `with` block enters it, so that is where the read is checked.
        "dir" => Pure,
        "__ctx_dir_enter" => DirTarget,
        "__ctx_dir_exit" => Pure,
        // `with TempDir {}` / `with TempFile {}` desugar to these. Entering
        // creates under a base directory named by a field; exiting deletes
        // the path it is handed back.
        "__ctx_tempdir_enter" | "__ctx_tempfile_enter" => TempBase,
        "__ctx_tempdir_exit" | "__ctx_tempfile_exit" => Needs(Write, Some(1)),

        // ── environment ──
        // `with env.scope({...})` sets and restores real environment
        // variables, so it is an env effect even though it looks lexical.
        // Entering is handed the scope object, exiting the saved map.
        "__ctx_envscope_enter" | "__ctx_envscope_exit" => EnvVars(0),
        "env" | "args" => Pure,

        // ── terminal ──
        // Reading what the operator typed, and writing to the terminal they
        // are watching, are not resources this model protects: the sandbox
        // bounds what a script reaches on the machine, and the console is
        // already the script's own. Gating them would break every progress
        // message without denying an attacker anything.
        "print" | "println" | "input" | "confirm" => Pure,

        // ── higher-order: the closure's own effects are checked when it runs ──
        "retry" | "timeout" | "run_task" | "tasks" | "compose" => Pure,

        // ── pure values and pure transformations ──
        "Ok" | "Err" | "abs" | "assert" | "bool" | "chr" | "dbg" | "dry_run" | "fail" | "float"
        | "help" | "int" | "max" | "min" | "ord" | "path" | "quefile_dir" | "range" | "regex"
        | "script_dir" | "secret" | "semver_parse" | "sleep" | "str" | "strict" | "typeof" => Pure,

        // ── removed, but still classified ──
        // A tombstone builtin raises "use X instead" and touches nothing.
        // Leaving these out would mean a sandboxed script got a permission
        // denial instead of the sentence telling it what to write.
        "Some" | "None" | "assert_eq" | "chars" | "chunk" | "config_delete" | "config_get"
        | "config_has" | "config_merge" | "config_paths" | "config_read" | "config_set"
        | "config_write" | "contains" | "each" | "enumerate" | "error" | "fields" | "filter"
        | "find" | "flat_map" | "flatten" | "fold" | "for_each" | "group_by" | "has_method"
        | "inspect" | "is_type" | "join" | "keys" | "len" | "map" | "methods" | "modules"
        | "now" | "partition" | "pop" | "push" | "replace" | "scope_depth" | "skip" | "sort_by"
        | "split" | "stdin" | "stderr" | "stdout" | "stream" | "stream_of" | "take" | "to_path"
        | "trim" | "type_info" | "values" | "var_info" | "vars" | "zip" | "all" | "any" => Pure,

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(specs: &[&str]) -> Policy {
        let mut p = Policy::default();
        for s in specs {
            p.allow(s).unwrap();
        }
        p
    }

    #[test]
    fn an_empty_policy_denies_everything() {
        let p = Policy::default();
        for cap in Capability::ALL {
            assert!(p.check(cap, "x").is_err(), "{:?}", cap);
        }
    }

    #[test]
    fn a_global_builtin_that_nobody_classified_is_denied() {
        // The first cut of this model checked std modules and `Path` methods
        // but not global builtins, so `open` wrote whatever it liked under
        // `--deny write`. The table now decides, and an omission from it
        // fails closed instead of quietly passing.
        assert!(global_effect("open").is_some());
        assert!(global_effect("a_builtin_nobody_has_written_yet").is_none());
    }

    #[test]
    fn every_global_builtin_is_classified() {
        // The list is extracted from the interpreter's own dispatch table, so
        // adding a builtin without classifying it fails here rather than in
        // somebody's sandbox.
        for name in crate::interpreter::BUILTIN_NAMES {
            assert!(
                global_effect(name).is_some(),
                "global builtin '{name}' has no entry in permissions::global_effect"
            );
        }
    }

    #[test]
    fn opening_a_file_takes_its_capability_from_the_mode() {
        assert!(matches!(
            global_effect("open"),
            Some(GlobalEffect::OpenPath)
        ));
    }

    #[test]
    fn streaming_from_a_file_is_a_read_and_streaming_text_is_not() {
        // The global `stream(x)` had to inspect its argument to know which
        // it was. `std.stream` splits the two, so the table can say.
        assert_eq!(
            std_effect("stream", "file"),
            Some((Capability::Read, Some(0)))
        );
        for pure in ["of", "stdout", "stderr", "stdin"] {
            assert_eq!(std_effect("stream", pure), None, "{pure}");
        }
    }

    #[test]
    fn reading_and_writing_a_config_file_are_the_parts_that_touch_disk() {
        // Everything else about a config is a Map method operating on a value
        // already in hand.
        assert_eq!(
            std_effect("config", "read"),
            Some((Capability::Read, Some(0)))
        );
        assert_eq!(
            std_effect("config", "write"),
            Some((Capability::Write, Some(0)))
        );
    }

    #[test]
    fn an_unscoped_grant_allows_anything_of_that_kind_only() {
        let p = policy(&["exec"]);
        assert!(p.check(Capability::Exec, "rm -rf /").is_ok());
        assert!(p.check(Capability::Net, "example.com").is_err());
    }

    #[test]
    fn a_path_grant_cannot_be_escaped_with_dot_dot() {
        // The whole point of resolving `..` ourselves: the target of a write
        // often does not exist yet, so canonicalize() would fail on exactly
        // the case worth policing.
        let cwd = std::env::current_dir().unwrap();
        let mut p = Policy::default();
        p.allow(&format!("write={}", cwd.join("build").display()))
            .unwrap();
        assert!(p.check(Capability::Write, "build/out.txt").is_ok());
        assert!(p
            .check(Capability::Write, "build/../../etc/passwd")
            .is_err());
    }

    #[test]
    fn a_relative_grant_is_resolved_against_the_cwd() {
        let p = policy(&["write=./build"]);
        assert!(p.check(Capability::Write, "build/a/b.txt").is_ok());
        assert!(p.check(Capability::Write, "dist/a.txt").is_err());
    }

    #[test]
    fn a_host_grant_covers_subdomains_but_not_lookalikes() {
        let p = policy(&["net=example.com"]);
        assert!(p
            .check(Capability::Net, "https://api.example.com/v1")
            .is_ok());
        assert!(p.check(Capability::Net, "example.com").is_ok());
        assert!(p.check(Capability::Net, "notexample.com").is_err());
        assert!(p.check(Capability::Net, "example.com.evil.net").is_err());
    }

    #[test]
    fn a_url_is_reduced_to_its_host() {
        assert_eq!(
            normalize(Capability::Net, "https://user@API.Example.com:8443/x?y"),
            "api.example.com"
        );
    }

    #[test]
    fn scoped_grants_accumulate() {
        let p = policy(&["write=./build", "write=./dist"]);
        assert!(p.check(Capability::Write, "build/a").is_ok());
        assert!(p.check(Capability::Write, "dist/a").is_ok());
    }

    #[test]
    fn an_unscoped_grant_is_not_narrowed_by_a_later_scoped_one() {
        let p = policy(&["write", "write=./build"]);
        assert!(p.check(Capability::Write, "/anywhere/at/all").is_ok());
    }

    #[test]
    fn deny_grants_everything_else() {
        let mut p = Policy::default();
        p.deny("net").unwrap();
        assert!(p.check(Capability::Exec, "ls").is_ok());
        assert!(p.check(Capability::Read, "/etc/hosts").is_ok());
        assert!(p.check(Capability::Net, "example.com").is_err());
        assert!(!p.is_empty(), "a deny-only policy must still be a policy");
    }

    #[test]
    fn a_typo_is_an_error_rather_than_a_silent_no_op() {
        let mut p = Policy::default();
        assert!(p.allow("exce").is_err());
        assert!(p.allow("read=").is_err());
    }

    #[test]
    fn an_unclassified_std_function_fails_closed() {
        // A newly added module function must be denied until somebody
        // classifies it, not quietly allowed.
        assert_eq!(
            std_effect("brand_new", "thing"),
            Some((Capability::Exec, None))
        );
    }

    #[test]
    fn every_std_function_is_classified() {
        // Falling through to the fail-closed default is safe but wrong: it
        // reports a file read as "exec" and refuses it under a read grant.
        // Every function a std module advertises needs a real entry.
        for module in crate::interpreter::std_modules::all_modules() {
            for func in module.functions {
                assert!(
                    std_is_classified(module.name, func),
                    "std function '{}.{}' has no entry in permissions::std_effect",
                    module.name,
                    func
                );
            }
        }
    }

    #[test]
    fn functions_that_read_a_file_only_when_given_a_path_are_classified_by_argument() {
        use crate::value::Value;
        let read = Some((Capability::Read, Some(0)));
        assert_eq!(
            std_dynamic_effect("hash", "sha256", &[Value::Path("f".into())]),
            read
        );
        assert_eq!(
            std_dynamic_effect("hash", "sha256", &[Value::String("f".into())]),
            None
        );
        assert_eq!(
            std_dynamic_effect("csv", "parse", &[Value::String("a,b\n1,2".into())]),
            None
        );
    }

    #[test]
    fn a_cli_policy_suggests_the_flag_and_an_embedder_policy_explains_the_grant() {
        let cli = policy(&["read=/data"]);
        let msg = cli
            .check(Capability::Read, "/etc/passwd")
            .unwrap_err()
            .message();
        assert!(msg.contains("--allow read=/etc/passwd"), "{}", msg);

        let embedded = Policy::deny_all().allow_read("/data").allow_read("/stage");
        let msg = embedded
            .check(Capability::Read, "/etc/passwd")
            .unwrap_err()
            .message();
        assert_eq!(
            msg,
            "permission denied: read '/etc/passwd': the path is outside the granted read \
             prefixes (/data, /stage)"
        );
        let msg = embedded
            .check(Capability::Write, "/data/x")
            .unwrap_err()
            .message();
        assert_eq!(
            msg,
            "permission denied: write '/data/x': no write access was granted"
        );
    }

    #[test]
    fn the_command_shape_is_the_first_word_and_the_first_shell_operator() {
        let shape =
            CommandShape::analyse([("/usr/bin/objcopy -O binary ", true), ("a b.elf", false)]);
        assert_eq!(shape.program.as_deref(), Some("/usr/bin/objcopy"));
        assert_eq!(shape.shell_meta, None);
        // An interpolated program path with spaces stays one word.
        let shape = CommandShape::analyse([("/opt/my tools/x", false), (" --version", true)]);
        assert_eq!(shape.program.as_deref(), Some("/opt/my tools/x"));
        let shape = CommandShape::analyse([("/bin/ok && /bin/rm -rf x", true)]);
        assert_eq!(shape.shell_meta, Some('&'));
    }

    #[test]
    fn an_exec_allow_list_admits_simple_invocations_of_listed_programs_only() {
        let p = Policy::deny_all()
            .allow_exec_program("/usr/bin/objcopy")
            .unwrap();
        let shape = |text: &str| CommandShape::analyse([(text, true)]);
        let check = |text: &str| p.check_command(text, &shape(text));
        assert!(check("/usr/bin/objcopy -O binary a b").is_ok());
        assert!(check("/usr/bin/objcopy a; rm -rf /").is_err());
        assert!(check("/usr/bin/objcopy $(rm -rf /)").is_err());
        assert!(
            check("objcopy a b").is_err(),
            "PATH lookup is not on the list"
        );
        assert!(check("/usr/bin/../bin/rm x").is_err());
        assert!(p.check_exec_program(Path::new("/usr/bin/objcopy")).is_ok());
        assert!(p.check_exec_program(Path::new("/usr/bin/strip")).is_err());
        // A CLI grant for the exact command line keeps working.
        let cli = policy(&["exec=make build"]);
        assert!(cli
            .check_command("make build", &shape("make build"))
            .is_ok());
    }

    #[test]
    fn pure_helpers_need_no_capability() {
        assert_eq!(std_effect("json", "parse"), None);
        assert_eq!(path_effect("parent"), None);
        assert_eq!(path_effect("write_text"), Some(Capability::Write));
        assert_eq!(path_effect("read_text"), Some(Capability::Read));
    }
}
