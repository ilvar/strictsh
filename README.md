# strictsh

An opinionated POSIX sh subset and deterministic diagnostic interface for coding
agents, in the same family as [strictpy](https://github.com/ilvar/strictpy) and
[strictrs](https://github.com/ilvar/strictrs).

Portable POSIX sh syntax, mandatory ShellCheck analysis, and additional rules that
keep scripts small, explicit and predictable. Implemented in Rust and checked
with strictrs. Uses tree-sitter-bash to distinguish code from inert text.

Derived from [strictbash](https://github.com/ilvar/strictbash). ShellCheck always
uses `--shell=sh`, including when a script declares another shell. This profile
targets the portable POSIX.1-2017 baseline supported by dash and other `/bin/sh`
implementations. The first statement is `set -u`; `pipefail` and pipelines are
forbidden. Split producers and consumers into individually checked commands.

## Install and use

Install the latest release into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/ilvar/strictsh/main/install.sh -o /tmp/strictsh-install.sh
sh /tmp/strictsh-install.sh
```

The installer selects a Linux or macOS x64/ARM64 binary and checks its SHA-256
before installing. Other architectures and Unix platforms build the **same
latest release tag** with Cargo. Source builds require rustup and a native C
compiler; the script installs the pinned Rust 1.97.1 toolchain via rustup.
ShellCheck remains required on PATH for linting.

Set `STRICTSH_VERSION=0.1.0` (or `v0.1.0`) to install a specific release:

```bash
STRICTSH_VERSION=0.1.0 sh /tmp/strictsh-install.sh
```

Unset, empty, or `latest` selects the latest release. A pinned version skips the
latest-release lookup and applies to both binary downloads and source builds.
Set `STRICTSH_INSTALL_DIR=/your/bin` to change the destination, or
`STRICTSH_BUILD_FROM_SOURCE=1` to force a source build. Download/checksum
failures stop installation and preserve the existing binary; they do not
silently trigger a source build. Binary installation requires a published
release; pinned source builds only require the selected Git tag.

To install from the current development branch instead:

Requires Rust 1.97.1 to build, ShellCheck on PATH (0.10.0 recommended), and a POSIX sh implementation for generated scripts. ShellCheck 0.9.0 is also supported.

```bash
cargo install --git https://github.com/ilvar/strictsh --locked
strictsh --help
strictsh check path/to/project
strictsh path/to/script.sh
strictsh new hello-strictsh
```

Checks never execute input scripts. ShellCheck is mandatory; every severity,
including style and optional checks, fails the gate. ShellCheck rc files and
SHELLCHECK_OPTS cannot weaken it. Inline directives are forbidden.

## Opinionated profile

```bash
#!/bin/sh
set -u

if output=$(curl --fail --silent --show-error 'https://example.com'); then
    printf '%s\n' "${output}"
else
    printf 'request failed\n' >&2 || exit 1
    exit 1
fi
```

Prefer explicit status handling over `set -e`, whose behavior depends on calling
context. Fallible commands, tests and substitution assignments need
`if`/`elif`, `while`/`until`, or an explicit handler such as `|| exit 1`.

| Stable code | Policy |
| --- | --- |
| `strictsh::sh_shebang` | Require a POSIX sh shebang. |
| `strictsh::required_options` | Start with exactly `set -u`. |
| `strictsh::fixed_options` | No further `set` or `shopt`, including `set -e`. |
| `strictsh::explicit_status` | Consume fallible command/test/assignment status. |
| `strictsh::no_dynamic_code` | No eval, sourcing, string traps, inline shell code or dispatch wrappers. |
| `strictsh::no_dynamic_command` | Command names must be literal. |
| `strictsh::no_indirection` | No indirect expansion or namerefs. |
| `strictsh::no_backticks` | Use `$(...)`. |
| `strictsh::no_process_substitution` | No hidden producer status in `<(...)` / `>(...)`. |
| `strictsh::no_pipeline` | No pipelines, including guarded pipelines: portable sh hides producer failures. |
| `strictsh::posix_syntax` | Reject Bash arrays, extended tests, declarations and function syntax. |
| `strictsh::no_background` | No asynchronous `&` jobs. |
| `strictsh::no_masked_status` | Substitute only in standalone, checked assignments, once per assignment. |
| `strictsh::no_star_args` | Preserve arguments with `"${@}"`. |
| `strictsh::prefer_printf` | Use printf instead of echo. |
| `strictsh::no_suppression` | Repair diagnostics; no ShellCheck directives. |
| `strictsh::syntax_error` | Invalid/unsupported shell syntax. |

The initial profile deliberately targets standalone glue scripts. Source
libraries, traps, background orchestration and shell-dispatch wrappers are
outside it. Move complex work into a strictrs Rust utility.

Status checks are syntactic, not a proof of runtime correctness. Output/control
builtins (`printf`, `:`, `true`, `false`, `return`, `exit`, `break`, `continue`)
are exempt unless redirected. Function bodies still need their own handling.
See the embedded manual for exact behavior, limitations and examples.

## Diagnostic contract

One JSON report on stdout, with diagnostics sorted by file, line, column, code
and message. Relative paths and 1-based character spans, exclusive end positions.
Sources are `strictsh` and `shellcheck`; ShellCheck codes use `shellcheck::SC…`.
All diagnostics are promoted to errors. `fixes` is empty in this check-only
release; existing scripts are never rewritten.

```json
{"ok":true,"error_count":0,"warning_count":0,"diagnostics":[]}
```

Exit `0` means clean, `1` means diagnostics remain, `2` means invocation or
operational failure. Operational errors go to stderr with empty stdout. Help
and version print plain text. No scripts found fails instead of claiming clean.

Directory checks find `.sh` and extensionless sh-shebang scripts;
explicit files work with any extension. Ignore `.git`, `target`, `node_modules`,
`vendor`, `.venv`, and nested symlinks. Unreadable files fail the operation.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
strictrs check .
cargo run --locked -- check templates
```

CI installs strictrs from a pinned commit and runs the same gate. Dependencies
are exact-pinned and Cargo.lock is committed. Property tests exercise inert-text
handling and determinism; integration tests cover discovery, real ShellCheck,
CLI failures, multiple diagnostics and generated-project execution.

CI also builds and smoke-tests native static Linux binaries for
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, plus macOS
`x86_64-apple-darwin` and `aarch64-apple-darwin`, on native runners. Every successful run uploads both binaries with individual `.sha256`
files. After the checks and both builds pass on `main`, CI creates a GitHub
release tagged `v<manifest version>`. Existing releases are left unchanged;
bump the package version and Cargo.lock to publish a new release. Manual
workflow dispatch can retry publication. Pull requests never publish releases.

Installer tests use isolated command stubs to cover both architectures, exact
release-tag source fallback, checksums, failures, cleanup and custom paths.

This release implements the checker, policy and project generator. Conservative
fixes and portable agent-skill installation are future work.
