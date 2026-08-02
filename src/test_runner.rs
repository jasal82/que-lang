//! The `que test` runner: discovery, execution and reporting.
//!
//! Kept in the library rather than the binary so the discovery and pass/fail
//! rules can be tested, and so an editor or CI integration can reuse them
//! without shelling out.

use crate::error::{QueError, Signal};
use crate::interpreter::Interpreter;
use crate::value::Value;
use std::path::{Path, PathBuf};

/// The prefix that marks a function as a test.
///
/// A naming convention rather than an attribute: it needs no new syntax, it
/// reads the same in an editor's outline as it does in the runner's output,
/// and a helper function in a test file is just one without the prefix.
pub const TEST_PREFIX: &str = "test_";

/// What one test did.
pub struct TestOutcome {
    pub name: String,
    /// `None` when the test passed.
    pub failure: Option<String>,
    /// Anything the test printed. Shown only on failure, so a passing suite
    /// stays readable and a failing test keeps the context that explains it.
    pub output: Vec<String>,
}

impl TestOutcome {
    pub fn passed(&self) -> bool {
        self.failure.is_none()
    }
}

/// The result of running one file.
pub struct FileReport {
    pub path: PathBuf,
    /// Set when the file could not be loaded at all: a lex, parse or top-level
    /// runtime error. Distinct from a failing test, because no test ran.
    pub load_error: Option<QueError>,
    pub outcomes: Vec<TestOutcome>,
}

impl FileReport {
    pub fn failed(&self) -> bool {
        self.load_error.is_some() || self.outcomes.iter().any(|o| !o.passed())
    }
}

/// Does this file look like it holds tests?
///
/// `foo_test.que` and `test_foo.que` anywhere, plus any `.que` file under a
/// directory named `tests`. The last rule is what lets a project keep its
/// tests in one place without renaming every file.
pub fn is_test_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("que") {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    if stem.ends_with("_test") || stem.starts_with(TEST_PREFIX) {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str() == "tests")
}

/// Directories never worth descending into.
fn is_ignored_dir(name: &str) -> bool {
    // `.que` is the task cache, `target` and `node_modules` are build output.
    // Anything else starting with `.` is a tool's private directory.
    name == "target" || name == "node_modules" || name.starts_with('.')
}

/// Expand the paths given on the command line into a list of test files.
///
/// An explicitly named file is always used, even if it does not match the
/// naming convention: naming a file is a clearer statement of intent than any
/// pattern. Directories are walked, and only matching files are picked up.
pub fn discover(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        if root.is_file() {
            found.push(root.clone());
        } else if root.is_dir() {
            walk(root, &mut found);
        }
    }
    found.sort();
    found.dedup();
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !is_ignored_dir(name) {
                walk(&path, found);
            }
        } else if is_test_file(&path) {
            found.push(path);
        }
    }
}

/// Names of the test functions declared at the top level of a module, in
/// source order. Tests run in the order they are written, so a file reads the
/// same way it runs.
fn test_names(module: &crate::ast::Module, filter: Option<&str>) -> Vec<String> {
    module
        .items
        .iter()
        .filter_map(|(_, item)| match item {
            crate::ast::Item::FnDecl(f) if f.name.starts_with(TEST_PREFIX) => {
                // A test takes no arguments: there is nobody to supply them.
                // A `test_`-prefixed function with required parameters is
                // almost certainly a helper, so leave it alone.
                if f.params.iter().any(|p| p.default.is_none()) {
                    None
                } else {
                    Some(f.name.clone())
                }
            }
            _ => None,
        })
        .filter(|name| filter.is_none_or(|f| name.contains(f)))
        .collect()
}

