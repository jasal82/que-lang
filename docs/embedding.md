# Embedding que

The `que_lang` library runs que scripts inside a Rust program. It is what
jbuild uses for hooks, rules and tasks (jbuild spec 10 §3, work packages
WP-7.1 and WP-7.2), but nothing in it is jbuild-specific.

```toml
[dependencies]
# The package is called `que` (it also builds the `que` binary); the
# library is `que_lang`.
que_lang = { package = "que", version = "1.2" }
```

An `Engine` (`src/embed/mod.rs`) is one interpreter configured by its host:

* a capability **policy**, deny-by-default, enforced by the same code as the
  CLI's `--allow`/`--deny` flags;
* **native modules**: Rust closures that scripts import like packages;
* **module loading** from explicit search paths and in-memory sources only;
* a per-call **timeout** and a **cancellation token**;
* an **access tracer** that sees every filesystem access the script makes;
* a **serde bridge** between que values and JSON, TOML and `Serialize` types;
* no process-global state, so engines run in parallel threads.

## Worked example

A `configure` hook with a `jbuild` host module, a kit module given as source,
and the inputs it read collected by the tracer. This example is compiled and
run as a doctest (`cargo test --doc`).

```rust
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use que_lang::embed::{AccessEvent, AccessKind, Engine, NativeError, NativeModule};
use que_lang::permissions::Policy;
use que_lang::value::Value;

#[derive(serde::Deserialize, Debug, PartialEq)]
struct ConfigureResult {
    defines: BTreeMap<String, bool>,
}

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let pkg_root = Path::new("/work/pkg");
let out_dir = Path::new("/work/out/hooks/configure");

// 1. What the hook may do (spec 10 §3.4): read the package, write its own
//    output directory, run one declared tool, read one variable, no network.
let policy = Policy::deny_all()
    .allow_read(pkg_root)
    .allow_write(out_dir)
    .allow_env("CI")
    .allow_exec_program("/usr/bin/objcopy")?;

// 2. The host module. Natives check the policy themselves through their
//    context, with the same rules and messages as the built-ins.
let diagnostics = Arc::new(Mutex::new(Vec::new()));
let sink = Arc::clone(&diagnostics);
let out = out_dir.to_path_buf();
let jbuild = NativeModule::new("jbuild")
    .function("out_dir", move |_ctx, _args| {
        Ok(Value::Path(out.to_string_lossy().into_owned()))
    })
    .function("warn", move |ctx, args| {
        let message = args.first().map(Value::display_string).unwrap_or_default();
        let at = ctx.location().map(|l| l.to_string()).unwrap_or_default();
        sink.lock().unwrap().push(format!("{at}: warning: {message}"));
        Ok(Value::Null)
    })
    .function("run", |ctx, args| {
        let tool = match args.first() {
            Some(Value::String(s)) | Some(Value::Path(s)) => s.clone(),
            _ => return Err(NativeError::type_mismatch("run(tool, args) needs a tool path")),
        };
        ctx.check_exec_program(Path::new(&tool))?;
        // ... spawn the tool with std::process::Command (no shell), polling
        // ctx.check_cancelled() while it runs ...
        Ok(Value::Map(BTreeMap::new()))
    });

// 3. Record the hook's inputs (spec 10 §3.2).
let events: Arc<Mutex<Vec<AccessEvent>>> = Arc::default();
let recorder = Arc::clone(&events);

let mut engine = Engine::builder()
    .policy(policy)
    .native_module(jbuild)
    .embedded_module("demo_kit/mod.que", "pub fn device_define() { \"TARGET_DEVICE\" }\n")
    .tracer(move |e: &AccessEvent| recorder.lock().unwrap().push(e.clone()))
    .timeout(Duration::from_secs(60))
    .build()?;

// Hooks normally come from a file: `engine.run_file("hooks/build.que")`.
engine.run_source(
    "build.que",
    r#"
import jbuild
import demo_kit

fn configure(ctx) {
    mut defines = {}
    if ctx.config.distro == "sickdistro" || ctx.package.root.join("device.cfg").exists() {
        defines[demo_kit.device_define()] = true
    }
    jbuild.warn("configured " + ctx.package.name)
    { defines: defines }
}
"#,
)?;

// 4. The context. Paths must be `Value::Path` so scripts can call `.join()`,
//    `.glob()` and friends on them; everything else can come from serde.
let mut package = BTreeMap::new();
package.insert("name".to_string(), Value::String("app".into()));
package.insert("root".to_string(), Value::Path(pkg_root.to_string_lossy().into()));
let config = que_lang::embed::to_value(&serde_json::json!({ "distro": "sickdistro" }))?;
let mut ctx = BTreeMap::new();
ctx.insert("package".to_string(), Value::Map(package));
ctx.insert("config".to_string(), config);

let result = engine.call("configure", vec![Value::Map(ctx)])?;
let result: ConfigureResult = que_lang::embed::from_value(&result)?;
assert_eq!(result.defines.get("TARGET_DEVICE"), Some(&true));

assert_eq!(diagnostics.lock().unwrap()[0], "build.que:10:5: warning: configured app");
// `||` short-circuited, so the hook checked nothing on disk.
assert!(events.lock().unwrap().iter().all(|e| e.kind != AccessKind::Exists));
# Ok(())
# }
```

