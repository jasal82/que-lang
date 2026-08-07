use std::env;
use std::fs;
use std::path::Path;
use std::process;

use que_lang::error::{Signal, EXIT_FAILURE, EXIT_USAGE};
use que_lang::formatter::Formatter;
use que_lang::interpreter::Interpreter;
use que_lang::lexer::Lexer;
use que_lang::linter::{Linter, Severity};
use que_lang::parser::Parser;
use que_lang::token::TokenKind;
use que_lang::value::Value;

use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Editor, CompletionType, Config};
use que_lang::completion::QueHelper;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();

    // Ask the console to interpret ANSI escape sequences before anything can
    // print. Only Windows needs telling, and only once per process.
    que_lang::interpreter::enable_ansi();

    if args.len() == 1 {
        repl();
        return;
    }

    // Scripts and tasks unwind on Ctrl-C so their `defer` blocks run. The REPL
    // is excluded: there, Ctrl-C cancels the current line, which rustyline
    // already handles in raw mode.
    que_lang::interrupt::install();

    match args[1].as_str() {
        "run" => cmd_run(&args[2..]),
        "tasks" => cmd_tasks(&args[2..]),
        "test" => cmd_test(&args[2..]),
        "install" => cmd_install(&args[2..]),
        "fmt" => cmd_fmt(&args[2..]),
        "lint" => cmd_lint(&args[2..]),
        "help" | "--help" | "-h" => print_usage(),
        "version" | "--version" | "-V" => print_version(),
        _ => {
            // Treat the argument as a script file (backward compatible).
            // Recognized flags: --strict.
            // Everything after `--` is forwarded to the script via `args()`.
            let tail = &args[1..];
            let mut script_args: Vec<String> = Vec::new();
            let mut script_pos: Option<usize> = None;
            let mut strict = false;
            let mut dry_run = false;
            let mut policy: Option<que_lang::permissions::Policy> = None;
            let mut i = 0;
            while i < tail.len() {
                let a = &tail[i];
                if a == "--" {
                    script_args = tail[i + 1..].to_vec();
                    break;
                } else if a == "--strict" {
                    strict = true;
                } else if a == "--dry-run" {
                    dry_run = true;
                } else if let Some(spec) = permission_flag(tail, &mut i) {
                    apply_permission(&mut policy, &spec);
                } else if script_pos.is_none() {
                    script_pos = Some(i);
                } else {
                    // Additional non-flag tokens before `--` are also passed
                    // through as script args, so `que script.que foo bar`
                    // works without an explicit `--`.
                    script_args.push(a.clone());
                }
                i += 1;
            }
            match script_pos {
                Some(idx) => run_file(&tail[idx], strict, dry_run, policy, script_args),
                None => {
                    eprintln!("Unknown command: {}", args[1]);
                    print_usage();
                    process::exit(EXIT_USAGE);
                }
            }
        }
    }
}

/// Recognise `--allow`/`--deny` in an argument loop.
///
/// Returns `(is_deny, spec)` and advances `i` past a detached value, so both
/// `--allow read=src` and `--allow=read=src` work. Returning `None` for
/// anything else lets the caller keep its existing fall-through.
fn permission_flag(tail: &[String], i: &mut usize) -> Option<(bool, String)> {
    let a = &tail[*i];
    let (flag, inline) = match a.split_once('=') {
        Some((f, v)) if f == "--allow" || f == "--deny" => (f, Some(v.to_string())),
        _ => (a.as_str(), None),
    };
    let deny = match flag {
        "--allow" => false,
        "--deny" => true,
        _ => return None,
    };
    let spec = match inline {
        Some(v) => v,
        None => {
            *i += 1;
            match tail.get(*i) {
                Some(v) => v.clone(),
                None => {
                    eprintln!("error: {} requires a capability, e.g. {} read=src", flag, flag);
                    process::exit(EXIT_USAGE);
                }
            }
        }
    };
    Some((deny, spec))
}

/// Fold one `--allow`/`--deny` spec into the policy, creating it on first use.
///
/// The policy stays `None` until a flag appears, so scripts run unrestricted
/// unless the caller asked for a sandbox. A malformed spec aborts rather than
/// being ignored: silently running unsandboxed after a typo in a security
/// flag is the worst possible failure mode.
fn apply_permission(policy: &mut Option<que_lang::permissions::Policy>, spec: &(bool, String)) {
    let p = policy.get_or_insert_with(que_lang::permissions::Policy::default);
    let (deny, text) = spec;
    let result = if *deny { p.deny(text) } else { p.allow(text) };
    if let Err(e) = result {
        eprintln!("error: {}", e);
        process::exit(EXIT_USAGE);
    }
}

/// Terminate after a SIGINT/SIGTERM unwind.
///
/// By the time this runs, every `defer` on the stack has already executed.
/// The exit code follows the shell convention of 128 + signal number, so a
/// caller can tell an interrupt apart from a script that merely failed.
fn exit_interrupted(sig: i32) -> ! {
    eprintln!("interrupted by {}", que_lang::interrupt::name_for(sig));
    process::exit(que_lang::interrupt::exit_code_for(sig));
}

fn print_version() {
    println!("Que v{}", VERSION);
}

