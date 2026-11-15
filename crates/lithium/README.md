# lithium

Lithium is a small kernel for the silicon RISC-V machine, written in assembly. It runs two user-mode
tasks, A and B, that each print five lines to the UART. A timer interrupt switches between them,
and each task ends with an `exit` system call. The kernel source is `kernel/kernel.s`, which the
crate assembles with chisel and loads into silicon at `RAM_BASE`. The crate is useful as a worked
example of a guest program that runs on silicon and as a fixed transcript that tests can check.

## Features

- Kernel written in RV64 assembly (`kernel/kernel.s`) and embedded with `include_str!`.
- Assembled at run time by chisel from the embedded source, by `image()`.
- Trap entry saves the current task's frame, then either handles a timer interrupt or a user-mode `ecall`.
- Timer interrupts are re-armed at `mtime + 5000` and pick the other task.
- Two user-mode tasks with their own stacks, started from frames stored in the image.
- System calls: write a line to the UART (`a7 = 1`) and exit (`a7 = 3`).
- Unknown trap causes go to a panic path that prints `lithium: panic`.
- Runs on a default silicon machine; the output is a fixed transcript for the same step budget.

## Public API

| Item | What it does |
|------|--------------|
| `KERNEL_SRC` | The kernel assembly source, as a `&str`. |
| `image()` | Assemble the kernel at `RAM_BASE`; returns the bytes or a `chisel::AsmError`. |
| `boot(config)` | Create a silicon `Machine` with the kernel loaded; returns it or an error string. |
| `run(steps)` | Boot a default machine, run up to `steps`, and return the `Exit` and the UART bytes. |

## Demo

    cargo run -p lithium --example demo

Real output:

```
assembled kernel: 1744 bytes
boot: kernel loaded into a default machine
exit: Pass
uart: 12 line(s)
  | lithium: boot
  | A0
  | A1
  | B0
  | B1
  | A2
  | A3
  | B2
  | B3
  | A4
  | B4
  | lithium: halt
```

The demo is in `examples/demo.rs`. It runs for a fixed budget of 2,000,000 steps and has no clock
or randomness, so the transcript is the same on every run. The `Pass` exit comes from the test
finisher that the kernel writes at halt.

## Tests

    cargo test -p lithium

The tests in `tests/kernel.rs` check that a default run ends with `Exit::Pass` and that the UART
transcript matches `tests/golden.txt` byte for byte. The gate is `bash tools/check.sh lithium` from
the repository root.
