//! `que install` — fetch the manifest's dependencies into `que_packages/`.
//!
//! See [`crate::manifest`] for why fetching shells out to `git`.

use std::path::Path;
use std::process::Command;

use crate::manifest::{self, Dependency, LockEntry, Manifest, Source};

#[derive(Debug)]
pub struct Report {
    pub lines: Vec<String>,
    /// Set when `--locked` was asked for and resolution would have changed
    /// the lockfile.
    pub lock_changed: bool,
}

/// Install every dependency of the manifest at `root`.
///
/// With `locked`, nothing is resolved that is not already pinned: an
/// unpinned dependency is an error rather than a silent fetch of whatever
/// the branch points at today. That is the mode CI wants.
pub fn install(root: &Path, locked: bool) -> Result<Report, String> {
    let Some(manifest) = manifest::load(root)? else {
        return Err(format!(
            "no que.toml in {}; dependencies are declared there",
            root.display()
        ));
    };
    install_manifest(root, &manifest, locked)
}

fn install_manifest(root: &Path, manifest: &Manifest, locked: bool) -> Result<Report, String> {
    let mut report = Report {
        lines: Vec::new(),
        lock_changed: false,
    };

    if manifest.dependencies.is_empty() {
        report.lines.push("no dependencies".to_string());
        return Ok(report);
    }

    let old_lock = manifest::read_lock(root);
    let packages = manifest::packages_dir(root);
    std::fs::create_dir_all(&packages)
        .map_err(|e| format!("cannot create {}: {}", packages.display(), e))?;

    let mut new_lock: Vec<LockEntry> = Vec::new();

    for dep in &manifest.dependencies {
        let dest = packages.join(&dep.dir_name);
        match &dep.source {
            Source::Path(rel) => {
                let src = root.join(rel);
                link_path_dependency(&src, &dest, dep)?;
                report
                    .lines
                    .push(format!("{} -> {} (path)", dep.name, src.display()));
            }
            Source::Git(url) => {
                let pinned = manifest::locked_revision(&old_lock, dep);
                let revision = match pinned {
                    Some(entry) => entry.revision.clone(),
                    None if locked => {
                        return Err(format!(
                            "dependency '{}' is not in que.lock; run `que install` without --locked and commit the result",
                            dep.name
                        ))
                    }
                    None => resolve_revision(url, dep.requirement.as_deref())?,
                };
                let action = fetch_git(url, &revision, &dest, &dep.name)?;
                report.lines.push(format!(
                    "{} {} @ {} ({})",
                    dep.name,
                    dep.requirement.as_deref().unwrap_or("default branch"),
                    &revision[..revision.len().min(12)],
                    action
                ));
                new_lock.push(LockEntry {
                    name: dep.name.clone(),
                    source: url.clone(),
                    requirement: dep.requirement.clone().unwrap_or_default(),
                    revision,
                });
            }
        }
    }

    // Dropping a dependency from the manifest must drop its pin too,
    // otherwise the lockfile grows entries nothing refers to.
    let changed = new_lock != sorted_by_name(&old_lock);
    if changed && !locked {
        manifest::write_lock(root, &new_lock)?;
    }
    report.lock_changed = changed;
    Ok(report)
}

fn sorted_by_name(entries: &[LockEntry]) -> Vec<LockEntry> {
    let mut v = entries.to_vec();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Make a path dependency reachable under `que_packages/`.
///
/// A symlink rather than a copy: a path dependency exists so that edits in
/// the other directory are picked up immediately, and a copy would silently
/// go stale.
fn link_path_dependency(src: &Path, dest: &Path, dep: &Dependency) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!(
            "dependency '{}' points at {}, which is not a directory",
            dep.name,
            src.display()
        ));
    }
    remove_existing(dest)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dest)
            .map_err(|e| format!("cannot link {}: {}", dest.display(), e))
    }
    #[cfg(windows)]
    {
        // Windows has directory symlinks, but creating one needs Developer
        // Mode or an elevated shell. Falling back to a copy keeps `que
        // install` working for everyone; the cost is that edits to the
        // dependency are not picked up until the next install.
        match std::os::windows::fs::symlink_dir(src, dest) {
            Ok(()) => Ok(()),
            Err(_) => crate::interpreter::helpers::copy_dir_recursive(src, dest)
                .map_err(|e| format!("cannot copy {}: {}", src.display(), e)),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        crate::interpreter::helpers::copy_dir_recursive(src, dest)
            .map_err(|e| format!("cannot copy {}: {}", src.display(), e))
    }
}

fn remove_existing(dest: &Path) -> Result<(), String> {
    // symlink_metadata, not metadata: a dangling symlink still has to go,
    // and `exists()` follows the link and reports false for one.
    if std::fs::symlink_metadata(dest).is_err() {
        return Ok(());
    }
    let result = if dest.is_dir() && !dest.is_symlink() {
        std::fs::remove_dir_all(dest)
    } else {
        std::fs::remove_file(dest)
    };
    result.map_err(|e| format!("cannot replace {}: {}", dest.display(), e))
}