fn print_usage() {
    eprintln!("Que v{}", VERSION);
    eprintln!();
    eprintln!("Usage: que [command] [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  <script.que>               Run a script file");
    eprintln!("  run [options] <task> [-- args...]  Run a task from a Quefile");
    eprintln!("                                     Args may be positional or named (key=value)");
    eprintln!("  tasks [options]             List available tasks");
    eprintln!("  test [options] [paths...]   Run tests");
    eprintln!("  install [options]           Fetch dependencies declared in que.toml");
    eprintln!("  fmt [options] [files...]    Format Que source files");
    eprintln!("  lint [files...]             Lint Que source files for issues");
    eprintln!("  help                        Show this help message");
    eprintln!("  version                     Show version information");
    eprintln!();
    eprintln!("Global options:");
    eprintln!("  --help, -h                  Show this help message");
    eprintln!("  --version, -V               Show version information");
    eprintln!();
    eprintln!("Run options:");
    eprintln!("  -f <file>                   Use a specific Quefile (default: auto-detect)");
    eprintln!("  -g, --global                Use the global Quefile ($QUE_HOME, ~/.config/que, ~/.que)");
    eprintln!("  --help, -h                  Show argument help for the task");
    eprintln!("  --dry-run                   Print effects instead of performing them");
    eprintln!("  --force, -B                 Run even if inputs and outputs say it is up to date");
    eprintln!("  --allow <cap>[=<list>]      Grant a capability (see Sandbox options)");
    eprintln!("  --deny <cap>                Deny a capability (see Sandbox options)");
    eprintln!("  -- arg1 arg2 ...            Pass positional arguments to the task");
    eprintln!("  -- key=value ...            Pass named arguments to the task");
    eprintln!();
    eprintln!("Quefile discovery:");
    eprintln!("  The nearest Quefile at or above the current directory is used, and its");
    eprintln!("  tasks run in the current directory — use quefile_dir() for paths relative");
    eprintln!("  to the Quefile. A task it does not define is looked up in the global");
    eprintln!("  Quefile.");
    eprintln!();
    eprintln!("Test options:");
    eprintln!("  --filter <text>             Only run tests whose name contains <text>");
    eprintln!("  [paths...]                  Files or directories (default: current directory)");
    eprintln!();
    eprintln!("Install options:");
    eprintln!("  --locked                    Fail instead of resolving anything not in que.lock");
    eprintln!();
    eprintln!("Format options:");
    eprintln!("  --check                     Check formatting without writing (exit 1 if unformatted)");
    eprintln!("  --diff                      Show diff without writing");
    eprintln!();
    eprintln!("Script options:");
    eprintln!("  --strict                    Enforce type annotations at runtime");
    eprintln!("  --dry-run                   Print effects instead of performing them");
    eprintln!();
    eprintln!("Sandbox options (accepted by both `que <script>` and `que run`):");
    eprintln!("  --allow <cap>[=<list>]      Grant a capability; everything else is denied");
    eprintln!("  --deny <cap>                Deny one capability; everything else is granted");
    eprintln!("      capabilities: read, write, exec, net, env");
    eprintln!("      examples: --allow read=src,. --allow net=api.example.com --deny exec");
    eprintln!("  Without any --allow/--deny flag the script runs unrestricted.");
    eprintln!();
    eprintln!("With no arguments, starts an interactive REPL.");
}

/// Filenames accepted as a Quefile, in the order they are tried.
const QUEFILE_NAMES: [&str; 3] = ["Quefile", "Quefile.que", "quefile.que"];

/// A Quefile the CLI can load, and where it came from.
#[derive(Clone)]
struct QuefileSource {
    /// Path to read: a bare filename when it sits in the current directory,
    /// the path walked up to otherwise.
    path: String,
    /// The user's global Quefile rather than a project one.
    global: bool,
}

impl QuefileSource {
    /// How a caller would invoke `que run` for a task in this file.
    fn run_prefix(&self) -> String {
        if self.global {
            "que run -g".to_string()
        } else {
            format!("que run [-f {}]", self.path)
        }
    }
}

/// The Quefiles a command should consider, in precedence order.
struct QuefileSelection {
    /// `-f <file>`, or the nearest Quefile at or above the current directory.
    project: Option<QuefileSource>,
    /// The user's global Quefile, consulted for tasks the project file does
    /// not define. `None` once `-f` or a missing global rules it out.
    global: Option<QuefileSource>,
}

/// Directories searched for the global Quefile, in order.
fn global_quefile_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(h) = env::var("QUE_HOME") {
        if !h.is_empty() {
            dirs.push(std::path::PathBuf::from(h));
        }
    }
    if let Ok(x) = env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            dirs.push(Path::new(&x).join("que"));
        }
    }
    if let Some(home) = que_lang::interpreter::home_dir() {
        dirs.push(Path::new(&home).join(".config").join("que"));
        dirs.push(Path::new(&home).join(".que"));
    }
    dirs
}

/// The nearest Quefile at or above the current directory.
///
/// Walking up is what lets `que run test` work from a subdirectory of a
/// project, the same way `git` works from anywhere inside a checkout. The
/// directory is *not* changed to match: a task runs where the user is
/// standing, so relative paths mean what they would have meant in the shell.
/// A Quefile that wants its own directory asks for `quefile_dir()`.
fn find_project_quefile() -> Option<QuefileSource> {
    let cwd = env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        for name in QUEFILE_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(if dir == cwd {
                    QuefileSource { path: name.to_string(), global: false }
                } else {
                    QuefileSource { path: candidate.display().to_string(), global: false }
                });
            }
        }
        dir = dir.parent()?;
    }
}

/// The user's global Quefile, if they have written one.
fn find_global_quefile() -> Option<QuefileSource> {
    for dir in global_quefile_dirs() {
        for name in QUEFILE_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(QuefileSource {
                    path: candidate.display().to_string(),
                    global: true,
                });
            }
        }
    }
    None
}

