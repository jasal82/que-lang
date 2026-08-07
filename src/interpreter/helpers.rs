//! Free-standing helper functions used across interpreter submodules.

use crate::error::*;
use crate::token::DurationUnit;
use crate::value::Value;

/// Recursively copy a directory tree from `src` to `dst`.
/// Creates `dst` and all intermediate directories as needed.
pub(crate) fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if file_type.is_symlink() {
            // Preserve symlinks by reading the link target and recreating
            #[cfg(unix)]
            {
                let target = std::fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(&target, &dest_path)?;
            }
            #[cfg(not(unix))]
            std::fs::copy(entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Resolve a copy/move destination the way `cp` and `mv` do: naming an
/// existing directory means *into* that directory, under the source's own
/// name. Anything else is the new name for the source.
///
/// Without this, `dir.copy_to(p"/tmp/")` would spill the contents of `dir`
/// directly into `/tmp` rather than producing `/tmp/dir`, and copying a file
/// into a directory would fail outright.
pub(crate) fn resolve_into_dir(src: &std::path::Path, dest: String) -> String {
    let dest_path = std::path::Path::new(&dest);
    if !dest_path.is_dir() {
        return dest;
    }
    match src.file_name() {
        Some(name) => dest_path.join(name).to_string_lossy().into_owned(),
        // `/` and `..` have no name to keep, so there is nothing to append.
        None => dest,
    }
}

/// The deepest directory of a glob that contains no wildcard, e.g. `src/lib`
/// for `src/lib/**/*.rs`.
///
/// This is the point the pattern starts *choosing* from, so it is the natural
/// origin for the matches: stripping it off `src/lib/a/b.rs` leaves `a/b.rs`,
/// the shape the copy should keep at the destination.
pub(crate) fn glob_base(pattern: &str) -> &str {
    let wildcard = pattern
        .find(['*', '?', '[', '{'])
        .unwrap_or(pattern.len());
    match pattern[..wildcard].rfind('/') {
        Some(slash) => &pattern[..slash],
        None => "",
    }
}

/// Match a glob pattern against the filesystem.
///
/// Every glob expansion in the language goes through here. They did not use
/// to: `Glob.expand()` expanded `~` and `{a,b}` before matching, while
/// `Path.glob()`, `for x in g"..."` and task `@inputs` handed the pattern to
/// `glob::glob` raw. Since `glob::glob` understands neither form, the same
/// pattern quietly meant different things depending on where it was written
/// — a `~` was looked for as a directory literally named `~`, and `{a,b}` as
/// one literally named `{a,b}`, so the mismatch showed up as "no matches"
/// rather than as an error.
///
/// Alternatives are expanded in the order written and their match sets
/// concatenated, the way a shell does it; `glob::glob` sorts within one
/// alternative. A malformed pattern is reported rather than swallowed, so a
/// caller that can fail — `copy_to`, `move_to` — can say so instead of
/// quietly copying nothing.
pub(crate) fn glob_expand(pattern: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let mut results = Vec::new();
    for pat in expand_braces(&expand_tilde(pattern)) {
        match glob::glob(&pat) {
            Ok(entries) => results.extend(entries.flatten()),
            Err(e) => return Err(format!("invalid glob pattern '{}': {}", pat, e)),
        }
    }
    Ok(results)
}

pub(crate) fn duration_to_ms(val: f64, unit: DurationUnit) -> f64 {
    match unit {
        DurationUnit::Milliseconds => val,
        DurationUnit::Seconds => val * 1000.0,
        DurationUnit::Minutes => val * 60_000.0,
        DurationUnit::Hours => val * 3_600_000.0,
        DurationUnit::Days => val * 86_400_000.0,
    }
}

/// The command interpreter to hand a command string to, and its "run this
/// string" flag.
///
/// Que's command literals are shell text, not argv, so there has to be a
/// shell. On Windows that is `cmd /C` unless `QUE_SHELL` says otherwise —
/// which is the escape hatch for anyone in a Git Bash or MSYS environment
/// where the POSIX shell is both present and what the script expects.
pub(crate) fn shell() -> (String, &'static str) {
    if let Ok(custom) = std::env::var("QUE_SHELL") {
        if !custom.trim().is_empty() {
            let flag = if custom.ends_with("cmd") || custom.ends_with("cmd.exe") {
                "/C"
            } else {
                "-c"
            };
            return (custom, flag);
        }
    }
    #[cfg(windows)]
    {
        ("cmd".to_string(), "/C")
    }
    #[cfg(not(windows))]
    {
        ("sh".to_string(), "-c")
    }
}

/// A command that does nothing and succeeds, for the dry-run substitute in
/// `spawn` (which must still hand back a real process handle).
pub(crate) fn shell_noop() -> &'static str {
    let (name, _) = shell();
    if name.ends_with("cmd") || name.ends_with("cmd.exe") {
        "rem"
    } else {
        ":"
    }
}

/// Build a `std::process::Command` that runs `text` through the shell.
pub(crate) fn shell_command(text: &str) -> std::process::Command {
    let (name, flag) = shell();
    let mut cmd = std::process::Command::new(name);
    cmd.arg(flag).arg(text);
    cmd
}

/// The user's home directory.
///
/// `HOME` first so a POSIX-style override keeps working under MSYS/Cygwin,
/// then Windows' `USERPROFILE`, then its `HOMEDRIVE`+`HOMEPATH` pair.
pub fn home_dir() -> Option<String> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Some(h);
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.is_empty() {
            return Some(h);
        }
    }
    match (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
        (Ok(d), Ok(p)) if !d.is_empty() && !p.is_empty() => Some(format!("{}{}", d, p)),
        _ => None,
    }
}

