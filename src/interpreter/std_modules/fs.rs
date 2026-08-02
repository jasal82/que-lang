//! std.fs module — File system operations.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "fs",
        functions: &[
            "read", "write", "exists",
            "read_secret",
            "atomic_write", "temp_file", "temp_dir",
            "copy_dir", "remove_dir", "find",
            "read_lines", "write_lines",
            "transform",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_fs(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "read" => {
                let path = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "fs.read requires 1 argument"))
                })?;
                let p = path_str(path, "fs.read")?;
                match std::fs::read_to_string(&p) {
                    Ok(content) => Ok(Value::Ok(Box::new(Value::String(content)))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "write" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fs.write requires 2 arguments",
                    )));
                }
                let p = path_str(&args[0], "fs.write")?;
                let content = args[1].display_string();
                if self.dry_run_skip(format!("write {} ({} bytes)", p, content.len())) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match std::fs::write(&p, &content) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "exists" => {
                let path = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(ErrorKind::ArityMismatch, "exists requires 1 argument"))
                })?;
                let p = path_str(path, "exists")?;
                Ok(Value::Bool(std::path::Path::new(&p).exists()))
            }
            "read_secret" => {
                // Mounted secret files (`/run/secrets/...`, a Kubernetes
                // projected volume) are the other place secrets arrive.
                // Reading one with `fs.read` yields a plain String that
                // nothing redacts, so this is the entry point that also
                // registers the value with the scrubber.
                let path = args.first().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fs.read_secret requires 1 argument",
                    ))
                })?;
                let p = path_str(path, "fs.read_secret")?;
                match std::fs::read_to_string(&p) {
                    Ok(content) => {
                        // A trailing newline is an artefact of how the file
                        // was written, not part of the token, and sending it
                        // in an Authorization header is a long debugging
                        // session.
                        let value = content.trim_end_matches(['\n', '\r']).to_string();
                        self.register_secret(&value);
                        Ok(Value::Ok(Box::new(Value::Secret(value))))
                    }
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "atomic_write" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fs.atomic_write requires 2 arguments: path, content",
                    )));
                }
                let dest = path_str(&args[0], "fs.atomic_write: first argument")?;
                let content = args[1].display_string();
                if self.dry_run_skip(format!("write {} ({} bytes, atomic)", dest, content.len())) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                let dest_path = std::path::Path::new(&dest);
                let parent = dest_path.parent().unwrap_or(std::path::Path::new("."));
                match tempfile_in(parent) {
                    Ok((tmp_path, tmp_content_path)) => {
                        let _ = tmp_path;
                        match std::fs::write(&tmp_content_path, &content) {
                            Ok(_) => match std::fs::rename(&tmp_content_path, &dest) {
                                Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                                Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                            },
                            Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                        }
                    }
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "temp_file" => {
                let prefix = match args.first() {
                    Some(Value::String(s)) | Some(Value::Path(s)) => s.clone(),
                    _ => "que_tmp_".to_string(),
                };
                let suffix = match args.get(1) {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let dir = temp_base_opt(args, "fs.temp_file")?;
                match create_temp_file_in(&prefix, &suffix, dir.as_deref()) {
                    Ok(path) => Ok(Value::Ok(Box::new(Value::Path(path)))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "temp_dir" => {
                let prefix = match args.first() {
                    Some(Value::String(s)) | Some(Value::Path(s)) => s.clone(),
                    _ => "que_tmp_".to_string(),
                };
                let dir = temp_base_opt(args, "fs.temp_dir")?;
                match create_temp_dir_in(&prefix, dir.as_deref()) {
                    Ok(path) => Ok(Value::Ok(Box::new(Value::Path(path)))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "copy_dir" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fs.copy_dir requires 2 arguments: src, dest",
                    )));
                }
                let src = path_str(&args[0], "fs.copy_dir: src")?;
                let dest = path_str(&args[1], "fs.copy_dir: dest")?;
                let src_path = std::path::Path::new(&src);
                if !src_path.exists() {
                    return Ok(Value::Err(Box::new(Value::String(format!(
                        "source directory does not exist: {}",
                        src
                    )))));
                }
                if self.dry_run_skip(format!("copy dir {} -> {}", src, dest)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                // `dest` names the tree itself, so an existing directory is
                // merged into rather than nested under -- that is the whole
                // difference from `src.copy_to(dest)`, which follows `cp`.
                match crate::interpreter::helpers::copy_dir_recursive(
                    src_path,
                    std::path::Path::new(&dest),
                ) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "remove_dir" => {
                let path = match args.first() {
                    Some(val) => path_str(val, "fs.remove_dir: argument")?,
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "fs.remove_dir requires 1 argument",
                    ))),
                };
                if self.dry_run_skip(format!("remove {}", path)) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match std::fs::remove_dir_all(&path) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "find" => {
                let dir = match args.first() {
                    Some(val) => path_str(val, "fs.find: first argument")?,
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "fs.find requires at least 1 argument",
                    ))),
                };
                let pattern = match args.get(1) {
                    Some(Value::String(s)) => Some(s.clone()),
                    _ => None,
                };
                match find_files(&dir, pattern.as_deref()) {
                    Ok(paths) => Ok(Value::List(paths.into_iter().map(Value::Path).collect())),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
                }
            }
            "read_lines" => {
                let path = match args.first() {
                    Some(val) => path_str(val, "fs.read_lines: argument")?,
                    None => return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "fs.read_lines requires a path argument",
                    ))),
                };
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        let lines: Vec<Value> = content.lines()
                            .map(|l| Value::String(l.to_string()))
                            .collect();
                        Ok(Value::Ok(Box::new(Value::List(lines))))
                    }
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "write_lines" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch, "fs.write_lines requires 2 arguments: lines, path",
                    )));
                }
                let lines_val = &args[0];
                let path = path_str(&args[1], "fs.write_lines: second argument")?;
                let lines: Vec<String> = match lines_val {
                    Value::List(items) => items.iter().map(|v| v.display_string()).collect(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch, "fs.write_lines: first argument must be a list",
                    ))),
                };
                let content = lines.join("\n") + "\n";
                if self.dry_run_skip(format!("write {} ({} lines)", path, lines.len())) {
                    return Ok(Value::Ok(Box::new(Value::Null)));
                }
                match std::fs::write(&path, content) {
                    Ok(_) => Ok(Value::Ok(Box::new(Value::Null))),
                    Err(e) => Ok(Value::Err(Box::new(Value::String(e.to_string())))),
                }
            }
            "transform" => {
                if args.len() < 2 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::ArityMismatch,
                        "fs.transform requires 2 arguments: path, fn(content) -> content",
                    )));
                }
                let p = path_str(&args[0], "fs.transform")?;
                let func_val = args[1].clone();
                let content = std::fs::read_to_string(&p).map_err(|e| {
                    Signal::Error(QueError::new(ErrorKind::IoError, e.to_string()))
                })?;
                let result = self.call_value(func_val, vec![Value::String(content)])?;
                let new_content = match &result {
                    Value::String(s) => s.clone(),
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "fs.transform: callback must return a String",
                    ))),
                };
                if self.dry_run_skip(format!("write {} ({} bytes, transformed)", p, new_content.len())) {
                    return Ok(Value::Null);
                }
                atomic_write_str(&p, &new_content)?;
                Ok(Value::Null)            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'fs.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