## Public API

Everything lives in `que_lang::embed` (`src/embed/`) and
`que_lang::permissions` (`src/permissions.rs`).

| Item | File | Purpose |
|---|---|---|
| `Engine`, `EngineBuilder` | `src/embed/mod.rs` | Configure and run an interpreter: `run_file`, `run_source`, `import`, `call`, `call_in`, `call_value`, `global`, `take_output`, `cancellation_token`, `set_timeout`, `policy` |
| `EmbedError`, `ScriptError`, `StackFrame` | `src/embed/mod.rs` | Errors. `ScriptError` has the message, the location (`file:line:col`) and the stack, outermost first; its `Display` prints all of them |
| `ModuleHandle` | `src/embed/mod.rs` | A module's exports, from `Engine::import("kit.license")` |
| `LANGUAGE_VERSION`, `language_version()` | `src/embed/mod.rs`, re-exported at the crate root | Language version for cache keys |
| `NativeModule`, `NativeContext`, `NativeError`, `ScriptLocation` | `src/embed/native.rs` | Native modules, and what a native function can do: `check`, `check_read/write/env/net`, `check_exec_program`, `record_access`, `check_cancelled`, `remaining_time`, `call` (call back into a que closure), `location`, `policy` |
| `CancellationToken`, `CancelReason` | `src/embed/limits.rs` | Cooperative cancellation and time limits |
| `AccessTracer`, `AccessEvent`, `AccessKind`, `Observation`, `EntryType`, `observe` | `src/embed/trace.rs` | Access tracing and re-validation |
| `to_json`, `from_json`, `to_toml`, `from_toml`, `to_value`, `from_value`, `ConvertError` | `src/embed/convert.rs` | Serde bridge |
| `Policy::deny_all`, `allow_read`, `allow_write`, `allow_env`, `allow_net_host`, `allow_exec_program`, `allow_all`, `check_exec_program`, `Denied` | `src/permissions.rs` | Programmatic policies |

## Policies

`Policy::deny_all()` grants nothing; each `allow_*` call adds a grant. It's
the same `Policy` type the CLI builds from `--allow`, checked by the same code
at the same dispatch points, so a script behaves the same under
`que --allow read=/x` and under an embedder's `allow_read("/x")`. Only the
wording of denials differs: an embedder's user can't pass `--allow`, so the
message explains the grant instead.

| Capability | Granted by | Matches |
|---|---|---|
| read | `allow_read(prefix)` | the path is below `prefix` after making it absolute and resolving `.`/`..` lexically |
| write | `allow_write(prefix)` | the same, for writes, creation, removal, and the destination of copies and moves |
| exec | `allow_exec_program(abs_path)` | a command whose first word is exactly that path and whose text has no shell operators (`;`, `&`, `\|`, `<`, `>`, `$`, backtick, parentheses, newline); also `NativeContext::check_exec_program` |
| env | `allow_env(name)` | reading that variable (`env.get`, `env.has`, …) |
| net | `allow_net_host(host)` | that host and its subdomains; nothing by default |

Typical denials:

```text
permission denied: read '/etc/passwd': the path is outside the granted read prefixes (/work/pkg)
permission denied: exec '/usr/bin/curl x': program '/usr/bin/curl' is not in the exec allow-list (/usr/bin/objcopy)
permission denied: exec '/usr/bin/objcopy a; rm -rf b': the command uses shell syntax (';'), so it can't be matched against the exec allow-list; run the program without shell operators
permission denied: env 'HOME': the variable is not in the env allow-list (CI)
permission denied: net 'https://example.com/x': network access is disabled
```

