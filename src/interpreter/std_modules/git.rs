//! std.git module — Git repository introspection via libgit2.
//!
//! `clone` is the exception: it shells out to the `git` binary instead of
//! using libgit2. Cloning is the one operation here that talks to a remote,
//! and the remotes people clone are behind credential helpers, SSH agents,
//! proxies and per-host config that libgit2 does not read. A clone that
//! ignores the user's `~/.gitconfig` fails on exactly the private repository
//! it was reached for, so this defers to the program that already works.

use crate::error::*;
use crate::value::{CmdModifiers, CmdPart, Value};
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "git",
        functions: &[
            "branch", "commit", "short_commit", "tag",
            "tags", "is_dirty", "is_clean", "remote_url",
            "clone",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_git(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "clone" => self.git_clone(args),
            "branch" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let head = repo.head().map_err(|e| sig_err(format!("git.branch: {}", e)))?;
                let name = head.shorthand().unwrap_or("HEAD").to_string();
                Ok(Value::String(name))
            }
            "commit" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let head = repo.head().map_err(|e| sig_err(format!("git.commit: {}", e)))?;
                let oid = head.peel_to_commit()
                    .map_err(|e| sig_err(format!("git.commit: {}", e)))?
                    .id();
                Ok(Value::String(oid.to_string()))
            }
            "short_commit" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let head = repo.head().map_err(|e| sig_err(format!("git.short_commit: {}", e)))?;
                let oid = head.peel_to_commit()
                    .map_err(|e| sig_err(format!("git.short_commit: {}", e)))?
                    .id();
                let full = oid.to_string();
                let short = &full[..std::cmp::min(7, full.len())];
                Ok(Value::String(short.to_string()))
            }
            "tag" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let head_oid = match repo.head() {
                    Ok(h) => match h.peel_to_commit() {
                        Ok(c) => c.id(),
                        Err(_) => return Ok(Value::Null),
                    },
                    Err(_) => return Ok(Value::Null),
                };
                let tag_names = repo.tag_names(None)
                    .map_err(|e| sig_err(format!("git.tag: {}", e)))?;
                for name in tag_names.iter().flatten() {
                    if let Ok(reference) = repo.revparse_single(&format!("refs/tags/{}", name)) {
                        let target_oid = reference.peel_to_commit()
                            .map(|c| c.id())
                            .unwrap_or_else(|_| reference.id());
                        if target_oid == head_oid {
                            return Ok(Value::String(name.to_string()));
                        }
                    }
                }
                Ok(Value::Null)
            }
            "tags" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let tag_names = repo.tag_names(None)
                    .map_err(|e| sig_err(format!("git.tags: {}", e)))?;
                let tags: Vec<Value> = tag_names.iter()
                    .flatten()
                    .map(|name| Value::String(name.to_string()))
                    .collect();
                Ok(Value::List(tags))
            }
            "is_dirty" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let dirty = repo_is_dirty(&repo)?;
                Ok(Value::Bool(dirty))
            }
            "is_clean" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let dirty = repo_is_dirty(&repo)?;
                Ok(Value::Bool(!dirty))
            }
            "remote_url" => {
                let repo = open_repo(&repo_path_arg(args))?;
                let remote_name = args.get(1).and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                }).unwrap_or_else(|| "origin".to_string());
                let remote = repo.find_remote(&remote_name)
                    .map_err(|e| sig_err(format!("git.remote_url: {}", e)))?;
                let url = remote.url().unwrap_or("").to_string();
                Ok(Value::String(url))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'git.{}'", func),
            ))),
        }
    }

    /// `git.clone(url, dest?, opts?) -> Ok(Path) | Err(String)`
    ///
    /// `opts` is `{ branch, depth, recursive, quiet }`. The destination is
    /// returned so the common `let d = git.clone(url, dest)?` reads as the
    /// path it just produced.
    fn git_clone(&mut self, args: &[Value]) -> IResult {
        let url = match args.first() {
            Some(Value::String(s)) => s.clone(),
            Some(other) => {
                return Err(sig_err(format!(
                    "git.clone() url must be a string, got {}",
                    other.type_name()
                )))
            }
            None => return Err(sig_err("git.clone() requires a url")),
        };

        // A trailing options map is optional, and so is the destination, so
        // the last argument decides which of the two it is.
        let mut rest = &args[1..];
        let opts = match rest.last() {
            Some(Value::Map(m)) => {
                rest = &rest[..rest.len() - 1];
                Some(m.clone())
            }
            _ => None,
        };
        if rest.len() > 1 {
            return Err(sig_err(
                "git.clone() takes a url, an optional destination and an optional options map",
            ));
        }
        let dest = match rest.first() {
            None | Some(Value::Null) => default_clone_dir(&url)?,
            Some(v) => crate::interpreter::helpers::path_arg(v, "git.clone() destination")?,
        };

        let opt = |key: &str| opts.as_ref().and_then(|m| m.get(key)).cloned();
        let flag = |key: &str| matches!(opt(key), Some(Value::Bool(true)));
        let quiet = flag("quiet");

        let mut parts = vec![CmdPart::Literal("git clone".to_string())];
        match opt("branch") {
            None | Some(Value::Null) => {}
            Some(Value::String(b)) => {
                parts.push(CmdPart::Literal(" --branch ".to_string()));
                parts.push(CmdPart::Interpolated(b));
            }
            Some(other) => {
                return Err(sig_err(format!(
                    "git.clone(): option 'branch' must be a String, got {}",
                    other.type_name()
                )))
            }
        }
        match opt("depth") {
            None | Some(Value::Null) => {}
            Some(Value::Int(n)) if n > 0 => {
                parts.push(CmdPart::Literal(format!(" --depth {}", n)))
            }
            Some(other) => {
                return Err(sig_err(format!(
                    "git.clone(): option 'depth' must be a positive Int, got {}",
                    other.display_string()
                )))
            }
        }
        if flag("recursive") {
            parts.push(CmdPart::Literal(" --recurse-submodules".to_string()));
        }
        if quiet {
            parts.push(CmdPart::Literal(" --quiet".to_string()));
        }
        // `--` so a url or destination beginning with `-` is an operand and
        // not a flag; the two operands are `Interpolated`, which is the part
        // kind that gets shell-escaped.
        parts.push(CmdPart::Literal(" -- ".to_string()));
        parts.push(CmdPart::Interpolated(url));
        parts.push(CmdPart::Literal(" ".to_string()));
        parts.push(CmdPart::Interpolated(dest.clone()));

        let mut mods = CmdModifiers::default();
        // Git writes progress to stderr. Forwarding it still captures, so a
        // long clone shows what it is doing and a failed one can still say
        // why.
        if !quiet {
            mods.forward_stderr = Some(Box::new(crate::value::StreamSink::Stderr));
        }

        // `run_cmd_parts` is where the exec permission check and `--dry-run`
        // live, so neither has to be repeated here.
        match self.run_cmd_parts(&parts, &mods)? {
            Value::ProcessResult { exit_code, stderr, .. } => {
                if exit_code == 0 {
                    Ok(Value::Ok(Box::new(Value::Path(dest))))
                } else {
                    let msg = stderr.trim();
                    Ok(Value::Err(Box::new(Value::String(if msg.is_empty() {
                        format!("git clone exited {}", exit_code)
                    } else {
                        msg.to_string()
                    }))))
                }
            }
            _ => Ok(Value::Ok(Box::new(Value::Path(dest)))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

/// The directory `git clone <url>` would pick on its own: the last path
/// segment of the url, without a `.git` suffix.
fn default_clone_dir(url: &str) -> Result<String, Signal> {
    let trimmed = url.trim_end_matches('/');
    let tail = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".git");
    if tail.is_empty() || tail == "." || tail == ".." {
        return Err(sig_err(format!(
            "git.clone(): cannot infer a directory name from '{}' — pass one explicitly",
            url
        )));
    }
    Ok(tail.to_string())
}

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn repo_path_arg(args: &[Value]) -> String {
    args.first()
        .and_then(|v| crate::interpreter::helpers::path_arg(v, "git").ok())
        .unwrap_or_else(|| ".".to_string())
}

fn open_repo(path: &str) -> Result<git2::Repository, Signal> {
    git2::Repository::discover(path)
        .map_err(|e| sig_err(format!("failed to open git repository at '{}': {}", path, e)))
}

fn repo_is_dirty(repo: &git2::Repository) -> Result<bool, Signal> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut opts))
        .map_err(|e| sig_err(format!("git status: {}", e)))?;
    Ok(!statuses.is_empty())
}