/// Ask the remote what a tag/branch resolves to, without cloning.
fn resolve_revision(url: &str, requirement: Option<&str>) -> Result<String, String> {
    // A requirement that is already a full commit id has nothing to resolve,
    // and `ls-remote` would not find it.
    if let Some(req) = requirement {
        if req.len() == 40 && req.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(req.to_string());
        }
    }

    let mut cmd = Command::new("git");
    cmd.arg("ls-remote").arg(url);
    if let Some(req) = requirement {
        cmd.arg(req);
    } else {
        cmd.arg("HEAD");
    }
    let out = run(cmd, "git ls-remote")?;

    // `ls-remote <tag>` lists both `refs/tags/v1` and the dereferenced
    // `refs/tags/v1^{}`. The latter is the commit an annotated tag points at,
    // which is what we want to record.
    let mut best: Option<String> = None;
    for line in out.lines() {
        let mut parts = line.split_whitespace();
        let (Some(oid), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name.ends_with("^{}") {
            return Ok(oid.to_string());
        }
        if best.is_none() {
            best = Some(oid.to_string());
        }
    }
    best.ok_or_else(|| {
        format!(
            "{} does not have {}",
            url,
            requirement.unwrap_or("a default branch")
        )
    })
}

/// What `fetch_git` did, for the report.
fn fetch_git(url: &str, revision: &str, dest: &Path, name: &str) -> Result<&'static str, String> {
    if dest.join(".git").is_dir() {
        if head_revision(dest).as_deref() == Some(revision) {
            return Ok("up to date");
        }
        let mut fetch = Command::new("git");
        fetch
            .current_dir(dest)
            .args(["fetch", "--quiet", "origin", revision]);
        // A server with uploadpack.allowReachableSHA1InWant disabled refuses
        // a request for a bare commit id, so fall back to fetching everything.
        if run(fetch, "git fetch").is_err() {
            let mut all = Command::new("git");
            all.current_dir(dest).args(["fetch", "--quiet", "origin"]);
            run(all, "git fetch")?;
        }
        checkout(dest, revision)?;
        return Ok("updated");
    }

    remove_existing(dest)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    let mut clone = Command::new("git");
    clone.args(["clone", "--quiet", url]).arg(dest);
    run(clone, &format!("git clone for '{}'", name))?;
    checkout(dest, revision)?;
    Ok("cloned")
}

fn head_revision(dir: &Path) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(["rev-parse", "HEAD"]);
    run(cmd, "git rev-parse").ok().map(|s| s.trim().to_string())
}

fn checkout(dir: &Path, revision: &str) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(["checkout", "--quiet", revision]);
    run(cmd, "git checkout").map(|_| ())
}

fn run(mut cmd: Command, what: &str) -> Result<String, String> {    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "git is not installed, and Que fetches dependencies with it".to_string()
        } else {
            format!("{} failed to start: {}", what, e)
        }
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{} failed: {}", what, stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("que_install_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_manifest_with_no_dependencies_installs_nothing() {
        let dir = tmp("empty");
        std::fs::write(dir.join("que.toml"), "[package]\nname = \"app\"\n").unwrap();
        let report = install(&dir, false).unwrap();
        assert_eq!(report.lines, vec!["no dependencies".to_string()]);
        assert!(!manifest::packages_dir(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_manifest_is_an_error_that_says_where_to_look() {
        let dir = tmp("nomanifest");
        let err = install(&dir, false).unwrap_err();
        assert!(err.contains("no que.toml"), "{}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_dependency_is_linked_not_copied() {
        let dir = tmp("pathdep");
        let shared = dir.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("mod.que"), "pub fn hi() { 1 }").unwrap();
        std::fs::write(
            dir.join("que.toml"),
            "[dependencies]\nshared = { path = \"shared\" }\n",
        )
        .unwrap();

        install(&dir, false).unwrap();
        let installed = manifest::packages_dir(&dir).join("shared");
        assert!(installed.is_symlink());

        // The point of a path dependency: an edit next door is picked up.
        std::fs::write(shared.join("mod.que"), "pub fn hi() { 2 }").unwrap();
        assert_eq!(
            std::fs::read_to_string(installed.join("mod.que")).unwrap(),
            "pub fn hi() { 2 }"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locked_refuses_to_resolve_something_that_is_not_pinned() {
        let dir = tmp("locked");
        std::fs::write(
            dir.join("que.toml"),
            "[dependencies]\nx = { git = \"https://example.invalid/x\", tag = \"v1\" }\n",
        )
        .unwrap();
        let err = install(&dir, true).unwrap_err();
        assert!(err.contains("not in que.lock"), "{}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_dependency_must_point_at_a_directory() {
        let dir = tmp("badpath");
        std::fs::write(
            dir.join("que.toml"),
            "[dependencies]\nx = { path = \"nowhere\" }\n",
        )
        .unwrap();
        let err = install(&dir, false).unwrap_err();
        assert!(err.contains("not a directory"), "{}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
