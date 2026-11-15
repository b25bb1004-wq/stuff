# Strata architecture book

This page explains how the Strata repository is put together: what each crate does, what it
depends on, where it stands, and how a change is checked before it counts as done. It is a map,
not a manual. Where this page is unsure of a detail, it says "see src" and points at the code.

Facts here come from `docs/PLAN.md`, `docs/SPEC.md`, the board in `LOOP.md`, the first 40 lines of
each `src/lib.rs`, and each `Cargo.toml`. If this page and those files disagree, they win.

## 1. What Strata is

Strata is one computing stack written from nothing but the Rust standard library. There is no
crate from crates.io anywhere in it. The stack starts with a RISC-V processor model (`quartz`),
runs small kernels on that model, and builds a language, compiler, virtual machine, database,
web server, network protocol, version control, package manager, shell, editor, graphics stack,
browser and game engine on top.

The idea from `docs/SPEC.md` is that each of the 25 "build your own X" systems is a layer of one
machine, so each layer is exercised by the layers above it. Each system must also work alone.

Three rules from `docs/SPEC.md` shape every crate:

1. Zero external code. `tools/check.sh` fails if `Cargo.lock` lists any outside source.
2. Portable cores. Logic lives in `core` plus `alloc` where practical, so the same code can run
   on the host, on the Strata CPU, or in a browser as WebAssembly.
3. Untrusted input never panics. Parsers and decoders return `Result`.

Only `quarry`, `vug`, the kernels and FFI shims may use `unsafe`, and each `unsafe` block must
carry a `// SAFETY:` line. Every crate's `lib.rs` has `#![forbid(unsafe_code)]` except `quarry`,
which has `#![deny(unsafe_op_in_unsafe_fn)]` instead. Whether `quarry` uses `unsafe` is for src
to answer.

## 2. Layers

The stack is read from the bottom up. Each band uses the one below it.

```text
+--------------------------------------------------------------------------------+
| APPS AND FRONT END                                                             |
|   slate (editor)  glint (widgets)  pigment (pixels)  spelunk (browser)         |
|   tectonic (games)  facet (window manager, not built)  lapidary and site (no crate yet) |
+--------------------------------------------------------------------------------+
| SERVICES, TOOLS AND LANGUAGE                                                   |
|   outcrop (HTTP)  tremor (transport)  varve (database)  drillcore (git-style)  |
|   cairn (packages)  seam (chain)  vug (containers, not built)  crust (shell)   |
|   mica (language)  geode (VM)  sinter (compiler)  shale (file system)          |
|   onyx (crypto)  assay (test kit, used by every crate)                         |
+--------------------------------------------------------------------------------+
| KERNEL AND CORE SERVICES                                                       |
|   flint (kernel in RV64 assembly, built)  mantle (Rust kernel, not built)     |
|   bedrock (hypervisor, not built)  quarry (allocator)  geyser (scheduler)     |
+--------------------------------------------------------------------------------+
| HARDWARE MODEL (software emulation)                                            |
|   quartz (RV64IMAC CPU, Zicsr, M/S/U modes, Sv39 paging, devices)            |
|   chisel (RV64 assembler and ELF64 writer, produces code for quartz)          |
+--------------------------------------------------------------------------------+
```

Notes on the diagram:

- "Hardware model" is an emulator, not silicon. `docs/PLAN.md` section 10 lists real hardware
  drivers as out of scope.
- `flint` is the only kernel in `crates/` that has passed its gate. `mantle` and `bedrock` are named
  in `docs/SPEC.md` under `machine/` and have no crate here yet.
- `assay` is a dev-dependency of almost every crate, so it is not part of the shipped stack.

## 3. Crates

The repository has 25 crates under `crates/`. The list below follows the layers above. Each
section gives what the crate does, its main public items as seen in `src/lib.rs`, the crates it
depends on in `Cargo.toml` (non-dev dependencies first; `assay` is a dev-dependency unless noted),
and its status from `LOOP.md`.

### 3.1 Hardware model and tooling

#### quartz

- What it does: RV64IMAC hart with Zicsr, M/S/U modes, traps, interrupts and Sv39 paging, following
  the RISC-V Privileged Specification 20211203. It is the CPU model that the kernels run on.
