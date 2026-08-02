//! std.git module — Git repository introspection via libgit2.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "git",
        functions: &[
            "branch", "commit", "short_commit", "tag",
            "tags", "is_dirty", "is_clean", "remote_url",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_git(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
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
}

// ── Private helpers ────────────────────────────────────────────────────────

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
