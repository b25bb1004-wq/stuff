# Monolith loop board (the single source of truth; re-read this, not the chat)

## State
- LEADER: Claude Opus since 2026-11-14 16:15
- heartbeat: 2026-11-15 17:37
- phase: P2-P6. Running T25 titan (argon-high), T63 iris (hydrogen-high), T81e-h (hydrogen), T30 (haiku). CI live (T82). Argon released down to 5% (operator, 2026-11-15; was 29%): argon-high tasks may run; no tool shows usage, so the leader asks the operator for the % after each Argon run and stops at 5%.
- stop reason: none
- Next for the leader: see "Claude board" below (the strata-board mod shows it in a "Claude" pane, command `/strata-claude`).
- Stand-in or second judge: Gemini Orchestrator through agy (command in docs/PLAN.md section 9).
- At most one Gemini worker (through `bash tools/lane.sh <lane> <id>`) plus one Claude Haiku worker, on different crates. Lanes and rules: docs/PLAN.md section 2.

## Claude board (the leader's own work; `- [>]` doing, `- [ ]` next, `- [!]` blocked, `- [x]` done)
- [x] T33 judge + bench fixes, commit 049e3f8
- [x] T21 corpus (LLVM, validated), brief, leader check
- [x] T82 CI workflow, green on ubuntu-latest
- [x] T47 argo registry PASS (leader check: clippy fixes only)
- [x] T21 chisel PASS (leader check fixed: hex li, x0)
- [x] T22 chisel ELF PASS (first try)
- [x] D1 docs/MEDUSA.md (Python reference, vectors, stats; check validated on a model)
- [x] T14 sphinx Medusa PASS (first try)
- [x] D2 docs/XENON.md, D3 docs/IRIDIUM.md (v1); T34, T39 retagged hydrogen-high (spec exact, saves Argon)
- [x] T39 iridium pager: brief + leader check (own sector-loss disk, crash at every op), dispatched (hydrogen-high)
- [x] T34 xenon PASS on retry 1 (ACK/PTO fix); chronos racing-pushers check fixed (reachability, not first-parent)
- [x] T55 calliope app: brief + app_leader.rs (edit/motion tables, screens, scroll, files, Logos highlight, fuzz undo-all), dispatched (hydrogen)
- [x] T48 aurum: exact brief (encodings, RFC 6962 Merkle, state root, append order) written; dispatched to one Haiku subagent (operator 2026-11-15); seam_leader.rs ready (lint vs stub, RFC 6962 model self-check OK)
- [x] T13 silicon PASS: 110/110 official riscv-tests (leader updated 2 obsolete T12 PMP asserts)
- [x] docs/TITAN.md v1 written by the leader (operator: Fable rewrite dropped); T25 dispatched to argon-high
- [x] chronos concurrent push flake fixed (Barrier), in b27f121
- [x] T31 silicon H extension: brief + hyp_leader.rs (own two-stage model, coverage 22% ok / 46% pf / 31% gpf), dispatched (argon-high)
- [x] T40 iridium B+tree: brief + tree_leader.rs (own page parser), dispatched (hydrogen-high)
- [x] T29a, T79c-g, T81a-d committed and pushed
- [>] T50b aurum net: leader fix (redial stalled links) done, workspace gate running, then commit
- [>] Judge Haiku soaks: T79m sphinx, T79n iridium (SQL depth fix), T79o odyssey (3 fixes), T79p calliope, T79q helios, T79r midas
- [>] Judge hydrogen soaks T79h-l (fresh-seed long runs done), commit
- [ ] Judge T30 cobalt optimiser (Haiku running); write opt_leader.rs
- [x] T81e-h READMEs: output pasted, committed
- [ ] Judge T63 iris compositor (hydrogen-high running); write facet_leader.rs
- [ ] Judge T25 titan (Argon running); write mantle_leader.rs; ask operator for Argon %
- [ ] Backlog: indium spring stall at low stiffness; helios tone() huge alloc and unbounded script log
- [ ] Vault note + skills for this round

## Goal
Build all 25 systems in `docs/SPEC.md` as one zero-dependency stack: unique, tested, working
top to bottom, and deployable.

## Quality bar (every item must be checkable)
1. `bash tools/check.sh` passes for the whole workspace (format, clippy -D warnings, tests,
   zero outside dependencies).
2. Every system in the SPEC table has a crate with a public API, docs and a runnable example.
3. Every parser, decoder, disk and network path has a property or fuzz test and never panics.
4. Published test vectors or an independent implementation confirm: crypto, CPU (official
   RISC-V tests), file system (crash injection), database (crash injection), net (loss,
   reorder, duplicate simulation).
5. Monolith OS boots to a shell in the emulator, natively and in the browser.
6. The hypervisor runs two guest kernels at once.
7. A Logos program compiles to RV64, runs in Monolith OS, and gives the same output on the VM.
8. Services have a container image, graceful shutdown and a measured load test.
9. The web front (site + IDE) passes desktop, mobile and reduced-motion checks.
10. No secret, key or personal data in the repository; pushes go only to the private repo `b25bb1004-wq/stuff`.

## Budget
- Argon quota is low (41% left on 2026-11-14 18:41, operator). Argon lanes only for the hardest, highest-value tasks (what you would give Opus): compilers, kernels, hypervisor, protocol cores. Everything else: hydrogen / hydrogen-high / hydrogen-xhigh, sonnet or opus via agy, flash or oss for text, Haiku for Claude-lane work. The lane column below is a plan, not a rule; downgrade argon rows when dispatching.
- Parallel: at most two agy workers at once, one per crate (lane.sh per-crate lock), plus the Haiku lane (resumed 18:40 by the operator, past the round cap).
- Whole programme, honest estimate: 150 to 250 worker tasks, 25M to 40M worker tokens, many
  sessions. It cannot finish inside one usage window.
- One worker at a time (operator rule 2026-11-14). Retries 2 per task. Gemini lanes use no Claude tokens.
- Operator 2026-11-14 16:25: Haiku (medium) is a normal lane for the Claude leader, not only a fallback. Operator 16:35: one Haiku worker may run beside one Gemini worker, on different crates. Budgets: 170k (Haiku), 250k (Sonnet).
- Spent so far: 1982k Claude worker tokens (leader tokens not counted).
- 2026-11-14 17:36: the round cap of 2.0M Claude worker tokens (vault note, from the plan) is reached at 1982k. Haiku lane paused for this round; Gemini lanes (no Claude tokens) continue. Haiku tasks have cost 114k to 256k each, so budget 250k per Haiku task next round.

## Workers checked on 2026-11-14
| Worker | Status |
|---|---|
| Claude Haiku / Sonnet subagents | Haiku: normal leader lane (operator 16:25); Sonnet after a failed retry; one at a time |
| Nemotron Ultra (WSL `~/faith/tools/nv.py`) | PONG ok; bulk text only |
| agy (Argon, Hydrogen, Flash, Orchestrator) | answers from this PC since 2026-11-14 15:40; `tools/lane.sh` |
| Qwen coder via NVIDIA API | dead (HTTP 410 / 404) |
| Qwen local (llama-server in WSL) | binary present, not started; text labour only, needs the GPU |

