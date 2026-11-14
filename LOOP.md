# Strata loop board (the single source of truth; re-read this, not the chat)

## State
- LEADER: Claude Opus since 2026-11-14 16:15
- heartbeat: 2026-11-14 17:34
- phase: P1 Cores. T04, T05, T07, T09, T10, T12, T16, T17, T19 PASS.
- stop reason: none
- Next for the leader (18:52, Claude usage limit hit): judge T18 when argon returns (check parked in briefs/leader/sinter_leader.rs). Briefs ready: T20 (haiku), T37 (hydrogen). Then T53, T43, T23 (not argon), T13 (needs riscv-tests download: ask operator).
- Stand-in or second judge: Gemini Orchestrator through agy (command in docs/PLAN.md section 9).
- At most one Gemini worker (through `bash tools/lane.sh <lane> <id>`) plus one Claude Haiku worker, on different crates. Lanes and rules: docs/PLAN.md section 2.

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
5. Strata OS boots to a shell in the emulator, natively and in the browser.
6. The hypervisor runs two guest kernels at once.
7. A Mica program compiles to RV64, runs in Strata OS, and gives the same output on the VM.
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
| T01 | P1 | onyx | SHA-2, HMAC, HKDF, ChaCha20-Poly1305 | haiku | | PASS |
| T02 | P1 | quarry | buddy + slab allocator, GlobalAlloc | haiku | | PASS |
| T03 | P1 | quartz | RV64IM decode, encode, execute | haiku | | PASS |
| T06 | P1 | shale | block layer, on-disk format, mkfs, read/write path | haiku | | PASS |
| T08 | P1 | mica | language design `docs/MICA.md` + bytecode `docs/MBC.md` | leader | | PASS |
| T11 | P1 | geode | module, verifier, binary encode/decode (`docs/MBC.md`) | haiku | T08 | PASS |
| T05 | P1 | geyser | fair scheduler core + simulator | hydrogen |  | PASS |
| T09 | P1 | mica | finish parser, printer, corpus, fuzz (files exist: read them first) | hydrogen | T08 | PASS |
| T04 | P1 | onyx | X25519, Ed25519 (brief written) | haiku | T01 | PASS |
| T17 | P1 | geode | interpreter, heap, mark-sweep GC, fuel, `Host` trait | argon | T11 | PASS |
| T10 | P1 | mica | type checker, layer checker, IR | argon-high | T09 | PASS |
| T19 | P1 | mica | reference interpreter for the typed IR (oracle for T18, T29, T30) | hydrogen | T10 | PASS |
| T18 | P1 | sinter | Mica IR to Geode bytecode; `mica run`, `mica test` commands | argon | T10 T17 T19 | PASS |
| T07 | P1 | shale | journal, fsck, crash injection at every write | haiku | T06 | PASS |
| T12 | P1 | quartz | Zicsr, M/S/U modes, traps, Sv39 paging | haiku | T03 | PASS |
| T13 | P1 | quartz | A and C extensions, official riscv-tests as data | hydrogen | T12 | TODO |
| T16 | P1 | quarry | const constructor, global allocator, big alignments, `check()` | haiku | T02 | PASS |
| T15 | P1 | geyser | work-stealing thread pool on std threads, driven by the core | haiku | T05 | PASS |
| T14 | P1 | onyx | Cinnabar: own cipher from `docs/CINNABAR.md`, labelled experimental | hydrogen | T01 D1 | TODO |
| T20 | P1 | onyx | ChaCha20 random generator, sealed-box helper | haiku | T04 | PASS |
| T21 | P2 | chisel | RV64IMAC assembler: text, labels, directives | hydrogen | T13 | TODO |
| T22 | P2 | chisel | ELF64 writer and loader | hydrogen | T21 | TODO |
| T23 | P2 | quartz | devices: UART, timer, interrupt controller, block, framebuffer, keys; `Machine` | argon | T12 | TODO |
| T24 | P2 | flint | kernel in pure assembly: boot, traps, console, timer, 2 tasks, 3 syscalls | argon | T22 T23 | TODO |
| T25 | P2 | mantle | Rust kernel: boot shim, traps, Sv39, heap (quarry), console | argon-high | T23 T16 | TODO |
| T26 | P2 | mantle | processes, user mode, syscalls, scheduler (geyser), ELF loading | argon-high | T25 T05 | TODO |
| T27 | P2 | mantle | block driver, shale mounted, file syscalls | argon | T26 T07 | TODO |
| T28 | P2 | mantle | userland: init, shell (crust), ls cat echo mkdir rm ps | hydrogen | T27 T57 | TODO |
| T29 | P2 | sinter | Mica IR to RV64 code, linear-scan registers, ELF out | argon-high | T10 T22 | TODO |
| T30 | P2 | sinter | constant folding, dead code, copy propagation | hydrogen | T29 | TODO |
| T31 | P2 | quartz | hypervisor extension: HS, VS, VU, two-stage paging | argon-high | T13 | TODO |
| T32 | P2 | bedrock | hypervisor: 2 guests (flint + mantle), virtual console and timer, isolation | argon-high | T31 T24 T26 | TODO |
| T33 | P2 | quartz | speed: decoded-block cache, benchmark | hydrogen | T23 | TODO |
| T34 | P3 | tremor | protocol core from `docs/TREMOR.md`, no I/O inside | argon-high | T04 T20 D2 | TODO |
| T35 | P3 | tremor | UDP transport, echo and file-transfer tools | hydrogen | T34 | TODO |
| T36 | P3 | outcrop | HTTP/1.1 parser and writer, no I/O inside | hydrogen |  | PASS |
| T37 | P3 | outcrop | server: thread pool, keep-alive, static files, routes, limits, graceful stop | hydrogen | T36 T15 | RUNNING |
| T38 | P3 | outcrop | WebSocket, ranges, chunked bodies | hydrogen | T37 | TODO |
| T39 | P3 | varve | pager, write-ahead log, crash recovery | argon-high | D3 | TODO |
| T40 | P3 | varve | B+tree with range scans | hydrogen | T39 | TODO |
| T41 | P3 | varve | SQL: parser, planner, executor, indexes, transactions | argon-high | T40 | TODO |
| T42 | P3 | varve | server, client tool, one writer and many readers | hydrogen | T41 T37 | TODO |
| T43 | P3 | drillcore | objects, refs, index, commit, log, branch, checkout | hydrogen | T01 | TODO |
| T44 | P3 | drillcore | diff (Myers), 3-way merge, status | argon | T43 | TODO |
| T45 | P3 | drillcore | push and pull over HTTP (outcrop) | hydrogen | T44 T37 | TODO |
| T46 | P3 | cairn | manifest, versions, dependency solver, lock file | argon |  | TODO |
| T47 | P3 | cairn | registry on outcrop, signed packages (Ed25519), install, remove | hydrogen | T46 T04 T37 | TODO |
| T48 | P3 | seam | blocks, Merkle tree, signed transactions, state, validation | argon | T04 | TODO |
| T49 | P3 | seam | contracts: Mica on Geode, fuel as gas, `chain` layer | hydrogen | T48 T18 | TODO |
| T50 | P3 | seam | node: gossip over tremor, sync, fork choice | argon-high | T49 T35 | TODO |
| T51 | P3 | vug | Linux container runtime: namespaces, cgroups v2, pivot root (runs in WSL) | argon-high |  | TODO |
| T52 | P3 | vug | images as cairn packages, `run`, `ps`, `kill` | hydrogen | T51 T47 | TODO |
| T53 | P3 | crust | shell language: parser, expansion, globbing | hydrogen-high |  | RUNNING |
| T57 | P3 | crust | executor, pipes, redirects, builtins, job control | hydrogen | T53 | TODO |
| T54 | P3 | slate | buffer (piece table), undo tree, multi-cursor, own regex engine | argon |  | TODO |
| T55 | P3 | slate | terminal interface, key maps, files, Mica highlighting | hydrogen | T54 T09 | TODO |
| T58 | P4 | pigment | framebuffer, lines, rectangles, anti-aliased polygons, blending | haiku |  | RUNNING |
| T59 | P4 | pigment | inflate, deflate, PNG read and write | argon | T58 | TODO |
| T60 | P4 | pigment | own bitmap font, text layout, wrapping | hydrogen | T58 | TODO |
| T61 | P4 | glint | widget tree, flex layout, events, focus, themes | argon | T60 | TODO |
| T62 | P4 | glint | widgets (button, field, list, scroll, tabs, menu), spring and ease animation | hydrogen | T61 | TODO |
| T63 | P4 | facet | compositor: windows, tiling and floating, damage, input, animated moves | argon | T62 | TODO |
| T64 | P4 | facet | client protocol + apps: terminal (crust), editor (slate), files | hydrogen | T63 T57 T55 | TODO |
| T65 | P4 | spelunk | HTML and CSS parsers | hydrogen |  | TODO |
| T66 | P4 | spelunk | cascade, block and inline layout, flex subset | argon-high | T65 T60 | TODO |
| T67 | P4 | spelunk | paint, fetch over HTTP, page scripts in Mica, browser shell in glint | argon | T66 T62 T18 T37 | TODO |
| T68 | P4 | tectonic | entity system, fixed time step, maths | hydrogen |  | TODO |
| T69 | P4 | tectonic | 2D renderer, collisions and impulse physics | argon | T68 T58 | TODO |
| T70 | P4 | tectonic | audio mixer, scenes, Mica scripting, one original small game | hydrogen | T69 T18 | TODO |
| T71 | P5 | web | WebAssembly builds with our own glue (no wasm-bindgen): Quartz + Mantle on a canvas, Mica playground | argon | T28 T18 | TODO |
| T72 | P5 | lapidary | IDE shell: editor, file tree, terminal, run, test | claude | T71 T55 | TODO |
| T73 | P5 | lapidary | language service: errors, hover, go to definition, format | hydrogen | T10 T72 | TODO |
| T74 | P5 | lapidary | debugger for Geode: breakpoints, step, inspect | hydrogen | T17 T72 | TODO |
| T75 | P5 | site | the Strata site: 25 systems, each with a live demo, scroll-driven motion | claude | T71 | TODO |
| T76 | P5 | site | accessibility, performance, cross-browser pass | claude | T75 | TODO |
| T77 | P6 | deploy | static binaries, container images, compose file, health checks | hydrogen | T42 T50 T47 | TODO |
| T78 | P6 | bench | load tools and numbers for outcrop, varve, tremor, seam in `docs/BENCH.md` | hydrogen | T77 | TODO |
| T79 | P6 | fuzz | one-hour soak of every parser and decoder with new seeds | hydrogen | all | TODO |
| T80 | P6 | review | security review of every crate, one report each | argon | all | TODO |
| T81 | P6 | docs | README and runnable example per system, architecture book | flash | all | TODO |
| T82 | P6 | ci | GitHub Actions workflow for the gate | hydrogen |  | TODO |
| T83 | P6 | release | version tag, change log, demo video | leader | all | TODO |

Later phases get their task rows when P1 is green (see the build order in `docs/SPEC.md`):
P2 chisel, sinter, quartz devices, flint, mantle, bedrock. P3 tremor, outcrop, varve, drillcore,
cairn, seam, vug, crust, slate. P4 pigment, glint, facet, spelunk, tectonic. P5 web front, IDE, containers, load tests. P6 hardening.

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
