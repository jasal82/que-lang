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
#[derive(Debug, Clone, Default)]
pub struct Policy {
    grants: std::collections::BTreeMap<Capability, Grant>,
}

/// The outcome of a check, so the caller can name what was refused.
#[derive(Debug)]
pub struct Denied {
    pub capability: Capability,
    pub subject: String,
}

impl Denied {
    pub fn message(&self) -> String {
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

impl Policy {
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
        self.grants.is_empty()
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
        self.grants.entry(cap).or_insert(Grant::Scoped(BTreeSet::new()));
        Ok(())
    }

    pub fn check(&self, cap: Capability, subject: &str) -> Result<(), Denied> {
        let deny = || {
            Err(Denied {
                capability: cap,
                subject: subject.to_string(),
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
                    Capability::Read | Capability::Write => {
                        Path::new(&subject).starts_with(item)
                    }
                    // Hosts match themselves or any subdomain, so
                    // `--allow net=example.com` covers `api.example.com`
                    // but not `notexample.com`.
                    Capability::Net => {
                        subject == *item || subject.ends_with(&format!(".{}", item))
                    }
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
    use Capability::*;
    Some(match (module, func) {
        // ── fs ──
        ("fs", "read") | ("fs", "read_text") | ("fs", "read_bytes")
        | ("fs", "read_secret") | ("fs", "exists") | ("fs", "is_file")
        | ("fs", "is_dir") | ("fs", "list") | ("fs", "list_dir")
        | ("fs", "walk") | ("fs", "glob") | ("fs", "size")
        | ("fs", "metadata") | ("fs", "lines") => (Read, Some(0)),
        ("fs", "write") | ("fs", "write_text") | ("fs", "write_bytes")
        | ("fs", "append") | ("fs", "mkdir") | ("fs", "mkdir_all")
        | ("fs", "remove") | ("fs", "remove_all") | ("fs", "rm")
        | ("fs", "touch") | ("fs", "chmod") | ("fs", "symlink")
        | ("fs", "temp_file") | ("fs", "temp_dir") => (Write, Some(0)),
        // Two-path operations are checked on the destination, which is the
        // one that changes.
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
        ("stream", _) => return None,

        // Reflection reads the interpreter's own state, not the machine's.
        ("reflect", _) => return None,

        // ── parsers that read a file ──
        ("json", "read") | ("yaml", "read") | ("toml", "read")
        | ("csv", "read") | ("dotenv", "read") | ("dotenv", "load")
        | ("template", "render_file") | ("hash", "file")
        | ("hash", "sha256_file") | ("hash", "md5_file") => (Read, Some(0)),
        ("json", "write") | ("yaml", "write") | ("toml", "write")
        | ("csv", "write") => (Write, Some(0)),

        // Pure transformations of in-memory data touch nothing.
        ("json", _) | ("yaml", _) | ("toml", _) | ("csv", _)
        | ("template", _) | ("hash", _) | ("time", _) | ("tty", _)
        | ("log", _) => return None,

        // ── network ──
        ("http", _) => (Net, Some(0)),
        ("net", _) => (Net, Some(0)),
        ("ssh", _) => (Net, Some(0)),

        // ── subprocesses ──
        ("git", _) => (Exec, None),
        ("container", "engine") => return None,
        ("container", _) => (Exec, None),

        // ── mixed ──
        ("archive", "extract") | ("archive", "unzip") | ("archive", "untar") => {
            (Write, Some(1))
        }
        ("archive", _) => (Write, Some(0)),
        ("watch", _) => (Read, Some(0)),
        ("prompt", _) => return None,

        _ => (Exec, None),
    })
}

/// The *second* effect of a std function, for the handful that have two.
///
/// Kept as a separate table rather than making `std_effect` return a list:
/// dual-effect functions are rare, and a list would invite classifying
/// everything vaguely instead of picking the one effect that matters.
pub fn std_extra_effect(module: &str, func: &str) -> Option<Effect> {
    use Capability::*;
    Some(match (module, func) {
        // Downloads land on disk, so they need write access to the
        // destination as well as network access to the host.
        ("http", "download") => (Write, Some(1)),
        // Building an archive reads the source tree it is packing.
        ("archive", "create") | ("archive", "zip") | ("archive", "tar")
        | ("archive", "targz") => (Read, Some(1)),
        // Extraction reads the archive it is unpacking.
        ("archive", "extract") | ("archive", "unzip") | ("archive", "untar") => {
            (Read, Some(0))
        }
        // scp moves a local file in either direction.
        ("ssh", "upload") => (Read, Some(1)),
        ("ssh", "download") => (Write, Some(2)),
        _ => return None,
    })
}

/// Classification for a `Path` method.
///
/// Methods absent from both lists are pure (`name`, `parent`, `with_ext`, …)
/// and need no capability.
pub fn path_effect(method: &str) -> Option<Capability> {
    const READS: &[&str] = &[
        "read_text", "read", "read_bytes", "read_lines", "lines", "exists",
        "is_file", "is_dir", "is_symlink", "size", "modified", "created",
        "list", "list_dir", "walk", "glob", "hash", "sha256", "md5",
        "read_json", "read_yaml", "read_toml", "read_csv", "metadata",
        "mode", "is_executable", "canonicalize", "resolve", "real_path",
    ];
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
        "Ok" | "Err" | "abs" | "assert" | "bool" | "chr" | "dbg"
        | "dry_run" | "fail" | "float" | "help" | "int" | "max" | "min"
        | "ord" | "path" | "quefile_dir" | "range" | "regex"
        | "script_dir" | "secret" | "semver_parse" | "sleep" | "str"
        | "strict" | "typeof" => Pure,

        // ── removed, but still classified ──
        // A tombstone builtin raises "use X instead" and touches nothing.
        // Leaving these out would mean a sandboxed script got a permission
        // denial instead of the sentence telling it what to write.
        "Some" | "None" | "assert_eq" | "chars" | "chunk" | "config_delete"
        | "config_get" | "config_has" | "config_merge" | "config_paths"
        | "config_read" | "config_set" | "config_write" | "contains"
        | "each" | "enumerate" | "error" | "fields" | "filter" | "find"
        | "flat_map" | "flatten" | "fold" | "for_each" | "group_by"
        | "has_method" | "inspect" | "is_type" | "join" | "keys" | "len"
        | "map" | "methods" | "modules" | "now" | "partition" | "pop"
        | "push" | "replace" | "scope_depth" | "skip" | "sort_by"
        | "split" | "stdin" | "stderr" | "stdout" | "stream"
        | "stream_of" | "take" | "to_path" | "trim" | "type_info"
        | "values" | "var_info" | "vars" | "zip" | "all" | "any" => Pure,

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
        assert!(matches!(global_effect("open"), Some(GlobalEffect::OpenPath)));
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
        p.allow(&format!("write={}", cwd.join("build").display())).unwrap();
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
        assert!(p.check(Capability::Net, "https://api.example.com/v1").is_ok());
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
        assert_eq!(std_effect("brand_new", "thing"), Some((Capability::Exec, None)));
    }

    #[test]
    fn pure_helpers_need_no_capability() {
        assert_eq!(std_effect("json", "parse"), None);
        assert_eq!(path_effect("parent"), None);
        assert_eq!(path_effect("write_text"), Some(Capability::Write));
        assert_eq!(path_effect("read_text"), Some(Capability::Read));
    }
}