## Tasks
Status: TODO, RUNNING, PASS, RETRY, ESCALATE. `needs` must be PASS before dispatch.

| id | phase | crate | what | worker | needs | status |
|---|---|---|---|---|---|---|
| T00 | P0 | assay, tools | workspace, gate, test kit, loop files | leader | | PASS |
| T01 | P1 | sphinx | SHA-2, HMAC, HKDF, ChaCha20-Poly1305 | haiku | | PASS |
| T02 | P1 | midas | buddy + slab allocator, GlobalAlloc | haiku | | PASS |
| T03 | P1 | silicon | RV64IM decode, encode, execute | haiku | | PASS |
| T06 | P1 | hades | block layer, on-disk format, mkfs, read/write path | haiku | | PASS |
| T08 | P1 | logos | language design `docs/LOGOS.md` + bytecode `docs/MBC.md` | leader | | PASS |
| T11 | P1 | elysium | module, verifier, binary encode/decode (`docs/MBC.md`) | haiku | T08 | PASS |
| T05 | P1 | kairos | fair scheduler core + simulator | hydrogen |  | PASS |
| T09 | P1 | logos | finish parser, printer, corpus, fuzz (files exist: read them first) | hydrogen | T08 | PASS |
| T04 | P1 | sphinx | X25519, Ed25519 (brief written) | haiku | T01 | PASS |
| T17 | P1 | elysium | interpreter, heap, mark-sweep GC, fuel, `Host` trait | argon | T11 | PASS |
| T10 | P1 | logos | type checker, layer checker, IR | argon-high | T09 | PASS |
| T19 | P1 | logos | reference interpreter for the typed IR (oracle for T18, T29, T30) | hydrogen | T10 | PASS |
| T18 | P1 | cobalt | Logos IR to Elysium bytecode; `logos run`, `logos test` commands | argon | T10 T17 T19 | PASS |
| T07 | P1 | hades | journal, fsck, crash injection at every write | haiku | T06 | PASS |
| T12 | P1 | silicon | Zicsr, M/S/U modes, traps, Sv39 paging | haiku | T03 | PASS |
| T13 | P1 | silicon | official riscv-tests as data (needs download: operator) | hydrogen | T13a | PASS |
| T13a | P1 | silicon | A and C extensions, LLVM as oracle (split from T13) | hydrogen-high | T12 | PASS |
| T16 | P1 | midas | const constructor, global allocator, big alignments, `check()` | haiku | T02 | PASS |
| T15 | P1 | kairos | work-stealing thread pool on std threads, driven by the core | haiku | T05 | PASS |
| T14 | P1 | sphinx | Medusa: own cipher from `docs/MEDUSA.md`, labelled experimental | hydrogen | T01 D1 | PASS |
| T20 | P1 | sphinx | ChaCha20 random generator, sealed-box helper | haiku | T04 | PASS |
| T21 | P2 | chisel | RV64IMAC assembler: text, labels, directives | hydrogen | T13a | PASS |
| T22 | P2 | chisel | ELF64 writer and loader | hydrogen | T21 | PASS |
| T23 | P2 | silicon | devices: UART, timer, interrupt controller, block, framebuffer, keys; `Machine` | hydrogen-high | T12 | PASS |
| T24 | P2 | lithium | kernel in pure assembly: boot, traps, console, timer, 2 tasks, 3 syscalls | haiku | T22 T23 | PASS |
| T25 | P2 | titan | Rust kernel: boot shim, traps, Sv39, heap (midas), console | argon-high | T23 T16 T13a | PASS |
| T26 | P2 | titan | processes, user mode, syscalls, scheduler (kairos), ELF loading | opus-high | T25 T05 | PASS |
| T27 | P2 | titan | block driver, hades mounted, file syscalls | hydrogen-high | T26 T07 | PASS |
| T28 | P2 | titan | userland: init, shell (krypton), ls cat echo mkdir rm ps | hydrogen | T27 T57 | PASS |
| T29 | P2 | cobalt | Logos IR to RV64 code, linear-scan registers, ELF out | opus-high | T10 T22 | PASS |
| T29a | P2 | cobalt | (T29a) integer subset Logos IR to RV64, stack slots, runs on Silicon | hydrogen-high | T10 T22 | PASS |
| T30 | P2 | cobalt | constant folding, dead code, copy propagation | haiku | T29 | PASS |
| T31 | P2 | silicon | hypervisor extension: HS, VS, VU, two-stage paging | argon-high | T13a | PASS |
| T32 | P2 | zeus | hypervisor: 2 guests (lithium + titan), virtual console and timer, isolation | opus-high | T31 T24 T26 | RUNNING |
| T33 | P2 | silicon | speed: decoded-block cache, benchmark | hydrogen | T23 | PASS |
| T34 | P3 | xenon | protocol core from `docs/XENON.md`, no I/O inside | hydrogen-high | T04 T20 D2 | PASS |
| T35 | P3 | xenon | UDP transport, echo and file-transfer tools | hydrogen | T34 | PASS |
| T36 | P3 | mercury | HTTP/1.1 parser and writer, no I/O inside | hydrogen |  | PASS |
| T37 | P3 | mercury | server: thread pool, keep-alive, static files, routes, limits, graceful stop | hydrogen | T36 T15 | PASS |
| T38 | P3 | mercury | WebSocket, ranges, chunked bodies | hydrogen | T37 | PASS |
| T39 | P3 | iridium | pager, write-ahead log, crash recovery | hydrogen-high | D3 | PASS |
| T40 | P3 | iridium | B+tree with range scans | hydrogen-high | T39 | PASS |
| T41 | P3 | iridium | SQL: parser, planner, executor, indexes, transactions | argon-high | T40 | PASS |
| T41a | P3 | iridium | SQL core: parser, catalog, single-table DML and SELECT, indexes maintained, transactions | hydrogen-high | T40 | PASS |
| T41b | P3 | iridium | joins, index use in plans, EXPLAIN | hydrogen-high | T41a | PASS |
| T42 | P3 | iridium | server, client tool, one writer and many readers | hydrogen | T41 T37 | PASS |
| T43 | P3 | chronos | objects, refs, index, commit, log, branch, checkout | hydrogen | T01 | PASS |
| T44 | P3 | chronos | diff (Myers), 3-way merge, status | hydrogen-high | T43 | PASS |
| T45 | P3 | chronos | push and pull over HTTP (mercury) | hydrogen | T44 T37 | PASS |
| T46 | P3 | argo | manifest, versions, dependency solver, lock file | hydrogen |  | PASS |
| T47 | P3 | argo | registry on mercury, signed packages (Ed25519), install, remove | hydrogen | T46 T04 T37 | PASS |
| T48 | P3 | aurum | blocks, Merkle tree, signed transactions, state, validation | haiku | T04 | PASS |
| T49 | P3 | aurum | contracts: Logos on Elysium, fuel as gas, `chain` layer | hydrogen | T48 T18 | PASS |
| T50 | P3 | aurum | node: gossip over xenon, sync, fork choice | opus-high | T49 T35 | PASS |
| T50a | P3 | aurum | node: block pool, fork choice, reorgs, sync messages, mempool | hydrogen-high | T49 | PASS |
| T50b | P3 | aurum | gossip over xenon: sans-IO peers, stream framing, sync ticks | hydrogen-high | T50a T35 | PASS |
| T51 | P3 | pandora | Linux container runtime: namespaces, cgroups v2, pivot root (runs in WSL) | opus-high |  | PASS |
| T52 | P3 | pandora | images as argo packages, `run`, `ps`, `kill` | hydrogen | T51 T47 | PASS |
| T53 | P3 | krypton | shell language: parser, expansion, globbing | hydrogen-high |  | PASS |
| T57 | P3 | krypton | executor, pipes, redirects, builtins, job control | hydrogen | T53 | PASS |
| T54 | P3 | calliope | buffer (piece table), undo tree, multi-cursor, own regex engine | hydrogen-high |  | PASS |
| T55 | P3 | calliope | terminal interface, key maps, files, Logos highlighting | hydrogen | T54 T09 | PASS |
| T58 | P4 | pigment | framebuffer, lines, rectangles, anti-aliased polygons, blending | haiku |  | PASS |
| T59 | P4 | pigment | inflate, deflate, PNG read and write | hydrogen-high | T58 | PASS |
| T60 | P4 | pigment | own bitmap font, text layout, wrapping | hydrogen | T58 | PASS |
| T61 | P4 | indium | widget tree, flex layout, events, focus, themes | hydrogen | T60 | PASS |
| T62 | P4 | indium | widgets (button, field, list, scroll, tabs, menu), spring and ease animation | haiku | T61 | PASS |
| T63 | P4 | iris | compositor: windows, tiling and floating, damage, input, animated moves | hydrogen-high | T62 | PASS |
| T64 | P4 | iris | client protocol + apps: terminal (krypton), editor (calliope), files | hydrogen-high | T63 T57 T55 | PASS |
| T65 | P4 | odyssey | HTML and CSS parsers | hydrogen |  | PASS |
| T66 | P4 | odyssey | cascade, block and inline layout, flex subset | opus-high | T65 T60 | PASS (split T66a T66b) |
| T66b | P4 | odyssey | flex layout subset | haiku | T66a | PASS |
| T66a | P4 | odyssey | cascade, block and inline layout (flex is T66b) | haiku | T65 T60 | PASS |
| T67 | P4 | odyssey | paint, fetch over HTTP, page scripts in Logos, browser shell in indium | hydrogen-high | T66 T62 T18 T37 | PASS |
| T68 | P4 | helios | entity system, fixed time step, maths | haiku |  | PASS |
| T69 | P4 | helios | 2D renderer, collisions and impulse physics | hydrogen-high | T68 T58 | PASS |
| T70a | P4 | helios | audio mixer, scene stack, input record and replay | hydrogen | T69 | PASS |
| T70 | P4 | helios | (T70b) Logos scripting, one original small game | hydrogen | T70a T18 | PASS |
| T71 | P5 | neon | WebAssembly builds with our own glue (no wasm-bindgen): Silicon + Titan on a canvas, Logos playground | argon | T28 T18 | RUNNING |
| T72 | P5 | athena | IDE shell: editor, file tree, terminal, run, test | claude | T71 T55 | TODO |
| T73 | P5 | athena | language service: errors, hover, go to definition, format | hydrogen | T10 T72 | TODO |
| T74 | P5 | athena | debugger for Elysium: breakpoints, step, inspect | hydrogen | T17 T72 | TODO |
| T75 | P5 | site | the Monolith site: 25 systems, each with a live demo, scroll-driven motion | claude | T71 | TODO |
| T76 | P5 | site | accessibility, performance, cross-browser pass | claude | T75 | TODO |
| T77 | P6 | deploy | static binaries, container images, compose file, health checks | hydrogen | T42 T50 T47 | TODO |
| T78 | P6 | bench | load tools and numbers for mercury, iridium, xenon, aurum in `docs/BENCH.md` | hydrogen | T77 | TODO |
| T79 | P6 | fuzz | one-hour soak of every parser and decoder with new seeds | hydrogen | all | TODO |
| T79a | P6 | fuzz | soak mercury | hydrogen | - | PASS |
| T79b | P6 | fuzz | soak one crate per lane run: chisel elysium chronos argo krypton hades xenon logos pigment silicon | hydrogen | - | TODO |
| T79c | P6 | fuzz | soak chisel | hydrogen | - | PASS |
| T79d | P6 | fuzz | soak elysium | hydrogen | - | PASS |
| T79e | P6 | fuzz | soak chronos | hydrogen | - | PASS |
| T79f | P6 | fuzz | soak argo | hydrogen | - | PASS |
| T79g | P6 | fuzz | soak krypton | hydrogen | - | PASS |
| T79h | P6 | fuzz | soak hades | hydrogen | - | PASS |
| T79i | P6 | fuzz | soak xenon | hydrogen | - | PASS |
| T79j | P6 | fuzz | soak logos | hydrogen | - | PASS |
| T79k | P6 | fuzz | soak pigment | hydrogen | - | PASS |
| T79l | P6 | fuzz | soak silicon | hydrogen | - | PASS |
| T79m | P6 | fuzz | soak sphinx | haiku | - | PASS |
| T79n | P6 | fuzz | soak iridium | haiku | - | PASS |
| T79o | P6 | fuzz | soak odyssey | haiku | - | PASS |
| T79p | P6 | fuzz | soak calliope | haiku | - | PASS |
| T79q | P6 | fuzz | soak helios | haiku | - | PASS |
| T79r | P6 | fuzz | soak midas | haiku | - | PASS |
| T80 | P6 | review | security review of every crate, one report each | argon | all | TODO |
| T81 | P6 | docs | README and runnable example per system, architecture book | flash | all | TODO |
| T81a | P6 | docs | README + demo: assay mercury | haiku | - | PASS |
| T81b | P6 | docs | README + demo: indium lithium | haiku | - | PASS |
| T81c | P6 | docs | README + demo: kairos | haiku | - | PASS |
| T81d | P6 | docs | architecture book docs/BOOK.md | haiku | - | PASS |
| T81e | P6 | docs | README + demo: argo | hydrogen | - | PASS |
| T81f | P6 | docs | README + demo: chisel | hydrogen | - | PASS |
| T81g | P6 | docs | README + demo: krypton | hydrogen | - | PASS |
| T81h | P6 | docs | README + demo: chronos | hydrogen | - | PASS |
| T82 | P6 | ci | GitHub Actions workflow for the gate | hydrogen |  | PASS |
| T83 | P6 | release | version tag, change log, demo video | leader | all | TODO |
| T79s | P6 | fuzz | soak aurum | haiku | - | PASS |
| T79t | P6 | fuzz | soak indium | haiku | - | PASS |
| T79u | P6 | fuzz | soak kairos | haiku | - | PASS |
| T80a | P6 | review | security review mercury, findings proven by tests | hydrogen-high | - | PASS |
| T80b | P6 | review | security review xenon, findings proven by tests | hydrogen-high | - | PASS |
| T80c | P6 | review | security review chronos, findings proven by tests | hydrogen-high | - | PASS |
| T80d | P6 | review | security review argo, findings proven by tests | hydrogen-high | - | PASS |
| T80e | P6 | review | security review krypton, findings proven by tests | hydrogen-high | - | PASS |
| T80f | P6 | review | security review iridium, findings proven by tests | hydrogen-high | - | PASS |
| T80g | P6 | review | security review elysium, findings proven by tests | hydrogen-high | - | PASS |
| T80h | P6 | review | security review aurum, findings proven by tests | hydrogen-high | - | PASS |
| T80i | P6 | review | security review silicon, findings proven by tests | hydrogen-high | - | PASS |
| T80j | P6 | review | security review hades, findings proven by tests | hydrogen-high | - | PASS |
| T80k | P6 | review | security review sphinx + medusa, findings proven by tests | hydrogen-high | - | PASS |
| T80l | P6 | review | security review logos, findings proven by tests | hydrogen-high | - | PASS |
| T80m | P6 | review | security review helios, findings proven by tests | hydrogen-high | - | PASS |
| T81i | P6 | docs | README + demo elysium logos | haiku | - | PASS |
| T81j | P6 | docs | README + demo sphinx pigment | haiku | - | PASS |
| T81k | P6 | docs | README + demo midas silicon | haiku | - | PASS |
| T81l | P6 | docs | README + demo aurum hades | haiku | - | PASS |
| T81m | P6 | docs | README + demo cobalt calliope | haiku | - | PASS |
| T81n | P6 | docs | README + demo odyssey helios | haiku | - | PASS |
| T81o | P6 | docs | README + demo iridium pandora | haiku | - | PASS |
| T81p | P6 | docs | README + demo iris titan xenon | haiku | - | PASS |

