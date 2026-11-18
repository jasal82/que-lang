//! Tests for the embedding API (`que_lang::embed`): native modules, policies,
//! the serde bridge, limits, concurrency, module loading and access tracing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use que_lang::embed::{
    AccessEvent, AccessKind, CancelReason, EmbedError, Engine, EntryType, NativeError,
    NativeModule, Observation,
};
use que_lang::permissions::Policy;
use que_lang::value::Value;

// ── helpers ──────────────────────────────────────────────────────────

/// A fresh, empty directory below cargo's scratch space for this test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("embedding")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir.canonicalize().expect("canonicalize scratch dir")
}

fn s(text: &str) -> Value {
    Value::String(text.to_string())
}

fn path(p: &Path) -> Value {
    Value::Path(p.to_string_lossy().into_owned())
}

fn script_error(result: Result<Value, EmbedError>) -> que_lang::embed::ScriptError {
    match result {
        Err(EmbedError::Script(e)) => e,
        other => panic!("expected a script error, got {:?}", other),
    }
}

fn engine_with(policy: Policy) -> Engine {
    Engine::builder().policy(policy).build().expect("engine")
}

/// The absolute path of a program, if this machine has it.
fn program(name: &str) -> Option<PathBuf> {
    ["/bin", "/usr/bin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

// ── native modules and calls ─────────────────────────────────────────

fn host_module() -> NativeModule {
    NativeModule::new("host")
        .function("add", |_ctx, args| match (args.first(), args.get(1)) {
            (Some(Value::Int(a)), Some(Value::Int(b))) => Ok(Value::Int(a + b)),
            _ => Err(NativeError::type_mismatch("add(a, b) expects two Ints")),
        })
        .function("apply", |ctx, args| {
            let f = args.first().cloned().unwrap_or(Value::Null);
            let arg = args.get(1).cloned().unwrap_or(Value::Null);
            ctx.call(&f, vec![arg])
        })
        .function("fail_now", |_ctx, args| {
            Err(NativeError::new(
                args.first().map(Value::display_string).unwrap_or_default(),
            ))
        })
        .function("line", |ctx, _args| {
            let loc = ctx.location().expect("a call has a location");
            Ok(Value::Int(loc.line as i64))
        })
}

#[test]
fn a_native_function_is_called_with_values_and_returns_a_value() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    engine
        .run_source(
            "main.que",
            "import host\n\
             fn configure(ctx) {\n\
                 { sum: host.add(ctx.a, ctx.b), name: ctx.package.name }\n\
             }\n",
        )
        .unwrap();
    let mut package = BTreeMap::new();
    package.insert("name".to_string(), s("app"));
    let mut ctx = BTreeMap::new();
    ctx.insert("a".to_string(), Value::Int(2));
    ctx.insert("b".to_string(), Value::Int(40));
    ctx.insert("package".to_string(), Value::Map(package));

    let result = engine.call("configure", vec![Value::Map(ctx)]).unwrap();

    let mut expected = BTreeMap::new();
    expected.insert("sum".to_string(), Value::Int(42));
    expected.insert("name".to_string(), s("app"));
    assert_eq!(result, Value::Map(expected));
}

#[test]
fn native_modules_support_aliases_and_selective_imports() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    let v = engine
        .run_source(
            "main.que",
            "import host as h\nimport host { add }\nh.add(1, 2) + add(3, 4)",
        )
        .unwrap();
    assert_eq!(v, Value::Int(10));
}

#[test]
fn a_native_function_can_call_back_into_a_que_closure() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    let v = engine
        .run_source("main.que", "import host\nhost.apply(|x| x * 3, 14)")
        .unwrap();
    assert_eq!(v, Value::Int(42));
}

#[test]
fn a_native_error_surfaces_as_a_que_error_with_a_stack_trace() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    engine
        .run_source(
            "hooks.que",
            "import host\n\
             fn outer() {\n\
                 inner()\n\
             }\n\
             fn inner() {\n\
                 host.fail_now(\"tool missing\")\n\
             }\n",
        )
        .unwrap();

    let err = script_error(engine.call("outer", vec![]));

    assert_eq!(err.message, "tool missing");
    let loc = err.location.clone().expect("location");
    assert_eq!(loc.file.as_deref(), Some("hooks.que"));
    assert_eq!(loc.line, 6);
    let frames: Vec<&str> = err.stack.iter().map(|f| f.function.as_str()).collect();
    assert_eq!(frames, ["outer", "inner"]);
    let rendered = err.to_string();
    assert!(rendered.starts_with("hooks.que:6:"), "{}", rendered);
    assert!(
        rendered.contains("in inner (called from hooks.que:3:"),
        "{}",
        rendered
    );
}

