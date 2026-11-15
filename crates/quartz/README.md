# quartz

quartz is a RISC-V processor emulator written in plain Rust. It runs 64-bit RISC-V machine code
one instruction at a time, and it can be wrapped in a small board with RAM and simple devices so
that a program can print to a serial port and report pass or fail. It is part of Strata and has no
external crates.

The crate models the RISC-V Privileged Specification (version 20211203), and its doc comments name
the rules each part follows.

## Features

- RV64IMAC hart with Zicsr, the M, S and U privilege modes, and Sv39 paging.
- Traps for illegal instructions, environment calls, breakpoints, misaligned and faulting accesses,
  and page faults, including guest-page faults and virtual-instruction faults.
- Interrupts for machine and supervisor timer, software and external lines.
- The virtualization bit is tracked in the hart.
- Board with RAM at `0x8000_0000`, a test finisher, CLINT, PLIC, UART, a block device, a key FIFO
  and an RGBA framebuffer.
- With `Config::decode_cache` on, `Machine` uses a decoded-instruction cache and an instruction TLB.
- Encoder and decoder for the instruction set, `inst::encode` and `inst::decode`.
- No dependencies outside the standard library and `alloc`.

## Public API

| Item | Purpose |
| --- | --- |
| `Cpu::new`, `Cpu::step`, `Cpu::run` | Create a bare hart; run one instruction, or up to a step limit, over a `Bus` |
| `Cpu` fields `x`, `pc`, `instret`, `mode`, `virt` | Registers, program counter, retired count, privilege, V bit |
| `Bus` (trait), `Fault`, `Ram` | Memory interface, its access errors, and plain RAM at a base address |
| `Trap`, `Step`, `Mode`, `Access` | Trap causes, one-tick outcomes, privilege levels, access kinds |
| `inst::Inst`, `inst::encode`, `inst::decode` | Instruction enum and its conversion to and from 32-bit words |
| `machine::Machine::new`, `load`, `step`, `run` | Board plus hart: load an image, tick, or run until the finisher is written |
| `machine::Config`, `machine::Exit`, `machine::RAM_BASE` | Board settings, run outcome, RAM base address |
| `machine::Board` | Devices: `uart_input`, `take_uart_output`, `press_key`, `framebuffer`, `disk`, `mtime` |

## Demo

Run the demo with:

```
cargo run -p quartz --example demo
```

Real output:

```
addi x1, x0, 5 encodes to 0x00500093
decodes back to the same instruction: true
machine run: Pass after 11 instructions
uart output: "Hi\n"
run ended by pass: true
bare hart: x1 = 5, instret = 1
bare hart: run returned Err(Ecall), is ecall: true
```

The demo encodes a small guest program, runs it on a machine with 1 MiB of RAM until the test
finisher is written, then runs a bare hart on plain RAM until an ECALL traps it.

## Tests

```
cargo test -p quartz
```

Unit tests sit beside the code, and integration tests in `crates/quartz/tests` cover the
instruction groups, CSRs, traps, Sv39 paging, the board devices and the decode cache. A timing
benchmark is in `crates/quartz/examples/bench.rs`. The gate is `bash tools/check.sh quartz`.
