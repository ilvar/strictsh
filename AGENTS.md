# strictsh

Implement portable POSIX sh analysis, using ShellCheck --shell=sh for semantic diagnostics and
tree-sitter-bash for syntax-aware policy. Keep Rust within the strictrs profile.
Do not execute scripts while linting, silently suppress diagnostics, invent
fixes, or add escape hatches that weaken the mandatory default profile.

All effects belong in the marked capability module. No unsafe, panicking APIs
or unchecked indexing in production, numeric `as` casts or glob imports.
Public functions need explicit return types. Handle all must-use values.

The JSON schema, stable codes, help text, ordering and exit statuses are public
contracts. Update focused tests and documentation with every behavior change.
Test parsed syntax, including inert text, nested statements and failure paths.

Before committing and pushing:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
strictrs check .
cargo run --locked -- check templates
cargo run --locked -- check install.sh
```

Rust is pinned to 1.97.1. Runtime dependencies are exact-pinned; commit Cargo.lock.
strictrs CI uses revision cf1142b2c73909a3ba2258f32f69a6a72a82cb1c.
Changes must pass local validation before publishing. Do not use CI to discover
known local failures. Keep changes coherent and inspect the staged diff.

The installer must pass strictsh and ShellCheck --shell=sh without suppression. Keep its
Rust toolchain pin synchronized with rust-toolchain.toml. Preserve exact-tag
source fallback, checksum failure behavior and cleanup regression tests.
CI publishes native Linux/macOS x64/ARM64 builds only after every gate passes;
Linux binaries are static MUSL builds. Keep macOS checksum verification portable.
Bump Cargo.toml and Cargo.lock together for a new release; published releases
must never be overwritten.