#[test]
fn a_native_error_can_be_caught_by_the_script() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    let v = engine
        .run_source(
            "main.que",
            "import host\nmut r = \"\"\ntry { host.fail_now(\"x\") } catch e { r = \"caught: \" + e }\nr",
        )
        .unwrap();
    assert_eq!(v, s("caught: x"));
}

#[test]
fn a_native_function_sees_the_script_location_of_its_call() {
    let mut engine = Engine::builder()
        .native_module(host_module())
        .build()
        .unwrap();
    let v = engine
        .run_source("main.que", "import host\n\nlet l = host.line()\nl")
        .unwrap();
    assert_eq!(v, Value::Int(3));
}

#[test]
fn calling_an_unknown_or_non_function_name_is_a_clear_error() {
    let mut engine = engine_with(Policy::deny_all());
    engine.run_source("main.que", "let answer = 42").unwrap();
    assert!(matches!(
        engine.call("missing", vec![]),
        Err(EmbedError::FunctionNotFound(name)) if name == "missing"
    ));
    assert!(matches!(
        engine.call("answer", vec![]),
        Err(EmbedError::NotCallable { name, .. }) if name == "answer"
    ));
}

#[test]
fn native_module_names_must_not_shadow_the_standard_library() {
    for name in ["fs", "std", "not an ident"] {
        let result = Engine::builder()
            .native_module(NativeModule::new(name))
            .build();
        assert!(matches!(result, Err(EmbedError::Config(_))), "{}", name);
    }
}

#[test]
fn script_output_is_captured_not_printed() {
    let mut engine = engine_with(Policy::deny_all());
    engine
        .run_source(
            "main.que",
            "println(\"one\")\nprint(\"two\")\nprintln(\"!\")",
        )
        .unwrap();
    assert_eq!(engine.take_output(), ["one", "two!"]);
}

#[test]
fn exit_is_reported_instead_of_ending_the_process() {
    let mut engine =
        engine_with(Policy::deny_all().allow_all(que_lang::permissions::Capability::Exec));
    let result = engine.run_source("main.que", "os.exit(3)");
    assert!(matches!(result, Err(EmbedError::Exit(3))), "{:?}", result);
}

// ── policies ─────────────────────────────────────────────────────────

#[test]
fn a_read_outside_the_granted_prefixes_is_denied_with_an_explanation() {
    let allowed = scratch("policy_read_allowed");
    let other = scratch("policy_read_other");
    std::fs::write(allowed.join("in.txt"), "inside").unwrap();
    std::fs::write(other.join("out.txt"), "outside").unwrap();
    let mut engine = engine_with(Policy::deny_all().allow_read(&allowed));
    engine
        .run_source("main.que", "import std.fs\nfn read(p) { fs.read(p)? }")
        .unwrap();

    assert_eq!(
        engine
            .call("read", vec![path(&allowed.join("in.txt"))])
            .unwrap(),
        s("inside")
    );
    let err = script_error(engine.call("read", vec![path(&other.join("out.txt"))]));
    let expected = format!(
        "permission denied: read '{}': the path is outside the granted read prefixes ({})",
        other.join("out.txt").display(),
        allowed.display()
    );
    assert_eq!(err.message, expected);
    assert!(
        !err.message.contains("--allow"),
        "no CLI hints for embedders"
    );
}

#[test]
fn a_dot_dot_path_cannot_escape_a_read_prefix() {
    let allowed = scratch("policy_dotdot");
    let mut engine = engine_with(Policy::deny_all().allow_read(&allowed));
    let escape = format!("{}/../../etc/hostname", allowed.display());
    let err = script_error(engine.run_source(
        "main.que",
        &format!("import std.fs\nfs.read(\"{}\")", escape),
    ));
    assert!(
        err.message.starts_with("permission denied: read"),
        "{}",
        err.message
    );
}

