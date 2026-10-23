# __NAME__

Read `strictsh --help` before changing scripts. Run `strictsh check .`
and `make test` before committing. Keep POSIX sh as small, standalone glue.
Use explicit status handling, fixed shell options, literal command names,
quoted arguments and printf. Do not suppress diagnostics or execute untrusted
scripts as part of linting. Move complex logic to Rust under strictrs.
