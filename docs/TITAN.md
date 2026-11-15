# Titan design (v1, written by the leader, 2026-11-15)

Titan is the Rust `no_std` kernel that runs on the Silicon machine (`crates/silicon`).
Target `riscv64gc-unknown-none-elf`. Only `core`, `alloc` and our own crates (midas, kairos, hades).

## Boot
- Silicon starts the hart at `RAM_BASE = 0x8000_0000` in M-mode. The kernel image is linked there
  (`crates/titan/kernel.ld`, entry `_start`).
- M-mode shim (assembly, at most about 120 lines, `global_asm!`): stack, clear `.bss`, `mtvec`,
  delegate all exceptions except M-ecall and all S interrupts to S-mode (`medeleg`, `mideleg`),
  arm the CLINT timer, set `mstatus.MPP = S`, `mepc = kmain`, `mret`.
- Timer: the shim takes the M timer interrupt, sets the next `mtimecmp` (period 10 ms of `mtime`),
  and raises `SSIP`; the kernel counts ticks in its S software interrupt handler.
- The kernel turns on Sv39 before anything else runs in S-mode code paths that touch the heap.

## Memory map (physical)
| Range | Use |
|---|---|
| `0x0010_0000` | test finisher: write `0x5555` pass, `(code << 16) \| 0x3333` fail |
| `0x0200_0000` | CLINT (`mtimecmp`, `mtime`) |
| `0x0C00_0000` | PLIC (unused in v1, drivers poll) |
| `0x1000_0000` | UART (console), `0x1000_1000` block device, `0x1000_2000` keys, `0x1000_3000` display |
| `0x3000_0000` | framebuffer |
| `0x8000_0000` | kernel text, rodata, data, bss, boot stack (64 KiB) |
| `_end` aligned to 2 MiB, 4 MiB | kernel heap (midas global allocator) |
| after the heap to end of RAM | 4 KiB page frames (bitmap frame allocator) |

## Virtual memory
- Sv39. Kernel table: identity map of all RAM and every MMIO range above, `R W X` as needed, `U = 0`,
  global. Text is `R X`, rodata `R`, data/bss/heap/frames `R W`.
- User space (T26): `0x0000_0000_0001_0000` up to `0x0000_003F_FFFF_F000`; each process has its own
  root table that also contains the kernel mappings (`U = 0`). `sstatus.SUM` stays 0; the kernel copies
  user memory through checked helpers that walk the process table.

## Traps
- One S-mode vector (`stvec`, direct mode). Saves all registers in a trap frame on the kernel stack.
- `scause` decoded to an enum (interrupt or exception, code). Page faults (12, 13, 15) in the kernel
  are fatal except inside the boot self-test, which records them; in user mode they kill the process.

## Syscalls (T26 onward; `ecall` from U, number in `a7`, args `a0..a5`, result in `a0`, negative = error)
| No | Call | | No | Call |
|---|---|---|---|---|
| 0 | exit(code) | | 8 | getpid() |
| 1 | write(fd, ptr, len) | | 9 | yield() |
| 2 | read(fd, ptr, len) | | 10 | sbrk(delta) |
| 3 | open(path_ptr, len, flags) | | 11 | mkdir(path_ptr, len) |
| 4 | close(fd) | | 12 | unlink(path_ptr, len) |
| 5 | spawn(path_ptr, len, argv_ptr, argc) | | 13 | readdir(fd, buf, len) |
| 6 | wait(pid) -> exit code | | 14 | ps(buf, len) |
| 7 | kill(pid) | | 15 | uptime() -> ticks |
Errors: -1 bad argument, -2 not found, -3 no memory, -4 bad fd, -5 exists, -6 not a directory, -7 busy.

## Processes and drivers
- Process: pid, state, root page table, user stack, trap frame, fd table, parent, exit code. The kairos
  scheduler core picks the next process on every timer tick and on `yield`, `wait`, blocking reads.
- Drivers poll in v1: UART (console, fd 0, 1, 2), block device (T27, hades mounted at `/`).
- Userland (T28) is ELF64 images built by our tools, loaded by the kernel's ELF loader.