#[test]
fn listing_globbing_and_iterating_are_reads() {
    let allowed = scratch("policy_listing_allowed");
    let other = scratch("policy_listing_other");
    std::fs::write(other.join("a.txt"), "").unwrap();
    let mut engine = engine_with(Policy::deny_all().allow_read(&allowed));
    engine
        .run_source(
            "main.que",
            "fn ls(d) { d.ls() }\n\
             fn walk(d) { d.walk() }\n\
             fn expand(pattern) { glob(pattern).expand() }\n\
             fn iterate(d) { mut n = 0\nfor f in d { n += 1 }\nn }\n",
        )
        .unwrap();
    for f in ["ls", "walk", "iterate"] {
        let err = script_error(engine.call(f, vec![path(&other)]));
        assert!(
            err.message.starts_with("permission denied: read"),
            "{}: {}",
            f,
            err.message
        );
    }
    let pattern = s(&format!("{}/*.txt", other.display()));
    let err = script_error(engine.call("expand", vec![pattern]));
    assert!(
        err.message.starts_with("permission denied: read"),
        "{}",
        err.message
    );
    // The same operations inside the prefix work.
    assert_eq!(
        engine.call("iterate", vec![path(&allowed)]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn copying_a_file_needs_read_access_to_its_source() {
    let readable = scratch("policy_copy_out");
    let secret = scratch("policy_copy_secret");
    std::fs::write(secret.join("key"), "k").unwrap();
    let mut engine = engine_with(
        Policy::deny_all()
            .allow_read(&readable)
            .allow_write(&readable),
    );
    engine
        .run_source("main.que", "fn copy(a, b) { a.copy_to(b) }")
        .unwrap();
    let err = script_error(engine.call(
        "copy",
        vec![path(&secret.join("key")), path(&readable.join("key"))],
    ));
    assert!(
        err.message.starts_with("permission denied: read"),
        "{}",
        err.message
    );
    assert!(!readable.join("key").exists());
}

#[test]
fn a_write_outside_the_granted_prefix_is_denied() {
    let out = scratch("policy_write_out");
    let elsewhere = scratch("policy_write_elsewhere");
    let mut engine = engine_with(Policy::deny_all().allow_read(&out).allow_write(&out));
    engine
        .run_source(
            "main.que",
            "import std.fs\nfn write(p) { fs.write(p, \"x\")? }",
        )
        .unwrap();
    engine
        .call("write", vec![path(&out.join("ok.txt"))])
        .unwrap();
    let err = script_error(engine.call("write", vec![path(&elsewhere.join("no.txt"))]));
    assert!(
        err.message.contains("outside the granted write prefixes"),
        "{}",
        err.message
    );
    assert!(!elsewhere.join("no.txt").exists());
}

#[cfg(unix)]
#[test]
fn only_allow_listed_programs_may_run() {
    let (Some(echo), Some(truth)) = (program("echo"), program("true")) else {
        return;
    };
    let policy = Policy::deny_all().allow_exec_program(&echo).unwrap();
    let mut engine = engine_with(policy);
    engine
        .run_source(
            "main.que",
            "fn run_echo(prog) { `${prog} hello`.out() }\n\
             fn run_other(prog) { `${prog}` }\n\
             fn run_chained(prog) { `${prog} hi; true` }\n",
        )
        .unwrap();

    assert_eq!(
        engine.call("run_echo", vec![path(&echo)]).unwrap(),
        s("hello")
    );

    let err = script_error(engine.call("run_other", vec![path(&truth)]));
    assert_eq!(
        err.message,
        format!(
            "permission denied: exec '{}': program '{}' is not in the exec allow-list ({})",
            truth.display(),
            truth.display(),
            echo.display()
        )
    );

    let err = script_error(engine.call("run_chained", vec![path(&echo)]));
    assert!(
        err.message.contains("uses shell syntax (';')"),
        "{}",
        err.message
    );
}

#[test]
fn exec_allow_list_entries_must_be_absolute() {
    assert!(Policy::deny_all().allow_exec_program("objcopy").is_err());
}

#[test]
fn a_native_function_checks_exec_with_the_same_policy() {
    let tool = Path::new("/opt/toolchain/bin/objcopy");
    let runner = NativeModule::new("tools").function("check", |ctx, args| {
        let program = args.first().map(Value::display_string).unwrap_or_default();
        ctx.check_exec_program(Path::new(&program))?;
        Ok(Value::Bool(true))
    });
    let mut engine = Engine::builder()
        .policy(Policy::deny_all().allow_exec_program(tool).unwrap())
        .native_module(runner)
        .build()
        .unwrap();
    engine
        .run_source("main.que", "import tools\nfn check(p) { tools.check(p) }")
        .unwrap();
    assert_eq!(
        engine.call("check", vec![path(tool)]).unwrap(),
        Value::Bool(true)
    );
    let err = script_error(engine.call("check", vec![s("/usr/bin/curl")]));
    assert_eq!(
        err.message,
        "permission denied: exec '/usr/bin/curl': program is not in the exec allow-list \
         (/opt/toolchain/bin/objcopy)"
    );
}

#[test]
fn only_allow_listed_environment_variables_can_be_read() {
    // PATH is set in every test environment; HOME may not be.
    let mut engine = engine_with(Policy::deny_all().allow_env("PATH"));
    let v = engine
        .run_source("main.que", "env.get(\"PATH\") != null")
        .unwrap();
    assert_eq!(v, Value::Bool(true));
    let err = script_error(engine.run_source("main2.que", "env.get(\"HOME\")"));
    assert_eq!(
        err.message,
        "permission denied: env 'HOME': the variable is not in the env allow-list (PATH)"
    );
}

#[test]
fn network_access_is_denied_by_default() {
    let mut engine = engine_with(Policy::deny_all());
    let err = script_error(engine.run_source(
        "main.que",
        "import std.http\nhttp.get(\"https://example.com/x\")",
    ));
    assert_eq!(
        err.message,
        "permission denied: net 'https://example.com/x': network access is disabled"
    );
}

#[test]
fn the_top_level_of_an_imported_module_runs_under_the_policy() {
    let kits = scratch("policy_module_top_level");
    std::fs::create_dir_all(kits.join("sneaky")).unwrap();
    std::fs::write(
        kits.join("sneaky/mod.que"),
        "import std.fs\nlet x = fs.read(\"/etc/hostname\")\npub fn f() { 1 }\n",
    )
    .unwrap();
    let mut engine = Engine::builder()
        .policy(Policy::deny_all())
        .search_path(&kits)
        .build()
        .unwrap();
    let err = script_error(engine.run_source("main.que", "import sneaky\nsneaky.f()"));
    assert!(
        err.message.starts_with("permission denied: read"),
        "{}",
        err.message
    );
}

#[test]
fn process_global_state_cannot_be_changed_in_embedded_mode() {
    let dir = scratch("isolation");
    let mut engine = engine_with(
        Policy::deny_all()
            .allow_read(&dir)
            .allow_env("QUE_EMBED_TEST_VAR"),
    );
    engine
        .run_source(
            "main.que",
            "fn set_env() { env.set(\"QUE_EMBED_TEST_VAR\", \"1\") }\n\
             fn change_dir(d) { cd(d) }\n\
             fn scoped_dir(d) { with dir(d) { 1 } }\n",
        )
        .unwrap();
    let cwd = std::env::current_dir().unwrap();
    for (f, args) in [
        ("set_env", vec![]),
        ("change_dir", vec![path(&dir)]),
        ("scoped_dir", vec![path(&dir)]),
    ] {
        let err = script_error(engine.call(f, args));
        assert!(
            err.message.contains("not available in embedded mode"),
            "{}: {}",
            f,
            err.message
        );
    }
    assert_eq!(std::env::current_dir().unwrap(), cwd);
    assert!(std::env::var("QUE_EMBED_TEST_VAR").is_err());
}

// ── serde bridge ─────────────────────────────────────────────────────

/// Data values that every format represents exactly.
fn data_samples() -> Vec<Value> {
    let mut nested = BTreeMap::new();
    nested.insert(
        "list".to_string(),
        Value::List(vec![Value::Int(1), s("two"), Value::Bool(false)]),
    );
    nested.insert("empty_map".to_string(), Value::Map(BTreeMap::new()));
    nested.insert("float".to_string(), Value::Float(-0.5));
    nested.insert("unicode".to_string(), s("grüß \u{1F600} \"quoted\""));
    vec![
        Value::Int(0),
        Value::Int(i64::MIN),
        Value::Int(i64::MAX),
        Value::Float(1.5e300),
        Value::Bool(true),
        s(""),
        Value::List(vec![]),
        Value::Map(nested),
    ]
}

/// A deterministic pseudo-random data value (no external property-testing
/// crate needed): a small LCG drives the shape.
fn random_value(seed: &mut u64, depth: u32) -> Value {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let r = (*seed >> 33) as i64;
    match r % if depth == 0 { 4 } else { 6 } {
        0 => Value::Int(r - (1 << 30)),
        1 => Value::Float((r as f64) / 7.0),
        2 => Value::Bool(r % 2 == 0),
        3 => Value::String(format!("s{}-{}", r, "é".repeat((r % 3) as usize))),
        4 => Value::List((0..r % 4).map(|_| random_value(seed, depth - 1)).collect()),
        _ => Value::Map(
            (0..r % 4)
                .map(|i| (format!("k{}", i), random_value(seed, depth - 1)))
                .collect(),
        ),
    }
}

#[test]
fn data_values_round_trip_through_json_and_toml() {
    let mut seed = 7;
    let randoms = (0..200).map(|_| random_value(&mut seed, 3));
    for value in data_samples().into_iter().chain(randoms) {
        let json = que_lang::embed::to_json(&value).unwrap();
        assert_eq!(
            que_lang::embed::from_json(&json),
            value,
            "json: {:?}",
            value
        );
        let toml = que_lang::embed::to_toml(&value).unwrap();
        assert_eq!(
            que_lang::embed::from_toml(&toml),
            value,
            "toml: {:?}",
            value
        );
    }
}

#[test]
fn json_and_toml_documents_round_trip_through_que_values() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{"a": [1, 2.5, null, true, "x"], "b": {"c": {}}, "n": -9007199254740993}"#,
    )
    .unwrap();
    assert_eq!(
        que_lang::embed::to_json(&que_lang::embed::from_json(&json)).unwrap(),
        json
    );

    let toml: toml::Value = toml::from_str("a = [1, 2]\n[t]\nx = \"y\"\nf = 0.25\n").unwrap();
    assert_eq!(
        que_lang::embed::to_toml(&que_lang::embed::from_toml(&toml)).unwrap(),
        toml
    );
}

#[test]
fn lossy_and_unrepresentable_conversions_are_documented_cases() {
    use que_lang::embed::{from_json, from_toml, to_json, to_toml};
    // Integers beyond i64 become floats.
    let big: serde_json::Value = serde_json::from_str("18446744073709551615").unwrap();
    assert_eq!(from_json(&big), Value::Float(18446744073709551615.0));
    // Typed strings lose their type.
    assert_eq!(
        to_json(&Value::Path("/a".into())).unwrap(),
        serde_json::json!("/a")
    );
    assert_eq!(
        to_json(&Value::Set(vec![Value::Int(1)])).unwrap(),
        serde_json::json!([1])
    );
    // TOML datetimes become strings.
    let doc: toml::Value = toml::from_str("d = 1979-05-27T07:32:00Z").unwrap();
    assert_eq!(
        from_toml(&doc),
        Value::Map(BTreeMap::from([(
            "d".to_string(),
            s("1979-05-27T07:32:00Z")
        )]))
    );
    // Errors name the place.
    let bad = Value::Map(BTreeMap::from([(
        "xs".to_string(),
        Value::List(vec![Value::Int(1), Value::Float(f64::NAN)]),
    )]));
    let err = to_json(&bad).unwrap_err();
    assert_eq!(err.path, "xs[1]");
    let err = to_toml(&Value::List(vec![Value::Null])).unwrap_err();
    assert_eq!(
        (err.path.as_str(), err.message.as_str()),
        ("[0]", "TOML has no null")
    );
    assert!(to_json(&Value::Secret("hunter2".into())).is_err());
}

#[test]
fn serde_types_convert_to_and_from_que_values() {
    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct Target {
        name: String,
        defines: BTreeMap<String, bool>,
        flags: Vec<String>,
        opt_level: Option<u8>,
    }
    let t = Target {
        name: "app".into(),
        defines: BTreeMap::from([("X".to_string(), true)]),
        flags: vec!["-O2".into()],
        opt_level: None,
    };
    let v = que_lang::embed::to_value(&t).unwrap();
    assert_eq!(que_lang::embed::from_value::<Target>(&v).unwrap(), t);

    // Integer map keys become strings.
    let v = que_lang::embed::to_value(&BTreeMap::from([(1, "a")])).unwrap();
    assert_eq!(v, Value::Map(BTreeMap::from([("1".to_string(), s("a"))])));

    // A script's struct instance deserialises like a map.
    let mut engine = engine_with(Policy::deny_all());
    let v = engine
        .run_source(
            "main.que",
            "struct T { name, defines, flags, opt_level }\n\
             T { name: \"lib\", defines: {}, flags: [], opt_level: 2 }",
        )
        .unwrap();
    let t: Target = que_lang::embed::from_value(&v).unwrap();
    assert_eq!((t.name.as_str(), t.opt_level), ("lib", Some(2)));
}

// ── limits ───────────────────────────────────────────────────────────

/// Generous: CI machines can be slow, and the point is "stops", not "fast".
const STOP_BOUND: Duration = Duration::from_secs(20);

#[test]
fn an_endless_loop_stops_at_the_timeout() {
    let mut engine = Engine::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let start = Instant::now();
    let result = engine.run_source("main.que", "mut n = 0\nwhile true { n += 1 }");
    assert!(
        matches!(result, Err(EmbedError::Cancelled(CancelReason::TimedOut(d))) if d == Duration::from_millis(200)),
        "{:?}",
        result
    );
    assert!(start.elapsed() < STOP_BOUND);
}

#[test]
fn a_timeout_stops_every_parallel_branch() {
    let mut engine = Engine::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let start = Instant::now();
    let result = engine.run_source(
        "main.que",
        "fn spin() { while true { } }\nparallel { spin(), spin() }",
    );
    assert!(
        matches!(
            result,
            Err(EmbedError::Cancelled(CancelReason::TimedOut(_)))
        ),
        "{:?}",
        result
    );
    assert!(start.elapsed() < STOP_BOUND);
}

#[test]
fn a_long_sleep_stops_at_the_timeout() {
    let mut engine = Engine::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    let start = Instant::now();
    let result = engine.run_source("main.que", "sleep(60s)");
    assert!(
        matches!(
            result,
            Err(EmbedError::Cancelled(CancelReason::TimedOut(_)))
        ),
        "{:?}",
        result
    );
    assert!(start.elapsed() < STOP_BOUND);
}

#[test]
fn the_timeout_applies_per_call() {
    let mut engine = Engine::builder()
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap();
    engine.run_source("main.que", "fn quick() { 1 }").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(engine.call("quick", vec![]).unwrap(), Value::Int(1));
}

#[cfg(unix)]
#[test]
fn a_hanging_child_process_is_killed_at_the_timeout() {
    let Some(sleep) = program("sleep") else {
        return;
    };
    let mut engine = Engine::builder()
        .policy(Policy::deny_all().allow_exec_program(&sleep).unwrap())
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap();
    engine
        .run_source("main.que", "fn hang(p) { `${p} 30` }")
        .unwrap();
    let start = Instant::now();
    let result = engine.call("hang", vec![path(&sleep)]);
    assert!(
        matches!(
            result,
            Err(EmbedError::Cancelled(CancelReason::TimedOut(_)))
        ),
        "{:?}",
        result
    );
    assert!(start.elapsed() < STOP_BOUND);
}

#[test]
fn cancelling_from_another_thread_stops_the_script_and_runs_defers() {
    let cleaned = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&cleaned);
    let host = NativeModule::new("host").function("cleanup", move |_ctx, _args| {
        *flag.lock().unwrap() = true;
        Ok(Value::Null)
    });
    let mut engine = Engine::builder().native_module(host).build().unwrap();
    let token = engine.cancellation_token();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        token.cancel();
    });
    let start = Instant::now();
    let result = engine.run_source(
        "main.que",
        "import host\n\
         fn work() {\n\
             defer host.cleanup()\n\
             mut r = \"\"\n\
             try { while true { } } catch e { r = \"swallowed\" }\n\
             r\n\
         }\n\
         work()",
    );
    canceller.join().unwrap();
    assert!(
        matches!(result, Err(EmbedError::Cancelled(CancelReason::Cancelled))),
        "{:?}",
        result
    );
    assert!(start.elapsed() < STOP_BOUND);
    assert!(*cleaned.lock().unwrap(), "defer ran on the way out");
}

