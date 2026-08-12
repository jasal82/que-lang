//! Command-line help.
//!
//! Every page is data rather than a pile of `println!`s, so a new flag is one
//! row in a table and cannot drift out of alignment with the rest. Rendering
//! is centralised: one two-column layout, one wrap width, one colour scheme.
//!
//! Help asked for explicitly goes to stdout — it is the output the user
//! requested, and piping it to a pager should work. Only the terse hints
//! printed beside an error go to stderr.

use colored::Colorize;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A labelled block of `flag  description` rows, plus optional prose.
struct Section {
    title: &'static str,
    rows: &'static [(&'static str, &'static str)],
    notes: &'static [&'static str],
}

/// One help screen: `que help <name>`.
struct Page {
    /// Command name, or `""` for the top-level page.
    name: &'static str,
    /// One line, also used as the command's entry in the top-level list.
    tagline: &'static str,
    usage: &'static [&'static str],
    sections: &'static [Section],
    footer: &'static [&'static str],
}

/// Shared by `que <script>` and `que run`, so it is written once.
const SANDBOX: Section = Section {
    title: "Sandbox",
    rows: &[
        (
            "--allow <cap>[=<list>]",
            "Grant one capability; everything else is denied",
        ),
        (
            "--deny <cap>",
            "Deny one capability; everything else is granted",
        ),
    ],
    notes: &[
        "Capabilities: read, write, exec, net, env.",
        "Examples: --allow read=src,. --allow net=api.example.com --deny exec",
        "Without any --allow or --deny flag the code runs unrestricted.",
    ],
};

const QUEFILE_DISCOVERY: &str = concat!(
    "The nearest Quefile at or above the current directory is used, and its tasks ",
    "run in the current directory — use quefile_dir() for paths relative to the ",
    "Quefile. A task it does not define is looked up in the global Quefile ",
    "($QUE_HOME, $XDG_CONFIG_HOME/que, ~/.config/que, ~/.que)."
);

const ROOT: Page = Page {
    name: "",
    tagline: "The Que scripting language for DevOps and build automation",
    usage: &[
        "que <script.que> [options] [-- args...]",
        "que <command> [options]",
        "que                              (starts the interactive REPL)",
    ],
    sections: &[
        Section {
            title: "Commands",
            rows: &[
                ("run", "Run a task from a Quefile"),
                ("tasks", "List the tasks a Quefile defines"),
                ("test", "Run tests"),
                ("install", "Fetch dependencies declared in que.toml"),
                ("fmt", "Format Que source files"),
                ("lint", "Check Que source files for common mistakes"),
                ("help [command]", "Show this message, or help for one command"),
                ("version", "Show version information"),
            ],
            notes: &[],
        },
        Section {
            title: "Script options",
            rows: &[
                ("--strict", "Enforce type annotations at runtime"),
                ("--dry-run", "Print effects instead of performing them"),
                ("--allow <cap>[=<list>]", "Grant a capability (see Sandbox)"),
                ("--deny <cap>", "Deny a capability (see Sandbox)"),
                ("-- <args...>", "Arguments for the script, readable with args()"),
            ],
            notes: &[],
        },
        Section {
            title: "Global options",
            rows: &[
                ("-h, --help", "Show this message"),
                ("-V, --version", "Show version information"),
            ],
            notes: &[],
        },
        SANDBOX,
    ],
    footer: &["Run `que help <command>` for the options of a single command."],
};

