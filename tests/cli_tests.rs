use std::fs;
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn que(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_que"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run que")
}

#[test]
fn version_commands_use_the_cargo_package_version() {
    for arg in ["--version", "-V", "version"] {
        let output = que(&[arg]);
        assert!(output.status.success(), "que {arg} failed");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            format!("Que v{VERSION}")
        );
    }
}

#[test]
fn help_includes_the_version_and_version_flags() {
    let output = que(&["--help"]);
    assert!(output.status.success());

    let help = String::from_utf8_lossy(&output.stderr);
    assert!(help.starts_with(&format!("Que v{VERSION}\n\nUsage:")));
    assert!(help.contains("--version, -V"));
}

#[test]
fn repl_banner_uses_the_cargo_package_version() {
    let test_home = std::env::temp_dir().join(format!("que-cli-test-{}", std::process::id()));
    fs::create_dir_all(&test_home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_que"))
        .env("NO_COLOR", "1")
        .env("HOME", &test_home)
        .output()
        .expect("failed to run que REPL");

    fs::remove_dir_all(test_home).ok();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with(&format!("Que v{VERSION}")));
}

// ── Quefile discovery: walk-up and the global Quefile ────────────────

/// A throwaway `$HOME` with a global Quefile plus a project directory
/// containing a `sub/` to invoke from.
struct Fixture {
    root: std::path::PathBuf,
}