pub(super) fn path_str(val: &Value, fn_name: &str) -> Result<String, Signal> {
    crate::interpreter::helpers::path_arg(val, fn_name)
}

pub(super) fn atomic_write_str(dest: &str, content: &str) -> Result<(), Signal> {
    let dest_path = std::path::Path::new(dest);
    let parent = dest_path.parent().unwrap_or(std::path::Path::new("."));
    let (_, tmp_path) = tempfile_in(parent).map_err(|e| {
        Signal::Error(QueError::new(ErrorKind::IoError, e))
    })?;
    std::fs::write(&tmp_path, content).map_err(|e| {
        Signal::Error(QueError::new(ErrorKind::IoError, e.to_string()))
    })?;
    std::fs::rename(&tmp_path, dest).map_err(|e| {
        Signal::Error(QueError::new(ErrorKind::IoError, e.to_string()))
    })?;
    Ok(())
}

fn tempfile_in(dir: &std::path::Path) -> Result<(String, String), String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp_name = format!(".que_atomic_{}", ts);
    let tmp_path = dir.join(&tmp_name);
    std::fs::File::create(&tmp_path)
        .map_err(|e| e.to_string())?;
    let path_str = tmp_path.to_string_lossy().into_owned();
    Ok((path_str.clone(), path_str))
}

