# __NAME__

Standalone POSIX sh glue using the strictsh profile. Requires a POSIX sh implementation,
ShellCheck 0.10.0 and strictsh.

```bash
sh main.sh
make check
make test
```

Install strictsh with `cargo install --git https://github.com/ilvar/strictsh --locked`.
Read `strictsh --help` for the full policy and diagnostic contract.