/// Report that there is nothing to run and exit.
fn exit_no_quefile(global_only: bool) -> ! {
    if global_only {
        eprintln!("error: no global Quefile found");
    } else {
        eprintln!("error: no Quefile found in this directory or any parent, and no global Quefile");
        eprintln!("Looked for: {}", QUEFILE_NAMES.join(", "));
    }
    let dirs: Vec<String> = global_quefile_dirs()
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    eprintln!("Global locations: {}", dirs.join(", "));
    if !global_only {
        eprintln!("Use -f <file> to specify a file explicitly.");
    }
    process::exit(EXIT_USAGE);
}

/// Resolve which Quefiles to use: `-f <file>` pins one, `-g` selects the
/// global one, and otherwise the nearest project file is used with the
/// global file behind it as a fallback.
fn resolve_quefile(args: &[String]) -> (QuefileSelection, Vec<String>) {
    let mut file: Option<String> = None;
    let mut force_global = false;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-f" {
            if i + 1 >= args.len() {
                eprintln!("error: -f requires a file argument");
                process::exit(EXIT_USAGE);
            }
            file = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "-g" || args[i] == "--global" {
            force_global = true;
            i += 1;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }

    if file.is_some() && force_global {
        eprintln!("error: -f and -g cannot be combined");
        process::exit(EXIT_USAGE);
    }

    let selection = if let Some(f) = file {
        QuefileSelection {
            project: Some(QuefileSource { path: f, global: false }),
            global: None,
        }
    } else if force_global {
        match find_global_quefile() {
            Some(g) => QuefileSelection { project: None, global: Some(g) },
            None => exit_no_quefile(true),
        }
    } else {
        let selection = QuefileSelection {
            project: find_project_quefile(),
            global: find_global_quefile(),
        };
        if selection.project.is_none() && selection.global.is_none() {
            exit_no_quefile(false);
        }
        selection
    };
    (selection, rest)
}

/// Load and execute a Quefile, returning the interpreter with all
/// definitions in scope.
fn load_quefile(file: &QuefileSource) -> Interpreter {
    let path = file.path.as_str();
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", path, e);
            process::exit(EXIT_USAGE);
        }
    };
    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {}", e);
            process::exit(e.process_exit_code());
        }
    };
    let mut parser = Parser::new(tokens);
    let module = match parser.parse_module() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse error: {}", e);
            process::exit(e.process_exit_code());
        }
    };
    let mut interp = Interpreter::new();
    interp.direct_output = true;
    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
    interp.set_script_path(abs_path);
    // A global task reads and writes files where the user is standing, not
    // where the Quefile lives, so its skip-detection cache belongs there too.
    // One cache under the home directory would otherwise describe every
    // project the user has ever run a global task in.
    if file.global {
        if let Ok(cwd) = env::current_dir() {
            interp.set_task_cache_dir(cwd);
        }
    }
    interp.init_module_loader();
    match interp.exec_module(&module) {
        Ok(_) => {}
        Err(Signal::Error(e)) => {
            eprintln!("runtime error: {}", e);
            process::exit(e.process_exit_code());
        }
        Err(Signal::Return(_)) => {}
        Err(Signal::Exit(code)) => {
            process::exit(code);
        }
        Err(Signal::Interrupted(sig)) => exit_interrupted(sig),
        Err(signal) => {
            eprintln!("runtime error: unexpected signal {:?}", signal);
            process::exit(1);
        }
    }
    interp
}

/// Collect all tasks from an interpreter's environment.
fn collect_tasks(interp: &Interpreter) -> Vec<(String, Box<que_lang::value::TaskData>)> {
    let all_vars = interp.env.list_vars();
    let mut tasks: Vec<(String, Box<que_lang::value::TaskData>)> = Vec::new();
    for (name, val, _) in all_vars {
        if let Value::Task(t) = val {
            tasks.push((name.clone(), t.clone()));
        }
    }
    tasks.sort_by(|a, b| a.0.cmp(&b.0));
    tasks
}

/// Parse run args: extract -f/-g, task name, --help/--dry-run/--force flags, and optional task args after `--`.
fn parse_run_args(
    args: &[String],
) -> (
    QuefileSelection,
    Option<String>,
    bool,
    bool,
    bool,
    Option<que_lang::permissions::Policy>,
    Vec<String>,
) {
    // Split on `--` first, before resolving quefile (so -f before `--` still works)
    let (before_sep, task_args) = if let Some(pos) = args.iter().position(|a| a == "--") {
        (&args[..pos], args[pos + 1..].to_vec())
    } else {
        (args, Vec::new())
    };

    let (selection, rest) = resolve_quefile(before_sep);
    let mut task_name: Option<String> = None;
    let mut help = false;
    let mut dry_run = false;
    let mut force = false;
    let mut policy: Option<que_lang::permissions::Policy> = None;
    let mut idx = 0;
    while idx < rest.len() {
        let arg = &rest[idx];
        if arg == "--help" || arg == "-h" {
            help = true;
        } else if arg == "--dry-run" {
            dry_run = true;
        } else if arg == "--force" || arg == "-B" {
            force = true;
        } else if let Some(spec) = permission_flag(&rest, &mut idx) {
            apply_permission(&mut policy, &spec);
        } else if task_name.is_none() {
            task_name = Some(arg.clone());
        } else {
            eprintln!("error: unexpected argument: {}", arg);
            eprintln!("Usage: que run [-f <file>] <task> [-- arg1 arg2 ...]");
            process::exit(EXIT_USAGE);
        }
        idx += 1;
    }
    (selection, task_name, help, dry_run, force, policy, task_args)
}

