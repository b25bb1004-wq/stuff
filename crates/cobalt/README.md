# cobalt

cobalt takes a checked Logos program (the IR that the `logos` front end produces) and turns it into
verified Elysium bytecode. It can then run `main`, unit tests and property tests on the Elysium virtual
machine, or lower the same program to RV64 assembly and an ELF file. It exists so that Logos programs
run on the Monolith stack without a second, hand-written implementation of the language.

## Features

- Compiles a checked Logos IR program into a Elysium module, with the index of `main`, the unit test
  and property test tables, and a source span for every instruction.
- Optimiser (`cobalt::opt::optimize`): constant folding, copy propagation and dead-code removal,
  reporting how many changes it made.
- Runs `main` with a text input and a fuel budget (`run_main`).
- Runs one unit test (`run_test`) or one property test (`run_prop`) with a seed, a case count and fuel.
- Differential tests check every result against the `logos` reference evaluator.
- RV64 backend (`cobalt::rv64`): emits RV64IM assembly text or a little-endian ELF64 executable for
  the integer subset of Logos.
- With the default `silicon` feature, `rv64::run` executes the ELF on the Silicon machine for a step limit.
- `logos` binary with `run` and `test` subcommands for a `.logos` file.

## Public API

| Item | What it does |
| --- | --- |
| `compile(&ir::Program) -> Compiled` | Compile checked IR to Elysium, with spans and test tables |
| `run_main(&Compiled, input, fuel) -> Outcome` | Run `main` |
| `run_test(&Compiled, index, fuel) -> Outcome` | Run one unit test |
| `run_prop(&Compiled, index, seed, cases, fuel) -> Outcome` | Run one property test |
| `opt::optimize(&mut Program) -> Stats` | Optimise IR in place |
| `rv64::emit_asm(&Program, entry) -> Result<String, CodegenError>` | RV64IM assembly text |
| `rv64::compile_elf(&Program, entry) -> Result<Vec<u8>, CodegenError>` | RV64 ELF64 bytes |
| `rv64::run(&Program, entry, steps)` | Run the ELF on Silicon (feature `silicon`) |

## Demo

```
cargo run -p cobalt --example demo
```

Real output:

```
1. parse and check 574 bytes of Logos source
   1 unit test(s), 1 property test(s)
2. optimise: 0 folded, 4 copies, 4 removed
3. compile: 7 Elysium functions, main = present
4. run main prints: 24
   main finished without a trap: true
5. unit test "a rectangle": pass
6. property "area is never negative" over 100 seeded cases (seed 1): pass
```

## Tests

```
cargo test -p cobalt
```

The gate for this crate is `bash tools/check.sh cobalt`.