/// Quote a value so the shell sees it as one argument.
///
/// The quoting has to match whichever shell `shell()` picked. cmd.exe has no
/// single quotes at all, so a POSIX-escaped path would arrive with literal
/// apostrophes around it; there, double quotes are the only option and an
/// embedded `"` is escaped by doubling it.
pub(crate) fn shell_escape(s: &str) -> String {
    let (name, _) = shell();
    if name.ends_with("cmd") || name.ends_with("cmd.exe") {
        return cmd_escape(s);
    }
    posix_escape(s)
}

fn cmd_escape(s: &str) -> String {
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' || c == '\\'
                || c == ':'
        })
    {
        return s.to_string();
    }
    // `%` would otherwise be expanded as a variable reference even inside
    // double quotes, and there is no way to escape it in a quoted string --
    // so it is replaced by the one form cmd leaves alone.
    format!("\"{}\"", s.replace('"', "\"\"").replace('%', "%%"))
}

fn posix_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub(crate) fn two_args<'a>(args: &'a [Value], name: &str) -> Result<(&'a Value, &'a Value), Signal> {
    if args.len() < 2 {
        return Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{} requires 2 arguments", name),
        )));
    }
    Ok((&args[0], &args[1]))
}

pub(crate) fn arg_str<'a>(args: &'a [Value], idx: usize, method: &str) -> Result<&'a str, Signal> {
    match args.get(idx) {
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{}() argument {} must be a string, got {}",
                method,
                idx,
                other.type_name()
            ),
        ))),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{}() requires at least {} arguments", method, idx + 1),
        ))),
    }
}

/// Coerce an argument that names a filesystem location to a string, with the
/// error a builtin wants. The conversion itself is `Value::as_path`, which is
/// where the String-is-a-Path rule lives.
pub(crate) fn path_arg(val: &Value, ctx: &str) -> Result<String, Signal> {
    val.as_path().ok_or_else(|| {
        Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{} requires a path or string, got {}", ctx, val.type_name()),
        ))
    })
}

/// Like `arg_str`, but for arguments that name a filesystem location.
pub(crate) fn arg_path_str(args: &[Value], idx: usize, method: &str) -> Result<String, Signal> {
    match args.get(idx) {
        Some(val) => path_arg(val, &format!("{}() argument {}", method, idx)),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{}() requires at least {} arguments", method, idx + 1),
        ))),
    }
}

pub(crate) fn arg_int(args: &[Value], idx: usize, method: &str) -> Result<i64, Signal> {
    match args.get(idx) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{}() argument {} must be an int, got {}",
                method,
                idx,
                other.type_name()
            ),
        ))),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{}() requires at least {} arguments", method, idx + 1),
        ))),
    }
}