/// The outcome of looking a task name up across the selected Quefiles.
enum TaskLookup {
    Found(QuefileSource, Interpreter, Box<que_lang::value::TaskData>),
    /// The name exists but is bound to something else — reported rather than
    /// fallen through, because a collision like that is a mistake worth seeing.
    NotATask(QuefileSource, String),
    Missing,
}

/// Find `task_name`, trying the project Quefile first and the global one only
/// if the project file does not define it.
///
/// The global file is not merely searched last, it is not *executed* at all
/// unless it is needed: a project run should never pay for — nor be disturbed
/// by — whatever the user keeps in their home directory.
fn lookup_task(selection: &QuefileSelection, task_name: &str) -> TaskLookup {
    if let Some(project) = &selection.project {
        let interp = load_quefile(project);
        match interp.env.get(task_name) {
            Some(Value::Task(t)) => return TaskLookup::Found(project.clone(), interp, t),
            Some(other) => {
                return TaskLookup::NotATask(project.clone(), other.type_name().to_string())
            }
            None => {}
        }
    }

    let Some(global) = &selection.global else {
        return TaskLookup::Missing;
    };
    let interp = load_quefile(global);
    match interp.env.get(task_name) {
        Some(Value::Task(t)) => TaskLookup::Found(global.clone(), interp, t),
        Some(other) => TaskLookup::NotATask(global.clone(), other.type_name().to_string()),
        None => TaskLookup::Missing,
    }
}

/// `que run [-f file | -g] [--help] <task> [-- arg1 arg2 ...]`
fn cmd_run(args: &[String]) {
    let (selection, task_name, help, dry_run, force, policy, task_args) = parse_run_args(args);

    let task_name = match task_name {
        Some(n) => n,
        None => {
            eprintln!("error: no task specified");
            eprintln!("Usage: que run [-f <file>] <task> [-- arg1 arg2 ...]");
            process::exit(EXIT_USAGE);
        }
    };

    let (source, mut interp, task) = match lookup_task(&selection, &task_name) {
        TaskLookup::Found(source, interp, task) => (source, interp, task),
        TaskLookup::NotATask(source, kind) => {
            eprintln!(
                "error: '{}' in {} is not a task (it's a {})",
                task_name, source.path, kind
            );
            process::exit(EXIT_USAGE);
        }
        TaskLookup::Missing => {
            match (&selection.project, &selection.global) {
                (Some(project), Some(global)) => {
                    eprintln!("error: no task named '{}' in {}", task_name, project.path);
                    eprintln!("       and none in the global Quefile {}", global.path);
                }
                (Some(project), None) => {
                    eprintln!("error: no task named '{}' in {}", task_name, project.path);
                }
                (None, Some(global)) => {
                    eprintln!(
                        "error: no task named '{}' in the global Quefile {}",
                        task_name, global.path
                    );
                }
                (None, None) => eprintln!("error: no task named '{}'", task_name),
            }
            process::exit(EXIT_USAGE);
        }
    };

    interp.dry_run = dry_run;
    interp.force_run = force;
    interp.permissions = policy;

    if help {
        print_task_help(&source, &task);
        return;
    }
    // Parse task args: `key=value` → named, plain values → positional
    let arg_values: Vec<(Option<String>, Value)> = task_args
        .iter()
        .map(|s| {
            if let Some(eq_pos) = s.find('=') {
                let key = s[..eq_pos].to_string();
                let val = s[eq_pos + 1..].to_string();
                (Some(key), Value::String(val))
            } else {
                (None, Value::String(s.clone()))
            }
        })
        .collect();
    match interp.execute_task(&task, arg_values) {
        Ok(_) => {}
        Err(Signal::Error(e)) => {
            eprintln!("task '{}' failed: {}", task_name, e);
            process::exit(e.process_exit_code());
        }
        Err(Signal::Exit(code)) => {
            process::exit(code);
        }
        Err(Signal::Interrupted(sig)) => exit_interrupted(sig),
        Err(_) => {}
    }
}

/// Print usage/argument help for a single task.
fn print_task_help(source: &QuefileSource, task: &que_lang::value::TaskData) {
    let run = source.run_prefix();
    // Usage line — show both positional and named forms for params
    if task.params.is_empty() {
        println!(
            "Usage: {} {}",
            run, task.name
        );
    } else {
        let param_usage: Vec<String> = task.params.iter().map(|p| {
            if p.rest {
                format!("[{}...]", p.name)
            } else if p.default.is_some() {
                format!("[{}=<{}>]", p.name, p.name)
            } else {
                format!("{}=<{}>", p.name, p.name)
            }
        }).collect();
        println!(
            "Usage: {} {} -- {}",
            run, task.name, param_usage.join(" ")
        );
        // Also show positional form if params exist
        let positional_usage: Vec<String> = task.params.iter().map(|p| {
            if p.rest {
                format!("[{}...]", p.name)
            } else if p.default.is_some() {
                format!("[{}]", p.name)
            } else {
                format!("<{}>", p.name)
            }
        }).collect();
        println!(
            "       {} {} -- {}  (positional)",
            run, task.name, positional_usage.join(" ")
        );
    }

    // Description
    if let Some(ref desc) = task.description {
        println!();
        println!("{}", desc);
    }

    // Dependencies
    if !task.depends_on.is_empty() {
        println!();
        println!("Depends on: {}", task.depends_on.join(", "));
    }

    // Parameters table
    if !task.params.is_empty() {
        println!();
        println!("Arguments:");
        let max_name = task.params.iter().map(|p| p.name.len()).max().unwrap_or(0);
        for param in &task.params {
            let type_str = param
                .type_ann
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| if param.rest { "List".to_string() } else { "any".to_string() });
            let opt_str = if param.rest {
                " [rest]"
            } else if param.default.is_some() {
                " [optional]"
            } else {
                " [required]"
            };
            println!(
                "  {:<width$}  {}{}",
                param.name,
                type_str,
                opt_str,
                width = max_name
            );
        }
    }
}

