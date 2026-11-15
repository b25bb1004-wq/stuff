# sinter

sinter takes a checked Mica program (the IR that the `mica` front end produces) and turns it into
verified Geode bytecode. It can then run `main`, unit tests and property tests on the Geode virtual
machine, or lower the same program to RV64 assembly and an ELF file. It exists so that Mica programs
run on the Strata stack without a second, hand-written implementation of the language.

## Features

- Compiles a checked Mica IR program into a Geode module, with the index of `main`, the unit test
  and property test tables, and a source span for every instruction.
- Optimiser (`sinter::opt::optimize`): constant folding, copy propagation and dead-code removal,
  reporting how many changes it made.
- Runs `main` with a text input and a fuel budget (`run_main`).
- Runs one unit test (`run_test`) or one property test (`run_prop`) with a seed, a case count and fuel.
- Differential tests check every result against the `mica` reference evaluator.
- RV64 backend (`sinter::rv64`): emits RV64IM assembly text or a little-endian ELF64 executable for
  the integer subset of Mica.
- With the default `quartz` feature, `rv64::run` executes the ELF on the Quartz machine for a step limit.
- `mica` binary with `run` and `test` subcommands for a `.mica` file.

## Public API

| Item | What it does |
| --- | --- |
| `compile(&ir::Program) -> Compiled` | Compile checked IR to Geode, with spans and test tables |
| `run_main(&Compiled, input, fuel) -> Outcome` | Run `main` |
| `run_test(&Compiled, index, fuel) -> Outcome` | Run one unit test |
| `run_prop(&Compiled, index, seed, cases, fuel) -> Outcome` | Run one property test |
| `opt::optimize(&mut Program) -> Stats` | Optimise IR in place |
| `rv64::emit_asm(&Program, entry) -> Result<String, CodegenError>` | RV64IM assembly text |
| `rv64::compile_elf(&Program, entry) -> Result<Vec<u8>, CodegenError>` | RV64 ELF64 bytes |
| `rv64::run(&Program, entry, steps)` | Run the ELF on Quartz (feature `quartz`) |

## Demo

```
cargo run -p sinter --example demo
```

Real output:

```
1. parse and check 574 bytes of Mica source
   1 unit test(s), 1 property test(s)
2. optimise: 0 folded, 4 copies, 4 removed
3. compile: 7 Geode functions, main = present
4. run main prints: 24
   main finished without a trap: true
5. unit test "a rectangle": pass
6. property "area is never negative" over 100 seeded cases (seed 1): pass
```

## Tests

```
cargo test -p sinter
```

The gate for this crate is `bash tools/check.sh sinter`.