- Main public items: `Trap`, `Bus`, `Fault`, `Ram`, and the modules `inst` and `machine`
  (`Machine`, `Config`, `Exit`, `RAM_BASE` as used by `flint`). See src for the rest.
- Depends on: no non-dev crates (`assay` in dev only).
- Status: PASS. Rows T03, T12, T13, T13a, T23, T31, T33 (RV64IM, privilege, official tests, A and C
  extensions, devices, hypervisor extension, decoded-block cache).

#### chisel

- What it does: RV64IMAC assembler that produces flat binary machine code matching LLVM 19, plus
  an ELF64 writer and loader.
- Main public items: `Output` (bytes and symbols), `AsmError` (line number and message), the
  `assemble` function (used by `flint`), and the public module `elf`. See src for the rest.
- Depends on: `quartz` (non-dev). `assay` in dev.
- Status: PASS. Rows T21 (assembler), T22 (ELF writer and loader).

### 3.2 Kernel and core libraries

#### flint

- What it does: a two-task RV64 kernel written in assembly and run on `quartz`. The kernel source
  is `kernel/kernel.s`, read at compile time with `include_str!`.
- Main public items: `KERNEL_SRC`, `image()` (assembled bytes), `boot(Config)` (a machine with the
  kernel loaded), `run(steps)` (returns the exit and UART output).
- Depends on: `quartz`, `chisel` (non-dev). `assay` in dev.
- Status: PASS. Row T24.

#### quarry

- What it does: buddy page allocator, a size-class slab heap, and a spin-locked `GlobalAlloc`
  wrapper. Everything works inside one memory region the caller supplies. It does not ask the OS
  for memory.
- Main public items: `Buddy`, `Heap`, `Locked`, the constants `PAGE` (4096 bytes) and `MAX_ALIGN`
  (4 MiB). See src for the methods.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T02, T16.

#### geyser

- What it does: deterministic fair scheduler core for many CPUs. The `pool` module adds a
  work-stealing thread pool on std threads and is compiled only with the `std` feature.
- Main public items: `Scheduler`, `Config`, `Class`, `Decision`, `Error`, `State`, `Stats`,
  `TaskId`. See src for the methods.
- Depends on: no non-dev crates. `assay` in dev. `outcrop` uses `geyser` as an optional dependency.
- Status: PASS. Rows T05 (scheduler core and simulator), T15 (thread pool).

### 3.3 Language, compiler and virtual machine

#### mica

- What it does: front end for the Mica language. It has a lexer, a syntax tree, a recursive-descent
  parser, a canonical printer, a type and layer checker, and a typed intermediate representation.
  An evaluator (`eval`) is also in the crate.
- Main public items: modules `ast`, `check`, `eval`, `ir`, `lexer`, `parser`, `print`; `Span`,
  `Diag`, `line_col`. See src for the rest.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T08 (design, see `docs/MICA.md`), T09 (parser and printer), T10 (checker and
  IR), T19 (reference interpreter for the typed IR).

#### geode

- What it does: the data model, verifier, binary format (MBC1) and interpreter for the Strata
  virtual machine. The format is described in `docs/MBC.md`. The VM has a heap with a mark-sweep
  garbage collector, fuel and heap limits, and a `Host` trait for calls out of the VM.
- Main public items: `decode`, `encode`, `DecodeError`, `Heap`, `Module`, `Const`, `Func`, `Op`,
  `Handle`, `Obj`, `Trap`, `Value`, `verify`, `VerifyError`, `Host`, `Limits`, `LoadError`,
  `NoHost`, `Vm`. See src for the rest.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T11 (module, verifier, binary format), T17 (interpreter, heap, GC, fuel).

#### sinter

- What it does: compiles Mica IR to Geode bytecode and runs the result. It re-exports `run_main`,
  `run_test` and `run_prop`, and has a public `rv64` module. The `Cargo.toml` declares a `[[bin]]`
  target, which is the command line tool (see `Cargo.toml` for its name).
- Main public items: `compile`, `run_main`, `run_test`, `run_prop`, `Compiled`, `CompiledProp`,
  and module `rv64`. See src for the rest.