/// `que tasks [-f file | -g]`
fn cmd_tasks(args: &[String]) {
    let (selection, extra) = resolve_quefile(args);

    if !extra.is_empty() {
        eprintln!("error: unexpected arguments: {}", extra.join(" "));
        eprintln!("Usage: que tasks [-f <file>] [-g]");
        process::exit(EXIT_USAGE);
    }

    let mut project_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut printed = false;

    if let Some(project) = &selection.project {
        let interp = load_quefile(project);
        let tasks = collect_tasks(&interp);
        project_names = tasks.iter().map(|(n, _)| n.clone()).collect();
        if tasks.is_empty() {
            println!("No tasks defined in {}", project.path);
        } else {
            print_task_section(
                &format!("Tasks in {}:", project.path),
                &tasks,
                &std::collections::HashSet::new(),
            );
        }
        printed = true;
    }

    if let Some(global) = &selection.global {
        let interp = load_quefile(global);
        let tasks = collect_tasks(&interp);
        if !tasks.is_empty() {
            if printed {
                println!();
            }
            // Names the project file already defines are unreachable without
            // `-g`, so they are listed but marked rather than quietly dropped.
            print_task_section(
                &format!("Global tasks in {}:", global.path),
                &tasks,
                &project_names,
            );
            printed = true;
        }
    }

    if !printed {
        println!("No tasks defined");
    }
}

/// Print one titled block of tasks, marking any name in `shadowed_by`.
fn print_task_section(
    title: &str,
    tasks: &[(String, Box<que_lang::value::TaskData>)],
    shadowed_by: &std::collections::HashSet<String>,
) {
    // Build display strings and find the longest left column for alignment
    let entries: Vec<(String, String)> = tasks.iter().map(|(name, data)| {
        let mut left = if data.depends_on.is_empty() {
            name.clone()
        } else {
            format!("{} [{}]", name, data.depends_on.join(", "))
        };
        if shadowed_by.contains(name) {
            left.push_str(" (shadowed)");
        }
        let desc = data.description.as_deref().unwrap_or("").to_string();
        (left, desc)
    }).collect();

    let max_left = entries.iter().map(|(l, _)| l.len()).max().unwrap_or(0);

    println!("{}", title);
    println!();
    for (left, desc) in &entries {
        if desc.is_empty() {
            println!("  {}", left);
        } else {
            println!("  {:<width$}  — {}", left, desc, width = max_left);
        }
    }
}

/// `que fmt [--check] [--diff] [files...]`
fn cmd_fmt(args: &[String]) {
    let mut check_only = false;
    let mut show_diff = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--check" => check_only = true,
            "--diff" => show_diff = true,
            _ => files.push(arg.clone()),
        }
    }

    // If no files specified, find all .que files in current directory (recursively)
    if files.is_empty() {
        files = find_que_files(".");
    }

    if files.is_empty() {
        eprintln!("No .que files found");
        process::exit(EXIT_USAGE);
    }

    let mut any_unformatted = false;

    for file_path in &files {
        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read '{}': {}", file_path, e);
                continue;
            }
        };

        let mut lexer = Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: {}: lex error: {}", file_path, e);
                continue;
            }
        };
        let mut parser = Parser::new(tokens);
        let module = match parser.parse_module() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("error: {}: parse error: {}", file_path, e);
                continue;
            }
        };

        let formatted = Formatter::new().format_module(&module);

        if source == formatted {
            continue;
        }

        any_unformatted = true;

        if check_only {
            println!("Would reformat: {}", file_path);
        } else if show_diff {
            println!("--- {}", file_path);
            println!("+++ {}", file_path);
            // Simple line-by-line diff
            let old_lines: Vec<&str> = source.lines().collect();
            let new_lines: Vec<&str> = formatted.lines().collect();
            let max = old_lines.len().max(new_lines.len());
            for i in 0..max {
                let old = old_lines.get(i).copied().unwrap_or("");
                let new = new_lines.get(i).copied().unwrap_or("");
                if old != new {
                    if !old.is_empty() {
                        println!("-{}", old);
                    }
                    if !new.is_empty() {
                        println!("+{}", new);
                    }
                }
            }
        } else {
            match fs::write(file_path, &formatted) {
                Ok(_) => println!("Formatted: {}", file_path),
                Err(e) => eprintln!("error: cannot write '{}': {}", file_path, e),
            }
        }
    }

    if check_only && any_unformatted {
        process::exit(1);
    }
}