pub(crate) fn value_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::String(a), Value::String(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

pub(crate) fn expect_int(args: &[Value], idx: usize, name: &str) -> Result<i64, Signal> {
    match args.get(idx) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!("{}() argument {} must be an int, got {}", name, idx + 1, other.type_name()),
        ))),
        None => Err(Signal::Error(QueError::new(
            ErrorKind::ArityMismatch,
            format!("{}() requires at least {} arguments", name, idx + 1),
        ))),
    }
}

/// Simple semver comparison (major.minor.patch).
pub(crate) fn cmp_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> (u64, u64, u64) {
        let parts: Vec<&str> = s.split('.').collect();
        let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };
    parse(a).cmp(&parse(b))
}

/// Check if a version satisfies a semver constraint string.
/// Supports: ">=1.2.0", "<2.0.0", ">=1.2.0, <2.0.0" (comma-separated AND).
pub(crate) fn check_semver_constraint(constraint: &str, version: &str) -> bool {
    let parse_ver = |s: &str| -> (u64, u64, u64) {
        let clean = s.trim().trim_start_matches('v').trim_start_matches('V');
        // Strip pre-release suffix for comparison
        let base = clean.split('-').next().unwrap_or(clean);
        let parts: Vec<&str> = base.split('.').collect();
        let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };

    let ver = parse_ver(version);
    
    for part in constraint.split(',') {
        let part = part.trim();
        let satisfied = if let Some(rest) = part.strip_prefix(">=") {
            ver >= parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix('>') {
            ver > parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix("<=") {
            ver <= parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix('<') {
            ver < parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix("==") {
            ver == parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix('=') {
            ver == parse_ver(rest)
        } else if let Some(rest) = part.strip_prefix("!=") {
            ver != parse_ver(rest)
        } else {
            // Exact match
            ver == parse_ver(part)
        };
        if !satisfied {
            return false;
        }
    }
    true
}

/// Expand `{a,b,c}` brace alternation into multiple patterns.
/// `src/{foo,bar}/*.rs` → `["src/foo/*.rs", "src/bar/*.rs"]`
/// Patterns without braces return a single-element vec.
pub(crate) fn expand_braces(pattern: &str) -> Vec<String> {
    // Find the first unescaped '{' and its matching '}'
    let chars: Vec<char> = pattern.chars().collect();
    let mut open = None;
    let mut depth = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = open {
                        let prefix = &pattern[..start];
                        let suffix = &pattern[i + 1..];
                        let inside = &pattern[start + 1..i];
                        // Split on top-level commas only
                        let mut alts: Vec<&str> = Vec::new();
                        let mut alt_start = 0;
                        let mut d = 0usize;
                        for (j, c2) in inside.char_indices() {
                            match c2 {
                                '{' => d += 1,
                                '}' => d -= 1,
                                ',' if d == 0 => {
                                    alts.push(&inside[alt_start..j]);
                                    alt_start = j + 1;
                                }
                                _ => {}
                            }
                        }
                        alts.push(&inside[alt_start..]);
                        let mut results = Vec::new();
                        for alt in alts {
                            let candidate = format!("{}{}{}", prefix, alt, suffix);
                            // Recursively expand remaining braces
                            results.extend(expand_braces(&candidate));
                        }
                        return results;
                    }
                }
            }
            _ => {}
        }
    }
    vec![pattern.to_string()]
}

/// Expand a leading `~` to the user's home directory.
pub(crate) fn expand_tilde(pattern: &str) -> String {
    if pattern == "~" || pattern.starts_with("~/") || pattern.starts_with("~\\") {
        if let Some(home) = home_dir() {
            return format!("{}{}", home, &pattern[1..]);
        }
    }
    pattern.to_string()
}

/// Simple glob matching (without filesystem access).
/// Supports `*` (any chars except /), `**` (any chars including /),
/// `?` (single non-/ char), and `{a,b,c}` alternation.
pub(crate) fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = expand_tilde(pattern);
    // Convert glob pattern to a regex
    let mut regex_str = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    // ** matches any path
                    if i + 2 < chars.len() && chars[i + 2] == '/' {
                        regex_str.push_str("(.*/)?");
                        i += 3;
                    } else {
                        regex_str.push_str(".*");
                        i += 2;
                    }
                } else {
                    regex_str.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                regex_str.push_str("[^/]");
                i += 1;
            }
            '{' => {
                // Alternation: {a,b,c} → (a|b|c)
                regex_str.push('(');
                i += 1;
                while i < chars.len() && chars[i] != '}' {
                    if chars[i] == ',' {
                        regex_str.push('|');
                    } else {
                        // Escape regex metacharacters inside alternation
                        match chars[i] {
                            '.' | '+' | '(' | ')' | '[' | ']' | '^' | '$' | '|' | '\\' => {
                                regex_str.push('\\');
                                regex_str.push(chars[i]);
                            }
                            c => regex_str.push(c),
                        }
                    }
                    i += 1;
                }
                regex_str.push(')');
                i += 1; // consume '}'
            }
            '.' | '+' | '(' | ')' | '[' | ']' | '^' | '$' | '|' | '\\' => {
                regex_str.push('\\');
                regex_str.push(chars[i]);
                i += 1;
            }
            c => {
                regex_str.push(c);
                i += 1;
            }
        }
    }
    regex_str.push('$');

    simple_regex_test(&regex_str, path)
}