What counts as a read: file reads, existence and type checks, metadata,
directory listings and walks, glob expansion (`Glob.expand/first/count/any`,
`Path.glob`, `for f in g"..."`), iterating a directory (`for f in dir`), and
the source of a copy or move. Globs are checked by their pattern, which shares
its prefix with every path it can match.

Native functions are the embedder's own code and aren't checked
automatically; they call `ctx.check_*` for the effects they perform.

## Limits and cancellation

`EngineBuilder::timeout(d)` limits every call into the engine (`run_*`,
`import`, `call*`) to `d` of wall-clock time, measured from the start of that
call. `Engine::cancellation_token()` returns a token that another thread can
cancel. Both are checked at every statement and loop iteration, while
sleeping (`sleep`, `retry` delays), and while waiting for a child process
(which is then killed). `parallel` branches and imported modules share the
limits. A cancelled script unwinds like Ctrl-C in the CLI: `defer` blocks run
and `try` can't catch it. The call returns `EmbedError::Cancelled(reason)`.

Long-running natives should poll `ctx.check_cancelled()`. Builtins that block
inside one library call (an HTTP request, a large `sort`) finish that call
before the limit is noticed.

## Module loading

`import <pkg>` is resolved against, in order: native modules, embedded sources
(`EngineBuilder::embedded_module("kit/mod.que", src)`), then each
`search_path(dir)` (a directory holding `<pkg>/mod.que`, like
`que_packages/`). Nothing else is consulted: not `que_packages/` next to the
script, not the global packages directory, not `que.toml`, not the
environment. `que install` is a CLI command and not reachable from scripts.
Local imports (`import .helper`) resolve next to the importing file, on disk
or among the embedded sources.

Module files aren't policy-checked when imported (the embedder chose the
search paths), but their top level runs under the engine's policy, and each
file loaded from disk is reported to the tracer as `AccessKind::ModuleLoad`.

## Access tracing

The tracer receives an `AccessEvent` for every access that goes through the
dispatch layer, synchronously and in order:

| Kind | Reported by |
|---|---|
| `Read` | `fs.read*`, `Path.read/size/modified`, `json/yaml/toml/csv/config.read`, `template.render_file`, `hash.*` of a `Path`, `stream.file`, `open(p)`, copy sources |
| `Exists` | `fs.exists`, `Path.exists/is_file/is_dir/is_link` |
| `ListDir` | `Path.ls`, `for f in dir` |
| `Walk` | `Path.walk/files/dirs`, `fs.find`, directory copy sources |
| `Glob` | `Path.glob`, `Glob.expand/first/count/any/copy_to/move_to`, `for f in g"..."` |
| `Write`, `CreateDir`, `Remove` | writes, `mkdir`, `delete`, copy and move destinations |
| `Exec` | every command when it starts (`path` is the displayed command line) |
| `ModuleLoad` | each module file read from disk |

Each event has the path as requested and in absolute form (`path`, lexically
normalised, and `canonical` if it exists), whether the operation succeeded
(failed reads of missing files are inputs too), and an `Observation`: the
entry type for reads and existence checks, the sorted entry names of a
listing, the sorted absolute paths of a walk or glob. Observations are made
with the interpreter's own glob and walk functions, so
`AccessEvent::is_unchanged()` (or comparing `observe(kind, &path)`) tells
reliably whether a listing, walk, glob or existence check would now give a
different answer. Reads aren't hashed: compare file contents yourself.
`AccessEvent` implements `Serialize` and `Deserialize` for storing.

## Serde bridge

`from_json` and `from_toml` never fail; `to_json` and `to_toml` fail with a
`ConvertError` that names the position (`targets.app.defines[2]`).
`to_value` and `from_value` convert any `Serialize`/`Deserialize` type via
JSON. Lossy and refused cases (also listed in `src/embed/convert.rs`):

* JSON integers above `i64::MAX` become `Float`; TOML datetimes become strings.
* `Path`, `Glob`, `Regex`, `Semver` become strings; `Set` and `Tuple` become
  arrays; struct instances become maps of their fields. Map key order isn't
  preserved (que maps are sorted).
