# athena

`athena` is the browser IDE shell for the Logos language and the Titan target. It is a Rust library that exposes the editor services (check, highlight, format, test runner, definition and hover, and a step debugger) and a `wasm32` build that runs them in the browser. It also ships a `pack` step that builds the static web bundle, and an `athena` command-line tool that runs the same checks from a terminal.

## Features

- `service::check` returns parser and type diagnostics with byte spans for a Logos source string
- `service::highlight` returns styled spans for the editor, built on `calliope::Style`
- `service::run_tests` runs `test` blocks and `prop` blocks under a fuel limit and reports per-test results
- `lang::format`, `lang::definition` and `lang::hover` provide formatting and editor navigation
- `debug::Debugger` steps a program with breakpoints and exposes frames and variables as `State`
- `pack::pack` (non-`wasm32` only) writes the web bundle: the wasm module, the Titan ELF image, HTML, CSS, JavaScript and the service worker
- `athena` binary with `check`, `fmt` and `test` subcommands; `fmt --write` rewrites the file in place

## Public API

| Item | Role |
| --- | --- |
| `service::check` | Parse and type-check source, returning `Diagnostic` values |
| `service::highlight` | Syntax-highlight spans as `(start, end, Style)` |
| `service::run_tests` | Run `test` and `prop` blocks with a fuel budget, returning `TestResult` values |
| `service::Diagnostic`, `service::TestResult` | Result types for check and test runs |
| `lang::format`, `lang::definition`, `lang::hover` | Formatter and editor navigation helpers |
| `debug::Debugger` | Step-by-step debugger: `start`, `step`, `step_budget`, `select`, `state` |
| `debug::State`, `debug::Status`, `debug::Step` | Debugger state, run status and step modes |
| `pack::pack` | Write the web bundle to an output directory (not on `wasm32`) |
| `wasm` module | `wasm32`-only bindings used by the browser build |

## Build and run

Build the web bundle. The output goes to `target/athena-web`:

```
cargo run -q --release -p athena --example pack
```

Run the command-line tool. Its help text is:

```
athena - Logos checker, formatter, and test runner

USAGE:
    athena <SUBCOMMAND> [OPTIONS] <FILE.logos | ->

SUBCOMMANDS:
    check FILE.logos           Parse and type-check Logos source
    fmt FILE.logos [--write]   Format Logos source (refuses files with comments)
    test FILE.logos            Run unit tests and property tests

OPTIONS:
    -h, --help                 Print this help message
    -V, --version              Print version information
```

Run it through Cargo:

```
cargo run -q --release -p athena --bin athena -- --help
```

## Demo

Run the test and property blocks of a Logos file from the corpus:

```
athena test crates/logos/tests/corpus/16_tests_and_props.logos
```

Real output (exit code 0):

```
PASS double basic
PASS double is even
PASS identity
summary: 3 passed, 0 failed
```

The file defines `double`, one `test` block with two asserts, and two `prop` blocks over bounded and unbounded integers.

## Tests

```
cargo test -p athena
```