/// Simple regex test (uses basic pattern matching without external crate).
pub(crate) fn simple_regex_test(pattern: &str, input: &str) -> bool {
    // Try to use Rust's built-in regex-like matching
    // For simplicity, we'll compile a basic regex using a minimal approach
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.is_match(input),
        Err(_) => false,
    }
}

/// Find first regex match.
pub(crate) fn simple_regex_find(pattern: &str, input: &str) -> Option<String> {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.find(input).map(|m| m.as_str().to_string()),
        Err(_) => None,
    }
}

/// Find all regex matches.
pub(crate) fn simple_regex_find_all(pattern: &str, input: &str) -> Vec<String> {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.find_iter(input).map(|m| m.as_str().to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Regex replace (first occurrence).
pub(crate) fn simple_regex_replace(pattern: &str, input: &str, replacement: &str) -> String {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.replace(input, replacement).to_string(),
        Err(_) => input.to_string(),
    }
}

pub(crate) fn simple_regex_replace_all(pattern: &str, input: &str, replacement: &str) -> String {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.replace_all(input, replacement).to_string(),
        Err(_) => input.to_string(),
    }
}

/// Regex captures.
pub(crate) fn simple_regex_captures(pattern: &str, input: &str) -> Vec<String> {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => {
            if let Some(caps) = re.captures(input) {
                (0..caps.len())
                    .filter_map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect()
            } else {
                Vec::new()
            }
        }
        Err(_) => Vec::new(),
    }
}

/// Named regex captures — returns a map of capture group name → matched string.
/// Groups that did not participate in the match are omitted from the map.
pub(crate) fn simple_regex_named_captures(pattern: &str, input: &str) -> std::collections::BTreeMap<String, String> {
    let mut result = std::collections::BTreeMap::new();
    let re = match regex_lite::Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return result,
    };
    if let Some(caps) = re.captures(input) {
        for name in re.capture_names().flatten() {
            if let Some(m) = caps.name(name) {
                result.insert(name.to_string(), m.as_str().to_string());
            }
        }
    }
    result
}

/// Regex split.
pub(crate) fn simple_regex_split(pattern: &str, input: &str) -> Vec<String> {
    match regex_lite::Regex::new(pattern) {
        Ok(re) => re.split(input).map(|s| s.to_string()).collect(),
        Err(_) => vec![input.to_string()],
    }
}





// ── log types ──────────────────────────────────────────────────────────────

/// Output format for log lines.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LogFormat { Text, Json }

/// Where a log sink writes to.
#[derive(Clone, Debug)]
pub(crate) enum LogSinkKind { Console, File(String) }

/// A registered log sink with optional overrides.
#[derive(Clone, Debug)]
pub(crate) struct LogSink {
    pub kind: LogSinkKind,
    pub level: Option<u8>,                                     // per-sink min level
    pub format: Option<LogFormat>,                             // per-sink format override
    pub filter: Option<std::collections::BTreeMap<String, crate::value::Value>>,  // field-match filter
}

