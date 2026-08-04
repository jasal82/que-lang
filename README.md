# Que

A scripting language for DevOps and build automation.

```que
import std.config

fn changelog_entry(version, notes) {
    let lines = notes.map(|n| "  - " + n)
    "## ${version}\n" + lines.join("\n")
}

@description("Tag and publish a new release")
@deps([test])
@inputs(["Cargo.toml"])
@outputs(["./dist/app"])
task release {
    let version = config.read(path("Cargo.toml")).unwrap().get_path("package.version")
    let tag = `git tag --list v${version}`.stdout.trim()
    if tag != "" {
        println("Version ${version} already tagged, skipping")
        return
    }
    path("./dist").mkdir()
    `cargo build --release`
    path("./target/release/app").copyTo(path("./dist/app"))
    `git tag v${version}`
    println(changelog_entry(version, ["built release binary", "tagged ${version}"]))
}
```

## Getting Started

Build from source (requires Rust):

```sh
cargo build --release
```

Run a script:

```sh
que script.que
```

Start the interactive REPL:

```sh
que
```

Run tasks from a Quefile:

```sh
que run build          # run a task
que tasks              # list available tasks
que run -g backup      # run a task from the global Quefile (~/.que/Quefile)
```

The nearest Quefile at or above the current directory is used, so `que run`
works from anywhere inside a project. Tasks run in the directory you invoked
`que` from — use `quefile_dir()` for paths relative to the Quefile itself.
Tasks it does not define fall back to your global Quefile.

Run tests:

```sh
que test               # run every *_test.que / test_*.que / tests/*.que file
que test --filter add  # only tests whose name contains "add"
```

Manage dependencies:

```sh
que install            # fetch what que.toml declares, write que.lock
que install --locked   # CI: fail rather than resolve anything unpinned
```

## Features

### Language Fundamentals

- **Immutable by default** — `let` bindings are immutable; use `mut` to opt in to mutation
- **Type inference** with a rich type system: int, float, string, bool, list, set, map, tuple, function
- **Pattern matching** and destructuring across `match`, `let`, and function parameters
- **First-class functions**, closures, and lambdas
- **Pipe operator** — `data |> normalize |> render` for readable pipelines
- **String interpolation** — `"deploying ${app.name} v${version}"`
- **Optional semicolons** — newlines work as statement terminators

### DevOps-Native Types

- **Paths** — typed filesystem paths with built-in methods (`mkdir`, `copy`, `delete`, `size`, `exists`)
- **Globs** — `glob("src/**/*.rs")` with expansion and matching
- **Commands** — backtick literals `` `git status` `` with interpolation, stdout/stderr capture, and exit code checking
- **Durations** — `5s`, `30m`, `2h` as first-class values with arithmetic
- **Semver** — `semver("1.2.3")` with comparison and component access
- **Secrets** — values that redact themselves in output
- **Streams** — lazy, chainable pipelines for file and data processing

### Task System

- Declarative task definitions with dependency graphs
- Automatic dependency resolution and diamond deduplication
- Input/output freshness tracking — tasks skip when outputs are newer than inputs
- Parameterized tasks with default values
- Introspection via `tasks()` and `run_task()` builtins

### Batteries Included

- ~90 built-in functions covering strings, collections, filesystem, JSON/YAML/TOML, HTTP, and more
- Config file parsing with a universal dot-path query syntax (`config.get_path("database.host")`)
- Format conversion between JSON, YAML, and TOML
- HTTP client for API calls
- Regex support
- Module system with imports, selective imports, and re-exports
- `retry` and `timeout` for resilient operations

### Tooling

- **Language server** (LSP) with diagnostics, completions, hover, go-to-definition, and document symbols
- **VS Code extension** with syntax highlighting, snippets, and LSP integration

## Tutorial

See [tutorial.md](tutorial.md) for a comprehensive walkthrough of the language covering all features with runnable examples.

## License

MIT