/// Read the `dir` key from a trailing options map, if one was passed.
///
/// The base directory has to be a choice: `$TMPDIR` is often a tmpfs too
/// small for a build artefact, on a different filesystem than the
/// destination (so the atomic rename that a temp file exists for degrades
/// into a copy), or cleaned out from under a long-running job.
fn temp_base_opt(args: &[Value], who: &str) -> Result<Option<String>, Signal> {
    let map = match args.last() {
        Some(Value::Map(m)) => m,
        _ => return Ok(None),
    };
    match map.get("dir") {
        None | Some(Value::Null) => Ok(None),
        Some(val) => path_str(val, &format!("{}: dir", who)).map(Some),
    }
}

pub(crate) fn create_temp_file_in(
    prefix: &str,
    suffix: &str,
    parent: Option<&str>,
) -> Result<String, String> {
    let base = temp_base(parent)?;
    // Retry rather than fail: `create_new` losing a race is the mechanism
    // working, not an error to report.
    for _ in 0..16 {
        let path = base.join(format!("{}{}{}", prefix, unique_token(), suffix));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!(
        "could not create a temporary file in {}",
        base.display()
    ))
}

pub(crate) fn create_temp_dir_in(prefix: &str, parent: Option<&str>) -> Result<String, String> {
    let base = temp_base(parent)?;
    for _ in 0..16 {
        let path = base.join(format!("{}{}", prefix, unique_token()));
        // `create_dir`, not `create_dir_all`: succeeding on a directory that
        // already exists is how a caller ends up sharing a "private"
        // temporary directory with whoever created it first.
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!(
        "could not create a temporary directory in {}",
        base.display()
    ))
}

/// Resolve the directory temporary entries are created in, creating it if a
/// caller named one explicitly.
fn temp_base(parent: Option<&str>) -> Result<std::path::PathBuf, String> {
    match parent {
        Some(p) => {
            let p = std::path::Path::new(p);
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            Ok(p.to_path_buf())
        }
        None => Ok(std::env::temp_dir()),
    }
}

/// An unpredictable name component for a temporary path.
///
/// A nanosecond timestamp is not one: the system temp directory is
/// world-writable, and a name an attacker can predict is a name they can
/// create first, as a symlink pointing wherever they like. `create_new` and
/// `create_dir` close the race; this makes it not worth entering.
fn unique_token() -> String {
    let mut bytes = [0u8; 12];
    let seeded = {
        #[cfg(unix)]
        {
            use std::io::Read;
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut bytes))
                .is_ok()
        }
        #[cfg(not(unix))]
        {
            false
        }
    };
    if !seeded {
        // Fallback: still unique within and across processes, just not
        // unguessable. The `create_new` check is what keeps this safe.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mix = ts
            ^ ((std::process::id() as u64) << 32)
            ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        bytes[..8].copy_from_slice(&mix.to_le_bytes());
    }
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn find_files(dir: &str, pattern: Option<&str>) -> Result<Vec<String>, String> {
    let mut results = Vec::new();
    find_files_recursive(std::path::Path::new(dir), pattern, &mut results)?;
    Ok(results)
}

fn find_files_recursive(
    dir: &std::path::Path,
    pattern: Option<&str>,
    results: &mut Vec<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_dir() {
            find_files_recursive(&path, pattern, results)?;
        } else if let Some(pat) = pattern {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if glob_matches(pat, &file_name) {
                results.push(path.to_string_lossy().into_owned());
            }
        } else {
            results.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    let pat_bytes = pattern.as_bytes();
    let name_bytes = name.as_bytes();
    glob_match_recursive(pat_bytes, name_bytes)
}

fn glob_match_recursive(pat: &[u8], name: &[u8]) -> bool {
    match (pat.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            if glob_match_recursive(&pat[1..], name) {
                return true;
            }
            if !name.is_empty() {
                return glob_match_recursive(pat, &name[1..]);
            }
            false
        }
        (Some(b'?'), Some(_)) => glob_match_recursive(&pat[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => glob_match_recursive(&pat[1..], &name[1..]),
        _ => false,
    }
}