/// Render one structured log field as JSON.
///
/// Numbers, booleans and null keep their type so a log query can compare
/// them numerically. Everything else becomes a string, because a nested
/// object in a log field is a schema the ingester did not agree to.
pub(crate) fn log_field_to_json(v: &crate::value::Value) -> serde_json::Value {
    use crate::value::Value;
    match v {
        Value::Int(n) => serde_json::Value::from(*n),
        // NaN and the infinities have no JSON spelling. Emitting them
        // unquoted is what makes the whole line unparseable, so they become
        // their text form instead of poisoning the record.
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::from(f.to_string())),
        Value::Bool(b) => serde_json::Value::from(*b),
        Value::Null => serde_json::Value::Null,
        other => serde_json::Value::from(other.display_string()),
    }
}

/// Parse a level name to its numeric value.
pub(crate) fn parse_log_level(s: &str) -> Result<u8, String> {    match s.to_lowercase().as_str() {
        "debug" => Ok(0),
        "info"  => Ok(1),
        "warn"  => Ok(2),
        "error" => Ok(3),
        other   => Err(format!("unknown log level '{}' (expected debug/info/warn/error)", other)),
    }
}

/// Parse a format string to LogFormat.
pub(crate) fn parse_log_format(s: &str) -> Result<LogFormat, String> {
    match s.to_lowercase().as_str() {
        "text" => Ok(LogFormat::Text),
        "json" => Ok(LogFormat::Json),
        other  => Err(format!("unknown log format '{}' (expected text/json)", other)),
    }
}

/// Create the default console sink (no overrides).
pub(crate) fn default_console_sink() -> LogSink {
    LogSink {
        kind: LogSinkKind::Console,
        level: None,
        format: None,
        filter: None,
    }
}