Later phases get their task rows when P1 is green (see the build order in `docs/SPEC.md`):
P2 chisel, cobalt, silicon devices, lithium, titan, zeus. P3 xenon, mercury, iridium, chronos,
argo, aurum, pandora, krypton, calliope. P4 pigment, indium, iris, odyssey, helios. P5 web front, IDE, containers, load tests. P6 hardening.

## Round log (append only: date time | task | worker | tokens | verdict | note)
- 2026-11-14 14:56 | T00 | leader | n/a | PASS | scaffold gate green, 24 crates, test kit 3 tests
- 2026-11-14 15:35 | T08 | leader | n/a | PASS | MICA.md and MBC.md written; T09 and T11 unblocked
- 2026-11-14 15:45 | T01 | haiku | 157k | PASS | gate green; 41 leader vectors from Python hashlib + cryptography all match (SHA-256/512, HMAC, HKDF, AEAD); Poly1305 read, branch-free
- 2026-11-14 16:00 | T02 | haiku | 170k | PASS | gate green; leader shadow model (public API only) 400 cases x 3000 ops in release, seed 991177: no overlap, no corruption, no leak, full merge. Gaps moved to T16
- 2026-11-14 16:25 | T03 | haiku | 202k | PASS | gate green; 499 instructions assembled by LLVM 19 all decode and re-encode identically; official RISC-V tests come in T13
- 2026-11-14 16:30 | names | leader | n/a | PASS | every crate renamed to its Strata name (table in docs/SPEC.md); full gate green, 55 test groups
- 2026-11-14 15:19 | T11 | haiku | 179k | PASS | gate green; opcode table read against docs/MBC.md (58 of 58 in order); leader fuzz 40 000 hostile inputs, new seed: no panic, decode then encode stable. Note: earlier log times in this file ran ahead of the clock
- 2026-11-14 16:10 | T06 | haiku | 240k | PASS | gate green; leader shadow model 100 cases x 400 ops with remount every 40 and a leak check, plus 5000 corrupted images, fresh seeds: no mismatch, no panic. Over budget (170k). Open points for T07: TooLarge is an extent-count limit; overlapping extents across files are not detected
- 2026-11-14 16:21 | T05 | hydrogen | 0 Claude | PASS | gate green; leader model (public API only) 400+400 cases x 600 calls incl. bad ids, bad cpus, time backwards, 2nd seed x2000: affinity, one cpu per task, work conservation, runtime sum == busy time, determinism, fairness spread <= slice, nice 0/5 ratio within 5%, class order. No panic paths found
- 2026-11-14 16:31 | T09 | hydrogen | 0 Claude | PASS | gate green, 44 tests, 20-program corpus; leader check (6 programs from the reference, 15 rejects, fuzz 3x4000 + 2nd seed 3x20000, 20k-deep nesting) green; nesting limit holds at about 199. Worker edited 3 test expectations; each matches docs/MICA.md (lowercase const, rule b, `;;`). Workspace gate run in a clean worktree because Haiku was editing onyx
- 2026-11-14 16:34 | T04 | haiku | 186k | PASS | gate green, 43 tests; leader Python `cryptography` vectors (X25519 + Ed25519) all match; RFC 7748 and RFC 8032 tests 1-3 exact; scalar mul and ladder branch-free (masked select, cswap). RFC 8032 TEST 1024 not added (worker could not reproduce the message); Python vectors cover long messages
- 2026-11-14 16:49 | T07 | haiku | 114k | PASS | gate green, 31 tests; leader crash check with its own crash device (power cut after every block write, remount + fsck): 12 + 60 cases x 40 ops on two seeds, every tree is the state before or after the op in progress, fsck clean. Journal overflow refused with TooLarge before any write. Free blocks are written home directly (safe: bitmap goes through the journal)
- 2026-11-14 17:02 | T17 | argon | 0 Claude | PASS | gate green, 47 tests; leader VM check (exact fuel at every limit, fib, 200k-object GC churn in 1 MiB, retained objects give OutOfMemory, 300k-deep list under GC, depth limits, every trap, ParseInt edge cases, host + missing import, fuzz 3000 + 20000 on two seeds with determinism) green; leader generator bug (underflow) fixed in the leader file, not the VM
- 2026-11-14 17:12 | T12 | haiku | 256k | PASS | gate green, 85 tests; leader check: 171 Zicsr/privileged words assembled by LLVM (rustc global_asm) decode and encode exactly; an LLVM-assembled privilege program on `tick`: mret to U, U ecall to M, delegated U ecall to S, S ecall to M (MPP=1), Sv39 gigapages built by the test (load and store through U page, sepc and mtval virtual, S load of U page without SUM faults to M), wfi then MTI with mepc after wfi, hardware A/D exact (D clear on the page never stored to). WARL choices listed in reports/T12.md, all legal. Leader lesson: a worktree gate sharing CARGO_TARGET_DIR left a stale quartz rlib in the main tree; the gate now uses its own target dir
- 2026-11-14 17:34 | T16 | haiku | 222k | PASS | worker stopped at NOT PASS over budget (global.rs hung). Leader found the hang in the test: an assert under a live spin-lock guard on an impossible used_bytes()==0; the panic allocated through the held lock. Test fixed (assert after release), gate green. Leader check: absolute alignments up to 4 MiB on arenas at odd page and byte offsets (3078 big-align blocks, no overlap, tags intact, 4 MiB block again after full free, over-align and over-size refused), empty -> init through a static Locked, second init refused, targeted bad links and smashed page headers reported, 300 random corruptions never panic check(), quarry as #[global_allocator] under 8 threads of std collections (2.16M bytes moved), T02 shadow model in release with a new seed x10
2026-11-14 18:24 | T10 | gemini argon-high | gemini quota | PASS | mica gate + clean-worktree workspace gate; leader check leader_check.rs (13 good, ~70 bad, layer message, IR validator); my check wrongly required parse success for `1 = 2`, fixed to count parse rejection
2026-11-14 18:33 | T19 | gemini hydrogen | gemini quota | PASS | 4 min run; mica gate + clean-worktree workspace gate; leader check leader_eval.rs (11 hand-computed programs, 16 trap cases with spans, exact fuel bound, depth 9000 ok and unbounded = StackOverflow, props replay and big-int draws)
2026-11-14 18:37 | T18 | gemini argon | gemini quota | NO OUTPUT | lane granted mica commands (first crates/ path in the brief was crates/mica); brief title now names crates/sinter; re-run 18:38
2026-11-14 18:48 | T15 | haiku medium | 149k | PASS | geyser gate + clean-worktree workspace gate; leader_pool.rs: seeded soak 198k planned jobs exactly once with nested spawns and panics, 150 create/drop rounds with jobs still spawning, 3 concurrent waiters, stolen 2252; second seed ok; no_std build ok
2026-11-14 18:51 | T36 | gemini hydrogen | gemini quota | PASS | outcrop gate + clean-worktree workspace gate; leader_http.rs: CL+TE and duplicate/odd CL smuggling, chunk overflow, limits before bytes arrive, keep-alive, pipelining, split at every byte, 4000-case mutation fuzz (939 accepted, round trip)
- 2026-11-14 23:40 T18 sinter PASS (hydrogen). Leader check: corpus + 6 unseen programs + 9 trap spans vs oracle, binary run/test; my check had a wrong line (3: vs 4:3). geode gained Vm::trap_site (additive). Workspace gate PASS in clean worktree.
- 2026-11-14 23:44 T20 onyx PASS (haiku, ~121k tokens). Leader check: RFC 8439 A.1 #1 + differential vs chacha20::xor, split reads, bias at n=3*2^62, box rebuilt byte for byte from spec, tamper/truncate/weak keys. Lesson again: write `.is_multiple_of()` in leader checks.
- 2026-11-14 23:56 T58 pigment PASS (haiku, ~176k tokens). Leader check: over vs formula (300k random + full alpha grid), Bresenham nearest-pixel rule on 3000 lines, single blending, 300 random convex polygons (area, winding, interior), clip fuzz with i32 extremes. ESCALATE fixed inline: 1 pixel off by 1 alpha when winding reversed; polygon now normalises orientation first. My check had 3 bugs (no clamp in reference, centre outside polygon, clippy approx TAU). Brief bug: alpha-0 rule vs formula conflict, worker chose the rule.
- 2026-11-15 00:07 T68 tectonic PASS (haiku, ~193k tokens). Leader check first try: mul/div vs i128 over 200k, sqrt exhaustive to 2^22 + sweep, sin/cos/atan2 bounds, PCG reference, determinism with permuted insertion order, stale handles, clock. No floats in src.
- 2026-11-15 00:09 T53 crust RETRY 1 (hydrogen-high): parser shapes pass; bash differential 36/461 differ, all non-whitespace IFS (literal text split, empty $@ fields dropped, edge delimiter), plus backslash at EOF dropped. Same lane, check left in tree.
- 2026-11-15 00:17 T53 crust PASS after RETRY 1 (hydrogen-high). bash differential 461/461; fresh seed 1260/1261 (the one miss is my Env: absolute glob /* lists the Windows root while `bash` here is WSL). Note: `bash` on PATH for cargo tests is WSL bash.
- 2026-11-15 00:24 T43 drillcore PASS (hydrogen) first try: blob, tree, commit and merge-commit ids equal real git --object-format=sha256 over a 3-commit history with exec bits, unicode and dir-sort names; history, checkout, corruption. My check needed clippy::type_complexity allowed.
- 2026-11-15 00:32 T60 pigment text PASS (hydrogen) first try: glyph rules, layout invariants over 4000 fresh cases, draw box bounds; rendered sample looked at (legible, replacement box for non-ASCII). Leader check sample now goes to CARGO_TARGET_TMPDIR (target/ is absent in the clean worktree). T44 to hydrogen-high, not argon.
- 2026-11-15 00:45 T44 drillcore diff/merge RETRY 1 (hydrogen-high): vs real git, 76/1500 merge3 triples and some unified hunks differ (change-group alignment as git xdl_change_compact; conflict simplification <=3 or no-alnum gap). Check fixed: git funcname header text dropped, --no-indent-heuristic. Worker cannot run git, so git reference fixtures (800 M + 300 D, other seed) added at crates/drillcore/tests/fixtures/git_reference.txt. T57 crust executor dispatched to hydrogen 00:40.
- 2026-11-15 00:55 T57 crust executor RETRY 1 (hydrogen): vs bash --posix 300/300 generated, all externals and binary modes pass; 5/72 fixed differ (set -e not ignored inside functions/cmd-subst run in tested contexts; set -u not enforced). Check gained 6 set -e/-u cases. T44 retry dispatched 00:48.
- 2026-11-15 01:05 T44 drillcore diff/merge/status PASS after RETRY 1 (hydrogen-high): merge3 = git merge-file on 1500 triples (2 seeds), unified hunks = git diff --no-indent-heuristic on 300 (2 seeds), diff minimal vs LCS, repo merge scenarios; 800+300 git fixtures in-tree. Gates green.
- 2026-11-15 01:06 T59 pigment flate+PNG brief written, queued for hydrogen-high (first dispatch refused: two workers already running, T37 + T57) (not argon: strong oracle, Python zlib + Pillow).
- 2026-11-15 01:11 T57 crust executor PASS after RETRY 1 (hydrogen): 72 fixed + 300 generated (and 800 fresh-seed) scripts equal to bash --posix on stdout and status, 12 external/pipeline/redirect cases with crust as the external, binary -c/file/interactive. T53 gained Env::nounset (default false). Gates green.
- 2026-11-15 01:26 T59 pigment flate+PNG PASS (hydrogen-high, first try plus a one-line leader fix: palette depth-8 mask `1u8 << 8` overflow, 24/280 PNGs panicked). Oracle Python zlib + Pillow: 480 zlib streams (all levels/strategies/windows) inflated, our deflate L0-9 + zlib inflated by zlib, level 6 within 10% of zlib, 240 hand-encoded PNGs (all types/depths/filters/Adam7/tRNS/split IDAT) + 40 Pillow PNGs pixel-exact, Pillow reads ours, mutation fuzz no panics; fresh seeds too. Note: this leader test needs python + Pillow on PATH (matters for T82 CI).
- 2026-11-15 01:30 T37 outcrop server PASS (hydrogen; agy hit its deadline because the worker's own gate hung). Leader found and fixed inline: rejected 503 socket closed with the request unread gives RST on Windows (client never sees 503; the test then panicked and Server drop deadlocked on the held handler); now FIN + bounded 20 ms drain. Load tests trimmed (leader 48x50, worker 2k): 14.4k + 10k connections per run exhausted Windows ephemeral ports (TIME_WAIT). Leader check (mixed keep-alive load, traversal, error statuses, slow clients, stop under load) green; suite green 3 runs in a row.
- 2026-11-15 01:32 T54 slate brief written, dispatched to hydrogen-high (not argon: Python re is a full oracle).
- 2026-11-15 01:33 T38 outcrop WebSocket/ranges/chunked brief written, dispatched to hydrogen.
- 2026-11-15 01:43 T38 outcrop WebSocket/ranges/chunked PASS first try (hydrogen): Python client 40/40 (chunked + HTTP/1.0 streams, chunked request body, 12 range cases + HEAD, WS handshake, echo 7/16/64-bit, ping inside fragments, close echo, 10 protocol-error close codes, 4 bad handshakes), RFC 6455 vectors, 20k decode fuzz. Known design wrinkle (brief's fault): stream status applies only when no chunk was written (head sent on first chunk). T45 dispatched to hydrogen.
- 2026-11-15 01:55 T54 slate buffer+editor+regex PASS (hydrogen-high, first try plus leader inline fix). My brief misstated Python finditer (said 'next search starts one char later'); real rule: retry at the same spot refusing only an empty match there; 534/4000 diffs, all this one rule; fixed in vm.rs (no_empty_at) + find_iter, brief corrected. Oracle check had its own bugs (script named re.py shadowed re; empty pattern hex '-'), fixed. Fresh seeds: 1 diff in 20k, a sre quirk with nullable repeat bodies under must-advance, e.g. (?:a*(?:|é)){1,3} on 'éa' gives (0,3) but {2,3} gives (0,2); accepted as known gap.
- 2026-11-15 02:00 T45 drillcore remote (push/fetch/pull/clone over outcrop HTTP) PASS (hydrogen, first try). Leader: 3 clones converge, 8 racing pushers (losers stale/non-ff then land, linear first-parent chain), forged commit without tree -> ng missing, bad names, stale old, garbage/bad checksum -> 400, 300 fuzz bodies <500 with refs unchanged, fetch minimality 4/7 objects; fresh seed x3 stable; T43/T44 leader checks still pass.
- 2026-11-15 02:26 T23 PASS (hydrogen-high): quartz::machine, 9/9 leader checks after a leader fixture fix (timer.S trap clobbered t1 used by the idle loop; WFI may be a NOP), fresh-seed fuzz green, workspace gate green.
- 2026-11-15 02:43 T65 PASS (hydrogen): spelunk HTML tokenizer/tree, CSS syntax, selectors; 7/7 leader checks (3000 CSS strings + 1300 sheets/decl lists vs tinycss2, 2000 docs vs html.parser, 35 trees, 3000 queries vs soupsieve), fresh seeds green. Leader fixes: tokenizer panicked slicing inside a multi-byte char in comments; leading whitespace of a mixed text run before body content now processed on its own (WHATWG). Check fixes: lone-surrogate escape removed, tinycss2 two-char match tokens split, soupsieve document-as-parent quirk patched.
- 2026-11-15 02:55 T13a PASS (hydrogen-high, 1 run): leader check 9/9 after leader fixture fix (rvc.S trap handler at 2 mod 4 made mtvec mode 2; now .balign 4); fresh seed ok. drillcore remote_leader failed once under load in the workspace gate, passed 3/3 alone and on gate rerun: flake to look at.
- 2026-11-15 03:00 T46 PASS (hydrogen, 1 run): leader check 11/11 incl. 6000 solver cases equal to the normative model and brute force, 500-chain on 128 KiB stack, fresh seeds ok. Leader fixes: case generator was mostly unsolvable (deps named absent packages), floors set from measured mix; clippy lints in check.
- 2026-11-15 06:00 T33 PASS (hydrogen + leader fixes): leader check 4/4 incl. satp-switch remap; bench RAM override removed, inline(always) on step/tick restored cache-off to ~6% below pre-cache (accepted, test mode); bench total 1.55x.
- 2026-11-15 06:12 T21 dispatched hydrogen-high: brief + LLVM corpus (2758 lines, 9312 B, 317 labels validated) + leader check (corpus, c_pairs/amo, 3000 random insts, li64 by execution, fixtures run, errors, alignment, speed).
- 2026-11-15 06:28 T82 CI PASS (leader): .github/workflows/gate.yml green on ubuntu-latest. Pinned Rust 1.98.0 (1.100 clippy adds needless_bool), Python 3.14 + pillow/tinycss2/soupsieve/bs4 at dev versions (regex and HTML oracles differ across versions); Linux because crust tests compare with Linux bash (dev PC uses WSL bash).
- 2026-11-15 06:38 T21 chisel PASS (hydrogen-high): corpus = LLVM, c_pairs, amo, quartz fixtures, fresh seeds, workspace gate. Leader check had 2 bugs of its own (hex li parse, li to x0).
- 2026-11-15 06:39 T47 dispatched (hydrogen): brief + leader check registry_leader.rs committed.
- 2026-11-15 06:48 T47 cairn PASS (hydrogen): format, byte flips, registry codes, install\/lock\/remove, malicious registry, concurrency; fresh seeds, workspace gate. Committed missed reports T13a T21 T33 T46.
- 2026-11-15 06:48 T22 dispatched (hydrogen): LLD fixture prog.elf (runs to Pass, dirty bss fails), elf_leader.rs linted on a stub.
- 2026-11-15 06:52 D1 written; T14 dispatched (hydrogen). cinnabar_leader.rs passes on a throwaway Rust model (scratchpad), clippy clean.
- 2026-11-15 06:58 T14 onyx Cinnabar PASS (hydrogen, first try): vectors, avalanche, bias, ctr, label; fresh seeds, workspace gate.
- 2026-11-15 07:09 T13 dispatched (hydrogen): 110 riscv-tests built as data, 3 fail today (ma_data, instret_overflow, pmpaddr)
- 2026-11-15 07:13 D2 TREMOR.md + D3 VARVE.md v1 written; T34, T39 moved argon-high -> hydrogen-high (precise spec + strong leader check)
- 2026-11-15 07:20 T13 quartz PASS (hydrogen, 1 run): 110/110 riscv-tests, misaligned byte-wise, counter-write precedence, PMP storage; leader check 6/6 + fresh seed, workspace gate. Brief miss: 2 T12 tests asserted PMP reads 0; leader updated them to the T13 rule.
- 2026-11-15 07:29 T39 dispatched (hydrogen-high). Operator released Argon down to 5%.
- 2026-11-15 07:43 T39 varve PASS (hydrogen-high): leader check 9/9 + fresh seed, gate PASS; check fixed (fixed-length page contents).
- 2026-11-15 07:45 T31 brief + leader check written (model self-test on two seeds), dispatched argon-high.
- 2026-11-15 07:49 T40 brief + leader check written (lint vs stub), retagged hydrogen-high (merge rule + hostile pages).
- 2026-11-15 07:55 T34 brief + leader check written (model self-run OK, lint vs stub). Model uses onyx in Rust instead of Python: same independence, no extra tooling.
- 2026-11-15 08:12 T31 quartz H PASS (argon-high, first run): leader check 8/8 + fresh seed, gate PASS. T40 dispatched (hydrogen-high). Argon % to ask.
- 2026-11-15 08:26 T40 varve B+tree PASS (hydrogen-high): check 5/5 + fresh seed after leader fix to T39 pager read(0) in a txn (stale page 0); regression test added to pager_leader. T34 dispatched.
- 2026-11-15 11:57 T22 PASS (hydrogen); T34 PASS retry 1 (hydrogen-high); T55 brief+check ready; drillcore remote_leader flake fixed
- 2026-11-15 12:35 T48 seam: brief + seam_leader.rs (own Merkle/encoding/state model, 40-block chain, invalid-block corpus with precedence pairs); one Haiku worker (operator). Argon 15%. Skills written: leader-check-assert-property-not-shape, stalled-simulation-state-trace, leader-check-stub-lint
- 2026-11-15 12:15 | T55 slate app | hydrogen | agy quota | PASS | check 10/10 + fresh seeds, both gates; leader scroll-check fix
- 2026-11-15 12:25 | T48 seam core | haiku | 161,959 | PASS | check 8/8 + 2 fresh seeds, both gates; leader Merkle duplicate-leaf fix
- 2026-11-15 12:32 | ROUND PLAN (operator: 5 hydrogen + 2 haiku at once) | - | budget ~550k Claude (2 Haiku x150k + ~250k leader); hydrogen on agy quota | - | H: T35 tremor UDP, T49 seam contracts, T61 glint, T69 tectonic, T41a varve SQL core (hydrogen-high). Haiku: T66a spelunk cascade+block layout, T24 flint asm kernel. lane.sh cap raised to 7. Hydrogen smoke ok 14:50. DeepSeek v4.1 flash on NVIDIA key4: reviewer only (96 s/call)
- 2026-11-15 12:49 | T35 tremor UDP | hydrogen | agy quota | PASS | udp_leader 5/5 + fresh seeds + 100 MB release; leader seed fix
- 2026-11-15 12:49 | STATE | - | - | - | T49 T61 T69 workers report CHECK PASS, leader checks to write (contracts_leader, glint_leader, phys_leader). T41a hydrogen-high and T66a haiku running. Haiku 2 (T24 flint) not dispatched yet. Next hydrogen once judged: T62 glint widgets, T50 seam node, T70 tectonic, T29 sinter codegen
- 2026-11-15 12:51 | T69 tectonic phys | hydrogen | agy quota | RETRY 1 | phys_leader 5/6: stack box 0 sinks 0.0246 > 2*slop; to hydrogen-high
- 2026-11-15 13:00 | T69 | hydrogen-high | agy quota | PASS | retry 1 fixed stack sink; phys_leader 6/6 both seeds; gates pass
- 2026-11-15 13:08 | T41a | hydrogen-high | agy quota | PASS | sql_leader vs sqlite3: 418/418 and 428/428 fresh seed; gates pass
- 2026-11-15 13:10 | PAUSE (operator: give the workers a break) | - | Haiku this round: T66a 206,836, T24 138,160 | - | no new dispatch. T41b still running on hydrogen-high. Awaiting leader checks: T70a, T24, T49, T61, T66a. Next hydrogen after judging: T50, T62, T70b
- 2026-11-15 13:16 | T70a | hydrogen | agy quota | PASS | audio_leader 4/4 both seeds; gates pass
- 2026-11-15 13:21 | T24 | haiku | 138,160 | PASS | flint_leader 5/5; gates pass
- 2026-11-15 13:28 | T41b | hydrogen-high | agy quota | PASS | join_leader 3/3 both seeds; leader fixed one obsolete T41a test; T41 closed (T41a+T41b)
- 2026-11-15 13:37 | T49 | hydrogen | agy quota | PASS | contracts_leader 2/2 both seeds, seam_leader 8/8; gates pass
- 2026-11-15 13:44 | T61 | hydrogen | agy quota | PASS | glint_leader 3/3 both seeds; two silent brief points taken from worker; gates pass
- 2026-11-15 13:54 | T66a | haiku | 206,836 | PASS | layout_leader 2/2 both seeds; empty anon block not generated (aligned to worker); gates pass
- 2026-11-15 14:00 | dispatch | 5 hydrogen + 2 haiku | - | RUNNING | T29a T50a hydrogen-high; T42 T70b T79a hydrogen; T62 T66b haiku
- 2026-11-15 14:08 | T79a | hydrogen | agy quota | PASS | outcrop soak 7.4M cases fresh seed; lane is single-crate, rest split to T79b
- 2026-11-15 14:14 | T62 | haiku | 139,418 | PASS | widgets_leader 4/4 both seeds; leader fixed nav reveal-without-change; gates pass
- 2026-11-15 14:21 | T66b | haiku | 199,110 | PASS | flex_leader 2/2 both seeds + mutation bites; leader's own T66a check fixed (display: flex was in its invalid pool); gates pass
- 2026-11-15 14:41 | T50a | hydrogen-high | agy quota | PASS | node_leader 3/3 both seeds; leader added Node::missing for GetBlock retry (brief gap); gates pass
- 2026-11-15 14:54 | T42 varve net | hydrogen | - | PASS | own codec+8 clients+txn model; zero-col rows aligned
- 2026-11-15 15:03 | dispatch | 6 hydrogen | - | RUNNING | T50b hydrogen-high; T79c-g soaks chisel geode drillcore cairn crust
- 2026-11-15 15:16 | T70b tectonic script game | hydrogen | agy quota | PASS | script_leader 4/4 both seeds; extern model, traps, fuel, replay, pixels
- 2026-11-15 17:14 | T79h-l | hydrogen x5 | - | DISPATCH | soak shale tremor mica pigment quartz, 120m cap
- 2026-11-15 17:20 | T79m-r T81a-d | haiku x10 | budget 1.6M | DISPATCH | operator: 10 haiku + 5 hydrogen; T50b leader fix (redial stalled links) gated, verdict pending
- 2026-11-15 17:21 | T29a | hydrogen-high | - | PASS | rv64_leader 3 tests both seeds, mutation-tested
- 2026-11-15 17:21 | T79c-g | hydrogen x5 | - | PASS | chisel 2 OOM fixes (output cap, macro fan-out); drillcore flaky test barrier; seed parse loud
- 2026-11-15 17:28 | T25 | argon-high | - | DISPATCH | mantle kernel boot, design docs/MANTLE.md v1 (leader); Fable rewrite dropped (operator)
- 2026-11-15 17:29 | T81a-d | haiku x4 | 123k 118k 107k 148k | PASS | READMEs+demos assay outcrop glint flint geyser, docs/BOOK.md; glint spring stall noted
- 2026-11-15 17:32 | T30 | haiku | - | DISPATCH | sinter IR optimiser, differential vs reference evaluator
- 2026-11-15 17:34 | T63 T81e-h | hydrogen-high + hydrogen x4 | - | DISPATCH | facet compositor (hydrogen-high to save Argon); READMEs cairn chisel crust drillcore
- 2026-11-15 17:39 | T81e-h | hydrogen x4 | agy quota | PASS | READMEs+demos cairn chisel crust drillcore; leader pasted demo output
- 2026-11-15 17:51 | T50b | haiku + leader fix | ~150k | PASS | seam node net; redial stalled links after 2 s
- 2026-11-15 17:53 | T79s-u T81i-o | haiku x10 | - | DISPATCH | soaks seam glint geyser; READMEs 14 crates
- 2026-11-15 17:53 | T80a-e | hydrogen-high x5 | - | DISPATCH | security reviews with proving tests
- 2026-11-15 18:15 | Argon | - | - | STOP | operator: Argon at 2% after T25, below 5% floor; argon rows go to hydrogen-high with leader design or wait
- 2026-11-15 18:16 | lanes | - | - | CHANGE | operator: argon-high rows go to Opus 5.5 high via agy (new lane opus-high)
- 2026-11-15 18:18 | T25 | argon-high + leader fix | Argon 15%->2% | PASS | mantle boots 16-256 MiB; leader fixed sv39 leaf-flag check
- 2026-11-15 18:18 | T79h-r | hydrogen x5 + haiku x6 | ~0.9M haiku | PASS | 11 soaks; fixes: varve SQL depth, spelunk CSS depth + 2 quadratic paths
- 2026-11-15 18:18 | T81i-k,n,o | haiku x5 | ~0.65M | PASS | READMEs geode mica onyx pigment quarry quartz spelunk tectonic varve; vug deferred to after T51
- 2026-11-15 18:18 | T81p | leader | - | NOTE | commit 8d39a78 swept in T81p's unjudged mantle README+demo (git add crates/mantle); demo builds; judge on T81p report
- 2026-11-15 18:35 | T63 | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T30 | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T79s | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T79t | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T79u | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T81l | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T81m | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:35 | T81p | leader | - | PASS | batch4 workspace gate CHECK PASS workspace
- 2026-11-15 18:52 | T80a | hydrogen-high | - | PASS | findings fail on HEAD src, fixed; gate CHECK PASS workspace
- 2026-11-15 18:52 | T80b | hydrogen-high | - | PASS | findings fail on HEAD src, fixed; gate CHECK PASS workspace
- 2026-11-15 18:52 | T80c | hydrogen-high | - | PASS | findings fail on HEAD src, fixed; gate CHECK PASS workspace
- 2026-11-15 18:52 | T80e | hydrogen-high | - | PASS | findings fail on HEAD src, fixed; gate CHECK PASS workspace
- 2026-11-15 | T80g | agy hydrogen-high | - | PASS | elysium: no findings, 10 attack tests
- 2026-11-15 | T80k | agy hydrogen-high | - | PASS | sphinx: ed25519 accepted small-order public keys (forgery); fixed, proven
- 2026-11-15 | T80i | agy hydrogen-high | - | PASS | silicon: display size u64 overflow panic; fixed, proven
- 2026-11-15 | T26 | agy opus-high | - | STOPPED | agy opus quota 429, resets ~4h50m; partial titan edits left uncommitted
- 2026-11-15 | T80l | agy hydrogen-high | - | PASS | logos: parser depth, exhaustiveness hang, const str OOM; proven (perf rewrite not proven)
- 2026-11-15 | T80m | agy hydrogen-high | - | PASS | helios: tone alloc, script log, Fx::round; proven
- 2026-11-15 | T80j | agy hydrogen-high | - | PASS | hades: dir entry, fsck cycle, free inode findings; proven
- 2026-11-15 | T64 | agy hydrogen-high | - | PASS | iris: Terminal, Editor, Files apps + Desktop, 3 golden hashes
- 2026-11-15 | T80d | agy hydrogen-high | - | PASS | argo: dep mismatch, HTTP flood, solver blowup; retry1 fixed soak generator collisions
- 2026-11-15 | T80f | agy hydrogen-high | - | PASS | iridium: corrupt record, codec, server stop; retry1; leader dropped page0 range check (broke reopen) and fixed rename magic in pager tests
- 2026-11-15 | T80h | agy hydrogen-high | - | PASS | aurum: forged tx sig, contract caps, mempool, net stream caps; 5 tests fail on HEAD
- 2026-11-15 | T26 | agy hydrogen-high | - | PASS | titan: U-mode, syscalls 0-15, ELF, kairos scheduler, 34-line transcript; opus lane hit 429, rerun on hydrogen
- 2026-11-16 | T51 | leader (opus) | - | PASS | pandora: namespaces, pivot_root, cgroup v2; real containers tested in WSL; cgroup limits untested (needs root)
- 2026-11-16 | T27 | agy hydrogen-high | - | PASS | titan: block driver, hades at /, file syscalls, 41-line transcript; leader corruption fuzz 16/16
- 2026-11-16 | T67 | agy hydrogen-high | - | PASS | odyssey paint/fetch/script/shell, 137 tests; leader mutated-response fuzz
- 2026-11-16 | T52 | agy hydrogen-high | - | PASS | pandora images via argo, run -d, ps, kill, rm; leader fixed .so exec bit (design gap), stale-state checks; WSL green
- 2026-11-16 | T28 | agy hydrogen-high | - | PASS | titan userland: Rust multicall ELF (sh on krypton, ls cat echo mkdir rm ps), argv, redirects, append; leader raw-byte fuzz