- Depends on: `chisel`, `geode`, `mica` (non-dev); `quartz` (optional). `assay` in dev.
- Status: PASS for T18 (Mica IR to Geode bytecode). T29 (IR to RV64 code, row TODO; the
  integer-subset row T29a is RUNNING) and T30 (optimisations, TODO) are not done.

### 3.4 Storage, network and services

#### onyx

- What it does: the cryptography crate. It has the standard primitives, each checked against
  published vectors: SHA-256 and SHA-512, HMAC, HKDF, ChaCha20, Poly1305, the ChaCha20-Poly1305 AEAD,
  X25519, Ed25519, a constant-time compare, a random generator, and a sealed-box helper. It also has
  `cinnabar`, the home-made experimental cipher from `docs/CINNABAR.md`, which is labelled
  experimental and is never the default.
- Main public items: `Error` (`BadTag`, `BadLength`, `WeakKey`, `BadSignature`), and modules `aead`,
  `chacha20`, `cinnabar`, `ct`, `ed25519`, `hkdf`, `hmac`, `poly1305`, `rng`, `sealed`, `sha256`,
  `sha512`, `x25519`.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T01 (SHA-2, HMAC, HKDF, ChaCha20-Poly1305), T04 (X25519, Ed25519), T14
  (Cinnabar, labelled experimental), T20 (ChaCha20 generator, sealed box).

#### shale

- What it does: the file system format. Version 1 on-disk layout, `mkfs`, mount, and path-based
  file and directory operations. Every operation writes through one private transaction and a
  write-ahead journal, so a power cut leaves either the old state or the new one. `fsck` checks a
  device for consistency.
- Main public items: `BLOCK` (4096 bytes), `BlockDevice`, `CrashDisk`, `MemDisk`, `mkfs`, `Fs`,
  `fsck`, `Report`, `Error`. See src for the rest.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T06 (block layer and format), T07 (journal, fsck, crash injection).

#### varve

- What it does: transactional 4 KiB pager with a redo write-ahead log and crash recovery, a B+tree
  with range scans, and a SQL subset with a server and client. The crate uses `std`; it is not
  `no_std`.
- Main public items: `Pager`, `Snapshot`, `CrashStorage`, `FileStorage`, `MemStorage`, `Storage`,
  `Tree`, `PAGE`, `USABLE` (4092 bytes before the CRC), `Page`, `Error`. Modules `net` and `sql`.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T39 (pager and WAL), T40 (B+tree), T41, T41a, T41b (SQL, core and joins with
  EXPLAIN), T42 (server and client).

#### outcrop

- What it does: HTTP/1.1 parser and writer with no I/O inside, and (with the `std` feature) a server
  on a thread pool with keep-alive, static files, routes, limits and graceful stop. A `ws` module
  covers WebSocket.
- Main public items: `parse_request`, `write_request`, `write_response`, `Error`, `Limits`,
  `Request`, `Response`. Modules `server` (std only) and `ws`.
- Depends on: `geyser` (optional). `assay` in dev.
- Status: PASS. Rows T36 (parser and writer), T37 (server), T38 (WebSocket, ranges, chunked bodies).
  Soak row T79a also PASS.

#### tremor

- What it does: encrypted, reliable, multi-stream transport protocol core. It is sans-I/O: the core
  does not touch sockets. A UDP layer sits on top (`udp` module).
- Main public items: `Connection`, `Server`, `MAX_DATAGRAM` (1200 bytes), `Error`, and the re-export
  of `udp::*`. See src for the rest.
- Depends on: `onyx` (non-dev). `assay` in dev.
- Status: PASS. Rows T34 (protocol core), T35 (UDP transport and tools).

#### drillcore

- What it does: a version control core in the style of Git with SHA-256 object ids. It stores
  objects, refs, an index and commits, and supports branches and checkout. It also has a diff
  (Myers), a 3-way merge and status. The `remote` module (std only) does push and pull over HTTP.
- Main public items: `diff`, `lines`, `unified`, `Edit`, `decode_commit`, `decode_tree`,
  `encode_commit`, `encode_tree`, `Commit`, `Mode`, `Signature`, `TreeEntry`, `from_hex`,
  `hash_object`, `to_hex`, `Error`, `Id`, `Kind`, `merge3`, `MergeError`, `Merged`, `Repo`,
  `Change`, `DirStore` (std), `Files`, `MemStore`, `Store`.