#[test]
fn a_token_cancelled_before_the_call_stops_it_immediately() {
    let token = que_lang::embed::CancellationToken::new();
    token.cancel();
    let mut engine = Engine::builder().cancellation_token(token).build().unwrap();
    let result = engine.run_source("main.que", "1");
    assert!(matches!(
        result,
        Err(EmbedError::Cancelled(CancelReason::Cancelled))
    ));
}

// ── concurrency ──────────────────────────────────────────────────────

#[test]
fn interpreters_run_concurrently_with_their_own_policies_and_native_modules() {
    let dirs: Vec<PathBuf> = (0..4)
        .map(|i| {
            let d = scratch(&format!("concurrent_{}", i));
            std::fs::write(d.join("id.txt"), i.to_string()).unwrap();
            d
        })
        .collect();
    let barrier = Arc::new(std::sync::Barrier::new(dirs.len()));
    let handles: Vec<_> = (0..dirs.len())
        .map(|i| {
            let own = dirs[i].clone();
            let neighbour = dirs[(i + 1) % dirs.len()].clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let host = NativeModule::new("host")
                    .function("id", move |_ctx, _args| Ok(Value::Int(i as i64)));
                let mut engine = Engine::builder()
                    .policy(Policy::deny_all().allow_read(&own))
                    .native_module(host)
                    .timeout(Duration::from_secs(60))
                    .build()
                    .unwrap();
                engine
                    .run_source(
                        "main.que",
                        "import host\nimport std.fs\n\
                         fn spin() { mut n = 0\nfor k in 0..20000 { n += k }\nn }\n\
                         fn read_id(d) { int(fs.read(d.join(\"id.txt\"))?) }\n",
                    )
                    .unwrap();
                barrier.wait();
                let mut results = Vec::new();
                for _ in 0..5 {
                    engine.call("spin", vec![]).unwrap();
                    let id = engine.run_source("id.que", "host.id()").unwrap();
                    let read = engine.call("read_id", vec![path(&own)]).unwrap();
                    let denied = engine.call("read_id", vec![path(&neighbour)]);
                    results.push((id, read, denied.is_err()));
                }
                (i, results)
            })
        })
        .collect();
    for handle in handles {
        let (i, results) = handle.join().expect("thread panicked");
        for (id, read, denied) in results {
            assert_eq!(id, Value::Int(i as i64));
            assert_eq!(read, Value::Int(i as i64));
            assert!(
                denied,
                "interpreter {} could read its neighbour's directory",
                i
            );
        }
    }
}

