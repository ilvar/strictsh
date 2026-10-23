#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{tempdir, TempDir};

struct Fixture {
    root: TempDir,
    bin: PathBuf,
    destination: PathBuf,
    scratch: PathBuf,
    curl_log: PathBuf,
    cargo_log: PathBuf,
    rustup_log: PathBuf,
}

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        let bin = root.path().join("commands");
        let destination = root.path().join("install with spaces");
        let scratch = root.path().join("scratch");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&scratch).unwrap();
        let payload = root.path().join("payload");
        executable(&payload, "#!/bin/bash\nprintf 'strictsh fixture\\n'\n");
        executable(&bin.join("uname"), "#!/bin/bash\ncase \"$1\" in -s) printf '%s' \"$MOCK_OS\";; -m) printf '%s' \"$MOCK_ARCH\";; *) exit 1;; esac\n");
        executable(
            &bin.join("curl"),
            r#"#!/bin/bash
printf '%s\n' "$@" >>"$CURL_LOG"
output=''
url=''
while [ "$#" -gt 0 ]; do
    if [ "$1" = --output ]; then output="$2"; shift; fi
    url="$1"
    shift
done
case "$url" in
    */releases/latest)
        if [ "${FAIL_LATEST:-0}" = 1 ]; then exit 22; fi
        printf 'https://github.com/ilvar/strictsh/releases/tag/%s' "${MOCK_TAG:-v9.8.7}"
        ;;
    *.sha256)
        if [ "${BAD_CHECKSUM:-0}" = 1 ]; then
            printf '%064d  %s\n' 0 "${url##*/}" | /usr/bin/sed 's/\.sha256$//' >"$output"
        else
            digest=$(/usr/bin/sha256sum "$PAYLOAD") || exit 1
            printf '%s  %s\n' "${digest%% *}" "${url##*/}" | /usr/bin/sed 's/\.sha256$//' >"$output"
        fi
        ;;
    *)
        if [ "${FAIL_DOWNLOAD:-0}" = 1 ]; then exit 22; fi
        /usr/bin/cp "$PAYLOAD" "$output" || exit 1
        ;;
esac
"#,
        );
        executable(
            &bin.join("cargo"),
            r#"#!/bin/bash
if [ "$1" = --version ]; then printf 'cargo fixture\n'; exit 0; fi
printf '%s\n' "$@" >>"$CARGO_LOG"
if [ "${FAIL_BUILD:-0}" = 1 ]; then exit 1; fi
while [ "$#" -gt 0 ]; do
    if [ "$1" = --root ]; then
        /usr/bin/mkdir -p "$2/bin" || exit 1
        /usr/bin/cp "$PAYLOAD" "$2/bin/strictsh" || exit 1
        exit 0
    fi
    shift
done
exit 1
"#,
        );
        executable(
            &bin.join("rustup"),
            r#"#!/bin/bash
if [ "${FAIL_RUSTUP:-0}" = 1 ]; then exit 1; fi
if [ "$1" = --version ]; then printf 'rustup fixture\n'; exit 0; fi
printf '%s\n' "$@" >>"$RUSTUP_LOG"
"#,
        );
        executable(
            &bin.join("shellcheck"),
            "#!/bin/bash\nprintf 'ShellCheck fixture\\n'\n",
        );
        let curl_log = root.path().join("curl.log");
        let cargo_log = root.path().join("cargo.log");
        let rustup_log = root.path().join("rustup.log");
        Self {
            root,
            bin,
            destination,
            scratch,
            curl_log,
            cargo_log,
            rustup_log,
        }
    }

    fn run(&self, os: &str, arch: &str, extra: &[(&str, &str)]) -> Output {
        let mut command = Command::new("/bin/sh");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("TMPDIR", &self.scratch)
            .env("HOME", self.root.path())
            .env("STRICTSH_INSTALL_DIR", &self.destination)
            .env_remove("STRICTSH_BUILD_FROM_SOURCE")
            .env_remove("STRICTSH_VERSION")
            .env("MOCK_OS", os)
            .env("MOCK_ARCH", arch)
            .env("PAYLOAD", self.root.path().join("payload"))
            .env("CURL_LOG", &self.curl_log)
            .env("CARGO_LOG", &self.cargo_log)
            .env("RUSTUP_LOG", &self.rustup_log);
        for (key, value) in extra {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert_eq!(
            fs::read_dir(&self.scratch).unwrap().count(),
            0,
            "temporary files leaked"
        );
        output
    }

    fn assert_installed(&self, output: &Output) {
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let installed = self.destination.join("strictsh");
        assert_eq!(
            fs::read(&installed).unwrap(),
            fs::read(self.root.path().join("payload")).unwrap()
        );
        assert_ne!(
            fs::metadata(installed).unwrap().permissions().mode() & 0o111,
            0
        );
    }
}

#[test]
fn downloads_latest_matching_binary_and_checksum_for_supported_architectures() {
    for (os, arch, target) in [
        ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
        ("Linux", "amd64", "x86_64-unknown-linux-musl"),
        ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
        ("Linux", "arm64", "aarch64-unknown-linux-musl"),
        ("Darwin", "x86_64", "x86_64-apple-darwin"),
        ("Darwin", "arm64", "aarch64-apple-darwin"),
    ] {
        let fixture = Fixture::new();
        fixture.assert_installed(&fixture.run(os, arch, &[]));
        let log = fs::read_to_string(&fixture.curl_log).unwrap();
        assert!(log.contains("/releases/latest"));
        assert!(log.contains(&format!("/releases/download/v9.8.7/strictsh-{target}\n")));
        assert!(log.contains(&format!(
            "/releases/download/v9.8.7/strictsh-{target}.sha256\n"
        )));
        assert!(!fixture.cargo_log.exists());
        assert!(!fixture.rustup_log.exists());
    }
}

