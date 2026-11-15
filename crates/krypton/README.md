# krypton

`krypton` is a `no_std` plus `alloc` POSIX shell parser, word expander, arithmetic evaluator, and globber written in safe Rust with zero external dependencies. It separates parsing and expansion from operating system I/O via the `Env` trait, while an optional `std` feature provides the `exec::Shell` execution engine and `krypton` binary.

## Features

- Recursive-descent POSIX shell parser producing `ast::Script` with simple commands, pipelines, `&&`/`||` lists, background `&`, subshells, brace groups, `if`, `while`/`until`, `for`, `case`, functions, redirections, and here-documents.
- Word expansion (`expand` and `expand_one`) supporting tilde, parameter operators (`#`, `-`, `=`, `?`, `+`, `%`, `%%`, `#`, `##`), command substitution, arithmetic expansion, IFS splitting, pathname expansion, and quote removal.
- 64-bit signed integer (`i64`) arithmetic evaluator (`arith`) with wrapping overflow.
- Glob pattern matcher (`glob_match`) supporting `*`, `?`, character classes (`[abc]`, `[a-z]`, `[!x]`, `[^x]`, `[[:class:]]`), and backslash escapes.
- Optional `std` execution engine (`exec::Shell`) that runs scripts with caller-provided `Read` and `Write` streams.
- `#![forbid(unsafe_code)]` across the crate.

## Public API

| Item | Kind | Purpose |
| --- | --- | --- |
| `parse` | Function | Parse a POSIX shell script into `ast::Script` |
| `expand` | Function | Expand words with IFS splitting and pathname globbing |
| `expand_one` | Function | Expand a single word without splitting or globbing |
| `arith` | Function | Evaluate a POSIX arithmetic expression to `i64` |
| `glob_match` | Function | Match a name against a shell glob pattern |
| `Env` | Trait | Environment interface for variables, commands, home, and directory listing |
| `ast` | Module | AST types (`Script`, `AndOr`, `Pipeline`, `Command`, `Simple`, `Word`, `Part`, `Param`, `ParamOp`, `Redir`, `RedirOp`) |
| `ParseError`, `ExpandError` | Types | Error types returned by parsing and expansion |
| `exec::Shell` | Struct (`std`) | Stateful shell runner with `new`, `set_cwd`, `run`, `status`, and `exited` |

## Demo

Run the example:

```sh
cargo run -p krypton --example demo
```

Expected output:

```text
parse: 2 top-level items
expand: ["echo", "hello world", "a.rs", "c.rs"]
expand_one: fallback
arith: 1 + 2 * 3 + COUNT = 10
glob_match: *.rs on lib.rs = true, on README.md = false
shell status: 0
shell out: product=42
shell out: item=one
shell out: item=two
```

## Tests

```sh
cargo test -p krypton
```