#[test]
fn cancelling_one_interpreter_leaves_another_running() {
    let mut slow = Engine::builder().build().unwrap();
    let mut other = Engine::builder().build().unwrap();
    let token = slow.cancellation_token();
    let t = std::thread::spawn(move || slow.run_source("main.que", "while true { }"));
    std::thread::sleep(Duration::from_millis(50));
    token.cancel();
    assert!(matches!(t.join().unwrap(), Err(EmbedError::Cancelled(_))));
    assert_eq!(
        other.run_source("main.que", "1 + 1").unwrap(),
        Value::Int(2)
    );
}

// ── module loading ───────────────────────────────────────────────────

#[test]
fn modules_load_from_explicit_search_paths_only() {
    let root = scratch("modules_search");
    let kits = root.join("kits");
    std::fs::create_dir_all(kits.join("demo_kit")).unwrap();
    std::fs::write(
        kits.join("demo_kit/mod.que"),
        "pub fn name() { \"kit\" }\n",
    )
    .unwrap();
    std::fs::write(
        kits.join("demo_kit/license.que"),
        "import .helpers\npub fn stage(ctx) { helpers.prefix() + ctx }\n",
    )
    .unwrap();
    std::fs::write(
        kits.join("demo_kit/helpers.que"),
        "pub fn prefix() { \"lic:\" }\n",
    )
    .unwrap();
    // A que_packages directory next to the script must not be consulted.
    let pkg = root.join("pkg");
    std::fs::create_dir_all(pkg.join("que_packages/local_only")).unwrap();
    std::fs::write(
        pkg.join("que_packages/local_only/mod.que"),
        "pub fn f() { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        pkg.join("hooks.que"),
        "import demo_kit\nimport demo_kit.license\nfn run() { demo_kit.name() + \"/\" + license.stage(\"x\") }\n",
    )
    .unwrap();

    let mut engine = Engine::builder()
        .policy(Policy::deny_all().allow_read(&root))
        .search_path(&kits)
        .build()
        .unwrap();
    engine.run_file(pkg.join("hooks.que")).unwrap();
    assert_eq!(engine.call("run", vec![]).unwrap(), s("kit/lic:x"));

    let handle = engine.import("demo_kit.license").unwrap();
    assert_eq!(
        engine.call_in(&handle, "stage", vec![s("y")]).unwrap(),
        s("lic:y")
    );

    let err = script_error(engine.run_source("other.que", "import local_only"));
    assert!(
        err.message.starts_with("module not found: local_only"),
        "{}",
        err.message
    );
    assert!(err.message.contains("embedded mode"), "{}", err.message);
}