* `NaN`/`±inf` can't go to JSON; `Null` can't go to TOML; `Secret`, functions,
  commands, streams, handles, `Ok`/`Err`, enums and durations can't be
  converted at all.
* `to_value` turns integer and bool map keys into strings and refuses other
  non-string keys.

## Language version

`que_lang::LANGUAGE_VERSION` (currently `1.2.1`) identifies the semantics of
evaluation and of the standard library, independent of the crate version.
**Compatibility rule:** it changes whenever a script could observe a
difference: any change to a stdlib function's behaviour or result, a changed
evaluation rule, a changed error. Bug fixes bump the patch level, compatible
additions the minor version, changes that can make existing scripts behave
differently the major version. Put the whole string into cache keys.

History: `1.2.1` made string lengths and indices count characters (see
`CHANGELOG.md`); embedders that cache script results must not reuse `1.2.0`
entries.

## Isolation and concurrency

Engines share no mutable state. An embedded interpreter:

* ignores the CLI's process-wide SIGINT flag (`que_lang::interrupt`); its host
  cancels it through its token;
* refuses `cd`, `with dir(...)`, `env.set/unset/load_file`, `env.scope`,
  `dotenv.load*` (they change the process's working directory or
  environment) and `input`, `confirm`, `prompt.*` (they read the terminal),
  with the error "… is not available in embedded mode";
* buffers `print`/`println` output (`Engine::take_output`) instead of writing
  to stdout, and turns `exit(n)` into `EmbedError::Exit(n)`.

Audit of the remaining `static`s: an atomic counter for unique temp-file names
(`std_modules/fs.rs`) and a cached "running as root" flag (`methods.rs`) are
safe to share. Reading environment variables (`TZ`, `TERM`, `QUE_SHELL`,
`HOME`) and the working directory is read-only and doesn't interfere. `log`
console sinks write to the process's stderr. Relative paths in scripts resolve
against the process's working directory, so pass absolute paths in contexts
and policies.

`tests/embedding.rs` runs four engines concurrently with different policies
and native modules, and cancels one engine while another keeps running.

## Notes for jbuild WP-7.2

* One `Engine` per hook invocation. Build the policy from the manifest
  (spec 10 §3.4): `allow_read` for the package root, dependency stages,
  declared inputs and kit directories; `allow_write` for the hook's output
  directory; `allow_exec_program` for each `permissions.exec` tool resolved to
  an absolute path; `allow_env` for each `permissions.env` name;
  `allow_net_host` only if `net = true` is approved.
* Register `jbuild` as a native module. `jbuild.run` should call
  `ctx.check_exec_program(tool)` and spawn without a shell;
  `jbuild.warn/error` can use `ctx.location()` as the default location.
* Kits: `search_path(<kits dir>)`, or `embedded_module` for built-in kits.
  `hooks/build.que#configure` is `run_file(...)` then `call("configure", ...)`;
  `demo_kit::license#stage` is `import("demo_kit.license")` then
  `call_in(&handle, "stage", ...)`.
* Cache key: the script and imported module contents (the tracer's
  `ModuleLoad` events list the files), `LANGUAGE_VERSION`, the context, the
  values of allow-listed env vars. Store the access events with the result; a
  cached result is valid if every `Read`/`ModuleLoad` file has the same content
  hash and every other event `is_unchanged()`.
* Errors: `EmbedError::Script(e)` prints `file:line:col: message` and the que
  stack; `Cancelled`, `Exit`, `FunctionNotFound` and `NotCallable` are the
  other cases to map.
* Build `ctx` with `Value::Path` for paths; convert returned data with
  `from_value::<T>()` and report `ConvertError` positions in schema errors.

## Limitations

* The capability model doesn't confine spawned programs (jbuild D21), and path
  grants are lexical: a symlink inside a granted prefix that points outside it
  is followed.
* `ScriptLocation::file` is the script's file name without directory, as in
  CLI error messages.
* `Exec` events are reported when a command starts; the exit status isn't part
  of the event.
* Tasks (`task` declarations) work in embedded mode, but their on-disk
  freshness cache (`.que/task-cache.json`) isn't traced.
* Some existing `git_*` integration tests are flaky when run in parallel (they
  also fail intermittently on `main`); unrelated to this work.

## Releasing

The crate version is 1.2.1. Publishing is a manual step for a maintainer with
a registry token: `cargo publish` from a clean checkout of the release commit.
