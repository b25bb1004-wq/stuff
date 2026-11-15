# mica

mica is the Strata source language: a small, statically typed language with records,
variants, pattern matching, layers and built-in property tests. This crate is its front
end and reference interpreter. It turns source text into a syntax tree, prints that tree
back as canonical text, checks types and layers, lowers the result to a typed
intermediate representation, and runs it. It exists so that a Mica program can be read,
checked and executed without any other tooling, and so that the same program always gives
the same result.

## Features

- Lexer and recursive-descent parser; the first error is returned as a `Diag` with a span.
- Nesting deeper than 200 levels of expressions, blocks, types or patterns is an error.
- Canonical printer: `print` writes a tree as source text, parenthesising only where needed.
- Type and layer checker: the first error wins; the result is a typed `Program`.
- Reference interpreter with an instruction fuel budget and a 10,000 frame call limit.
- Unit tests (`test "..."` blocks) and seeded property tests (`prop "..." (x: int)`).
- Extern functions are looked up by name; a missing one traps with `NoExtern`.
- `no_std` with the `alloc` crate, `#![forbid(unsafe_code)]`, no external dependencies.

## Public API

| Item | What it does |
| --- | --- |
| `lexer::lex(src)` | Splits source text into tokens. |
| `parser::parse(src)` | Parses a whole source file into an `ast::Module`. |
| `print::print(&module)` | Prints an `ast::Module` as canonical source text. |
| `check::check(&module)` | Type and layer checks a module and lowers it to an `ir::Program`. |
| `eval::run_main(&program, input, fuel)` | Runs `main`, feeding `input` lines to `io.read_line`. |
| `eval::run_test(&program, index, fuel)` | Runs the unit test at `index`. |
| `eval::run_prop(&program, index, seed, cases, fuel)` | Runs a property with a seed and a case count. |
| `eval::Outcome`, `eval::Trap` | Captured output, the result, and the reasons a run stops. |
| `line_col(src, offset)` | Converts a byte offset to a 1-based line and column. |
| `Span`, `Diag` | A byte range in the source, and an error with its span and message. |
| `ast`, `ir` | The syntax tree and the typed intermediate representation. |

## Demo

The demo parses a small program with a type, a function, a `main`, two unit tests and a
property. It then checks it, runs each part and reports a diagnostic for a broken program.
It needs no input and is deterministic.

```
cargo run -p mica --example demo
```

Output:

```
1. parsed 7 items, canonical text has 26 lines
2. checked: 7 functions, 2 tests, 1 properties
3. main output "12", ok = true
4. test "rectangle": pass
4. test "circle": AssertFailed("a circle of radius 2") at bytes 323..375
5. property with seed 42, 100 cases: ok = true
6. broken program rejected at line 2, column 5: unknown function 'print'
```

The "circle" test fails on purpose, to show how a failed assertion is reported.

## Tests

```
cargo test -p mica
```

Unit tests sit beside the code in `src/` and integration tests are in `tests/`, which
includes a corpus of accepted and rejected programs. The gate for this crate is
`bash tools/check.sh mica`.