#[test]
fn modules_load_from_embedded_sources() {
    let mut engine = Engine::builder()
        .embedded_module(
            "kit/mod.que",
            "import .util\npub fn hello(n) { util.greet(n) }\n",
        )
        .embedded_module("kit/util.que", "pub fn greet(n) { \"hello \" + n }\n")
        .embedded_module("helper.que", "pub fn twice(x) { x * 2 }\n")
        .build()
        .unwrap();
    let v = engine
        .run_source(
            "main.que",
            "import kit\nimport .helper\nkit.hello(\"que\") + \" \" + str(helper.twice(21))",
        )
        .unwrap();
    assert_eq!(v, s("hello que 42"));
    let kit = engine.import("kit").unwrap();
    assert!(kit.exports.contains_key("hello"));
}

#[test]
fn embedded_module_paths_must_be_relative_que_files() {
    for bad in ["/abs/mod.que", "kit/../x.que", "kit/mod.txt"] {
        let result = Engine::builder().embedded_module(bad, "").build();
        assert!(matches!(result, Err(EmbedError::Config(_))), "{}", bad);
    }
}

#[test]
fn a_syntax_error_names_the_file() {
    let mut engine = Engine::builder().build().unwrap();
    let err = script_error(engine.run_source("broken.que", "let = 1"));
    assert_eq!(
        err.location.and_then(|l| l.file).as_deref(),
        Some("broken.que")
    );
}