impl super::Interpreter {
    /// Emit a log line through all registered sinks, respecting level gates and filters.
    ///
    /// `level` — human label ("DEBUG", "INFO", …)
    /// `level_num` — 0..3
    /// `context` — fields from a Logger instance (empty for root logger)
    /// `args` — [message, optional_fields_map]
    pub(crate) fn log_emit_to_sinks(
        &mut self,
        level: &str,
        level_num: u8,
        context: &std::collections::BTreeMap<String, crate::value::Value>,
        args: &[crate::value::Value],
    ) -> IResult {
        // Global level gate
        if level_num < self.log_level {
            return Ok(crate::value::Value::Null);
        }

        // Extract message and per-call fields
        let msg = args.first().map(|v| v.display_string()).unwrap_or_default();
        let mut merged = context.clone();
        if let Some(crate::value::Value::Map(m)) = args.get(1) {
            for (k, v) in m {
                merged.insert(k.clone(), v.clone());
            }
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Iterate over sinks (clone the vec to avoid borrow conflict with self.output)
        let sinks = self.log_sinks.clone();
        let default_format = self.log_format.clone();

        for sink in &sinks {
            // Per-sink level check
            if let Some(sink_level) = sink.level {
                if level_num < sink_level {
                    continue;
                }
            }

            // Field-match filter
            if let Some(ref filter) = sink.filter {
                let mut matches = true;
                for (fk, fv) in filter {
                    match merged.get(fk) {
                        Some(v) if v.display_string() == fv.display_string() => {}
                        _ => { matches = false; break; }
                    }
                }
                if !matches { continue; }
            }

            // Format the line
            let format = sink.format.as_ref().unwrap_or(&default_format);
            let line = match format {
                LogFormat::Text => {
                    if merged.is_empty() {
                        format!("[{}] {} | {}", level, ts, msg)
                    } else {
                        let kv: Vec<String> = merged.iter()
                            .map(|(k, v)| format!("{}={}", k, v.display_string()))
                            .collect();
                        format!("[{}] {} | {} | {}", level, ts, msg, kv.join(" "))
                    }
                }
                LogFormat::Json => {
                    // Built through serde_json rather than by concatenating
                    // strings: a log line carrying a message with a newline,
                    // a tab or a control character is exactly the line a CI
                    // log ingester chokes on, and hand-rolled escaping had
                    // covered only `\` and `"`.
                    let mut obj = serde_json::Map::new();
                    obj.insert("level".to_string(), serde_json::Value::from(level));
                    obj.insert("timestamp".to_string(), serde_json::Value::from(ts));
                    obj.insert("message".to_string(), serde_json::Value::from(msg.clone()));
                    for (k, v) in &merged {
                        obj.insert(k.clone(), log_field_to_json(v));
                    }
                    serde_json::Value::Object(obj).to_string()
                }
            };

            // Write to destination
            match &sink.kind {
                LogSinkKind::Console => {
                    self.emit(line);
                }
                LogSinkKind::File(path) => {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                    {
                        let _ = writeln!(f, "{}", line);
                    }
                }
            }
        }

        Ok(crate::value::Value::Null)
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    /// These tests mutate process-global environment variables, which every
    /// other test in this binary shares. Serialising them is cheaper than
    /// chasing the intermittent failure that otherwise appears once a month.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_custom_shell_overrides_the_platform_default() {
        let _g = env_guard();
        // QUE_SHELL is the escape hatch for a Windows box running Git Bash,
        // where a POSIX shell is both present and what scripts expect.
        std::env::set_var("QUE_SHELL", "/usr/bin/bash");
        assert_eq!(shell(), ("/usr/bin/bash".to_string(), "-c"));
        std::env::set_var("QUE_SHELL", "cmd.exe");
        assert_eq!(shell(), ("cmd.exe".to_string(), "/C"));
        std::env::remove_var("QUE_SHELL");
    }

    #[test]
    fn an_empty_shell_override_is_ignored_rather_than_obeyed() {
        let _g = env_guard();
        // An unset-looking variable should not leave the interpreter with no
        // shell at all.
        std::env::set_var("QUE_SHELL", "   ");
        let (name, _) = shell();
        assert!(!name.trim().is_empty());
        std::env::remove_var("QUE_SHELL");
    }

    #[test]
    fn the_platform_default_shell_matches_the_platform() {
        let _g = env_guard();
        std::env::remove_var("QUE_SHELL");
        let (name, flag) = shell();
        if cfg!(windows) {
            assert_eq!((name.as_str(), flag), ("cmd", "/C"));
        } else {
            assert_eq!((name.as_str(), flag), ("sh", "-c"));
        }
    }

    #[test]
    fn cmd_quoting_uses_double_quotes_because_cmd_has_no_single_ones() {
        assert_eq!(cmd_escape("C:\\Program Files\\app"), "\"C:\\Program Files\\app\"");
        assert_eq!(cmd_escape("plain.txt"), "plain.txt");
        assert_eq!(cmd_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn a_percent_is_neutralised_because_cmd_expands_it_even_when_quoted() {
        assert_eq!(cmd_escape("100%PATH%"), "\"100%%PATH%%\"");
    }

    #[test]
    fn posix_quoting_is_unchanged() {
        assert_eq!(posix_escape("plain.txt"), "plain.txt");
        assert_eq!(posix_escape("two words"), "'two words'");
        assert_eq!(posix_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn the_home_directory_falls_back_to_the_windows_variables() {
        let _g = env_guard();
        let saved = std::env::var("HOME").ok();
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", "C:\\Users\\dev");
        assert_eq!(home_dir().as_deref(), Some("C:\\Users\\dev"));

        std::env::remove_var("USERPROFILE");
        std::env::set_var("HOMEDRIVE", "D:");
        std::env::set_var("HOMEPATH", "\\devs\\dev");
        assert_eq!(home_dir().as_deref(), Some("D:\\devs\\dev"));

        std::env::remove_var("HOMEDRIVE");
        std::env::remove_var("HOMEPATH");
        if let Some(h) = saved {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    fn an_empty_home_is_treated_as_unset() {
        let _g = env_guard();
        // An exported-but-empty HOME is common in containers and must not
        // expand `~/x` to `/x`.
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", "");
        std::env::set_var("USERPROFILE", "C:\\Users\\dev");
        assert_eq!(home_dir().as_deref(), Some("C:\\Users\\dev"));
        std::env::remove_var("USERPROFILE");
        match saved {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn a_backslash_tilde_path_expands_too() {
        let _g = env_guard();
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/home/dev");
        assert_eq!(expand_tilde("~\\src"), "/home/dev\\src");
        assert_eq!(expand_tilde("~/src"), "/home/dev/src");
        assert_eq!(expand_tilde("~notme/src"), "~notme/src");
        match saved {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }
}
