# Strata: one computer, built from nothing

Strata is a complete computing stack written from scratch: the processor, the hypervisor and
kernels that run on it, the language and compiler that target it, the tools around it, and the
network services above it. All 25 "build your own X" systems are layers of one machine, so each
layer is exercised by the layers above it instead of being a separate toy.

## Rules that never bend
1. **Zero external code.** Rust standard library only. No crates from crates.io, no C libraries
   beyond what the OS gives. `tools/check.sh` fails if `Cargo.lock` lists any outside source.
   (The web front end may load animation libraries; nothing else may.)
2. **Portable cores.** Every system keeps its logic in a core that needs only `core` + `alloc`
   where practical, so the same code runs in three places: natively on the host, inside Strata OS
   on the Strata CPU, and in a browser tab as WebAssembly.
3. **No `unsafe`** except in `mem`, `cage`, the kernels and FFI shims, and there every block
   carries a `// SAFETY:` line.
4. **Untrusted input never panics.** Parsers, decoders, network and disk code return `Result`.
5. **A system is done only when** its gate passes: format, clippy with warnings as errors,
   unit tests, property tests from `strata-testkit`, and a check the orchestrator runs itself
   (published test vectors, a differential test, or a fault-injection run).
6. **Honest labels.** Anything experimental (above all the home-made cipher) says so in its docs
   and is never the default for real security.

## The machine
- ISA: RISC-V RV64IMAC + Zicsr, M/S/U privilege modes, Sv39 paging, later the H extension.
  A real, documented ISA means our emulator can be proven against the official test suite, our
  kernels can also boot on other RISC-V machines, and Rust can compile our OS code for it.
- Devices: UART, timer (CLINT), interrupt controller (PLIC), block device, framebuffer,
  keyboard and pointer, network card.
- Display: the framebuffer is shown on a browser canvas (emulator compiled to WebAssembly) and
  can be dumped to an image file for golden-image tests.

## The 25 systems
| # | Item from the list | Crate / folder | What it is in Strata |
|---|---|---|---|
| 1 | Operating system | `machine/os` | Strata OS: Rust `no_std` kernel + userland on the Strata CPU |
| 2 | Database | `crates/db` | Paged B+tree store, write-ahead log, SQL subset, server |
| 3 | Virtual machine | `crates/vm` | Bytecode VM with garbage collector for the Mica language |
| 4 | Web server | `crates/httpd` | HTTP/1.1 server on our own thread pool, serves the web front |
| 5 | Game engine | `crates/engine` | ECS, software renderer, physics, audio mixer, on the framebuffer |
| 6 | Compiler | `crates/micac` | Mica to RV64 machine code and to VM bytecode |
| 7 | Programming language | `crates/mica` | Mica: lexer, parser, type checker, IR |
| 8 | Browser | `crates/browser` | HTML + CSS parser, layout, paint, to the framebuffer |
| 9 | Blockchain | `crates/chain` | Proof-of-stake chain, contracts run on the VM, stored in `db` |
| 10 | Encryption algorithm | `crates/crypto` | Standard primitives proven on vectors + `strand`, our own experimental cipher |
| 11 | CPU emulator | `crates/cpu` | RV64 emulator with MMU, traps and devices |
| 12 | File system | `crates/fs` | Journaled, checksummed, extent-based file system + mkfs/fsck |
| 13 | Container runtime | `crates/cage` | Linux namespaces, cgroups v2, pivot_root (tested in WSL) |
| 14 | Package manager | `crates/pkg` | Signed packages, version solver, registry served by `httpd` |
| 15 | Shell | `crates/sh` | POSIX-style shell, on the host and inside Strata OS |
| 16 | Window manager | `crates/wm` | Compositing window manager on the framebuffer |
| 17 | GUI toolkit | `crates/gui` | Retained-mode widgets, layout, text, animation |
| 18 | Text editor | `crates/edit` | Rope-based modal editor: terminal, Strata OS and web |
| 19 | IDE | `web/ide` | Browser IDE: editor + compiler + emulator + debugger in WebAssembly |
| 20 | Version control | `crates/vcs` | Content-addressed commits, branches, 3-way merge, sync |
| 21 | Network protocol | `crates/net` | Encrypted reliable transport over UDP with streams |
| 22 | Kernel in assembly | `machine/akern` | A small kernel in pure RV64 assembly, built by our assembler |
| 23 | Scheduler | `crates/sched` | Fair scheduler core (used by the OS) + work-stealing pool (used by `httpd`) |
| 24 | Memory allocator | `crates/mem` | Buddy + slab allocator, usable as the Rust global allocator |
| 25 | Hypervisor | `machine/hv` | Type-1 hypervisor on the H extension, runs several guest kernels |

Support crates: `testkit` (test helpers), `asm` (RV64 assembler and ELF writer), `gfx` (pixels,
shapes, fonts, PNG output).

## Build order (each phase ends with something that runs)
- **P0 Foundation**: workspace, gate, test kit, loop files. DONE 2026-11-14.
- **P1 Cores** (independent, parallel): crypto, mem, cpu, sched, fs, mica + vm.
- **P2 Machine**: asm, micac, cpu privilege + devices, akern, os, hv.
- **P3 Services**: net, httpd, db, vcs, pkg, chain, cage, sh, edit.
- **P4 Pixels**: gfx, gui, wm, browser, engine.
- **P5 Front**: WebAssembly builds, IDE, the Strata site (motion design), containers, load tests.
- **P6 Hardening**: fuzzing, review agents, crash and fault injection, docs.

## What "deployable at scale" means here
- Services (`httpd`, `db`, `chain`, `pkg` registry, `net`): one static binary each, a container
  image, a load test with published numbers, and graceful shutdown.
- Machine layers (`cpu`, `os`, `akern`, `hv`): boot in the emulator on every platform and in the
  browser; the emulator passes the official RISC-V tests.
- Tools (`sh`, `edit`, `vcs`, `micac`, `cage`): single binaries with their own test suites.
Numbers are published only from runs that were actually made; nothing is claimed in advance.