/// `que install [--locked]`
///
/// Resolution is anchored at the package root — the nearest ancestor with a
/// que.toml — so the command works from anywhere inside a project, the way
/// every other tool in a repository does.
fn cmd_install(args: &[String]) {
    use colored::Colorize;

    let mut locked = false;
    for arg in args {
        match arg.as_str() {
            "--locked" => locked = true,
            other => {
                eprintln!("error: unknown option: {}", other);
                eprintln!("Usage: que install [--locked]");
                process::exit(EXIT_USAGE);
            }
        }
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = que_lang::module_loader::find_package_root(&cwd);

    match que_lang::install::install(&root, locked) {
        Ok(report) => {
            for line in &report.lines {
                println!("  {}", line);
            }
            if locked && report.lock_changed {
                eprintln!(
                    "{}",
                    "error: que.lock is out of date with que.toml; run `que install` and commit it"
                        .red()
                );
                process::exit(EXIT_FAILURE);
            }
            println!("{}", "install complete".green());
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            process::exit(EXIT_FAILURE);
        }
    }
}

/// `que lint [files...]`
/// `que test [--filter <text>] [paths...]`
fn cmd_test(args: &[String]) {
    use colored::Colorize;
    use que_lang::test_runner;

    let mut filter: Option<String> = None;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("error: --filter requires a value");
                    process::exit(EXIT_USAGE);
                };
                filter = Some(value.clone());
                i += 2;
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option: {}", other);
                eprintln!("Usage: que test [--filter <text>] [paths...]");
                process::exit(EXIT_USAGE);
            }
            other => {
                roots.push(std::path::PathBuf::from(other));
                i += 1;
            }
        }
    }
    if roots.is_empty() {
        roots.push(std::path::PathBuf::from("."));
    }

    let files = test_runner::discover(&roots);
    if files.is_empty() {
        eprintln!("No test files found.");
        eprintln!("A test file is named `*_test.que` or `test_*.que`, or lives under a `tests/` directory.");
        process::exit(EXIT_USAGE);
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut load_errors = 0usize;

    for file in &files {
        let report = test_runner::run_file(file, filter.as_deref());
        let display = file.display();

        if let Some(err) = &report.load_error {
            load_errors += 1;
            println!("{} {}", "ERROR".red().bold(), display);
            println!("       {}", err);
            continue;
        }
        if report.outcomes.is_empty() {
            continue;
        }

        println!("{}", display.to_string().bold());
        for outcome in &report.outcomes {
            match &outcome.failure {
                None => {
                    passed += 1;
                    println!("  {} {}", "ok  ".green(), outcome.name);
                }
                Some(message) => {
                    failed += 1;
                    println!("  {} {}", "FAIL".red().bold(), outcome.name);
                    for line in message.lines() {
                        println!("       {}", line);
                    }
                    // Output is shown only here: it is the context that
                    // explains the failure, and noise everywhere else.
                    if !outcome.output.is_empty() {
                        println!("       {}", "--- output ---".dimmed());
                        for line in &outcome.output {
                            println!("       {}", line.dimmed());
                        }
                    }
                }
            }
        }
    }

    let total = passed + failed;
    println!();
    if total == 0 && load_errors == 0 {
        // Reporting success for a run that verified nothing is how a broken
        // filter or a renamed file goes unnoticed in CI for a month.
        match &filter {
            Some(f) => eprintln!("error: no test matches --filter {}", f),
            None => eprintln!(
                "error: no tests found in {} file(s); a test is a top-level `fn {}…()`",
                files.len(),
                test_runner::TEST_PREFIX
            ),
        }
        process::exit(EXIT_USAGE);
    }
    if failed == 0 && load_errors == 0 {
        println!("{}", format!("{} passed", total).green().bold());
        return;
    }
    let mut summary = format!("{} of {} failed", failed, total);
    if load_errors > 0 {
        summary.push_str(&format!(", {} file(s) could not be loaded", load_errors));
    }
    println!("{}", summary.red().bold());
    process::exit(EXIT_FAILURE);
}

fn cmd_lint(args: &[String]) {
    let mut files: Vec<String> = args.to_vec();

    if files.is_empty() {
        files = find_que_files(".");
    }

    if files.is_empty() {
        eprintln!("No .que files found");
        process::exit(EXIT_USAGE);
    }

    let mut total_warnings = 0;
    let mut total_errors = 0;

    for file_path in &files {
        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read '{}': {}", file_path, e);
                continue;
            }
        };

        let mut lexer = Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: {}: lex error: {}", file_path, e);
                continue;
            }
        };
        let mut parser = Parser::new(tokens);
        let module = match parser.parse_module() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("error: {}: parse error: {}", file_path, e);
                continue;
            }
        };

        let diagnostics = Linter::new().lint_module(&module);
        for diag in &diagnostics {
            if let Some(line) = diag.line {
                eprintln!("{}:{}  {} [{}]", file_path, line, diag.message, diag.rule);
            } else {
                eprintln!("{}  {} [{}]", file_path, diag.message, diag.rule);
            }
        }
        total_errors += diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        total_warnings += diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
    }

    if total_errors > 0 || total_warnings > 0 {
        eprintln!();
        eprintln!("{} error(s), {} warning(s) found", total_errors, total_warnings);
        process::exit(1);
    }
}

/// Recursively find all .que files in a directory.
fn find_que_files(dir: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                // Skip hidden dirs and common non-source dirs
                if !dir_name.starts_with('.') && dir_name != "target" && dir_name != "node_modules"
                {
                    result.extend(find_que_files(&path.to_string_lossy()));
                }
            } else if let Some(ext) = path.extension() {
                if ext == "que" {
                    result.push(path.to_string_lossy().to_string());
                }
            } else if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if name == "Quefile" {
                    result.push(path.to_string_lossy().to_string());
                }
            }
        }
    }
    result.sort();
    result
}

