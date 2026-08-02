//! std.watch module — Watch files and directories for changes.
//!
//! Implemented by polling rather than by inotify/FSEvents/ReadDirectoryChangesW.
//!
//! That is a deliberate trade. Native watch APIs are lower-latency and cheaper
//! at rest, but the filesystems a DevOps script actually runs against are
//! frequently the ones they do not work on: bind-mounted Docker volumes, NFS
//! and SMB shares, `/mnt/c` under WSL, and network home directories all either
//! drop events silently or deliver none at all. A watcher that misses changes
//! is worse than a slower one, because the failure is a build that quietly
//! stops rebuilding rather than an error anyone sees. Polling has no such
//! blind spot, needs no dependency, and the latency budget here is a human
//! noticing their editor saved a file.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub(super) fn module() -> StdModule {
    StdModule {
        name: "watch",
        functions: &["wait", "run", "snapshot"],
    }
}

/// One observed change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

impl ChangeKind {
    fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Created => "created",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
        }
    }
}

/// What a file looked like at snapshot time.
///
/// Size is compared alongside mtime because many filesystems store mtime at
/// one-second granularity, and a rewrite within the same second is exactly
/// what a build loop produces.
type Snapshot = BTreeMap<String, (Option<std::time::SystemTime>, u64)>;

/// Options accepted by `wait` and `run`.
struct WatchOpts {
    interval: Duration,
    debounce: Duration,
    timeout: Option<Duration>,
    ignore: Vec<String>,
    initial: bool,
    times: Option<u64>,
}

impl Default for WatchOpts {
    fn default() -> Self {
        WatchOpts {
            interval: Duration::from_millis(250),
            debounce: Duration::from_millis(200),
            timeout: None,
            ignore: vec![
                ".git/**".to_string(),
                "**/.git/**".to_string(),
                "target/**".to_string(),
                "**/target/**".to_string(),
                "node_modules/**".to_string(),
                "**/node_modules/**".to_string(),
            ],
            initial: false,
            times: None,
        }
    }
}

impl Interpreter {
    pub(crate) fn call_watch(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "snapshot" => {
                let roots = watch_roots(args.first(), "watch.snapshot")?;
                let opts = watch_opts(args.get(1), "watch.snapshot")?;
                let snap = take_snapshot(&roots, &opts);
                Ok(Value::Int(snap.len() as i64))
            }
            "wait" => {
                let roots = watch_roots(args.first(), "watch.wait")?;
                let opts = watch_opts(args.get(1), "watch.wait")?;
                match self.watch_once(&roots, &opts)? {
                    Some(changes) => Ok(Value::Ok(Box::new(Value::List(changes)))),
                    None => Ok(Value::Err(Box::new(Value::String(
                        "watch timed out with no changes".to_string(),
                    )))),
                }
            }
            "run" => {
                let roots = watch_roots(args.first(), "watch.run")?;
                let callback = args.get(1).cloned().ok_or_else(|| {
                    sig_err("watch.run requires a callback: watch.run(paths, fn(changes) { ... })")
                })?;
                let opts = watch_opts(args.get(2), "watch.run")?;

                // A dry run must not block forever waiting for an edit that
                // is never coming. Announcing and returning keeps
                // `que --dry-run` a thing you can run on any script.
                if self.dry_run_skip(format!(
                    "watch {} path{}",
                    roots.len(),
                    if roots.len() == 1 { "" } else { "s" }
                )) {
                    return Ok(Value::Ok(Box::new(Value::Int(0))));
                }

                let mut runs: i64 = 0;
                if opts.initial {
                    self.call_value(callback.clone(), vec![Value::List(Vec::new())])?;
                    runs += 1;
                }
                loop {
                    if let Some(limit) = opts.times {
                        if runs as u64 >= limit {
                            return Ok(Value::Ok(Box::new(Value::Int(runs))));
                        }
                    }
                    match self.watch_once(&roots, &opts)? {
                        Some(changes) => {
                            self.call_value(callback.clone(), vec![Value::List(changes)])?;
                            runs += 1;
                        }
                        // Timed out: report what did happen rather than
                        // pretending the whole call failed.
                        None => return Ok(Value::Ok(Box::new(Value::Int(runs)))),
                    }
                }
            }
            _ => Err(sig_err(format!("unknown function 'watch.{}'", func))),
        }
    }

    /// Block until something under `roots` changes, or the timeout expires.
    ///
    /// Returns the changed entries as `{ path, kind }` maps, or `None` on
    /// timeout.
    fn watch_once(
        &mut self,
        roots: &[String],
        opts: &WatchOpts,
    ) -> Result<Option<Vec<Value>>, Signal> {
        let started = Instant::now();
        let mut previous = take_snapshot(roots, opts);

        loop {
            // Polled here rather than only at statement boundaries: a script
            // blocked in `watch.run` is otherwise unkillable by Ctrl-C, and
            // the whole point of unwinding is that `defer` gets to run.
            self.check_interrupt()?;

            if let Some(limit) = opts.timeout {
                if started.elapsed() >= limit {
                    return Ok(None);
                }
            }

            std::thread::sleep(opts.interval);
            let current = take_snapshot(roots, opts);
            if current == previous {
                continue;
            }

            // Debounce: an editor's save is a write, a rename and a chmod,
            // and a compiler's output is hundreds of files. Waiting for the
            // tree to stop moving turns that into one rebuild instead of
            // three hundred.
            let mut settled = current;
            if !opts.debounce.is_zero() {
                loop {
                    self.check_interrupt()?;
                    std::thread::sleep(opts.debounce);
                    let again = take_snapshot(roots, opts);
                    if again == settled {
                        break;
                    }
                    settled = again;
                }
            }

            let changes = diff_snapshots(&previous, &settled);
            if changes.is_empty() {
                // Everything that changed changed back. Nothing to report.
                previous = settled;
                continue;
            }
            return Ok(Some(changes));
        }
    }
}

