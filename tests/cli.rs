use serde_json::Value;
use std::fs;
use std::process::{Command, Output};
use tempfile::tempdir;

const CLEAN: &str = "#!/bin/sh\nset -u\nprintf 'hello\\n'\n";

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_strictsh"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn real_shellcheck_clean_report_and_exit_status() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("clean.sh");
    fs::write(&file, CLEAN).unwrap();
    let output = cli(&["check", file.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"ok\":true,\"error_count\":0,\"warning_count\":0,\"diagnostics\":[]}\n"
    );
}

#[test]
fn elif_conditions_are_clean_in_full_checker() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("elif.sh");
    fs::write(
        &file,
        "#!/bin/sh\nset -u\nif [ -f first ]; then\n    :\nelif grep -q 'needle' file; then\n    :\nelif value=$(printf 'ready'); then\n    printf '%s' \"${value}\"\nfi\n",
    )
    .unwrap();
    let output = cli(&["check", file.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["diagnostics"], serde_json::json!([]));
    assert!(output.stderr.is_empty());
}

#[test]
fn collects_multiple_policy_and_shellcheck_errors_without_execution() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join("should-not-exist");
    let file = dir.path().join("bad.sh");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\nset -e\necho $undefined\neval date\ntouch '{}'\n",
            marker.display()
        ),
    )
    .unwrap();
    let output = cli(&[file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!marker.exists());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert!(value["error_count"].as_u64().unwrap() >= 5);
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == "shellcheck::SC2086"));
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["at"]["file"] == "bad.sh"));
    assert!(output.stderr.is_empty());
}

#[test]
fn discovery_is_sorted_skips_build_output_and_accepts_extensionless_scripts() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("z.sh"), CLEAN.replace("printf", "echo")).unwrap();
    fs::write(dir.path().join("a"), CLEAN.replace("printf", "echo")).unwrap();
    fs::create_dir(dir.path().join("target")).unwrap();
    fs::write(dir.path().join("target/bad.sh"), "invalid").unwrap();
    let output = cli(&["check", dir.path().to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.first().unwrap()["at"]["file"], "a");
    assert!(diagnostics
        .iter()
        .all(|item| !item["at"]["file"].as_str().unwrap().contains("target")));
    assert_eq!(
        output.stdout,
        cli(&["check", dir.path().to_str().unwrap()]).stdout
    );
}

#[test]
fn help_aliases_and_invalid_invocations_obey_output_contract() {
    let expected = cli(&["--help"]);
    for args in [
        &["-h"][..],
        &["help"][..],
        &["check", "--help"][..],
        &["new", "--help"][..],
    ] {
        let output = cli(args);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, expected.stdout);
    }
    for args in [
        &["--unknown"][..],
        &["check", "one", "two"][..],
        &["new"][..],
        &["check", "/nonexistent/strictsh"][..],
    ] {
        let output = cli(args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let empty = tempdir().unwrap();
    assert_eq!(
        cli(&["check", empty.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn missing_or_broken_shellcheck_fails_closed() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("clean.sh");
    fs::write(&file, CLEAN).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_strictsh"))
        .arg(&file)
        .env("PATH", dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ShellCheck is required"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let stub = dir.path().join("shellcheck");
        for body in [
            "if [ \"$1\" = --version ]; then exit 0; fi\nprintf 'not JSON'",
            "if [ \"$1\" = --version ]; then exit 0; fi\nexit 2",
            "if [ \"$1\" = --version ]; then exit 0; fi\nprintf '{\"comments\":[]}'\nexit 1",
        ] {
            fs::write(&stub, format!("#!/bin/bash\n{body}\n")).unwrap();
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_strictsh"))
                .arg(&file)
                .env("PATH", dir.path())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn environment_cannot_suppress_shellcheck() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.sh");
    fs::write(&file, "#!/bin/sh\nset -u\nprintf '%s' $undefined\n").unwrap();
    fs::write(dir.path().join(".shellcheckrc"), "disable=all\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_strictsh"))
        .arg(&file)
        .env("SHELLCHECK_OPTS", "--exclude=SC2086,SC2154")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("shellcheck::SC2086"));
}

#[test]
fn generation_is_deterministic_clean_runnable_and_refuses_overwrite() {
    let dir = tempdir().unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_strictsh"))
            .args(["new", "hello"])
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let project = dir.path().join("hello");
    let checked = cli(&["check", project.to_str().unwrap()]);
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let smoke = Command::new("sh")
        .arg("tests/smoke.sh")
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        smoke.status.success(),
        "{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
    assert_eq!(run().status.code(), Some(2));
    let second = tempdir().unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_strictsh"))
        .args(["new", "hello"])
        .current_dir(second.path())
        .status()
        .unwrap()
        .success());
    for relative in [
        "main.sh",
        "tests/smoke.sh",
        "AGENTS.md",
        "README.md",
        "Makefile",
    ] {
        assert_eq!(
            fs::read(project.join(relative)).unwrap(),
            fs::read(second.path().join("hello").join(relative)).unwrap()
        );
    }
    for name in ["../escape", "", "Bad", "a/b"] {
        assert_eq!(
            Command::new(env!("CARGO_BIN_EXE_strictsh"))
                .args(["new", name])
                .current_dir(dir.path())
                .output()
                .unwrap()
                .status
                .code(),
            Some(2)
        );
    }
}

#[test]
fn full_checker_forces_sh_semantics_and_rejects_bashisms() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("portable.sh");
    for body in [
        "if [[ -f file ]]; then :; fi",
        "items=(one two)",
        "local value=one",
        "printf '%s' \"${value//a/b}\"",
        "if read -r -a items; then :; fi",
        "printf x | cat || exit 1",
        "set -o pipefail",
    ] {
        fs::write(&file, format!("#!/bin/bash\nset -u\n{body}\n")).unwrap();
        let output = cli(&[file.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(1), "{body}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            value["diagnostics"].as_array().unwrap().iter().any(|item| {
                item["source"] == "shellcheck"
                    || item["code"] == "strictsh::no_pipeline"
                    || item["code"] == "strictsh::fixed_options"
            }),
            "{body}: {value}"
        );
    }
}

#[test]
fn sh_discovery_skips_bash_files_and_other_shell_shebangs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("clean.sh"), CLEAN).unwrap();
    fs::write(
        dir.path().join("ignored.bash"),
        "#!/bin/bash\necho ignored\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("bash-script"),
        "#!/bin/bash\necho ignored\n",
    )
    .unwrap();
    let output = cli(&[dir.path().to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        cli(&[dir.path().join("ignored.bash").to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
}