- Depends on: `onyx`, and `outcrop` (optional, for the remote part). `assay` in dev.
- Status: PASS. Rows T43 (objects, refs, commit, branch, checkout), T44 (diff and merge, status),
  T45 (push and pull).

#### cairn

- What it does: package manager core. It has a manifest, versions, a dependency solver and a lock
  file. The `registry` and `client` modules are the registry and client side. Signed packages
  use Ed25519 (T47 row).
- Main public items: modules `client`, `lock`, `manifest`, `pkg`, `registry`, `solve`, `version`.
  See src for the types inside them.
- Depends on: `onyx`, `outcrop` (non-dev). `assay` in dev. The `Cargo.toml` does not say what
  each one is used for; see src.
- Status: PASS. Rows T46 (manifest, solver, lock file), T47 (registry, signed packages, install).

#### seam

- What it does: blockchain core. It has blocks, a Merkle tree (RFC 6962 style), signed transfers,
  account state and chain validation. Contracts run on `geode` with fuel as gas (per the T49 row).
  A public `node` module exists for the network side (see src).
- Main public items: `Block`, `Header`, `Chain`, `contract_address`, `Receipt`, `DEPLOY`,
  `FUEL_PER_FEE`, `merkle_proof`, `merkle_root`, `merkle_verify`, `Tx`, the types `Hash` and
  `Address` (32 bytes each), `MAX_DATA` (1024 bytes), `MAX_TXS` (1000), and `DecodeError`.
- Depends on: `geode`, `mica`, `onyx`, `sinter`, `tremor` (non-dev). `assay` in dev.
- Status: PASS for T48 (blocks, Merkle tree, transactions, validation), T49 (contracts, `chain`
  layer), T50a (block pool, fork choice, reorgs, mempool). T50b (gossip over `tremor`) is RUNNING.
  T50 (the full node) is TODO.

#### vug

- What it does: nothing yet. `src/lib.rs` says "Not built yet". The plan is a Linux container
  runtime (namespaces, cgroups v2, pivot_root) that runs in WSL.
- Main public items: none.
- Depends on: `assay` in dev only.
- Status: TODO. Rows T51 (container runtime) and T52 (images, run, ps, kill).

#### crust

- What it does: POSIX-style shell. The crate is `no_std` with `alloc`. It contains a parser, a word
  expander, an arithmetic evaluator and a globber. The `exec` module (std feature) runs commands,
  pipes, redirects, builtins and job control.
- Main public items: `parse`, `expand`, `expand_one`, `arith`, `glob_match`, `Env`, `ExpandError`,
  `ParseError`, module `ast`, and `exec` with the feature `std`.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T53 (shell language), T57 (executor, pipes, redirects, builtins, job control).

### 3.5 Editor, graphics and front end

#### slate

- What it does: modal text editor. The buffer is a piece table, the editor has an undo tree and
  multiple cursors, and there is a regex engine written for the crate (linear time, per the crate
  doc). The terminal screen and key decoding are in their own modules. Mica highlighting is built in.
- Main public items: `App`, `Mode`, `Buffer`, `Cursor`, `highlight`, `Style`, `decode_keys`,
  `parse_keys`, `Key`, `Regex`, `RegexError`, `Screen`, `ansi`, `Editor`.
- Depends on: `mica` (non-dev). `assay` in dev.
- Status: PASS. Rows T54 (buffer, undo tree, regex), T55 (terminal interface, key maps, highlighting).

#### pigment

- What it does: pixels. A framebuffer (`Canvas`), lines, rectangles, anti-aliased polygons and
  source-over blending. It also has DEFLATE, zlib, CRC-32 and Adler-32, PNG read and write, a
  bitmap font, and text layout with wrapping. The crate doc says no input panics and no allocation
  grows with coordinates.
- Main public items: `Canvas`, `Rect`, `Rgba`, `over`, `deflate`, `inflate`, `zlib_compress`,
  `zlib_decompress`, `crc32`, `adler32`, `FlateError`, `glyph`, `GLYPH_W`, `GLYPH_H`, `encode_png`,
  `decode_png`, `PngError`, `layout`, `measure`, `Line`.
