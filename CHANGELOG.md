# Changelog

Draft: finalised at release (T83) after T75c and T74 land.

## 0.1.0 (2026-11-16)

25 systems in Rust with zero external crates, each with a command-line tool and a web app,
running on our own RISC-V emulator and in the browser via WebAssembly.

Draft: the first Monolith release, built from the tasks that passed the loop board.

### Systems

#### aurum (blockchain)
- Blocks, Merkle tree, signed transactions, state and validation (`b5e3fdb`)
- Contracts: Logos on Elysium with fuel as gas and a contract state root (`2673217`)
- Node with block pool, fork choice, reorgs, sync messages and mempool (`8ba7b21`)
- Node networking over xenon: gossip, sync ticks, redial of stalled links (`6e7f2ed`)
- Security fixes with attack tests (`879f930`)
- Command-line tool (`711a720`)

#### argo (packages)
- Manifest, versions, dependency solver and lock file (`3760068`)
- Registry on mercury with Ed25519-signed packages, atomic install and remove (`c1a1057`)
- Security fixes with attack tests (`879f930`)
- Command-line tool (`fdfdfed`)

#### athena (web IDE)
- Browser IDE with editor, file tree, run, test and a Titan terminal (`c267834`)
- Language service: errors, hover, go to definition, format (`ef269a1`)
- VS Code-style workbench, installable offline PWA (`2d1b22d`)
- Command-line tool (`51ae44d`)

#### calliope (terminal editor)
- Piece-table buffer, undo tree, multi-cursor editing, regex subset checked against Python re (`5aa962d`)
- Terminal interface with modes, screens, files and Logos highlighting (`fefa744`)
- Command-line tool (`fdfdfed`)

#### chronos (version control)
- Git-compatible SHA-256 objects, refs, index, commit and checkout (`888f2cb`)
- Myers diff, unified output, 3-way merge matching git, status and merge (`5a12be3`)
- Push, fetch, pull and clone over mercury HTTP (`a6166e7`)
- Security fixes with attack tests (`2e5a752`)
- Command-line tool (`fdfdfed`)

#### cobalt (compiler)
- Logos IR to Elysium bytecode, with `logos run` and `logos test` (`9f2f5f4`)
- Integer subset of Logos IR to RV64 with stack slots, runs on Silicon (`b27f121`)
- Constant folding, dead code removal and copy propagation (`d038519`)
- Command-line tool: ir, opt, build (`b013e1d`)

#### elysium (virtual machine)
- Module format, verifier and binary encode and decode (`e4c5fd8`)
- Interpreter with heap, mark-sweep GC, fuel and the Host trait (`2545880`)
- Pausable execution: start, resume and frames (`dfe4a54`)
- Security review: 10 attack tests, no findings (`f297400`)
- Command-line tool: verify, dis, run (`024b1cd`)

#### hades (block storage)
- Block layer, on-disk format, mkfs and read/write path (`1aee4fd`)
- Write-ahead journal, fsck and crash injection at every write (`595b9cb`)
- Security fixes with attack tests (`f065214`)
- Command-line tool (`f55e499`)

#### helios (game engine)
- Entity system, fixed time step, maths and deterministic world hash (`3be16df`)
- 2D renderer with fixed-point collisions and impulse physics (`0289a3a`)
- Audio mixer, scene stack, input record and replay (`d04a156`)
- Logos scripting and one original small game (`f52e3a1`)
- Security fixes with attack tests (`7baf618`)
- Command-line tool (`711a720`)

#### indium (widgets)
- Flex layout, focus, events and themed rendering (`0e0b4ee`)
- Button, field, list, scroll, tabs and menu widgets with spring and ease animation (`9571901`)
- Command-line tool (`51ae44d`)

#### iridium (database)
- Pager with write-ahead log and crash recovery (`15ba367`)
- B+tree with range scans (`709dcc9`)
- SQL single-table statements matching sqlite3 STRICT, then joins, index plans and EXPLAIN (`99ce847`, `56035f9`)
- Network server with one writer and many readers, plus a client tool (`e279980`)
- Security fixes with attack tests (`879f930`)
- Command-line tool (`0d6dd00`)

#### iris (compositor)
- Windows, tiling and floating layouts, damage tracking, input and animated moves (`d038519`)
- Client protocol with terminal, editor, files and a desktop (`f065214`)
- Command-line tool (`51ae44d`)

#### kairos (scheduler)
- Fair scheduler core and simulator (`c6cb83a`)
- Work-stealing thread pool on std threads (`7d02ee7`)
- Command-line tool (`fdfdfed`)

#### krypton (shell)
- Shell parser, word expansion and globbing, checked against bash (`ad9db4d`)
- Executor, pipelines, redirects, builtins and background jobs (`8385048`)
- Security fixes: stack and blow-up limits, with tests (`2e5a752`)

#### lithium (assembly kernel)
- Two-task RV64 kernel in pure assembly on Silicon (`7667e5c`)
- Guest of the zeus hypervisor (`2da4f62`)
- Command-line tool: run, image (`3b859eb`)

