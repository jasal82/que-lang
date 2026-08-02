//! std.archive module — Create and extract tar.gz and zip archives.

use crate::error::*;
use crate::value::Value;
use crate::interpreter::helpers::{arg_path_str, path_arg};
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "archive",
        functions: &["tar_gz", "zip", "extract", "list"],
    }
}

impl Interpreter {
    pub(crate) fn call_archive(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "tar_gz" => {
                let out_path = arg_path_str(args, 0, "archive.tar_gz")?;
                let files = match args.get(1) {
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(sig_type("archive.tar_gz", "List")),
                };
                let prefix = match args.get(2) {
                    Some(Value::String(s)) | Some(Value::Path(s)) => Some(s.clone()),
                    Some(Value::Null) | None => None,
                    _ => return Err(sig_type("archive.tar_gz", "String for prefix")),
                };
                create_tar_gz(&out_path, &files, prefix.as_deref()).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "zip" => {
                let out_path = arg_path_str(args, 0, "archive.zip")?;
                let files = match args.get(1) {
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(sig_type("archive.zip", "List")),
                };
                let prefix = match args.get(2) {
                    Some(Value::String(s)) | Some(Value::Path(s)) => Some(s.clone()),
                    Some(Value::Null) | None => None,
                    _ => return Err(sig_type("archive.zip", "String for prefix")),
                };
                create_zip_archive(&out_path, &files, prefix.as_deref()).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "extract" => {
                let src = arg_path_str(args, 0, "archive.extract")?;
                let dest = match args.get(1) {
                    Some(Value::Null) | None => ".".to_string(),
                    Some(v) => path_arg(v, "archive.extract: destination")?,
                };
                extract_archive(&src, &dest).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "list" => {
                let src = arg_path_str(args, 0, "archive.list")?;
                let entries = list_archive(&src).map_err(|e| sig_err(e))?;
                Ok(Value::List(entries.into_iter().map(Value::String).collect()))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'archive.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn sig_type(name: &str, expected: &str) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::TypeMismatch,
        format!("{}: expected {}", name, expected),
    ))
}

/// A single resolved archive entry.
struct ArchiveEntry {
    local: String,
    archive: String,
    is_dir: bool,
    /// Optional Unix permission bits (e.g. 0o755). None → use the file's own mode.
    mode: Option<u32>,
}

fn resolve_archive_entries(
    sources: &[Value],
    prefix: Option<&str>,
) -> Result<Vec<ArchiveEntry>, String> {
    let mut entries = Vec::new();
    for src_val in sources {
        match src_val {
            Value::Path(p) | Value::String(p) => {
                let p = src_val.as_path().unwrap_or_else(|| p.clone());
                let local_path = std::path::Path::new(&p);
                let basename = local_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone());
                let archive_base = join_archive_path(prefix, &basename);
                expand_source(local_path, &p, &archive_base, &mut entries)?;
            }
            Value::Map(m) => {
                let src_str = match m.get("src").and_then(|v| v.as_path()) {
                    Some(s) => s,
                    None => return Err("archive entry map must have a \"src\" key of type Path or String".into()),
                };
                let dest_str = match m.get("dest") {
                    Some(Value::Path(d)) | Some(Value::String(d)) => d.clone(),
                    _ => return Err("archive entry map must have a \"dest\" key of type Path or String".into()),
                };
                // Optional Unix permission bits: { src: ..., dest: ..., mode: 0o755 }
                let mode = match m.get("mode") {
                    Some(Value::Int(n)) => Some(*n as u32),
                    _ => None,
                };
                // Strip leading/trailing slashes; normalize "." to "" (archive root).
                // For files: dest is the exact archive path.
                // For directories: dest is the archive prefix for the directory's contents,
                //   so { src: p"./build", dest: "myapp-1.0" } puts build/foo → myapp-1.0/foo,
                //   and { src: p"./build", dest: "." } puts build/foo → foo (archive root).
                let dest_clean = normalize_dest(&dest_str);
                let archive_base = join_archive_path(prefix, &dest_clean);
                let local_path = std::path::Path::new(&src_str);
                expand_source_with_mode(local_path, &src_str, &archive_base, mode, &mut entries)?;
            }
            other => return Err(format!(
                "archive source list must contain Paths or {{src, dest}} maps, got {}",
                other.type_name()
            )),
        }
    }
    Ok(entries)
}

/// Normalise a user-supplied `dest` string into a clean archive path component.
/// Leading/trailing slashes and backslashes are removed; `"."` becomes `""`.
fn normalize_dest(dest: &str) -> String {
    let s = dest.replace('\\', "/");
    let s = s.trim_matches('/');
    if s == "." { String::new() } else { s.to_string() }
}

fn join_archive_path(prefix: Option<&str>, component: &str) -> String {
    match prefix {
        Some(p) if !p.is_empty() => {
            let p = p.trim_end_matches('/');
            let c = component.trim_start_matches('/');
            if c.is_empty() { p.to_string() } else { format!("{}/{}", p, c) }
        }
        _ => component.to_string(),
    }
}

/// Combine an archive base prefix with a relative path component.
/// Avoids producing paths with leading `./` or double slashes.
fn make_archive_path(base: &str, rel: &str) -> String {
    match (base.is_empty(), rel.is_empty()) {
        (true,  true)  => String::new(),
        (true,  false) => rel.to_string(),
        (false, true)  => base.to_string(),
        (false, false) => format!("{}/{}", base, rel),
    }
}

fn expand_source(
    local: &std::path::Path,
    local_str: &str,
    archive_base: &str,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<(), String> {
    expand_source_with_mode(local, local_str, archive_base, None, entries)
}

fn expand_source_with_mode(
    local: &std::path::Path,
    local_str: &str,
    archive_base: &str,
    mode: Option<u32>,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<(), String> {
    if local.is_file() {
        entries.push(ArchiveEntry {
            local: local_str.to_string(),
            archive: archive_base.to_string(),
            is_dir: false,
            mode,
        });
    } else if local.is_dir() {
        expand_dir(local, local, archive_base, entries)?;
    } else {
        return Err(format!("archive source does not exist: {}", local_str));
    }
    Ok(())
}

fn expand_dir(
    base_dir: &std::path::Path,
    dir: &std::path::Path,
    archive_base: &str,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<(), String> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Skip directories we cannot read rather than aborting the whole archive.
            return Ok(());
        }
        Err(e) => return Err(format!("cannot read {}: {}", dir.display(), e)),
    };
    let items: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();

    if items.is_empty() {
        // Empty directory — record as a directory entry so it is preserved
        // in the archive (tar DIRTYPE entry, zip directory entry).
        let rel = dir.strip_prefix(base_dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let archive_path = make_archive_path(archive_base, &rel);
        // Skip a completely empty archive_path (would be invalid).
        if !archive_path.is_empty() {
            let archive_path = if archive_path.ends_with('/') {
                archive_path
            } else {
                format!("{}/", archive_path)
            };
            entries.push(ArchiveEntry {
                local: dir.to_string_lossy().into_owned(),
                archive: archive_path,
                is_dir: true,
                mode: None,
            });
        }
        return Ok(());
    }

    for item in &items {
        let path = item.path();
        let rel = path.strip_prefix(base_dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let archive_path = make_archive_path(archive_base, &rel);
        if path.is_dir() {
            expand_dir(base_dir, &path, archive_base, entries)?;
        } else if path.is_file() {
            entries.push(ArchiveEntry {
                local: path.to_string_lossy().into_owned(),
                archive: archive_path,
                is_dir: false,
                mode: None,
            });
        }
        // Symlinks and other special file types are skipped.
    }
    Ok(())
}

fn create_tar_gz(
    out_path: &str,
    sources: &[Value],
    prefix: Option<&str>,
) -> Result<(), String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::{Builder, Header};
    let entries = resolve_archive_entries(sources, prefix)?;
    let file = std::fs::File::create(out_path).map_err(|e| e.to_string())?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(enc);
    for entry in &entries {
        if entry.is_dir {
            // Add an explicit directory entry to preserve empty directories.
            let mut header = Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(entry.mode.unwrap_or(0o755));
            header.set_cksum();
            let name = if entry.archive.ends_with('/') {
                entry.archive.clone()
            } else {
                format!("{}/", entry.archive)
            };
            builder
                .append_data(&mut header, &name, std::io::empty())
                .map_err(|e| format!("tar: dir entry {}: {}", name, e))?;
        } else if let Some(mode) = entry.mode {
            // Entry has explicit mode — build a custom header instead of
            // reading the file's own permission bits.
            let mut header = Header::new_gnu();
            let metadata = std::fs::metadata(&entry.local)
                .map_err(|e| format!("tar: metadata {}: {}", entry.local, e))?;
            header.set_metadata(&metadata);
            header.set_mode(mode);
            header.set_cksum();
            let file = std::fs::File::open(&entry.local)
                .map_err(|e| format!("tar: open {}: {}", entry.local, e))?;
            builder
                .append_data(&mut header, &entry.archive, file)
                .map_err(|e| format!("tar: {} → {}: {}", entry.local, entry.archive, e))?;
        } else {
            builder
                .append_path_with_name(&entry.local, &entry.archive)
                .map_err(|e| format!("tar: {} → {}: {}", entry.local, entry.archive, e))?;
        }
    }
    builder.finish().map_err(|e| e.to_string())
}

fn create_zip_archive(
    out_path: &str,
    sources: &[Value],
    prefix: Option<&str>,
) -> Result<(), String> {
    use zip::write::ZipWriter;
    use zip::write::SimpleFileOptions;
    let entries = resolve_archive_entries(sources, prefix)?;
    let file = std::fs::File::create(out_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for entry in &entries {
        if entry.is_dir {
            // Add an explicit directory entry to preserve empty directories.
            let name = if entry.archive.ends_with('/') {
                entry.archive.clone()
            } else {
                format!("{}/", entry.archive)
            };
            zip.add_directory(&name, options.unix_permissions(entry.mode.unwrap_or(0o755)))
                .map_err(|e| format!("zip: dir {}: {}", name, e))?;
        } else {
            let metadata = std::fs::metadata(&entry.local)
                .map_err(|e| format!("zip: metadata {}: {}", entry.local, e))?;
            // `SimpleFileOptions::default()` writes 0o644 for everything, so
            // zipping a build script and unzipping it produced a file that
            // would not run. tar preserved the source mode all along; this
            // makes the two formats agree.
            let mut opts = options.unix_permissions(entry.mode.unwrap_or_else(|| source_mode(&metadata)));
            if let Some(mtime) = zip_mtime(&metadata) {
                opts = opts.last_modified_time(mtime);
            }
            zip.start_file(&entry.archive, opts)
                .map_err(|e| format!("zip: {}: {}", entry.archive, e))?;
            let mut src = std::fs::File::open(&entry.local)
                .map_err(|e| format!("zip: open {}: {}", entry.local, e))?;
            std::io::copy(&mut src, &mut zip)
                .map_err(|e| format!("zip: {}: {}", entry.archive, e))?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// The source file's own permission bits, or a sane default off unix.
fn source_mode(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

/// The source file's modification time in zip's date format.
///
/// A zip whose every entry is stamped "now" defeats every downstream
/// mtime comparison -- incremental builds, rsync, `make` -- for the sake of
/// a field the format already has room for.
fn zip_mtime(metadata: &std::fs::Metadata) -> Option<zip::DateTime> {
    use chrono::{Datelike, Timelike};
    let modified = metadata.modified().ok()?;
    let stamp: chrono::DateTime<chrono::Local> = modified.into();
    zip::DateTime::from_date_and_time(
        u16::try_from(stamp.year()).ok()?,
        stamp.month() as u8,
        stamp.day() as u8,
        stamp.hour() as u8,
        stamp.minute() as u8,
        stamp.second() as u8,
    )
    .ok()
}

fn extract_archive(src: &str, dest: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let lower = src.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        extract_tar_gz(src, dest)
    } else if lower.ends_with(".tar") {
        extract_tar(src, dest)
    } else if lower.ends_with(".zip") {
        extract_zip(src, dest)
    } else {
        Err(format!("unsupported archive format for: {}", src))
    }
}

fn extract_tar_gz(src: &str, dest: &str) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);
    archive.unpack(dest).map_err(|e| e.to_string())
}

fn extract_tar(src: &str, dest: &str) -> Result<(), String> {
    use tar::Archive;
    let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let mut archive = Archive::new(file);
    archive.unpack(dest).map_err(|e| e.to_string())
}

fn extract_zip(src: &str, dest: &str) -> Result<(), String> {
    use zip::ZipArchive;
    let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
    archive.extract(dest).map_err(|e| e.to_string())
}

fn list_archive(src: &str) -> Result<Vec<String>, String> {
    let lower = src.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        use flate2::read::GzDecoder;
        use tar::Archive;
        let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
        let dec = GzDecoder::new(file);
        let mut archive = Archive::new(dec);
        archive.entries().map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .collect()
    } else if lower.ends_with(".tar") {
        use tar::Archive;
        let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
        let mut archive = Archive::new(file);
        archive.entries().map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .collect()
    } else if lower.ends_with(".zip") {
        use zip::ZipArchive;
        let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
        let archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
        Ok((0..archive.len()).map(|i| {
            archive.name_for_index(i).unwrap_or("").to_string()
        }).collect())
    } else {
        Err(format!("unsupported archive format: {}", src))
    }
}