fn run_file(
    path: &str,
    strict: bool,
    dry_run: bool,
    permissions: Option<que_lang::permissions::Policy>,
    script_args: Vec<String>,
) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", path, e);
            process::exit(EXIT_USAGE);
        }
    };
    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {}", e);
            process::exit(e.process_exit_code());
        }
    };
    let mut parser = Parser::new(tokens);
    let module = match parser.parse_module() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse error: {}", e);
            process::exit(e.process_exit_code());
        }
    };
    let mut interp = Interpreter::new();
    interp.direct_output = true;
    interp.strict = strict;
    interp.dry_run = dry_run;
    interp.permissions = permissions;
    interp.set_script_args(script_args);
    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
    interp.set_script_path(abs_path);
    interp.init_module_loader();
    match interp.exec_module(&module) {
        Ok(_) => {}
        Err(Signal::Error(e)) => {
            eprintln!("runtime error: {}", e);
            process::exit(e.process_exit_code());
        }
        Err(Signal::Return(_)) => {}
        Err(Signal::Exit(code)) => {
            process::exit(code);
        }
        Err(Signal::Interrupted(sig)) => exit_interrupted(sig),
        Err(signal) => {
            eprintln!("runtime error: unexpected signal {:?}", signal);
            process::exit(1);
        }
    }
}

fn repl() {
    use colored::Colorize;

    println!(
        "{}  —  type {} for an overview, {} to inspect a value,",
        format!("Que v{}", VERSION).cyan().bold(),
        "help()".green(),
        "?name".green()
    );
    println!(
        "              {} to list bindings, {} or Ctrl-D to exit. Tab for completion.",
        ":vars".green(),
        ":q".green()
    );

    let interp = std::rc::Rc::new(std::cell::RefCell::new(Interpreter::new()));

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut rl: Editor<QueHelper, DefaultHistory> =
        Editor::with_config(config).expect("failed to initialise line editor");
    rl.set_helper(Some(QueHelper::new(interp.clone())));

    // Load history from ~/.que_history if it exists.
    let history_path = dirs_history_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    loop {
        // Read potentially multiline input.
        let input = match read_multiline_input(&mut rl) {
            ReadResult::Input(s) => s,
            ReadResult::Empty => continue,
            ReadResult::Eof => break,
        };

        let _ = rl.add_history_entry(&input);

        // Handle REPL meta-commands (lines starting with ':' or '?').
        let input = {
            let mut interp_mut = interp.borrow_mut();
            match handle_meta_command(&input, &mut interp_mut) {
                MetaResult::Rewritten(s) => s,
                MetaResult::Handled => continue,
                MetaResult::Quit => break,
                MetaResult::NotMeta => input,
            }
        };

        let mut lexer = Lexer::new(&input);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{} {}", "lex error:".red().bold(), e);
                continue;
            }
        };
        let mut parser = Parser::new(tokens);
        let module = match parser.parse_module() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{} {}", "parse error:".red().bold(), e);
                continue;
            }
        };
        let mut interp_mut = interp.borrow_mut();
        let prev_output_len = interp_mut.output.len();
        match interp_mut.exec_module(&module) {
            Ok(val) => {
                for line in &interp_mut.output[prev_output_len..] {
                    println!("{}", line);
                }
                match &val {
                    que_lang::value::Value::Null => {}
                    v => println!("{} {}", "=>".green().bold(), v),
                }
            }
            Err(Signal::Error(e)) => {
                for line in &interp_mut.output[prev_output_len..] {
                    println!("{}", line);
                }
                eprintln!("{} {}", "error:".red().bold(), e);
            }
            Err(Signal::Return(v)) => {
                for line in &interp_mut.output[prev_output_len..] {
                    println!("{}", line);
                }
                println!("{} {}", "=>".green().bold(), v);
            }
            Err(Signal::Exit(code)) => {
                for line in &interp_mut.output[prev_output_len..] {
                    println!("{}", line);
                }
                process::exit(code);
            }
            // A signal seen mid-evaluation cancels the entry, not the session.
            Err(Signal::Interrupted(_)) => {
                que_lang::interrupt::clear();
                eprintln!("{}", "interrupted".yellow());
            }
            Err(_) => {}
        }
    }

    // Save history for next session.
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    println!();
}

enum ReadResult {
    Input(String),
    Empty,
    Eof,
}

/// Read a potentially multiline expression from the REPL.
///
/// Continues prompting with `  ...> ` if the input has:
///   - unclosed brackets / braces / parentheses, or
///   - a trailing operator that implies continuation (e.g. `|>`, `+`, `,`).
fn read_multiline_input(rl: &mut Editor<QueHelper, DefaultHistory>) -> ReadResult {
    let mut buffer = String::new();
    let mut first_line = true;

    loop {
        // The prompts are colourised by `QueHelper::highlight_prompt`, which
        // matches on these exact strings.
        let prompt = if first_line {
            que_lang::completion::PROMPT
        } else {
            que_lang::completion::CONTINUATION_PROMPT
        };
        match rl.readline(prompt) {
            Ok(line) => {
                if first_line && line.trim().is_empty() {
                    return ReadResult::Empty;
                }

                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);
                first_line = false;

                if is_input_complete(&buffer) {
                    return ReadResult::Input(buffer);
                }
                // Otherwise, keep reading lines.
            }
            Err(ReadlineError::Interrupted) => {
                if first_line {
                    // Ctrl-C on empty prompt — ignore, keep REPL running.
                    return ReadResult::Empty;
                } else {
                    // Ctrl-C during multiline — discard accumulated input.
                    use colored::Colorize;
                    eprintln!("{}", "(input discarded)".yellow());
                    return ReadResult::Empty;
                }
            }
            Err(ReadlineError::Eof) => {
                if first_line {
                    return ReadResult::Eof;
                } else {
                    // Ctrl-D during multiline — try to execute what we have.
                    return ReadResult::Input(buffer);
                }
            }
            Err(_) => return ReadResult::Eof,
        }
    }
}

