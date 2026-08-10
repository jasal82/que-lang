//! `que.toml` manifest and `que.lock` lockfile.
//!
//! Before this existed, the only way to share Que code between repositories
//! was to copy it or to vendor it by hand: `import` resolved bare names
//! against `que_packages/`, but nothing ever put anything there, and nothing
//! recorded what version was in there.
//!
//! A manifest names the dependencies; `que install` fetches them into
//! `que_packages/` and writes a lockfile recording the exact commit each one
//! resolved to. The lockfile is what makes a checkout reproducible: it is
//! meant to be committed.
//!
//! Fetching shells out to `git` rather than linking a git client. Que is a
//! DevOps tool, so `git` is already on the machine, and shelling out inherits
//! credential helpers, ssh-agent, proxies and every corporate auth setup for
//! free — none of which we could reproduce.

use std::path::{Path, PathBuf};

/// Where a dependency's source comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A git URL. The requirement is a tag, branch or revision.
    Git(String),
    /// A directory on this machine, relative to the manifest.
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The name as written in the manifest.
    pub name: String,
    /// The directory name under `que_packages/`. Import paths are
    /// identifiers, so a hyphenated package name is stored with underscores,
    /// matching what the module loader looks for.
    pub dir_name: String,
    pub source: Source,
    /// Tag, branch or revision for a git source. `None` means the remote's
    /// default branch.
    pub requirement: Option<String>,
    /// A directory inside the git checkout that holds the package, for a
    /// repository that ships several packages (or ships one alongside
    /// unrelated code). `None` means the repository root is the package.
    pub subdir: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub dependencies: Vec<Dependency>,
}

/// A resolved dependency, as recorded in `que.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub name: String,
    pub source: String,
    pub requirement: String,
    /// The commit this resolved to. Empty for a path dependency, which has
    /// no revision to pin.
    pub revision: String,
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join("que.toml")
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join("que.lock")
}

pub fn packages_dir(root: &Path) -> PathBuf {
    root.join("que_packages")
}

/// Where git checkouts of `subdir` dependencies are kept.
///
/// A `subdir` package is only part of its repository, so the checkout cannot
/// be the package directory itself. It lives here instead, and
/// `que_packages/<name>` points into it. The leading dot keeps it out of
/// import resolution: import paths are identifiers, so no import can name it.
pub fn sources_dir(root: &Path) -> PathBuf {
    packages_dir(root).join(".sources")
}

/// Read and parse `<root>/que.toml`.
///
/// Returns `Ok(None)` when there is no manifest: a single script with no
/// dependencies is a legitimate Que program and must not need one.
pub fn load(root: &Path) -> Result<Option<Manifest>, String> {
    let path = manifest_path(root);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    parse(&text).map(Some)
}

pub fn parse(text: &str) -> Result<Manifest, String> {
    let doc: toml::Value = text
        .parse()
        .map_err(|e| format!("que.toml is not valid TOML: {}", e))?;

    let mut manifest = Manifest::default();

    if let Some(pkg) = doc.get("package") {
        manifest.name = pkg.get("name").and_then(|v| v.as_str()).map(String::from);
        manifest.version = pkg.get("version").and_then(|v| v.as_str()).map(String::from);
    }

    let Some(deps) = doc.get("dependencies") else {
        return Ok(manifest);
    };
    let table = deps
        .as_table()
        .ok_or_else(|| "[dependencies] must be a table".to_string())?;

    for (name, spec) in table {
        manifest.dependencies.push(parse_dependency(name, spec)?);
    }
    manifest.dependencies.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(manifest)
}

