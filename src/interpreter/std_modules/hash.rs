//! std.hash module — Cryptographic hashing (SHA-256, SHA-512, MD5) and integrity verification.

use crate::error::*;
use crate::value::Value;
use crate::interpreter::helpers::path_arg;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "hash",
        functions: &[
            "sha256", "sha512", "md5",
            "verify", "write_checksums", "verify_checksums",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_hash(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "sha256" => {
                let val = args.first().ok_or_else(|| sig_arity("hash.sha256", 1))?;
                let digest = hash_value_sha256(val).map_err(|e| sig_err(e))?;
                Ok(Value::String(digest))
            }
            "sha512" => {
                let val = args.first().ok_or_else(|| sig_arity("hash.sha512", 1))?;
                let digest = hash_value_sha512(val).map_err(|e| sig_err(e))?;
                Ok(Value::String(digest))
            }
            "md5" => {
                let val = args.first().ok_or_else(|| sig_arity("hash.md5", 1))?;
                let digest = hash_value_md5(val).map_err(|e| sig_err(e))?;
                Ok(Value::String(digest))
            }
            "verify" => {
                let path_val = args.first().ok_or_else(|| sig_arity("hash.verify", 2))?;
                let expected_val = args.get(1).ok_or_else(|| sig_arity("hash.verify", 2))?;
                let path_str = path_arg(path_val, "hash.verify")?;
                let expected = match expected_val {
                    Value::String(s) => s.clone(),
                    _ => return Err(sig_type("hash.verify", "String for expected")),
                };
                let ok = integrity_verify(&path_str, &expected).map_err(|e| sig_err(e))?;
                Ok(Value::Bool(ok))
            }
            "write_checksums" => {
                let files_val = args.first().ok_or_else(|| sig_arity("hash.write_checksums", 2))?;
                let out_path_val = args.get(1).ok_or_else(|| sig_arity("hash.write_checksums", 2))?;
                let files: Vec<String> = match files_val {
                    Value::List(l) => l.iter()
                        .map(|v| path_arg(v, "hash.write_checksums"))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(sig_type("hash.write_checksums", "List")),
                };
                let out_path = path_arg(out_path_val, "hash.write_checksums")?;
                write_checksums_file(&files, &out_path).map_err(|e| sig_err(e))?;
                Ok(Value::Null)
            }
            "verify_checksums" => {
                let path_val = args.first().ok_or_else(|| sig_arity("hash.verify_checksums", 1))?;
                let path_str = path_arg(path_val, "hash.verify_checksums")?;
                let ok = verify_checksums_file(&path_str).map_err(|e| sig_err(e))?;
                Ok(Value::Bool(ok))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'hash.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn sig_arity(name: &str, n: usize) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::ArityMismatch,
        format!("{} requires {} argument(s)", name, n),
    ))
}

fn sig_type(name: &str, expected: &str) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::TypeMismatch,
        format!("{}: expected {}", name, expected),
    ))
}

use sha2::Digest;

fn read_bytes_for_hash(val: &Value) -> Result<Vec<u8>, String> {
    match val {
        Value::Path(p) => {
            std::fs::read(p).map_err(|e| format!("hash: cannot read '{}': {}", p, e))
        }
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(format!("hash: expected Path or String, got {}", val.type_name())),
    }
}

fn hash_value_sha256(val: &Value) -> Result<String, String> {
    let bytes = read_bytes_for_hash(val)?;
    let mut h = sha2::Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

fn hash_value_sha512(val: &Value) -> Result<String, String> {
    let bytes = read_bytes_for_hash(val)?;
    let mut h = sha2::Sha512::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

fn hash_value_md5(val: &Value) -> Result<String, String> {
    use md5::Md5;
    let bytes = read_bytes_for_hash(val)?;
    let mut h = Md5::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

fn integrity_verify(path: &str, expected: &str) -> Result<bool, String> {
    let val = Value::Path(path.to_string());
    let (alg, hex) = expected.split_once(':').unwrap_or(("sha256", expected));
    let actual = match alg {
        "sha256" => hash_value_sha256(&val)?,
        "sha512" => hash_value_sha512(&val)?,
        "md5"    => hash_value_md5(&val)?,
        other    => return Err(format!("unknown hash algorithm '{}'", other)),
    };
    Ok(actual == hex)
}

fn write_checksums_file(files: &[String], out_path: &str) -> Result<(), String> {
    let mut lines = Vec::new();
    for f in files {
        let val = Value::Path(f.clone());
        let digest = hash_value_sha256(&val)?;
        let fname = std::path::Path::new(f)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.clone());
        lines.push(format!("{}  {}", digest, fname));
    }
    std::fs::write(out_path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn verify_checksums_file(checksums_path: &str) -> Result<bool, String> {
    let dir = std::path::Path::new(checksums_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let content = std::fs::read_to_string(checksums_path).map_err(|e| e.to_string())?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (hash, fname) = if let Some(idx) = line.find("  ") {
            (&line[..idx], line[idx+2..].trim())
        } else if let Some(idx) = line.find(' ') {
            (&line[..idx], line[idx+1..].trim())
        } else {
            return Err(format!("malformed checksum line: {}", line));
        };
        let full_path = format!("{}/{}", dir, fname);
        let val = Value::Path(full_path);
        let actual = hash_value_sha256(&val)?;
        if actual != hash {
            return Ok(false);
        }
    }
    Ok(true)
}