- Depends on: no non-dev crates. `assay` in dev.
- Status: PASS. Rows T58 (framebuffer, lines, polygons), T59 (inflate, deflate, PNG), T60 (font, text
  layout).

#### glint

- What it does: GUI toolkit core. A retained widget tree (`Ui`) with flex layout, focus, events and
  software rendering. Widgets and animation (`anim` module, `widgets` module) are in their own
  modules. Colours and rectangles come from `pigment`.
- Main public items: `Ui`, `Action`, `Align`, `Dir`, `Event`, `Insets`, `Justify`, `Key`, `Kind`,
  `NodeId`, `Style`, `Theme`, and re-exports `Canvas`, `Rect`, `Rgba` from `pigment`.
- Depends on: `pigment` (non-dev). `assay` in dev.
- Status: PASS. Rows T61 (widget tree, flex layout, events, focus, themes), T62 (widgets, spring and
  ease animation).

#### facet

- What it does: nothing yet. `src/lib.rs` says "Not built yet". The plan is a compositing window
  manager with tiling and floating windows and animated moves.
- Main public items: none.
- Depends on: `assay` in dev only.
- Status: TODO. Rows T63 (compositor) and T64 (client protocol and apps).

#### spelunk

- What it does: browser core. It has an HTML5 tokenizer and tree builder, a CSS Syntax 3 parser, a
  Selectors Level 4 matcher, style, and layout modules. It is a subset, not a full web platform
  (see `docs/PLAN.md` section 10).
- Main public items: modules `css`, `dom`, `html`, `layout`, `select`, `style`. See src for types.
- Depends on: `pigment` (non-dev). `assay` in dev.
- Status: Partly done. PASS for T65 (HTML and CSS parsers), T66a (cascade, block and inline layout)
  and T66b (flex subset). T66 as a single row is TODO, and T67 (paint, fetch, page scripts, browser
  shell) is TODO.

#### tectonic

- What it does: game engine core. Integer-only fixed-point maths so the same inputs give the same
  bits on every platform. It has an entity system, a fixed time step, a 2D renderer, collisions and
  impulse physics, an audio mixer, scenes, a script module (Mica), and a world hash.
- Main public items: `Clock`; modules `audio`, `draw`, `ecs`, `fx`, `phys`, `rng`, `scene`, `script`.
- Depends on: `geode`, `mica`, `pigment`, `sinter` (non-dev). `assay` in dev.
- Status: PASS. Rows T68 (entities, fixed step, maths), T69 (renderer, collisions, physics), T70a
  (audio mixer, scene stack, input replay), T70 (Mica scripting and one small game).

### 3.6 Test kit

#### assay

- What it does: the shared test kit. It has a deterministic random generator (`Rng`, SplitMix64),
  a `forall` runner that prints the failing seed, and hex helpers for published test vectors. It is
  the only crate that every other crate lists as a dev-dependency.
- Main public items: `Rng` with `new`, `u64`, `range`, `below` and a weighted boolean helper;
  `forall`; hex helpers (see src).
- Depends on: no crates.
- Status: PASS. Row T00 (workspace, gate, test kit, loop files).

## 4. Quality gate

### 4.1 The script

`tools/check.sh` is the gate. Run it from the repository root:

- `bash tools/check.sh <crate>` checks one crate. It sets its own build directory
  (`target/w-<crate>`) so that parallel workers do not block each other, and it formats only that
  crate.
- `bash tools/check.sh` with no argument checks the whole workspace.

The script runs four steps in order, and stops at the first failure:

1. `cargo fmt --check` (for the crate, or `--all`).
2. `cargo clippy --all-targets -- -D warnings`, so every warning is an error.
3. `cargo test -q` with the same selection.
4. A zero-dependency check. If `Cargo.lock` has any `source = ` line, the gate fails with
   "external dependency found". If there is no lock file or no such line, this step passes.

When everything passes, the script prints `CHECK PASS <crate>` (or `CHECK PASS workspace`). That
last line is what a worker's report quotes. The gate proves the code compiles, is lint clean, and
passes its unit and property tests. It does not prove the code is correct against an outside
standard. That is the job of the leader checks.

### 4.2 Rules a crate must follow