fn parse_dependency(name: &str, spec: &toml::Value) -> Result<Dependency, String> {
    let dir_name = name.replace('-', "_");

    // Shorthand: `name = "https://host/repo#v1.2.0"`.
    if let Some(s) = spec.as_str() {
        let (url, req) = match s.split_once('#') {
            Some((u, r)) => (u.to_string(), Some(r.to_string())),
            None => (s.to_string(), None),
        };
        return Ok(Dependency {
            name: name.to_string(),
            dir_name,
            source: Source::Git(url),
            requirement: req,
            subdir: None,
        });
    }

    let table = spec.as_table().ok_or_else(|| {
        format!(
            "dependency '{}' must be a git URL string or a table with `git` or `path`",
            name
        )
    })?;

    let git = table.get("git").and_then(|v| v.as_str());
    let path = table.get("path").and_then(|v| v.as_str());

    // Both would mean two different answers to "where does this code come
    // from", and nothing sensible to record in the lockfile.
    if git.is_some() && path.is_some() {
        return Err(format!(
            "dependency '{}' sets both `git` and `path`; pick one",
            name
        ));
    }

    let requirement = ["rev", "tag", "branch", "version"]
        .iter()
        .find_map(|k| table.get(*k).and_then(|v| v.as_str()).map(String::from));

    let source = match (git, path) {
        (Some(url), None) => Source::Git(url.to_string()),
        (None, Some(p)) => Source::Path(PathBuf::from(p)),
        _ => {
            return Err(format!(
                "dependency '{}' needs a `git` URL or a `path`",
                name
            ))
        }
    };

    if matches!(source, Source::Path(_)) && requirement.is_some() {
        return Err(format!(
            "dependency '{}' is a path dependency, so `rev`/`tag`/`branch` cannot apply to it",
            name
        ));
    }

    let subdir = match table.get("subdir") {
        None => None,
        Some(v) => {
            let s = v.as_str().ok_or_else(|| {
                format!("dependency '{}': `subdir` must be a string", name)
            })?;
            if matches!(source, Source::Path(_)) {
                return Err(format!(
                    "dependency '{}' is a path dependency, so `subdir` cannot apply to it; \
                     point `path` at the sub-directory instead",
                    name
                ));
            }
            Some(normalize_subdir(name, s)?)
        }
    };

    Ok(Dependency {
        name: name.to_string(),
        dir_name,
        source,
        requirement,
        subdir,
    })
}

/// Check that a `subdir` stays inside the checkout.
///
/// The value comes from a manifest that may itself have been fetched, so an
/// absolute path or a `..` component would let it name a directory outside
/// `que_packages/` — and `que install` would then link that into the build.
fn normalize_subdir(name: &str, raw: &str) -> Result<String, String> {
    let cleaned = raw.trim_matches('/').replace('\\', "/");
    let bad = cleaned.is_empty()
        || raw.starts_with('/')
        || Path::new(raw).is_absolute()
        || cleaned.split('/').any(|c| c == ".." || c == "." || c.is_empty());
    if bad {
        return Err(format!(
            "dependency '{}': `subdir` must be a relative path inside the repository, not '{}'",
            name, raw
        ));
    }
    Ok(cleaned)
}

/// Read `<root>/que.lock`. A missing or unreadable lockfile is not an error:
/// it just means nothing is pinned yet.
pub fn read_lock(root: &Path) -> Vec<LockEntry> {
    let Ok(text) = std::fs::read_to_string(lock_path(root)) else {
        return Vec::new();
    };
    parse_lock(&text)
}