const PAGES: &[Page] = &[
    Page {
        name: "run",
        tagline: "Run a task from a Quefile",
        usage: &[
            "que run [options] <task> [-- args...]",
            "que run <task> --help            (argument help for that task)",
        ],
        sections: &[
            Section {
                title: "Options",
                rows: &[
                    ("-f <file>", "Use a specific Quefile (default: auto-detect)"),
                    ("-g, --global", "Use the global Quefile"),
                    (
                        "-B, --force",
                        "Run even if inputs and outputs say it is up to date",
                    ),
                    ("--dry-run", "Print effects instead of performing them"),
                    ("--allow <cap>[=<list>]", "Grant a capability (see Sandbox)"),
                    ("--deny <cap>", "Deny a capability (see Sandbox)"),
                    ("-h, --help", "Show argument help for <task>, or this message"),
                ],
                notes: &[],
            },
            Section {
                title: "Task arguments",
                rows: &[
                    ("-- value1 value2", "Positional arguments, in declared order"),
                    ("-- key=value", "Named arguments, in any order"),
                ],
                notes: &["The two forms may be mixed; named arguments win."],
            },
            Section {
                title: "Quefile discovery",
                rows: &[],
                notes: &[QUEFILE_DISCOVERY],
            },
            SANDBOX,
        ],
        footer: &[],
    },
    Page {
        name: "tasks",
        tagline: "List the tasks a Quefile defines",
        usage: &["que tasks [-f <file>] [-g]"],
        sections: &[
            Section {
                title: "Options",
                rows: &[
                    ("-f <file>", "Use a specific Quefile (default: auto-detect)"),
                    ("-g, --global", "List only the global Quefile's tasks"),
                    ("-h, --help", "Show this message"),
                ],
                notes: &[],
            },
            Section {
                title: "Quefile discovery",
                rows: &[],
                notes: &[
                    QUEFILE_DISCOVERY,
                    "Global tasks a project task hides are listed as (shadowed): reach them with `que run -g <task>`.",
                ],
            },
        ],
        footer: &[],
    },
    Page {
        name: "test",
        tagline: "Run tests",
        usage: &["que test [--filter <text>] [paths...]"],
        sections: &[
            Section {
                title: "Options",
                rows: &[
                    ("--filter <text>", "Only run tests whose name contains <text>"),
                    ("-h, --help", "Show this message"),
                ],
                notes: &[],
            },
            Section {
                title: "Arguments",
                rows: &[(
                    "[paths...]",
                    "Files or directories to search (default: current directory)",
                )],
                notes: &[],
            },
            Section {
                title: "What counts as a test",
                rows: &[],
                notes: &[
                    "A test file is named `*_test.que` or `test_*.que`, or lives under a `tests/` directory.",
                    "A test is a top-level function whose name starts with `test_`.",
                    "The command fails if nothing ran, so a stale filter cannot pass for success.",
                ],
            },
        ],
        footer: &[],
    },
    Page {
        name: "install",
        tagline: "Fetch dependencies declared in que.toml",
        usage: &["que install [--locked] [-g]"],
        sections: &[
            Section {
                title: "Options",
                rows: &[
                    (
                        "--locked",
                        "Fail instead of resolving anything not in que.lock",
                    ),
                    ("-g, --global", "Install into the global Quefile's directory"),
                    ("-h, --help", "Show this message"),
                ],
                notes: &[],
            },
            Section {
                title: "Notes",
                rows: &[],
                notes: &[
                    "Packages are resolved from the nearest ancestor directory holding a que.toml and unpacked into que_packages/ beside it.",
                    "Use --locked in CI: it verifies que.lock is current instead of updating it.",
                ],
            },
        ],
        footer: &[],
    },
    Page {
        name: "fmt",
        tagline: "Format Que source files",
        usage: &["que fmt [--check] [--diff] [files...]"],
        sections: &[
            Section {
                title: "Options",
                rows: &[
                    (
                        "--check",
                        "Report files that need formatting; exit 1 if any do",
                    ),
                    ("--diff", "Print a diff instead of writing the file"),
                    ("-h, --help", "Show this message"),
                ],
                notes: &[],
            },
            Section {
                title: "Arguments",
                rows: &[(
                    "[files...]",
                    "Files to format (default: every .que file below the current directory)",
                )],
                notes: &[],
            },
        ],
        footer: &[],
    },
    Page {
        name: "lint",
        tagline: "Check Que source files for common mistakes",
        usage: &["que lint [files...]"],
        sections: &[
            Section {
                title: "Options",
                rows: &[("-h, --help", "Show this message")],
                notes: &[],
            },
            Section {
                title: "Arguments",
                rows: &[(
                    "[files...]",
                    "Files to lint (default: every .que file below the current directory)",
                )],
                notes: &[],
            },
            Section {
                title: "Notes",
                rows: &[],
                notes: &["Findings are written to stderr and exit 1; a clean run prints nothing."],
            },
        ],
        footer: &[],
    },
];

