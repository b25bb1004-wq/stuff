# geode

geode is the Strata virtual machine. It defines the bytecode module, a binary format
(MBC1) that stores a module as bytes, a verifier that checks a module before it runs,
and an interpreter that runs verified modules with fuel, heap and call-depth limits.
It exists so that a program written in Strata can run in a sandbox: a module from an
untrusted source is decoded and checked, and cannot run past the limits the host sets.

## Features

- Module model: constants, host imports, functions and an entry function.
- MBC1 binary format: `encode` writes a module to bytes, `decode` reads it back.
- Decoder that reads hostile bytes: it checks every count and length against the input
  and returns an error instead of panicking.
- Verifier: checks register, constant, function, import and jump operands, argument
  counts and that every function ends in `Ret`, `Jump` or `Trap`.
- Interpreter with three limits: fuel, live heap bytes and call depth.
- Mark-and-sweep heap with a byte limit.
- Host functions: the embedder implements the `Host` trait and supplies the imports.
- Traps report why a run stopped and, through `trap_site`, which instruction raised it.
- `no_std` with the `alloc` crate, `#![forbid(unsafe_code)]`, no external dependencies.

## Public API

| Item | What it does |
| --- | --- |
| `Module` | Constants, imports, functions and the entry index. |
| `Func`, `Const` | A function (name, params, regs, code) and a constant pool entry. |
| `Op` | One instruction. |
| `verify(&Module)` | Checks every verifier rule; `Ok` means the module is safe to run. |
| `encode(&Module)` | Writes a module as MBC1 bytes. |
| `decode(&[u8])` | Reads MBC1 bytes into a `Module`; returns `DecodeError` on bad input. |
| `Vm::new(module, host_names)` | Verifies the module and checks every import is provided. |
| `Vm::run(host, limits)` | Runs the entry function with no arguments. |
| `Vm::call(name, args, host, limits)` | Runs a named function with arguments. |
| `Vm::fuel_used`, `Vm::trap_site` | Fuel spent, and where the last trap happened. |
| `Host` trait, `NoHost` | Supplies host functions; `NoHost` traps on every call. |
| `Limits` | Fuel, maximum heap bytes and maximum call depth. |
| `Value`, `Obj`, `Handle`, `Trap` | Runtime values, heap objects and reasons a run stops. |
| `Heap` | The object heap, reachable from the VM and from host calls. |

## Demo

The demo builds a three-function module by hand, encodes and decodes it, then runs it
under several conditions. It needs no input and is deterministic.

```
cargo run -p geode --example demo
```

Output:

```
1. verify: Ok(())
2. encoded 72 bytes, starts with [77, 66, 67, 49]
3. decoded, equal to original: true
4. load without host: missing import 'twice'
5. run main: Int(84), fuel used 7
6. call mul(9, 9): Int(81)
7. run with fuel 2 trapped: OutOfFuel
8. call div: Err(DivideByZero), trap site Some((2, 2))
```

## Tests

```
cargo test -p geode
```

Unit tests sit beside the code in `src/` and integration tests are in `tests/`. The gate
for this crate is `bash tools/check.sh geode`.