pub fn parse_lock(text: &str) -> Vec<LockEntry> {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(list) = doc.get("package").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| {
            Some(LockEntry {
                name: item.get("name")?.as_str()?.to_string(),
                source: item.get("source")?.as_str()?.to_string(),
                requirement: item
                    .get("requirement")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                revision: item
                    .get("revision")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Render a lockfile. Entries are sorted by name so the file does not churn
/// between machines and produce spurious diffs.
pub fn render_lock(entries: &[LockEntry]) -> String {
    let mut sorted: Vec<&LockEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::from(
        "# Generated by `que install`. Commit this file: it is what makes\n\
         # a checkout of this repository resolve to the same code.\n",
    );
    for e in sorted {
        out.push_str("\n[[package]]\n");
        out.push_str(&format!("name = {}\n", toml_string(&e.name)));
        out.push_str(&format!("source = {}\n", toml_string(&e.source)));
        out.push_str(&format!("requirement = {}\n", toml_string(&e.requirement)));
        out.push_str(&format!("revision = {}\n", toml_string(&e.revision)));
    }
    out
}

pub fn write_lock(root: &Path, entries: &[LockEntry]) -> Result<(), String> {
    let path = lock_path(root);
    std::fs::write(&path, render_lock(entries))
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Find the lock entry that still applies to a dependency.
///
/// A pin is only reusable when both the source and the requirement still
/// match. Changing either in the manifest is a request to resolve again.
pub fn locked_revision<'a>(lock: &'a [LockEntry], dep: &Dependency) -> Option<&'a LockEntry> {
    let Source::Git(ref url) = dep.source else {
        return None;
    };
    let req = dep.requirement.as_deref().unwrap_or("");
    lock.iter().find(|e| {
        e.name == dep.name && e.source == *url && e.requirement == req && !e.revision.is_empty()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_table_form() {
        let m = parse(
            r#"
[package]
name = "app"
version = "0.2.0"

[dependencies]
deploy-tools = { git = "https://example.com/deploy", tag = "v1.2.0" }
"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("app"));
        assert_eq!(m.version.as_deref(), Some("0.2.0"));
        let dep = &m.dependencies[0];
        assert_eq!(dep.name, "deploy-tools");
        // Import paths are identifiers, so the directory cannot keep the hyphen.
        assert_eq!(dep.dir_name, "deploy_tools");
        assert_eq!(dep.source, Source::Git("https://example.com/deploy".into()));
        assert_eq!(dep.requirement.as_deref(), Some("v1.2.0"));
    }

    #[test]
    fn parses_the_url_shorthand() {
        let m = parse("[dependencies]\nhelpers = \"https://example.com/h#v2\"\n").unwrap();
        let dep = &m.dependencies[0];
        assert_eq!(dep.source, Source::Git("https://example.com/h".into()));
        assert_eq!(dep.requirement.as_deref(), Some("v2"));
    }

    #[test]
    fn parses_a_path_dependency() {
        let m = parse("[dependencies]\nlocal = { path = \"../shared\" }\n").unwrap();
        assert_eq!(m.dependencies[0].source, Source::Path("../shared".into()));
    }

    #[test]
    fn rejects_a_dependency_with_no_source() {
        let err = parse("[dependencies]\nx = { tag = \"v1\" }\n").unwrap_err();
        assert!(err.contains("needs a `git` URL or a `path`"), "{}", err);
    }

    #[test]
    fn rejects_a_dependency_with_two_sources() {
        let err = parse("[dependencies]\nx = { git = \"u\", path = \"p\" }\n").unwrap_err();
        assert!(err.contains("pick one"), "{}", err);
    }

    #[test]
    fn rejects_a_revision_on_a_path_dependency() {
        let err = parse("[dependencies]\nx = { path = \"p\", tag = \"v1\" }\n").unwrap_err();
        assert!(err.contains("path dependency"), "{}", err);
    }

    #[test]
    fn parses_a_subdir_so_one_repository_can_ship_several_packages() {
        let m = parse(
            "[dependencies]\nque-std = { git = \"https://example.com/que\", tag = \"v1\", subdir = \"stdlib/\" }\n",
        )
        .unwrap();
        assert_eq!(m.dependencies[0].subdir.as_deref(), Some("stdlib"));
    }

    #[test]
    fn rejects_a_subdir_that_escapes_the_checkout() {
        for bad in ["../..", "/etc", "lib/../../secrets"] {
            let err = parse(&format!(
                "[dependencies]\nx = {{ git = \"u\", subdir = \"{}\" }}\n",
                bad
            ))
            .unwrap_err();
            assert!(err.contains("inside the repository"), "{}: {}", bad, err);
        }
    }

    #[test]
    fn rejects_a_subdir_on_a_path_dependency() {
        let err = parse("[dependencies]\nx = { path = \"p\", subdir = \"lib\" }\n").unwrap_err();
        assert!(err.contains("point `path` at the sub-directory"), "{}", err);
    }

    #[test]
    fn a_manifest_without_dependencies_is_fine() {
        assert!(parse("[package]\nname = \"app\"\n").unwrap().dependencies.is_empty());
    }

    #[test]
    fn a_lockfile_round_trips() {
        let entries = vec![
            LockEntry {
                name: "b".into(),
                source: "https://example.com/b".into(),
                requirement: "v1".into(),
                revision: "beef".into(),
            },
            LockEntry {
                name: "a".into(),
                source: "https://example.com/a".into(),
                requirement: String::new(),
                revision: "cafe".into(),
            },
        ];
        let parsed = parse_lock(&render_lock(&entries));
        // Sorted by name so the file does not churn between machines.
        assert_eq!(parsed[0].name, "a");
        assert_eq!(parsed[1].revision, "beef");
    }

    #[test]
    fn a_pin_stops_applying_when_the_requirement_changes() {
        let lock = vec![LockEntry {
            name: "x".into(),
            source: "u".into(),
            requirement: "v1".into(),
            revision: "abc".into(),
        }];
        let mut dep = Dependency {
            name: "x".into(),
            dir_name: "x".into(),
            source: Source::Git("u".into()),
            requirement: Some("v1".into()),
            subdir: None,
        };
        assert!(locked_revision(&lock, &dep).is_some());
        dep.requirement = Some("v2".into());
        assert!(locked_revision(&lock, &dep).is_none());
    }
}