// ── language version ─────────────────────────────────────────────────

#[test]
fn the_language_version_is_exported() {
    assert_eq!(que_lang::LANGUAGE_VERSION, que_lang::language_version());
    assert_eq!(que_lang::LANGUAGE_VERSION.split('.').count(), 3);
}

// ── access tracing ───────────────────────────────────────────────────

fn traced_engine(policy: Policy) -> (Engine, Arc<Mutex<Vec<AccessEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let engine = Engine::builder()
        .policy(policy)
        .tracer(move |e: &AccessEvent| sink.lock().unwrap().push(e.clone()))
        .build()
        .unwrap();
    (engine, events)
}

#[test]
fn tracing_reports_every_read_existence_check_glob_and_listing() {
    let root = scratch("trace_exact");
    std::fs::create_dir_all(root.join("src/sub")).unwrap();
    std::fs::write(root.join("src/a.c"), "a").unwrap();
    std::fs::write(root.join("src/b.c"), "b").unwrap();
    std::fs::write(root.join("src/sub/c.h"), "c").unwrap();
    std::fs::write(root.join("VERSION"), "1.0").unwrap();

    let (mut engine, events) = traced_engine(Policy::deny_all().allow_read(&root));
    engine
        .run_source(
            "hook.que",
            "import std.fs\n\
             fn configure(root) {\n\
                 let v = fs.read(root.join(\"VERSION\"))?\n\
                 let has_cfg = root.join(\"config.toml\").exists()\n\
                 let sources = root.join(\"src\").glob(\"*.c\")\n\
                 let entries = root.join(\"src\").ls()\n\
                 mut n = 0\n\
                 for f in root.join(\"src/sub\") { n += 1 }\n\
                 { version: v, has_cfg: has_cfg, sources: sources.len(), entries: entries.len(), n: n }\n\
             }\n",
        )
        .unwrap();
    let result = engine.call("configure", vec![path(&root)]).unwrap();
    assert!(matches!(result, Value::Map(_)));

    let events = events.lock().unwrap().clone();
    let summary: Vec<(AccessKind, &str, String, bool, Observation)> = events
        .iter()
        .map(|e| {
            (
                e.kind,
                e.operation.as_str(),
                e.path.display().to_string(),
                e.success,
                e.observed.clone(),
            )
        })
        .collect();
    let r = root.display().to_string();
    assert_eq!(
        summary,
        vec![
            (
                AccessKind::Read,
                "fs.read",
                format!("{r}/VERSION"),
                true,
                Observation::Entry(EntryType::File)
            ),
            (
                AccessKind::Exists,
                "Path.exists",
                format!("{r}/config.toml"),
                true,
                Observation::Entry(EntryType::Missing)
            ),
            (
                AccessKind::Glob,
                "Path.glob",
                format!("{r}/src/*.c"),
                true,
                Observation::Entries(vec![format!("{r}/src/a.c"), format!("{r}/src/b.c")]),
            ),
            (
                AccessKind::ListDir,
                "Path.ls",
                format!("{r}/src"),
                true,
                Observation::Entries(vec!["a.c".into(), "b.c".into(), "sub".into()]),
            ),
            (
                AccessKind::ListDir,
                "for-in",
                format!("{r}/src/sub"),
                true,
                Observation::Entries(vec!["c.h".into()]),
            ),
        ]
    );
    assert_eq!(events[0].requested, format!("{r}/VERSION"));
    assert_eq!(
        events[0].canonical.as_deref(),
        Some(root.join("VERSION").as_path())
    );
    assert!(events.iter().all(AccessEvent::is_unchanged));

    // A new file in the globbed directory invalidates the glob and the
    // listing, but not the read or the existence check.
    std::fs::write(root.join("src/new.c"), "n").unwrap();
    let changed: Vec<AccessKind> = events
        .iter()
        .filter(|e| !e.is_unchanged())
        .map(|e| e.kind)
        .collect();
    assert_eq!(changed, [AccessKind::Glob, AccessKind::ListDir]);
    // Creating the file whose absence was observed invalidates that check.
    std::fs::write(root.join("config.toml"), "").unwrap();
    assert!(!events[1].is_unchanged());
}

