//! std.csv module — CSV parsing and serialization.

use crate::error::*;
use crate::value::Value;
use crate::interpreter::helpers::arg_path_str;
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "csv",
        functions: &["parse", "parse_str", "write", "to_string"],
    }
}

impl Interpreter {
    pub(crate) fn call_csv(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "parse" => {
                let input_val = args.first().ok_or_else(|| sig_arity("csv.parse", 1))?;
                let text = match input_val {
                    Value::Path(p) | Value::String(p) => {
                        // A String is either a filename or the CSV itself, so
                        // `~` is expanded only for the filename reading.
                        let candidate = crate::interpreter::helpers::expand_tilde(p);
                        if std::path::Path::new(&candidate).exists() && !p.contains('\n') {
                            std::fs::read_to_string(&candidate).map_err(|e| sig_err(e.to_string()))?
                        } else {
                            p.clone()
                        }
                    }
                    _ => return Err(sig_type("csv.parse", "Path or String")),
                };
                let delimiter = args.get(1).and_then(|v| match v {
                    Value::String(s) => Some(s.chars().next().unwrap_or(',')),
                    _ => None,
                }).unwrap_or(',');
                let rows = csv_parse_str(&text, true, delimiter).map_err(|e| sig_err(e))?;
                Ok(Value::List(rows))
            }
            "parse_str" => {
                let text = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("csv.parse_str", "String")),
                };
                let delimiter = args.get(1).and_then(|v| match v {
                    Value::String(s) => Some(s.chars().next().unwrap_or(',')),
                    _ => None,
                }).unwrap_or(',');
                let rows = csv_parse_str(&text, true, delimiter).map_err(|e| sig_err(e))?;
                Ok(Value::List(rows))
            }
            "write" => {
                let rows = match args.first() {
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(sig_type("csv.write", "List")),
                };
                let path_str = arg_path_str(args, 1, "csv.write")?;
                let delimiter = args.get(2).and_then(|v| match v {
                    Value::String(s) => Some(s.chars().next().unwrap_or(',')),
                    _ => None,
                }).unwrap_or(',');
                let content = csv_write_rows(&rows, delimiter).map_err(|e| sig_err(e))?;
                std::fs::write(&path_str, content).map_err(|e| sig_err(e.to_string()))?;
                Ok(Value::Null)
            }
            "to_string" => {
                let rows = match args.first() {
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(sig_type("csv.to_string", "List")),
                };
                let delimiter = args.get(1).and_then(|v| match v {
                    Value::String(s) => Some(s.chars().next().unwrap_or(',')),
                    _ => None,
                }).unwrap_or(',');
                let content = csv_write_rows(&rows, delimiter).map_err(|e| sig_err(e))?;
                Ok(Value::String(content))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'csv.{}'", func),
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

fn csv_parse_str(
    text: &str,
    headers: bool,
    delimiter: char,
) -> Result<Vec<Value>, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let parse_row = |line: &str| -> Vec<String> {
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                if in_quotes && chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            } else if c == delimiter && !in_quotes {
                fields.push(current.clone());
                current.clear();
            } else {
                current.push(c);
            }
        }
        fields.push(current);
        fields
    };

    if headers {
        let header_fields = parse_row(lines[0]);
        let mut rows = Vec::new();
        for line in &lines[1..] {
            if line.trim().is_empty() { continue; }
            let values = parse_row(line);
            let mut map: BTreeMap<String, Value> = BTreeMap::new();
            for (i, h) in header_fields.iter().enumerate() {
                let v = values.get(i).cloned().unwrap_or_default();
                map.insert(h.clone(), Value::String(v));
            }
            rows.push(Value::Map(map));
        }
        Ok(rows)
    } else {
        let mut rows = Vec::new();
        for line in &lines {
            if line.trim().is_empty() { continue; }
            let fields = parse_row(line);
            rows.push(Value::List(
                fields.into_iter().map(Value::String).collect()
            ));
        }
        Ok(rows)
    }
}

fn csv_write_rows(
    rows: &[Value],
    delimiter: char,
) -> Result<String, String> {
    use std::fmt::Write as FmtWrite;
    if rows.is_empty() {
        return Ok(String::new());
    }
    let csv_field = |s: &str, delim: char| -> String {
        if s.contains(delim) || s.contains('"') || s.contains('\n') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = String::new();
    if let Value::Map(first_map) = &rows[0] {
        let headers: Vec<&str> = first_map.keys().map(|k| k.as_str()).collect();
        let _ = writeln!(out, "{}", headers.iter().map(|h| csv_field(h, delimiter)).collect::<Vec<_>>().join(&delimiter.to_string()));
        for row in rows {
            if let Value::Map(m) = row {
                let fields: Vec<String> = headers.iter().map(|h| {
                    m.get(*h).map(|v| v.display_string()).unwrap_or_default()
                }).map(|s| csv_field(&s, delimiter)).collect();
                let _ = writeln!(out, "{}", fields.join(&delimiter.to_string()));
            }
        }
    } else {
        for row in rows {
            let fields = match row {
                Value::List(l) => l.iter().map(|v| csv_field(&v.display_string(), delimiter)).collect::<Vec<_>>(),
                other => vec![csv_field(&other.display_string(), delimiter)],
            };
            let _ = writeln!(out, "{}", fields.join(&delimiter.to_string()));
        }
    }
    Ok(out)
}