#[test]
fn macos_checksum_verification_works_without_gnu_sha256sum() {
    let fixture = Fixture::new();
    executable(&fixture.bin.join("sha256sum"), "#!/bin/bash\nexit 127\n");
    fixture.assert_installed(&fixture.run("Darwin", "arm64", &[]));
    assert!(!fixture.cargo_log.exists());
}

#[test]
fn pinned_versions_download_the_selected_tag_without_latest_lookup() {
    for version in ["0.1.2", "v0.1.2"] {
        let fixture = Fixture::new();
        fixture.assert_installed(&fixture.run(
            "Linux",
            "x86_64",
            &[("STRICTSH_VERSION", version), ("FAIL_LATEST", "1")],
        ));
        let log = fs::read_to_string(&fixture.curl_log).unwrap();
        assert!(!log.contains("/releases/latest"));
        assert!(log.contains("/releases/download/v0.1.2/strictsh-x86_64-unknown-linux-musl\n"));
        assert!(
            log.contains("/releases/download/v0.1.2/strictsh-x86_64-unknown-linux-musl.sha256\n")
        );
        assert!(!fixture.cargo_log.exists());
    }
}

#[test]
fn pinned_source_builds_use_the_selected_tag_without_any_release_lookup() {
    for (arch, forced) in [("riscv64", "0"), ("x86_64", "1")] {
        let fixture = Fixture::new();
        fixture.assert_installed(&fixture.run(
            "Linux",
            arch,
            &[
                ("STRICTSH_VERSION", "v0.1.2"),
                ("STRICTSH_BUILD_FROM_SOURCE", forced),
                ("FAIL_LATEST", "1"),
            ],
        ));
        assert!(fs::read_to_string(&fixture.cargo_log)
            .unwrap()
            .contains("--tag\nv0.1.2\n--locked\n"));
        assert!(!fixture.curl_log.exists());
    }
}

#[test]
fn empty_version_and_explicit_latest_keep_default_behavior() {
    for version in ["", "latest"] {
        let fixture = Fixture::new();
        fixture.assert_installed(&fixture.run("Linux", "x86_64", &[("STRICTSH_VERSION", version)]));
        let log = fs::read_to_string(&fixture.curl_log).unwrap();
        assert!(log.contains("/releases/latest"));
        assert!(log.contains("/releases/download/v9.8.7/"));
    }
}

#[test]
fn invalid_version_is_rejected_before_network_or_build_and_preserves_install() {
    for version in [
        "main",
        "../v0.1.2",
        "vv0.1.2",
        "0.1",
        "0.1.2;touch file",
        " 0.1.2",
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.destination.join("strictsh"), "existing install").unwrap();
        let output = fixture.run("Linux", "riscv64", &[("STRICTSH_VERSION", version)]);
        assert!(
            !output.status.success(),
            "invalid version accepted: {version}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("STRICTSH_VERSION must be"));
        assert!(!fixture.curl_log.exists());
        assert!(!fixture.cargo_log.exists());
        assert!(!fixture.rustup_log.exists());
        assert_eq!(
            fs::read_to_string(fixture.destination.join("strictsh")).unwrap(),
            "existing install"
        );
    }
}

#[test]
fn exotic_architectures_and_other_unix_hosts_build_the_exact_latest_tag() {
    for (os, arch) in [
        ("Linux", "riscv64"),
        ("Linux", "armv7l"),
        ("FreeBSD", "amd64"),
    ] {
        let fixture = Fixture::new();
        fixture.assert_installed(&fixture.run(os, arch, &[]));
        let cargo = fs::read_to_string(&fixture.cargo_log).unwrap();
        assert!(cargo.starts_with("+1.97.1\ninstall\n"));
        assert!(cargo.contains("--tag\nv9.8.7\n--locked\n"));
        assert!(cargo.contains("--git\nhttps://github.com/ilvar/strictsh\n"));
        assert_eq!(
            fs::read_to_string(&fixture.rustup_log).unwrap(),
            "toolchain\ninstall\n1.97.1\n--profile\nminimal\n"
        );
        assert!(!fs::read_to_string(&fixture.curl_log)
            .unwrap()
            .contains("/releases/download/"));
    }
}

#[test]
fn supports_forced_source_build_and_warns_about_missing_shellcheck() {
    let fixture = Fixture::new();
    executable(&fixture.bin.join("shellcheck"), "#!/bin/bash\nexit 127\n");
    let output = fixture.run("Linux", "x86_64", &[("STRICTSH_BUILD_FROM_SOURCE", "1")]);
    fixture.assert_installed(&output);
    assert!(fixture.cargo_log.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ShellCheck is required"));
}

#[test]
fn failed_download_checksum_build_or_release_lookup_preserves_existing_install() {
    for (arch, flag, value) in [
        ("x86_64", "BAD_CHECKSUM", "1"),
        ("x86_64", "FAIL_DOWNLOAD", "1"),
        ("x86_64", "FAIL_LATEST", "1"),
        ("x86_64", "MOCK_TAG", "main"),
        ("riscv64", "FAIL_BUILD", "1"),
        ("riscv64", "FAIL_RUSTUP", "1"),
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.destination.join("strictsh"), "existing install").unwrap();
        let output = fixture.run("Linux", arch, &[(flag, value)]);
        assert!(!output.status.success(), "failure ignored: {flag}");
        assert_eq!(
            fs::read_to_string(fixture.destination.join("strictsh")).unwrap(),
            "existing install"
        );
        if arch == "x86_64" {
            assert!(
                !fixture.cargo_log.exists(),
                "failed prebuilt install silently built from source"
            );
        }
    }
}