#[test]
fn tracing_records_failed_reads_writes_and_module_loads() {
    let root = scratch("trace_more");
    std::fs::create_dir_all(root.join("kits/k")).unwrap();
    std::fs::write(root.join("kits/k/mod.que"), "pub fn f() { 1 }\n").unwrap();
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut engine = Engine::builder()
        .policy(Policy::deny_all().allow_read(&root).allow_write(&out))
        .search_path(root.join("kits"))
        .tracer(move |e: &AccessEvent| sink.lock().unwrap().push(e.clone()))
        .build()
        .unwrap();
    engine
        .run_source(
            "hook.que",
            "import std.fs\nimport k\n\
             fn run(root, out) {\n\
                 let missing = fs.read(root.join(\"nope.txt\"))\n\
                 out.join(\"gen.txt\").write_text(\"x\")\n\
                 glob(root.to_string() + \"/kits/*\").count()\n\
             }\n",
        )
        .unwrap();
    engine.call("run", vec![path(&root), path(&out)]).unwrap();

    let events = events.lock().unwrap();
    let kinds: Vec<(AccessKind, &str, bool)> = events
        .iter()
        .map(|e| (e.kind, e.operation.as_str(), e.success))
        .collect();
    assert_eq!(
        kinds,
        [
            (AccessKind::ModuleLoad, "import", true),
            (AccessKind::Read, "fs.read", false),
            (AccessKind::Write, "Path.write_text", true),
            (AccessKind::Glob, "Glob.count", true),
        ]
    );
    assert_eq!(events[0].path, root.join("kits/k/mod.que"));
    assert!(events[1].error.is_some());
    assert_eq!(events[1].observed, Observation::Entry(EntryType::Missing));
}
