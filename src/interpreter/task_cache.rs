//! Persistent record of what each task consumed and produced.
//!
//! Modification times cannot answer "has this input changed?". A fresh clone,
//! a restored CI cache and a container layer all stamp files with a time that
//! has nothing to do with their contents, so a timestamp-only check rebuilds
//! everything in exactly the environments where a build cache matters most.
//! The reverse is just as common: `touch`ing a file, or switching branches and
//! back, rewrites the timestamp while the bytes stay identical.
//!
//! So mtime is demoted to a cheap first question. If every input is older than
//! every output, nothing has changed and the task is skipped without reading a
//! byte — the fast path stays as fast as it was. Only when a timestamp says
//! "maybe" are the contents hashed and compared against what the last
//! successful run recorded.
//!
//! The record also carries the argument/environment hash, which used to live
//! only in memory. That meant a task with parameters could never be skipped
//! across two invocations of `que`, which is every invocation in CI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What one task looked like at the end of its last successful run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TaskEntry {
    /// Hash of the task's arguments and tracked environment variables.
    pub args_hash: u64,
    /// Content hash of each declared input.
    pub inputs: BTreeMap<String, String>,
    /// Content hash of each declared output.
    pub outputs: BTreeMap<String, String>,
}

/// The cache file, loaded lazily and written after each successful task.
///
/// Written eagerly rather than at exit because a build that is interrupted
/// half way through should still get credit for the tasks that finished.
#[derive(Debug, Default, Clone)]
pub struct TaskCache {
    path: Option<PathBuf>,
    entries: BTreeMap<String, TaskEntry>,
    loaded: bool,
}

/// Hash a file's contents. `None` when the file cannot be read, which callers
/// treat as "not comparable", i.e. run the task.
pub fn hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        // A directory has no contents of its own to hash. Its mtime is still
        // meaningful, so let the timestamp check speak for it.
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

impl TaskCache {
    /// Point the cache at `<dir>/.que/task-cache.json` and read it if present.
    ///
    /// Called on first use rather than at startup so scripts that declare no
    /// tasks never touch the filesystem for this.
    pub fn load_from(&mut self, dir: &Path) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let path = dir.join(".que").join("task-cache.json");
        self.entries = read_entries(&path).unwrap_or_default();
        self.path = Some(path);
    }

    pub fn get(&self, task: &str) -> Option<&TaskEntry> {
        self.entries.get(task)
    }

    /// Record a task's state and persist immediately.
    ///
    /// A failure to write is deliberately silent: a read-only or missing
    /// directory should cost the build its cache, not its exit code.
    pub fn record(&mut self, task: &str, entry: TaskEntry) {
        if self.entries.get(task) == Some(&entry) {
            return;
        }
        self.entries.insert(task.to_string(), entry);
        if let Some(path) = &self.path {
            let _ = write_entries(path, &self.entries);
        }
    }
}

fn read_entries(path: &Path) -> Option<BTreeMap<String, TaskEntry>> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let obj = json.get("tasks")?.as_object()?;
    let mut entries = BTreeMap::new();
    for (name, raw) in obj {
        entries.insert(
            name.clone(),
            TaskEntry {
                args_hash: raw.get("args").and_then(|v| v.as_u64()).unwrap_or(0),
                inputs: read_hashes(raw.get("inputs")),
                outputs: read_hashes(raw.get("outputs")),
            },
        );
    }
    Some(entries)
}

fn read_hashes(value: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    value
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn write_entries(path: &Path, entries: &BTreeMap<String, TaskEntry>) -> std::io::Result<()> {
    let mut tasks = serde_json::Map::new();
    for (name, entry) in entries {
        tasks.insert(
            name.clone(),
            serde_json::json!({
                "args": entry.args_hash,
                "inputs": write_hashes(&entry.inputs),
                "outputs": write_hashes(&entry.outputs),
            }),
        );
    }
    // `version` so a future format change can be detected rather than
    // misread: an unrecognised file parses to no entries and everything
    // rebuilds once, which is the safe direction to fail in.
    let doc = serde_json::json!({ "version": 1, "tasks": tasks });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)
}

fn write_hashes(map: &BTreeMap<String, String>) -> serde_json::Value {
    serde_json::Value::Object(
        map.iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("que-task-cache-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn records_survive_a_reload() {
        let dir = temp_dir("reload");
        let mut cache = TaskCache::default();
        cache.load_from(&dir);
        cache.record(
            "build",
            TaskEntry {
                args_hash: 7,
                inputs: [("a.c".to_string(), "abc".to_string())].into_iter().collect(),
                outputs: [("a.o".to_string(), "def".to_string())].into_iter().collect(),
            },
        );

        let mut reloaded = TaskCache::default();
        reloaded.load_from(&dir);
        let entry = reloaded.get("build").expect("entry should have been persisted");
        assert_eq!(entry.args_hash, 7);
        assert_eq!(entry.inputs.get("a.c").map(String::as_str), Some("abc"));
        assert_eq!(entry.outputs.get("a.o").map(String::as_str), Some("def"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_cache_file_is_not_an_error() {
        let dir = temp_dir("missing");
        let mut cache = TaskCache::default();
        cache.load_from(&dir);
        assert!(cache.get("build").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_cache_file_rebuilds_rather_than_failing() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(dir.join(".que")).unwrap();
        std::fs::write(dir.join(".que").join("task-cache.json"), "{ not json").unwrap();
        let mut cache = TaskCache::default();
        cache.load_from(&dir);
        assert!(cache.get("build").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_contents_hash_identically_regardless_of_timestamp() {
        let dir = temp_dir("hash");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, b"same contents").unwrap();
        std::fs::write(&b, b"same contents").unwrap();
        assert_eq!(hash_file(&a), hash_file(&b));

        std::fs::write(&b, b"different").unwrap();
        assert_ne!(hash_file(&a), hash_file(&b));

        assert_eq!(hash_file(&dir.join("nope.txt")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
