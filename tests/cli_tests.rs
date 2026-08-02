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