From `briefs/_RULES.md` and `docs/SPEC.md`: library code does not `unwrap`, `expect`, index out of
range or overflow on caller data, and returns the crate's `Error` instead. Tests may unwrap. Every
public item has a one-line doc comment. The public API stays as the brief states it, since other
crates are built against it. Tests (unit, published vectors, `assay::forall` properties) are part of
the deliverable.

### 4.3 Leader checks

A passing gate is not enough. `docs/PLAN.md` section 5 gives each task a "Check" that the leader
runs itself. Those checks live in `briefs/leader/`, which holds 70 files:

- 58 files named `*_leader.rs`. These are test files the leader writes independently, to be placed
  in `crates/<name>/tests/` for a check. For example, `asym_leader.rs` is the test file for the
  X25519 and Ed25519 work (T04, per `docs/PLAN.md` section 5).
- 5 shell scripts (`build_*.sh`) that build fixtures, such as `build_riscv_tests.sh` and
  `build_elf_fixture.sh`; 4 generators (`gen_*.py`) that write case files; and `cinnabar_ref.py`,
  a Python reference for the Cinnabar cipher.
- 2 text case files (`sql_cases.txt`, `join_cases.txt`).

`docs/PLAN.md` section 6 describes the kinds of leader checks: published vectors from Python,
assembler output compared with LLVM, shadow models for state machines (allocator, file system,
scheduler, database, editor), and hostile input run through `forall` with bit flips and
truncation.

### 4.4 Verdicts

The loop in `docs/PLAN.md` section 3 ends each task with one verdict in the `LOOP.md` round log:

- PASS: the gate passes, the leader's check passes, and the diff was read.
- RETRY: one specific defect is added to the brief and the task is run again, with at most two
  retries.
- ESCALATE: the task is handed to a stronger lane. Two ESCALATEs in a row stop the loop.

A report from a worker is never a PASS by itself. The leader runs the gate and its own check.

## 5. Not built yet

These rows are TODO in `LOOP.md` (the board table) and have no finished code. Crates with no
code yet are marked.

| Row | System | What it is |
|---|---|---|
| T25 | mantle | Rust kernel: boot shim, traps, Sv39, heap, console (no crate yet) |
| T26 | mantle | processes, user mode, syscalls, scheduler, ELF loading |
| T27 | mantle | block driver, shale mounted, file syscalls |
| T28 | mantle | userland: init, shell, ls, cat, echo, mkdir, rm, ps |
| T29 | sinter | Mica IR to RV64 code, linear-scan registers, ELF out (T29a RUNNING) |
| T30 | sinter | constant folding, dead code, copy propagation |
| T32 | bedrock | hypervisor with two guests (no crate yet) |
| T50 | seam | full node: gossip over tremor, sync, fork choice (T50a PASS, T50b RUNNING) |
| T51 | vug | Linux container runtime (`vug` crate is a stub) |
| T52 | vug | images as cairn packages, run, ps, kill |
| T63 | facet | compositor: windows, tiling, floating, damage, animated moves (stub) |
| T64 | facet | client protocol and apps: terminal, editor, files |
| T66 | spelunk | cascade and layout as one row (T66a and T66b PASS) |
| T67 | spelunk | paint, fetch over HTTP, page scripts, browser shell in glint |
| T71 | web | WebAssembly builds with our own glue (no crate yet) |
| T72 | lapidary | IDE shell: editor, file tree, terminal, run, test (no crate yet) |
| T73 | lapidary | language service: errors, hover, go to definition, format |
| T74 | lapidary | debugger for Geode: breakpoints, step, inspect |
| T75 | site | the Strata site with a live demo for each system |
| T76 | site | accessibility, performance and cross-browser pass |
| T77 | deploy | static binaries, container images, compose file, health checks |
| T78 | bench | load tools and numbers for outcrop, varve, tremor and seam (`docs/BENCH.md`) |
| T79 | fuzz | one-hour soak of every parser and decoder (T79a PASS, others RUNNING or TODO) |
| T79b | fuzz | soak one crate per lane run (TODO) |
| T80 | review | security review of every crate, one report each |
| T81 | docs | README and runnable example per system (T81a to T81d RUNNING) |
| T83 | release | version tag, change log, demo video (operator publishes) |

Rows marked RUNNING in `LOOP.md` (T29a, T50b, T79c to T79r, T81a to T81d) are in progress and are
not counted as done.
