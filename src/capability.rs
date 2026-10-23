//! Filesystem and process boundary. Checked scripts are never executed.
// strictrs: capability
mod platform {
    use crate::{lint_source, report, Diagnostic, Location, Report};
    use serde::Deserialize;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    pub use std::process::ExitCode;

    #[derive(Deserialize)]
    struct ShellReport {
        comments: Vec<ShellDiagnostic>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ShellDiagnostic {
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
        code: u32,
        message: String,
    }

    pub fn check(path: &Path) -> Result<Report, String> {
        let root = path
            .canonicalize()
            .map_err(|error| format!("cannot access {}: {error}", path.display()))?;
        let mut files = Vec::new();
        collect(&root, &mut files)?;
        files.sort();
        if files.is_empty() {
            return Err("no POSIX sh scripts found".to_owned());
        }
        let version = Command::new("shellcheck")
            .arg("--version")
            .output()
            .map_err(|error| format!("ShellCheck is required on PATH: {error}"))?;
        if !version.status.success() {
            return Err("ShellCheck --version failed".to_owned());
        }
        let mut diagnostics = Vec::new();
        let base = if root.is_dir() {
            root.clone()
        } else {
            root.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        for file in files {
            let source = fs::read_to_string(&file)
                .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
            let label = file
                .strip_prefix(&base)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            diagnostics.extend(lint_source(&label, &source)?);
            let output = Command::new("shellcheck")
                .args([
                    "--norc",
                    "--shell=sh",
                    "--format=json1",
                    "--enable=all",
                    "--severity=style",
                    "--",
                ])
                .arg(&file)
                .env_remove("SHELLCHECK_OPTS")
                .output()
                .map_err(|error| format!("cannot run ShellCheck: {error}"))?;
            if !matches!(output.status.code(), Some(0 | 1)) {
                return Err(format!(
                    "ShellCheck failed for {label}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if !output.stderr.is_empty() {
                return Err(format!(
                    "unexpected ShellCheck stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            let parsed: ShellReport = serde_json::from_slice(&output.stdout)
                .map_err(|error| format!("invalid ShellCheck JSON for {label}: {error}"))?;
            if (output.status.success() && !parsed.comments.is_empty())
                || (output.status.code() == Some(1) && parsed.comments.is_empty())
            {
                return Err(format!("ShellCheck status/report mismatch for {label}"));
            }
            diagnostics.extend(parsed.comments.into_iter().map(|item| {
                Diagnostic {
                    level: "error".to_owned(),
                    source: "shellcheck".to_owned(),
                    code: format!("shellcheck::SC{}", item.code),
                    message: item.message,
                    at: Location {
                        file: label.clone(),
                        line: item.line,
                        col: item.column,
                        end_line: item.end_line,
                        end_col: item.end_column,
                        snippet: source
                            .lines()
                            .nth(item.line.saturating_sub(1))
                            .unwrap_or_default()
                            .to_owned(),
                    },
                    fixes: Vec::new(),
                }
            }));
        }
        Ok(report(diagnostics))
    }

    fn collect(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        if path.is_file() {
            files.push(path.to_path_buf());
            return Ok(());
        }
        let mut entries = fs::read_dir(path)
            .map_err(|error| format!("cannot walk {}: {error}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let child = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | "node_modules" | "vendor" | ".venv")
                ) {
                    collect(&child, files)?;
                }
            } else if kind.is_file() {
                let extension = child.extension().and_then(|value| value.to_str());
                if matches!(extension, Some("sh")) {
                    files.push(child);
                } else if extension.is_none() {
                    let bytes = fs::read(&child).map_err(|error| error.to_string())?;
                    let first = bytes
                        .split(|byte| *byte == b'\n')
                        .next()
                        .unwrap_or_default();
                    if matches!(
                        first,
                        b"#!/bin/sh" | b"#!/usr/bin/sh" | b"#!/usr/bin/env sh"
                    ) {
                        files.push(child);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn new_project(name: &str) -> Result<Report, String> {
        if !name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
            || !name
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "-_".contains(ch))
        {
            return Err("name must start with a lowercase letter and contain only lowercase letters, digits, '-' or '_'".to_owned());
        }
        let destination = Path::new(name);
        if destination.exists() {
            return Err(format!("destination already exists: {name}"));
        }
        let staging = PathBuf::from(format!(".{name}.strictsh-tmp"));
        fs::create_dir(&staging).map_err(|error| error.to_string())?;
        let result = write_template(&staging, name);
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        if let Err(error) = fs::rename(&staging, destination) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error.to_string());
        }
        Ok(report(Vec::new()))
    }

    fn write_template(root: &Path, name: &str) -> Result<(), String> {
        for (file, content) in [
            ("main.sh", include_str!("../templates/main.sh")),
            ("tests/smoke.sh", include_str!("../templates/smoke.sh")),
            ("AGENTS.md", include_str!("../templates/AGENTS.md")),
            ("README.md", include_str!("../templates/README.md")),
            (
                "Makefile",
                "check:\n\tstrictsh check .\n\ntest:\n\tsh tests/smoke.sh\n",
            ),
        ] {
            let path = root.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(path, content.replace("__NAME__", name))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn run() -> Result<u8, String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let help = match args.as_slice() {
            [arg] => matches!(arg.as_str(), "--help" | "-h" | "help"),
            [command, arg] => {
                matches!(command.as_str(), "check" | "new")
                    && matches!(arg.as_str(), "--help" | "-h" | "help")
            }
            _ => false,
        };
        if help {
            print!("{}", include_str!("help.txt"));
            return Ok(0);
        }
        if args.first().is_some_and(|arg| arg == "--version") && args.len() == 1 {
            println!("strictsh {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        let value = match args.as_slice() {
            [] => check(Path::new("."))?,
            [command] if command == "check" => check(Path::new("."))?,
            [command, path] if command == "check" => check(Path::new(path))?,
            [command, name] if command == "new" => new_project(name)?,
            [path] if !path.starts_with('-') && path != "new" => check(Path::new(path))?,
            _ => {
                return Err(
                    "usage: strictsh [check [PATH] | new NAME | --help | --version]".to_owned(),
                )
            }
        };
        let stdout = io::stdout();
        let mut output = stdout.lock();
        serde_json::to_writer(&mut output, &value).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
        Ok(if value.ok { 0 } else { 1 })
    }
}

pub use platform::{check, new_project, run, ExitCode};