/// Compare two snapshots into `{ path, kind }` maps, sorted by path so a
/// callback sees the same order every time.
fn diff_snapshots(before: &Snapshot, after: &Snapshot) -> Vec<Value> {
    let mut changes: Vec<(String, ChangeKind)> = Vec::new();
    for (path, state) in after {
        match before.get(path) {
            None => changes.push((path.clone(), ChangeKind::Created)),
            Some(old) if old != state => changes.push((path.clone(), ChangeKind::Modified)),
            Some(_) => {}
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            changes.push((path.clone(), ChangeKind::Deleted));
        }
    }
    changes.sort();
    changes
        .into_iter()
        .map(|(path, kind)| {
            let mut m = BTreeMap::new();
            m.insert("path".to_string(), Value::Path(path));
            m.insert("kind".to_string(), Value::String(kind.as_str().to_string()));
            Value::Map(m)
        })
        .collect()
}

fn take_snapshot(roots: &[String], opts: &WatchOpts) -> Snapshot {
    let mut snap = Snapshot::new();
    for root in roots {
        let path = std::path::Path::new(root);
        if path.is_dir() {
            walk_into(path, opts, &mut snap, 0);
        } else {
            record(path, opts, &mut snap);
        }
    }
    snap
}

/// Depth cap: a symlink loop under a watched directory would otherwise spin
/// forever, once per poll.
const MAX_DEPTH: usize = 64;

fn walk_into(dir: &std::path::Path, opts: &WatchOpts, snap: &mut Snapshot, depth: usize) {
    if depth > MAX_DEPTH || is_ignored(dir, opts) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // A directory that vanished or cannot be read is not an error worth
        // aborting a watch loop over; it will show up as a deletion.
        Err(_) => return,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk_into(&path, opts, snap, depth + 1),
            Ok(_) => record(&path, opts, snap),
            Err(_) => {}
        }
    }
}

fn record(path: &std::path::Path, opts: &WatchOpts, snap: &mut Snapshot) {
    if is_ignored(path, opts) {
        return;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        snap.insert(
            path.to_string_lossy().into_owned(),
            (meta.modified().ok(), meta.len()),
        );
    }
}

fn is_ignored(path: &std::path::Path, opts: &WatchOpts) -> bool {
    if opts.ignore.is_empty() {
        return false;
    }
    let text = path.to_string_lossy();
    opts.ignore.iter().any(|pattern| {
        glob::Pattern::new(pattern)
            .map(|p| p.matches(&text))
            .unwrap_or(false)
    })
}

fn watch_roots(arg: Option<&Value>, who: &str) -> Result<Vec<String>, Signal> {
    let mut roots = Vec::new();
    match arg {
        Some(Value::List(items)) => {
            for item in items {
                roots.push(one_root(item, who)?);
            }
        }
        Some(other) => roots.push(one_root(other, who)?),
        None => {
            return Err(sig_err(format!(
                "{} requires a path or a list of paths",
                who
            )))
        }
    }
    if roots.is_empty() {
        return Err(sig_err(format!("{}: no paths to watch", who)));
    }
    Ok(roots)
}

