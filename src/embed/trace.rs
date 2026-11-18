//! Access tracing: a record of what a script looked at on the filesystem.
//!
//! An embedder that caches a script's result needs to know what the result
//! depended on. Every filesystem read, existence check, directory listing,
//! walk and glob that goes through the interpreter's dispatch layer (the
//! layer that also enforces the capability policy) is reported to an
//! [`AccessTracer`] as an [`AccessEvent`]. Writes, directory creation,
//! removal, commands and module loads are reported too, so the record is
//! complete enough to audit.
//!
//! Each event carries an [`Observation`]: what the filesystem looked like for
//! that access, computed by [`observe`] with the same functions the
//! interpreter uses (its glob expansion, its directory walk). To check later
//! whether a cached result is still valid, call [`AccessEvent::is_unchanged`]
//! (or compare [`observe`] yourself) for every recorded listing, walk, glob and
//! existence check, and compare the content of every recorded read. File
//! contents are not hashed here: the embedder has its own content hashing
//! and cache.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What kind of access an event records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKind {
    /// File contents were read (also metadata such as size or mtime).
    Read,
    /// The existence or type of a path was checked.
    Exists,
    /// A directory was listed (one level).
    ListDir,
    /// A directory was walked recursively.
    Walk,
    /// A glob pattern was expanded. `path` is the absolute pattern.
    Glob,
    /// A file was written, appended to, copied or moved to `path`.
    Write,
    /// A directory was created.
    CreateDir,
    /// A file or directory was removed.
    Remove,
    /// A command was run. `path` is the command line as displayed (secrets
    /// redacted); it is not a filesystem path.
    Exec,
    /// A module file was loaded by `import`.
    ModuleLoad,
}

/// The type of a filesystem entry, following symlinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    Missing,
    File,
    Dir,
    /// A symlink whose target doesn't exist.
    DanglingSymlink,
    Other,
}

/// What the filesystem looked like for one access.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    /// Nothing to compare (writes, commands).
    None,
    /// The type of the entry at the path (reads, existence checks, module
    /// loads, and listings/walks of something that isn't a readable
    /// directory).
    Entry(EntryType),
    /// The sorted result of a listing (entry names), walk (absolute paths of
    /// every file and directory below) or glob (absolute matched paths).
    Entries(Vec<String>),
}

/// One filesystem access performed by a script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessEvent {
    pub kind: AccessKind,
    /// The operation that caused it, e.g. `fs.read`, `Path.glob`,
    /// `Glob.expand`, `for-in`, `import`.
    pub operation: String,
    /// The path or pattern as the script passed it.
    pub requested: String,
    /// The absolute, lexically normalised form of `requested` (`~` expanded,
    /// `.` and `..` resolved, symlinks not followed). For [`AccessKind::Glob`]
    /// the absolute pattern, for [`AccessKind::Exec`] the command line.
    pub path: PathBuf,
    /// `path` with symlinks resolved, when it exists.
    pub canonical: Option<PathBuf>,
    /// Whether the operation succeeded. A failed read of a missing file is
    /// still an input: creating the file changes the result.
    pub success: bool,
    /// The error message when the operation failed.
    pub error: Option<String>,
    /// What the filesystem looked like, see [`observe`].
    pub observed: Observation,
}

impl AccessEvent {
    /// Whether observing the same path again gives the same result. Always
    /// true for kinds with nothing to observe (writes, commands); for reads
    /// it only compares the entry type, so compare contents separately.
    pub fn is_unchanged(&self) -> bool {
        observe(self.kind, &self.path) == self.observed
    }
}

/// Receives access events. Called synchronously on the interpreter's thread,
/// in the order the accesses happen; keep it cheap.
///
/// Implemented for any `Fn(&AccessEvent) + Send + Sync`.
pub trait AccessTracer: Send + Sync {
    fn record(&self, event: &AccessEvent);
}

impl<F> AccessTracer for F
where
    F: Fn(&AccessEvent) + Send + Sync,
{
    fn record(&self, event: &AccessEvent) {
        self(event)
    }
}

/// Observe `path` the way an access of `kind` sees it.
///
/// `path` should be absolute (as in [`AccessEvent::path`]); a relative one
/// resolves against the process's working directory.
pub fn observe(kind: AccessKind, path: &Path) -> Observation {
    match kind {
        AccessKind::Read | AccessKind::Exists | AccessKind::ModuleLoad => {
            Observation::Entry(entry_type(path))
        }
        AccessKind::ListDir => match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                Observation::Entries(names)
            }
            Err(_) => Observation::Entry(entry_type(path)),
        },
        AccessKind::Walk => {
            let mut entries = Vec::new();
            match crate::interpreter::walk_all(path, &mut entries) {
                Ok(()) => {
                    entries.sort();
                    Observation::Entries(entries)
                }
                Err(_) => Observation::Entry(entry_type(path)),
            }
        }
        AccessKind::Glob => {
            let mut matches: Vec<String> =
                crate::interpreter::helpers::glob_expand(&path.to_string_lossy())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
            matches.sort();
            Observation::Entries(matches)
        }
        AccessKind::Write | AccessKind::CreateDir | AccessKind::Remove | AccessKind::Exec => {
            Observation::None
        }
    }
}

/// The type of the entry at `path`, following symlinks.
pub fn entry_type(path: &Path) -> EntryType {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => EntryType::File,
        Ok(m) if m.is_dir() => EntryType::Dir,
        Ok(_) => EntryType::Other,
        Err(_) => match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => EntryType::DanglingSymlink,
            _ => EntryType::Missing,
        },
    }
}

/// Build the event for an access of `requested`.
pub(crate) fn event(
    kind: AccessKind,
    operation: &str,
    requested: &str,
    outcome: Result<(), String>,
) -> AccessEvent {
    let path = match kind {
        AccessKind::Exec => PathBuf::from(requested),
        _ => {
            crate::permissions::absolute_path(&crate::interpreter::helpers::expand_tilde(requested))
        }
    };
    let canonical = match kind {
        AccessKind::Exec | AccessKind::Glob => None,
        _ => std::fs::canonicalize(&path).ok(),
    };
    let observed = observe(kind, &path);
    let (success, error) = match outcome {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e)),
    };
    AccessEvent {
        kind,
        operation: operation.to_string(),
        requested: requested.to_string(),
        path,
        canonical,
        success,
        error,
        observed,
    }
}
