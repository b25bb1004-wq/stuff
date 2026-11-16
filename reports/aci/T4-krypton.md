# T4-krypton report

- Tool converted: krypton (crates/krypton). New src/json_mode.rs, hook in src/main.rs (--describe and --json run before the text path), tests/aci.rs (11 tests). Cargo.toml: aci optional, `std = ["dep:aci"]`; Cargo.lock gains the krypton to aci edge.
- Commands: `-c SCRIPT [ARG0 ARGS...]` (effect exec), `FILE [ARGS...]` (exec), `interactive` (exec, always unsupported under --json). `--json -- FILE` also runs a file.
- Gate: `bash tools/check.sh krypton` ends `CHECK PASS krypton`. `cargo build -p krypton --no-default-features --lib` also builds.
- unsupported: `krypton --json` with no script (interactive shell), hint `run: krypton --json -c SCRIPT`, exit 1.
- usage (exit 2, cmd ""): unknown option (`--bogus`, `-`); `--json -c` without a script.
- Not done or open: truncated output carries no hint (krypton has no flag to read the rest); `--help` and `--version` under --json are usage errors; a syntax error under --json exits 1 (text path exits 2); the `-` form (script from stdin) is not offered under --json.
- TDD: tests were written before the code, but the first gate run was after implementation, so no red run was observed.