/// Look a command page up, accepting the `--help` spelling too.
fn page(name: &str) -> Option<&'static Page> {
    let name = name.trim_start_matches('-');
    PAGES.iter().find(|p| p.name == name)
}

/// The top-level help screen.
pub fn print_usage() {
    render(&ROOT);
}

/// Help for one command, falling back to the top-level screen.
///
/// An unknown name is not an error here: the user asked for help, and the
/// full list of commands is the most useful answer to "help for what?".
pub fn print_command_help(name: &str) {
    match page(name) {
        Some(p) => render(p),
        None => render(&ROOT),
    }
}

/// A one-line reminder printed to stderr beside an error message.
///
/// Deliberately short: the error is the message, and the whole manual would
/// bury it.
pub fn print_usage_hint(command: &str) {
    let Some(p) = page(command) else { return };
    for (i, line) in p.usage.iter().enumerate() {
        let label = if i == 0 { "Usage:" } else { "      " };
        eprintln!("{} {}", label.bold(), line);
    }
    eprintln!("Run `{}` for more.", format!("que help {}", p.name).cyan());
}

/// Section heading style, used here and by the task listings in main.
pub fn heading(text: &str) -> String {
    text.bold().to_string()
}

/// Left-column style: flags, command names, task names.
pub fn term(text: &str) -> String {
    text.cyan().to_string()
}

fn render(p: &Page) {
    let width = wrap_width();

    if p.name.is_empty() {
        println!("{} {}", "Que".bold(), format!("v{}", VERSION).bold());
    } else {
        println!("{} {}", "que".bold(), p.name.bold().cyan());
    }
    for line in wrap(p.tagline, width) {
        println!("{}", line);
    }

    println!();
    println!("{}", heading("Usage"));
    for line in p.usage {
        println!("  {}", line);
    }

    for section in p.sections {
        println!();
        println!("{}", heading(section.title));
        print_rows(section.rows, width);
        if !section.rows.is_empty() && !section.notes.is_empty() {
            println!();
        }
        for note in section.notes {
            for line in wrap(note, width.saturating_sub(2)) {
                println!("  {}", line);
            }
        }
    }

    if !p.footer.is_empty() {
        println!();
        for note in p.footer {
            for line in wrap(note, width) {
                println!("{}", line);
            }
        }
    }
}

/// Two columns, aligned on the widest term, with the description wrapped and
/// hanging-indented under itself.
fn print_rows(rows: &[(&str, &str)], width: usize) {
    if rows.is_empty() {
        return;
    }
    // A single very long flag must not push every description off the screen,
    // so the column stops growing and long terms simply overflow their cell.
    const MAX_TERM: usize = 26;
    let column = rows
        .iter()
        .map(|(t, _)| t.chars().count())
        .filter(|w| *w <= MAX_TERM)
        .max()
        .unwrap_or(MAX_TERM);

    let indent = 2 + column + 2;
    let desc_width = width.saturating_sub(indent).max(20);

    for (t, desc) in rows {
        let len = t.chars().count();
        let pad = column.saturating_sub(len);
        if desc.is_empty() {
            println!("  {}", term(t));
            continue;
        }
        let lines = wrap(desc, desc_width);
        if len > column {
            // Overflowing term: give the description its own aligned lines.
            println!("  {}", term(t));
            for line in &lines {
                println!("{:indent$}{}", "", line, indent = indent);
            }
            continue;
        }
        println!("  {}{:pad$}  {}", term(t), "", lines[0], pad = pad);
        for line in &lines[1..] {
            println!("{:indent$}{}", "", line, indent = indent);
        }
    }
}

/// Greedy word wrap. Long words are left intact rather than split, because a
/// broken flag name or path is worse than a line that runs a little long.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let len = current.chars().count();
        if !current.is_empty() && len + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// How wide to wrap prose: the terminal, clamped so help stays readable on a
/// very narrow window and does not sprawl across a very wide one.
pub fn wrap_width() -> usize {
    let cols = console::Term::stdout()
        .size_checked()
        .map(|(_, cols)| cols as usize)
        .unwrap_or(80);
    cols.clamp(40, 100)
}