#### logos (language)
- Language reference, version 1 (`686eead`)
- Parser, printer, corpus and fuzz tests (`861e28a`)
- Type checker, layer checker and typed IR (`a8bb929`)
- Reference interpreter for the typed IR (`920da3b`)
- Security fixes with attack tests (`7baf618`)

#### mercury (HTTP server)
- HTTP/1.1 parser and writer (`e887c02`)
- Thread pool, keep-alive, static files, routes, limits and graceful stop (`0ba8a29`)
- WebSocket, range requests and chunked bodies (`03c4b83`)
- Security fixes: request smuggling and header injection, with tests (`2e5a752`)
- Command-line tool: parse, serve, get (`6e53acb`)

#### midas (allocator)
- Buddy and slab allocator with GlobalAlloc (`01d6a40`)
- Const empty heap, global allocator, large alignments and a hardened check (`06fabc5`)
- Command-line tool (`fdfdfed`)

#### odyssey (browser engine)
- HTML and CSS parsers and selectors (`cdd464d`)
- Cascade, block and inline layout, flex subset (`eccc11b`, `c5ac6b3`)
- Paint, HTTP fetch, page scripts and a browser shell (`0b6df5d`)
- Command-line tool: tokens, tree, select, render (`cf3541a`)

#### pandora (containers)
- Linux container runtime: namespaces, pivot_root and cgroup v2 (`c2dfd2e`)
- Images as argo packages, with pull, run, ps, kill and rm (`5ba093d`)

#### silicon (RISC-V emulator)
- RV64IMAC decode, encode and execute, checked against LLVM (`910d2bf`, `b616e75`)
- Zicsr, M/S/U modes, traps and Sv39 paging (`0fb7694`)
- Hypervisor extension with two-stage paging (`096b2ac`)
- Devices: UART, timer, interrupt controller, block, framebuffer and keys (`307298a`)
- Decoded-instruction cache, 1.55x in the benchmark (`049e3f8`)
- Command-line tool (`f55e499`)

#### sphinx (crypto)
- SHA-2, HMAC, HKDF and ChaCha20-Poly1305, verified against Python (`062c2e3`)
- X25519 and Ed25519 (`717f428`)
- ChaCha20 random generator and sealed box (`793713c`)
- Medusa, an experimental block cipher labelled as such (`4d84fe1`)
- Rejects small-order Ed25519 public keys, with attack tests (`bb6d94e`)
- Command-line tool: hash, hmac, keygen, sign, verify, seal, open (`b6c917e`)

#### titan (kernel)
- Kernel boots with traps, Sv39 paging, heap and console (`8d39a78`)
- Processes, user mode, syscalls, scheduler and ELF loading (`58e24c3`)
- Block driver with hades mounted at /, and file syscalls (`d5f88f0`)
- Userland: init, krypton shell, ls, cat, echo, mkdir, rm, ps (`6c33a72`)
- Command-line tool, with the kernel running in the browser build (`0d6dd00`)

#### xenon (protocol)
- Protocol core with no I/O inside (`f4dc733`)
- UDP transport, lossy-link handling, echo and file-transfer tools (`04686cb`)
- Security fix: RTT overflow, with attack tests (`2e5a752`)

#### zeus (hypervisor)
- Hypervisor on the H extension, with lithium and titan guests and isolation (`2da4f62`)
- zeusctl command-line tool: zeus with lithium and titan guests on Silicon (`5c3fb71`)

### Site and apps
- Site shell with 25 sections and 8 live demos (`82528b1`)
- 9 more live demos (`18e1ed1`)
- Neon: Silicon and Titan run on a web page via WebAssembly, with our own JS glue and a Logos playground (`f13a443`)
- App kit and launcher at /apps/, with 17 installable apps (`a928635`)

### Tools
- One command-line tool per system, with conventions in docs/APPS.md (`b54a74f`)
- Load tools and numbers for mercury, iridium, xenon and aurum in docs/BENCH.md (`2d12113`)
- Architecture book docs/BOOK.md, plus READMEs with runnable demos (`badb970`, `7939ff0`)
- Workspace, gate and test kit (`4b22fee`)
- CI: GitHub Actions workflow runs the gate on Linux, green (`0ef99b1`)
- Fuzz soaks passed per crate: chisel, elysium, chronos, argo, krypton (`b27f121`), hades through midas (`8d39a78`)
- chisel (support crate): RV64IMAC assembler (`e9c41f3`), ELF64 writer and loader (`f4dc733`)
- pigment (support crate): framebuffer, lines and anti-aliased polygons (`43822a4`), inflate, deflate and PNG read and write (`5d141d1`), bitmap font and text wrapping (`f0953cb`)

### Deploy
- Service binaries, health probe, scratch images and compose file (`5239fe4`)
- One public web image: site at /, Athena at /athena/, /healthz; services stay private (`3950848`)