impl Fixture {
    fn new(tag: &str, project: Option<&str>, global: Option<&str>) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "que-quefile-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("home/.que")).unwrap();
        fs::create_dir_all(root.join("project/sub")).unwrap();
        if let Some(text) = project {
            fs::write(root.join("project/Quefile"), text).unwrap();
        }
        if let Some(text) = global {
            fs::write(root.join("home/.que/Quefile"), text).unwrap();
        }
        Fixture { root }
    }

    /// Run `que` from `dir`, with the fixture's home as the only place a
    /// global Quefile could come from.
    fn que(&self, dir: &str, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_que"))
            .args(args)
            .current_dir(self.root.join(dir))
            .env("NO_COLOR", "1")
            .env("HOME", self.root.join("home"))
            .env_remove("QUE_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .expect("failed to run que")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn run_walks_up_to_the_nearest_quefile_but_stays_in_the_callers_directory() {
    let fixture = Fixture::new(
        "walkup",
        Some(
            "task build {\n    println(\"dir=\" + str(quefile_dir()))\n    println(\"cwd=\" + str(path(\".\").resolve()))\n}\n",
        ),
        None,
    );

    let output = fixture.que("project/sub", &["run", "build"]);

    assert!(output.status.success(), "{:?}", output);
    let project = fixture.root.join("project");
    assert!(
        stdout(&output).contains(&format!("dir={}", project.display())),
        "{}",
        stdout(&output)
    );
    // The task runs where the user stood, not where the Quefile lives.
    assert!(
        stdout(&output).contains(&format!("cwd={}", project.join("sub").display())),
        "{}",
        stdout(&output)
    );
}

#[test]
fn cd_moves_the_process_and_hands_back_the_directory_it_left() {
    // The scoped form is not language syntax: `cd` returning the old
    // directory is enough to write it as a `Contextual` impl in Que.
    let quefile = r#"
struct Dir { path }

impl Contextual for Dir {
    fn enter(self) -> Path { cd(self.path) }
    fn exit(self, previous) { cd(previous) }
}

task build {
    println("before=" + str(path(".").resolve()))
    with Dir { path: p"sub" } {
        println("inside=" + str(path(".").resolve()))
    }
    println("after=" + str(path(".").resolve()))
}
"#;
    let fixture = Fixture::new("cd", Some(quefile), None);

    let output = fixture.que("project", &["run", "build"]);

    assert!(output.status.success(), "{:?}", output);
    let project = fixture.root.join("project");
    let out = stdout(&output);
    assert!(out.contains(&format!("before={}", project.display())), "{out}");
    assert!(out.contains(&format!("inside={}", project.join("sub").display())), "{out}");
    assert!(out.contains(&format!("after={}", project.display())), "{out}");
}

#[test]
fn with_dir_scopes_the_move_to_the_block_including_on_failure() {
    // The built-in spelling of the Contextual above. Restoring on the error
    // path is the half a hand-written `cd` pair usually gets wrong, so the
    // failing case is the one worth pinning down.
    let quefile = r#"
task build {
    println("before=" + str(path(".").resolve()))
    with dir(p"sub") {
        println("inside=" + str(path(".").resolve()))
    }
    println("after=" + str(path(".").resolve()))
}

task boom {
    defer { println("unwind=" + str(path(".").resolve())) }
    with dir(p"sub") {
        fail("boom")
    }
}
"#;
    let fixture = Fixture::new("with-dir", Some(quefile), None);
    let project = fixture.root.join("project");

    let output = fixture.que("project", &["run", "build"]);
    assert!(output.status.success(), "{:?}", output);
    let out = stdout(&output);
    assert!(out.contains(&format!("before={}", project.display())), "{out}");
    assert!(out.contains(&format!("inside={}", project.join("sub").display())), "{out}");
    assert!(out.contains(&format!("after={}", project.display())), "{out}");

    let failed = fixture.que("project", &["run", "boom"]);
    assert!(!failed.status.success(), "{:?}", failed);
    let out = stdout(&failed);
    assert!(out.contains(&format!("unwind={}", project.display())), "{out}");
}

#[test]
fn a_global_quefile_can_import_packages_installed_with_install_g() {
    // The global Quefile's own directory is its package root, so its
    // dependencies live beside it rather than in whatever project the user
    // happens to be standing in. `-g` is what makes that reachable without
    // knowing where the global directory is.
    let fixture = Fixture::new(
        "global-install",
        Some("task local { println(\"project\") }\n"),
        Some("import shared { greet }\n\ntask hello {\n    println(greet(\"global\"))\n}\n"),
    );

    let pkg = fixture.root.join("home/.que/shared");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("mod.que"), "pub fn greet(who) { \"hi \" + who }").unwrap();
    fs::write(
        fixture.root.join("home/.que/que.toml"),
        "[dependencies]\nshared = { path = \"shared\" }\n",
    )
    .unwrap();

    // Installed from the project directory: the point of the flag.
    let installed = fixture.que("project/sub", &["install", "-g"]);
    assert!(installed.status.success(), "{:?}", installed);
    assert!(fixture
        .root
        .join("home/.que/que_packages/shared")
        .exists());

    // The task is not in the project Quefile, so it falls through to the
    // global one — whose import has to resolve against the global directory.
    let output = fixture.que("project/sub", &["run", "hello"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(stdout(&output).contains("hi global"), "{}", stdout(&output));
}

// ── Task parameters: the rest parameter ──────────────────────────────

#[test]
fn a_rest_parameter_collects_every_remaining_command_line_argument() {
    // The count is the caller's, not the Quefile's: a task fed from a shell
    // has no say in how many paths the glob expanded to.
    let quefile = r#"
task build(target, ...files) {
    println("target=" + target)
    println("n=" + str(files.len()))
    for f in files { println("file=" + f) }
}
"#;
    let fixture = Fixture::new("rest-param", Some(quefile), None);

    let output = fixture.que("project", &["run", "build", "--", "app", "a.txt", "b.txt", "c.txt"]);
    assert!(output.status.success(), "{:?}", output);
    let out = stdout(&output);
    assert!(out.contains("target=app"), "{out}");
    assert!(out.contains("n=3"), "{out}");
    for f in ["a.txt", "b.txt", "c.txt"] {
        assert!(out.contains(&format!("file={f}")), "{out}");
    }

    // Nothing left over is an empty list, not a missing argument: a rest
    // parameter is never required.
    let empty = fixture.que("project", &["run", "build", "--", "app"]);
    assert!(empty.status.success(), "{:?}", empty);
    assert!(stdout(&empty).contains("n=0"), "{}", stdout(&empty));
}

#[test]
fn arguments_no_parameter_can_claim_are_refused_rather_than_dropped() {
    // Quietly ignoring the tail turns a typo into a run that looks like it
    // worked, so the count is checked before the body gets to start.
    let quefile = "task build(target) {\n    println(\"target=\" + target)\n}\n";
    let fixture = Fixture::new("extra-args", Some(quefile), None);

    let output = fixture.que("project", &["run", "build", "--", "app", "stray"]);

    assert!(!output.status.success(), "{:?}", output);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("takes 1 positional argument, but 2 were given"), "{err}");
    assert!(err.contains("...name"), "{err}");
    // The check runs ahead of the body, so the task never started.
    assert!(!stdout(&output).contains("target=app"), "{}", stdout(&output));
}

#[test]
fn a_rest_parameter_is_a_task_spelling_and_must_come_last() {
    let cases = [
        (
            "task build(...files, target) {\n    println(target)\n}\n",
            "must be the last parameter",
        ),
        (
            "task build(...files = []) {\n    println(str(files))\n}\n",
            "cannot have a default",
        ),
        (
            "fn f(...xs) {\n    println(str(xs))\n}\ntask build {\n    f()\n}\n",
            "only allowed on a task parameter",
        ),
    ];

    for (quefile, expected) in cases {
        let fixture = Fixture::new("rest-invalid", Some(quefile), None);
        let output = fixture.que("project", &["run", "build"]);
        assert!(!output.status.success(), "{quefile}\n{:?}", output);
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{quefile}\nexpected {expected:?}, got: {err}");
    }
}

#[test]
fn a_task_the_project_quefile_lacks_comes_from_the_global_one() {    let fixture = Fixture::new(
        "fallback",
        Some("task build {\n    println(\"project build\")\n}\n"),
        Some("task backup {\n    println(\"global backup\")\n}\n"),
    );

    let output = fixture.que("project", &["run", "backup"]);

    assert!(output.status.success(), "{:?}", output);
    assert!(stdout(&output).contains("global backup"), "{}", stdout(&output));
}

#[test]
fn the_project_quefile_shadows_a_global_task_unless_g_is_given() {
    let fixture = Fixture::new(
        "shadow",
        Some("task build {\n    println(\"project build\")\n}\n"),
        Some("task build {\n    println(\"global build\")\n}\n"),
    );

    let local = fixture.que("project", &["run", "build"]);
    assert!(local.status.success(), "{:?}", local);
    assert!(stdout(&local).contains("project build"), "{}", stdout(&local));

    let global = fixture.que("project", &["run", "-g", "build"]);
    assert!(global.status.success(), "{:?}", global);
    assert!(stdout(&global).contains("global build"), "{}", stdout(&global));
}

#[test]
fn tasks_lists_the_global_quefile_and_marks_shadowed_names() {
    let fixture = Fixture::new(
        "listing",
        Some("task build {\n    println(\"project build\")\n}\n"),
        Some("task build {\n    1\n}\ntask backup {\n    1\n}\n"),
    );

    let output = fixture.que("project", &["tasks"]);

    assert!(output.status.success(), "{:?}", output);
    let listing = stdout(&output);
    assert!(listing.contains("Global tasks in"), "{listing}");
    assert!(listing.contains("backup"), "{listing}");
    assert!(listing.contains("build (shadowed)"), "{listing}");
}

#[test]
fn an_unknown_task_names_both_the_project_and_the_global_quefile() {
    let fixture = Fixture::new(
        "missing",
        Some("task build {\n    println(\"project build\")\n}\n"),
        Some("task backup {\n    1\n}\n"),
    );

    let output = fixture.que("project", &["run", "nope"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no task named 'nope'"), "{stderr}");
    assert!(stderr.contains("global Quefile"), "{stderr}");
}

#[test]
fn f_and_g_cannot_be_combined() {
    let fixture = Fixture::new("conflict", Some("task build { 1 }\n"), Some("task build { 1 }\n"));

    let output = fixture.que("project", &["run", "-f", "Quefile", "-g", "build"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("-f and -g cannot be combined"),
        "{:?}",
        output
    );
}

#[test]
fn a_glob_rooted_at_the_quefile_directory_still_detects_a_changed_input() {
    // `quefile_dir() / "src/*.txt"` produces a Path, not a String. Left
    // unexpanded it would name a file that cannot exist, so nothing would ever
    // look stale and the task would skip for good.
    let fixture = Fixture::new(
        "rooted-glob",
        Some(concat!(
            "import std.fs { read, write }\n",
            "@inputs([quefile_dir() / \"src/*.txt\"])\n",
            "@outputs([quefile_dir() / \"build/out.txt\"])\n",
            "task build {\n",
            "    let d = quefile_dir() / \"build\"\n",
            "    d.mkdir()\n",
            "    write(d / \"out.txt\", read(quefile_dir() / \"src/a.txt\").unwrap())\n",
            "}\n",
        )),
        None,
    );
    let src = fixture.root.join("project/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "v1\n").unwrap();

    let first = fixture.que("project/sub", &["run", "build"]);
    assert!(first.status.success(), "{:?}", first);
    assert!(stdout(&first).contains("[DONE] build"), "{}", stdout(&first));

    let second = fixture.que("project/sub", &["run", "build"]);
    assert!(stdout(&second).contains("[SKIP] build"), "{}", stdout(&second));

    fs::write(src.join("a.txt"), "v2\n").unwrap();
    let third = fixture.que("project/sub", &["run", "build"]);
    assert!(stdout(&third).contains("[DONE] build"), "{}", stdout(&third));
}

#[test]
fn force_runs_a_task_that_would_otherwise_be_skipped() {
    // Deleting the cache file is not enough on its own — the mtime fast path
    // answers before the cache is ever consulted — so there has to be a way to
    // say "run it anyway" that does not involve deleting build artifacts.
    let fixture = Fixture::new(
        "force-run",
        Some(concat!(
            "import std.fs { read, write }\n",
            "@inputs([quefile_dir() / \"src/a.txt\"])\n",
            "@outputs([quefile_dir() / \"build/out.txt\"])\n",
            "task build {\n",
            "    let d = quefile_dir() / \"build\"\n",
            "    d.mkdir()\n",
            "    write(d / \"out.txt\", read(quefile_dir() / \"src/a.txt\").unwrap())\n",
            "}\n",
        )),
        None,
    );
    let src = fixture.root.join("project/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "v1\n").unwrap();

    let first = fixture.que("project/sub", &["run", "build"]);
    assert!(first.status.success(), "{:?}", first);
    assert!(stdout(&first).contains("[DONE] build"), "{}", stdout(&first));

    let second = fixture.que("project/sub", &["run", "build"]);
    assert!(stdout(&second).contains("[SKIP] build"), "{}", stdout(&second));

    for flag in ["--force", "-B"] {
        let forced = fixture.que("project/sub", &["run", flag, "build"]);
        assert!(forced.status.success(), "{:?}", forced);
        assert!(
            stdout(&forced).contains("[DONE] build"),
            "{} did not force a run: {}",
            flag,
            stdout(&forced)
        );
    }

    // A forced run still records what it produced, so the next plain run can
    // skip again — forcing once must not poison the cache.
    let after = fixture.que("project/sub", &["run", "build"]);
    assert!(stdout(&after).contains("[SKIP] build"), "{}", stdout(&after));
}