enum MetaResult {
    /// Not a meta-command — pass the original input through.
    NotMeta,
    /// Rewrite to this Que source and execute normally.
    Rewritten(String),
    /// Handled entirely (e.g. :reset, :load); skip evaluation this iteration.
    Handled,
    /// Exit the REPL.
    Quit,
}

fn is_bare_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Recognise REPL meta-commands. Lines starting with `:` or `?` are intercepted
/// before being passed to the lexer.
fn handle_meta_command(input: &str, interp: &mut Interpreter) -> MetaResult {
    use colored::Colorize;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return MetaResult::NotMeta;
    }

    // `?expr` → help(expr); `?` alone → help()
    if let Some(rest) = trimmed.strip_prefix('?') {
        let r = rest.trim();
        return if r.is_empty() {
            MetaResult::Rewritten("help()".to_string())
        } else if is_bare_ident(r) {
            // Bare names get string lookup so `?fs` finds the std module
            // even when no value `fs` is bound.
            MetaResult::Rewritten(format!("help(\"{}\")", r))
        } else {
            MetaResult::Rewritten(format!("help({})", r))
        };
    }

    if !trimmed.starts_with(':') {
        return MetaResult::NotMeta;
    }

    let body = &trimmed[1..];
    let (cmd, arg) = match body.find(char::is_whitespace) {
        Some(i) => (&body[..i], body[i..].trim()),
        None => (body, ""),
    };

    match cmd {
        "h" | "help" => {
            if arg.is_empty() {
                MetaResult::Rewritten("help()".to_string())
            } else if is_bare_ident(arg) {
                MetaResult::Rewritten(format!("help(\"{}\")", arg))
            } else {
                MetaResult::Rewritten(format!("help({})", arg))
            }
        }
        "t" | "type" => {
            if arg.is_empty() {
                eprintln!("{} :t <expr>", "usage:".yellow().bold());
                MetaResult::Handled
            } else {
                MetaResult::Rewritten(format!("typeof({})", arg))
            }
        }
        "m" | "methods" => {
            if arg.is_empty() {
                eprintln!("{} :m <expr>", "usage:".yellow().bold());
                MetaResult::Handled
            } else {
                MetaResult::Rewritten(format!("({}).methods()", arg))
            }
        }
        "i" | "inspect" => {
            if arg.is_empty() {
                eprintln!("{} :i <expr>", "usage:".yellow().bold());
                MetaResult::Handled
            } else {
                MetaResult::Rewritten(format!("({}).inspect()", arg))
            }
        }
        // `vars` lives in std.reflect now, but a REPL shortcut that made you
        // import something first would not be a shortcut.
        "v" | "vars" => {
            MetaResult::Rewritten("import std.reflect\nreflect.vars()".to_string())
        }
        "r" | "reset" => {
            *interp = Interpreter::new();
            println!("{}", "(interpreter reset)".yellow());
            MetaResult::Handled
        }
        "q" | "quit" | "exit" => MetaResult::Quit,
        "load" => {
            if arg.is_empty() {
                eprintln!("{} :load <file.que>", "usage:".yellow().bold());
                return MetaResult::Handled;
            }
            match std::fs::read_to_string(arg) {
                Ok(src) => MetaResult::Rewritten(src),
                Err(e) => {
                    eprintln!("{} cannot read {}: {}", "error:".red().bold(), arg.green(), e);
                    MetaResult::Handled
                }
            }
        }
        _ => {
            eprintln!(
                "{} unknown meta-command {} — try {} for help",
                "error:".red().bold(),
                format!(":{}", cmd).green(),
                ":h".green()
            );
            MetaResult::Handled
        }
    }
}

/// Heuristic: returns `true` when the accumulated buffer looks like a
/// complete, ready-to-evaluate input.
fn is_input_complete(input: &str) -> bool {
    let mut lexer = Lexer::new(input);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        // Lex error (e.g. unterminated string) — let the user see the error.
        Err(_) => return true,
    };

    // Count delimiter depth.
    let mut depth: i32 = 0;
    for token in &tokens {
        match &token.kind {
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            _ => {}
        }
    }
    if depth > 0 {
        return false;
    }

    // Check whether the last meaningful token is something that clearly
    // expects more input on the next line.
    let last = tokens
        .iter()
        .rev()
        .find(|t| !matches!(t.kind, TokenKind::Newline | TokenKind::Eof));

    if let Some(tok) = last {
        !matches!(
            tok.kind,
            TokenKind::PipeArrow
                | TokenKind::Eq
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::FatArrow
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Power
                | TokenKind::And
                | TokenKind::Or
                | TokenKind::Comma
                | TokenKind::Spread
                | TokenKind::Dot
                | TokenKind::NullCoalesce
                | TokenKind::Shl
                | TokenKind::Shr
                | TokenKind::BitAnd
                | TokenKind::BitXor
        )
    } else {
        true
    }
}

/// Returns the path for persistent REPL history (~/.que_history).
fn dirs_history_path() -> Option<String> {
    let home = env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| env::var("USERPROFILE").ok().filter(|h| !h.is_empty()))?;
    Some(
        Path::new(&home)
            .join(".que_history")
            .to_string_lossy()
            .to_string(),
    )
}