/// Load one file and run the tests in it.
///
/// Top-level code runs once, before any test, and whatever it defines is
/// shared by all of them — the same bargain as every other test runner. Each
/// test then runs in its own scope, so a `let` inside one test cannot be seen
/// by the next.
pub fn run_file(path: &Path, filter: Option<&str>) -> FileReport {
    let mut report = FileReport {
        path: path.to_path_buf(),
        load_error: None,
        outcomes: Vec::new(),
    };

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report.load_error = Some(QueError::runtime(format!("cannot read: {}", e)));
            return report;
        }
    };
    let module = match crate::lexer::Lexer::new(&source)
        .tokenize()
        .and_then(|tokens| crate::parser::Parser::new(tokens).parse_module())
    {
        Ok(m) => m,
        Err(e) => {
            report.load_error = Some(e);
            return report;
        }
    };

    let names = test_names(&module, filter);
    if names.is_empty() {
        return report;
    }

    let mut interp = Interpreter::new();
    // Buffered, not direct: a test's output is only interesting when it fails.
    interp.direct_output = false;
    if let Ok(abs) = std::fs::canonicalize(path) {
        interp.set_script_path(abs);
    }
    interp.init_module_loader();

    if let Err(signal) = interp.exec_module(&module) {
        match signal {
            Signal::Error(e) => {
                report.load_error = Some(e);
                return report;
            }
            Signal::Exit(code) => {
                report.load_error = Some(QueError::runtime(format!(
                    "the file called exit({}) before any test ran",
                    code
                )));
                return report;
            }
            Signal::Interrupted(sig) => {
                report.load_error = Some(QueError::runtime(format!(
                    "interrupted by signal {}",
                    sig
                )));
                return report;
            }
            _ => {}
        }
    }

    for name in names {
        let Some(func) = interp.env.get(&name) else {
            // Shadowed or removed by top-level code. Report it rather than
            // silently running one test fewer than the file declares.
            report.outcomes.push(TestOutcome {
                name: name.clone(),
                failure: Some("not defined when the tests ran".to_string()),
                output: Vec::new(),
            });
            continue;
        };
        interp.output.clear();
        interp.partial_line.clear();
        let result = interp.call_test(func);
        interp.flush_partial();
        let output = std::mem::take(&mut interp.output);
        report.outcomes.push(TestOutcome {
            name,
            failure: describe_failure(result),
            output,
        });
    }

    report
}

/// Turn a test's result into a failure message, or `None` if it passed.
fn describe_failure(result: Result<Value, Signal>) -> Option<String> {
    match result {
        // A test that hands back `Err(...)` has failed. `Err` is a value here
        // rather than a raised error only because it was never used in a
        // position that converts it, and a test is exactly such a position.
        Ok(Value::Err(payload)) => Some(payload.display_string()),
        Ok(_) => None,
        Err(Signal::Error(e)) => Some(e.to_string()),
        Err(Signal::Exit(code)) => Some(format!("called exit({})", code)),
        Err(Signal::Interrupted(sig)) => Some(format!("interrupted by signal {}", sig)),
        Err(other) => Some(format!("unexpected {:?}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_test_files_by_name_or_directory() {
        assert!(is_test_file(Path::new("math_test.que")));
        assert!(is_test_file(Path::new("test_math.que")));
        assert!(is_test_file(Path::new("tests/anything.que")));
        assert!(is_test_file(Path::new("a/b/tests/c/deep.que")));

        assert!(!is_test_file(Path::new("math.que")));
        assert!(!is_test_file(Path::new("testing.que")));
        assert!(!is_test_file(Path::new("math_test.txt")));
    }

    fn parse(source: &str) -> crate::ast::Module {
        let tokens = crate::lexer::Lexer::new(source).tokenize().unwrap();
        crate::parser::Parser::new(tokens).parse_module().unwrap()
    }

    #[test]
    fn collects_prefixed_zero_argument_functions_in_source_order() {
        let module = parse(
            r#"
fn helper() { 1 }
fn test_b() { 1 }
fn test_a() { 1 }
fn test_needs_args(x) { x }
fn test_defaulted(x = 1) { x }
"#,
        );
        assert_eq!(
            test_names(&module, None),
            vec!["test_b", "test_a", "test_defaulted"]
        );
    }

    #[test]
    fn filter_matches_a_substring_of_the_name() {
        let module = parse("fn test_alpha() { 1 }\nfn test_beta() { 1 }\n");
        assert_eq!(test_names(&module, Some("alph")), vec!["test_alpha"]);
        assert_eq!(test_names(&module, Some("zzz")).len(), 0);
    }

    #[test]
    fn a_returned_err_counts_as_a_failure() {
        let failure = describe_failure(Ok(Value::Err(Box::new(Value::String(
            "boom".to_string(),
        )))));
        assert_eq!(failure.as_deref(), Some("boom"));
        assert_eq!(describe_failure(Ok(Value::Int(1))), None);
    }
}