fn one_root(v: &Value, who: &str) -> Result<String, Signal> {
    match v {
        Value::Path(_) | Value::String(_) => crate::interpreter::helpers::path_arg(v, who),
        // A glob is expanded once, here, so a file created later that matches
        // it would not be seen. Watching the containing directory and
        // filtering in the callback is the honest way to express that.
        Value::Glob(_) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{}: watch a directory rather than a glob — a glob is expanded once, so files created afterwards would never be noticed",
                who
            ),
        ))),
        other => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{}: expected a Path or String, got {}", who, other.type_name()),
        ))),
    }
}

fn watch_opts(arg: Option<&Value>, who: &str) -> Result<WatchOpts, Signal> {
    let mut opts = WatchOpts::default();
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
    if let Some(v) = map.get("interval") {
        opts.interval = duration_opt(v, who, "interval")?;
    }
    if let Some(v) = map.get("debounce") {
        opts.debounce = duration_opt(v, who, "debounce")?;
    }
    match map.get("timeout") {
        Some(Value::Null) | None => {}
        Some(v) => opts.timeout = Some(duration_opt(v, who, "timeout")?),
    }
    if let Some(Value::Bool(b)) = map.get("initial") {
        opts.initial = *b;
    }
    match map.get("times") {
        Some(Value::Int(n)) if *n > 0 => opts.times = Some(*n as u64),
        Some(Value::Int(_)) => {
            return Err(sig_err(format!("{}: times must be positive", who)))
        }
        _ => {}
    }
    match map.get("ignore") {
        Some(Value::List(items)) => {
            opts.ignore = items.iter().map(|v| v.display_string()).collect();
        }
        Some(Value::String(s)) | Some(Value::Glob(s)) => opts.ignore = vec![s.clone()],
        _ => {}
    }
    Ok(opts)
}

fn duration_opt(v: &Value, who: &str, key: &str) -> Result<Duration, Signal> {
    let ms = match v {
        Value::Duration(val, unit) => super::super::helpers::duration_to_ms(*val, *unit),
        Value::Int(n) => *n as f64,
        Value::Float(f) => *f,
        other => {
            return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "{}: {} must be a Duration, got {}",
                    who,
                    key,
                    other.type_name()
                ),
            )))
        }
    };
    if ms < 0.0 {
        return Err(sig_err(format!("{}: {} must not be negative", who, key)));
    }
    Ok(Duration::from_millis(ms as u64))
}

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> WatchOpts {
        WatchOpts {
            ignore: Vec::new(),
            ..WatchOpts::default()
        }
    }

    #[test]
    fn diff_reports_creation_modification_and_deletion() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let mut before = Snapshot::new();
        before.insert("a".into(), (Some(t), 1));
        before.insert("b".into(), (Some(t), 1));
        let mut after = Snapshot::new();
        after.insert("a".into(), (Some(t), 1));
        after.insert("b".into(), (Some(t), 2));
        after.insert("c".into(), (Some(t), 1));

        let changes = diff_snapshots(&before, &after);
        let described: Vec<String> = changes
            .iter()
            .map(|v| match v {
                Value::Map(m) => format!(
                    "{}:{}",
                    m["path"].display_string(),
                    m["kind"].display_string()
                ),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(described, vec!["b:modified", "c:created"]);
    }

    #[test]
    fn a_same_second_rewrite_is_still_a_change() {
        // Many filesystems store mtime at one-second granularity, so size has
        // to be part of the comparison or a fast rebuild loop goes unnoticed.
        let t = std::time::SystemTime::UNIX_EPOCH;
        let mut before = Snapshot::new();
        before.insert("a".into(), (Some(t), 10));
        let mut after = Snapshot::new();
        after.insert("a".into(), (Some(t), 11));
        assert_eq!(diff_snapshots(&before, &after).len(), 1);
    }

    #[test]
    fn ignored_paths_are_left_out_of_the_snapshot() {
        let mut o = opts();
        o.ignore = vec!["**/target/**".to_string()];
        assert!(is_ignored(std::path::Path::new("/x/target/debug/app"), &o));
        assert!(!is_ignored(std::path::Path::new("/x/src/main.rs"), &o));
    }

    #[test]
    fn a_glob_root_is_refused_with_an_explanation() {
        let err = one_root(&Value::Glob("src/**/*.rs".into()), "watch.run").unwrap_err();
        let text = format!("{:?}", err);
        assert!(text.contains("expanded once"), "{}", text);
    }

    #[test]
    fn durations_are_accepted_in_any_unit() {
        use crate::token::DurationUnit;
        let d = duration_opt(
            &Value::Duration(2.0, DurationUnit::Seconds),
            "watch.run",
            "interval",
        )
        .unwrap();
        assert_eq!(d, Duration::from_millis(2000));
    }
}
