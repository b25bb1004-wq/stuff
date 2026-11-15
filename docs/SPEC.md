# Monolith: one computer, built from nothing

Monolith is a complete computing stack written from scratch: the processor, the hypervisor and
kernels that run on it, the language and compiler that target it, the tools around it, and the
network services above it. All 25 "build your own X" systems are layers of one machine, so each
layer is exercised by the layers above it instead of being a separate toy.

## Rules that never bend
1. **Zero external code.** Rust standard library only. No crates from crates.io, no C libraries
   beyond what the OS gives. `tools/check.sh` fails if `Cargo.lock` lists any outside source.
   (The web front end may load animation libraries; nothing else may.)
2. **Portable cores.** Every system keeps its logic in a core that needs only `core` + `alloc`
   where practical, so the same code runs in three places: natively on the host, inside Monolith OS
   on the Monolith CPU, and in a browser tab as WebAssembly.
3. **No `unsafe`** except in `midas`, `pandora`, the kernels and FFI shims, and there every block
   carries a `// SAFETY:` line.
4. **Untrusted input never panics.** Parsers, decoders, network and disk code return `Result`.
5. **A system is done only when** its gate passes: format, clippy with warnings as errors,
   unit tests, property tests from `assay`, and a check the orchestrator runs itself
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

## The 25 systems, and their names
Every layer is named after something from the ground beneath us, chosen for what it does.

| # | Item from the list | Name | Where | What it is, and why the name |
|---|---|---|---|---|
| 1 | Operating system | **Titan** | `machine/titan` | Rust `no_std` kernel + userland on the Monolith CPU. The thick layer everything rides on |
| 2 | Database | **Iridium** | `crates/iridium` | Paged B+tree, write-ahead log, SQL subset, server. A iridium is one year's layer of sediment: a log laid down in order |
| 3 | Virtual machine | **Elysium** | `crates/elysium` | Bytecode VM with garbage collector. A sealed world inside a plain rock |
| 4 | Web server | **Mercury** | `crates/mercury` | HTTP/1.1 server on our own thread pool. Where the rock breaks the surface and the world can see it |
| 5 | Game engine | **Helios** | `crates/helios` | ECS, software renderer, physics, audio. The thing that moves worlds |
| 6 | Compiler | **Cobalt** | `crates/cobalt` | Logos to RV64 machine code and to bytecode. Sintering fuses loose powder into solid metal |
| 7 | Programming language | **Logos** | `crates/logos` | Lexer, parser, type checker, IR. Logos splits into perfect thin layers, like its effect system |
| 8 | Browser | **Odyssey** | `crates/odyssey` | HTML + CSS parser, layout, paint. Spelunking is exploring caves |
| 9 | Blockchain | **Aurum** | `crates/aurum` | Proof-of-stake chain, contracts on Elysium, stored in Iridium. One unbroken aurum of ore |
| 10 | Encryption algorithm | **Sphinx** + **Medusa** | `crates/sphinx` | Sphinx: standard primitives proven on vectors. Medusa: our own cipher. Beautiful, and toxic if handled carelessly: experimental, never the default |
| 11 | CPU emulator | **Silicon** | `crates/silicon` | RV64 emulator with MMU, traps, devices. The crystal that keeps time for every real processor |
| 12 | File system | **Hades** | `crates/hades` | Journaled, checksummed, extent-based. Hades splits into flat pages |
| 13 | Container runtime | **Pandora** | `crates/pandora` | Linux namespaces, cgroups v2, pivot_root. A pandora is a small sealed cavity in rock |
| 14 | Package manager | **Argo** | `crates/argo` | Signed packages, version solver, registry. A stack of stones that marks the trail |
| 15 | Shell | **Krypton** | `crates/krypton` | POSIX-style shell. The outermost shell of the planet |
| 16 | Window manager | **Iris** | `crates/iris` | Compositing window manager. Each window is one face of the cut stone |
| 17 | GUI toolkit | **Indium** | `crates/indium` | Widgets, layout, text, animation. What a polished surface gives back |
| 18 | Text editor | **Calliope** | `crates/calliope` | Rope-based modal editor. The first thing people wrote on |
| 19 | IDE | **Athena** | `web/athena` | Browser IDE: editor + compiler + emulator + debugger. The workshop where stones are cut |
| 20 | Version control | **Chronos** | `crates/chronos` | Content-addressed commits, branches, 3-way merge. A core sample is history you can read layer by layer |
| 21 | Network protocol | **Xenon** | `crates/xenon` | Encrypted reliable transport over UDP. Signals that travel through the earth |
| 22 | Kernel in assembly | **Lithium** | `machine/lithium` | A small kernel in pure RV64 assembly. Small, hard, makes the first spark |
| 23 | Scheduler | **Kairos** | `crates/kairos` | Fair scheduler core + work-stealing pool. Erupts on schedule |
| 24 | Memory allocator | **Midas** | `crates/midas` | Buddy + slab allocator. Where blocks are cut |
| 25 | Hypervisor | **Zeus** | `machine/zeus` | Type-1 hypervisor on the H extension. Nothing sits lower |

Support: **Assay** (`crates/assay`, the test kit: an assay proves what a sample is really made of),
**Chisel** (`crates/chisel`, RV64 assembler and ELF writer), **Pigment** (`crates/pigment`, pixels,
shapes, fonts, PNG output). The whole thing is **Monolith**; the language is **Logos**; its bytecode is MBC.

## Build order (each phase ends with something that runs)
- **P0 Foundation**: workspace, gate, test kit, loop files. DONE 2026-11-14.
- **P1 Cores** (independent, parallel): sphinx, midas, silicon, kairos, hades, logos + elysium.
- **P2 Machine**: chisel, cobalt, silicon privilege + devices, lithium, titan, zeus.
- **P3 Services**: xenon, mercury, iridium, chronos, argo, aurum, pandora, krypton, calliope.
- **P4 Pixels**: pigment, indium, iris, odyssey, helios.
- **P5 Front**: WebAssembly builds, Athena, the Monolith site (motion design), containers, load tests.
- **P6 Hardening**: fuzzing, review agents, crash and fault injection, docs.

## What "deployable at scale" means here
- Services (`mercury`, `iridium`, `aurum`, `argo` registry, `xenon`): one static binary each, a container
  image, a load test with published numbers, and graceful shutdown.
- Machine layers (`silicon`, `titan`, `lithium`, `zeus`): boot in the emulator on every platform and in the
  browser; the emulator passes the official RISC-V tests.
- Tools (`krypton`, `calliope`, `chronos`, `cobalt`, `pandora`): single binaries with their own test suites.
Numbers are published only from runs that were actually made; nothing is claimed in advance.
