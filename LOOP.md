# Strata loop board (the single source of truth; re-read this, not the chat)

## State
- LEADER: Claude Opus since 2026-11-14 16:15
- heartbeat: 2026-11-15 06:30
- phase: P2 Machine. Running T21 chisel (hydrogen-high). CI live (T82). Argon released down to 5% (operator, 2026-11-15; was 29%): argon-high tasks may run; no tool shows usage, so the leader asks the operator for the % after each Argon run and stops at 5%.
- stop reason: none
- Next for the leader: see "Claude board" below (the strata-board mod shows it in a "Claude" pane, command `/strata-claude`).
- Stand-in or second judge: Gemini Orchestrator through agy (command in docs/PLAN.md section 9).
- At most one Gemini worker (through `bash tools/lane.sh <lane> <id>`) plus one Claude Haiku worker, on different crates. Lanes and rules: docs/PLAN.md section 2.

## Claude board (the leader's own work; `- [>]` doing, `- [ ]` next, `- [!]` blocked, `- [x]` done)
- [x] T33 judge + bench fixes, commit 049e3f8
- [x] T21 corpus (LLVM, validated), brief, leader check
- [x] T82 CI workflow, green on ubuntu-latest
- [x] T47 cairn registry PASS (leader check: clippy fixes only)
- [x] T21 chisel PASS (leader check fixed: hex li, x0)
- [>] T22 chisel ELF: LLD fixture + brief + leader check, dispatched (hydrogen)
- [x] D1 docs/CINNABAR.md (Python reference, vectors, stats; check validated on a model)
- [x] T14 onyx Cinnabar PASS (first try)
- [x] D2 docs/TREMOR.md, D3 docs/VARVE.md (v1); T34, T39 retagged hydrogen-high (spec exact, saves Argon)
- [x] T39 varve pager: brief + leader check (own sector-loss disk, crash at every op), dispatched (hydrogen-high)
- [>] T34 tremor: brief + tremor_leader.rs (handshake model from onyx primitives, own lossy simulator), dispatched (hydrogen-high)
- [ ] T55 slate app: brief + app_leader.rs (edit/motion tables, screens, scroll, files, Mica highlight, fuzz undo-all) ready; dispatch hydrogen when a lane frees
- [x] T13 quartz PASS: 110/110 official riscv-tests (leader updated 2 obsolete T12 PMP asserts)
- [ ] Write docs/MANTLE.md v1 before T25 (argon-high, needs Argon budget)
- [ ] Look into the drillcore remote_leader flake
- [x] T31 quartz H extension: brief + hyp_leader.rs (own two-stage model, coverage 22% ok / 46% pf / 31% gpf), dispatched (argon-high)
- [x] T40 varve B+tree: brief + tree_leader.rs (own page parser), dispatched (hydrogen-high)
- [ ] Argon (released to 5%, ask % after T31): then D4 MANTLE.md + T25, then T66

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
| T13 | P1 | quartz | official riscv-tests as data (needs download: operator) | hydrogen | T13a | PASS |
| T13a | P1 | quartz | A and C extensions, LLVM as oracle (split from T13) | hydrogen-high | T12 | PASS |
| T16 | P1 | quarry | const constructor, global allocator, big alignments, `check()` | haiku | T02 | PASS |
| T15 | P1 | geyser | work-stealing thread pool on std threads, driven by the core | haiku | T05 | PASS |
| T14 | P1 | onyx | Cinnabar: own cipher from `docs/CINNABAR.md`, labelled experimental | hydrogen | T01 D1 | PASS |
| T20 | P1 | onyx | ChaCha20 random generator, sealed-box helper | haiku | T04 | PASS |
| T21 | P2 | chisel | RV64IMAC assembler: text, labels, directives | hydrogen | T13a | PASS |
| T22 | P2 | chisel | ELF64 writer and loader | hydrogen | T21 | RUNNING |
| T23 | P2 | quartz | devices: UART, timer, interrupt controller, block, framebuffer, keys; `Machine` | hydrogen-high | T12 | PASS |
| T24 | P2 | flint | kernel in pure assembly: boot, traps, console, timer, 2 tasks, 3 syscalls | argon | T22 T23 | TODO |
| T25 | P2 | mantle | Rust kernel: boot shim, traps, Sv39, heap (quarry), console | argon-high | T23 T16 T13a | TODO |
| T26 | P2 | mantle | processes, user mode, syscalls, scheduler (geyser), ELF loading | argon-high | T25 T05 | TODO |
| T27 | P2 | mantle | block driver, shale mounted, file syscalls | argon | T26 T07 | TODO |
| T28 | P2 | mantle | userland: init, shell (crust), ls cat echo mkdir rm ps | hydrogen | T27 T57 | TODO |
| T29 | P2 | sinter | Mica IR to RV64 code, linear-scan registers, ELF out | argon-high | T10 T22 | TODO |
| T30 | P2 | sinter | constant folding, dead code, copy propagation | hydrogen | T29 | TODO |
| T31 | P2 | quartz | hypervisor extension: HS, VS, VU, two-stage paging | argon-high | T13a | PASS |
| T32 | P2 | bedrock | hypervisor: 2 guests (flint + mantle), virtual console and timer, isolation | argon-high | T31 T24 T26 | TODO |
| T33 | P2 | quartz | speed: decoded-block cache, benchmark | hydrogen | T23 | PASS |
| T34 | P3 | tremor | protocol core from `docs/TREMOR.md`, no I/O inside | hydrogen-high | T04 T20 D2 | RUNNING |
| T35 | P3 | tremor | UDP transport, echo and file-transfer tools | hydrogen | T34 | TODO |
| T36 | P3 | outcrop | HTTP/1.1 parser and writer, no I/O inside | hydrogen |  | PASS |
| T37 | P3 | outcrop | server: thread pool, keep-alive, static files, routes, limits, graceful stop | hydrogen | T36 T15 | PASS |
| T38 | P3 | outcrop | WebSocket, ranges, chunked bodies | hydrogen | T37 | PASS |
| T39 | P3 | varve | pager, write-ahead log, crash recovery | hydrogen-high | D3 | PASS |
| T40 | P3 | varve | B+tree with range scans | hydrogen-high | T39 | PASS |
| T41 | P3 | varve | SQL: parser, planner, executor, indexes, transactions | argon-high | T40 | TODO |
| T42 | P3 | varve | server, client tool, one writer and many readers | hydrogen | T41 T37 | TODO |
| T43 | P3 | drillcore | objects, refs, index, commit, log, branch, checkout | hydrogen | T01 | PASS |
| T44 | P3 | drillcore | diff (Myers), 3-way merge, status | hydrogen-high | T43 | PASS |
| T45 | P3 | drillcore | push and pull over HTTP (outcrop) | hydrogen | T44 T37 | PASS |
| T46 | P3 | cairn | manifest, versions, dependency solver, lock file | hydrogen |  | PASS |
| T47 | P3 | cairn | registry on outcrop, signed packages (Ed25519), install, remove | hydrogen | T46 T04 T37 | PASS |
| T48 | P3 | seam | blocks, Merkle tree, signed transactions, state, validation | argon | T04 | TODO |
| T49 | P3 | seam | contracts: Mica on Geode, fuel as gas, `chain` layer | hydrogen | T48 T18 | TODO |
| T50 | P3 | seam | node: gossip over tremor, sync, fork choice | argon-high | T49 T35 | TODO |
| T51 | P3 | vug | Linux container runtime: namespaces, cgroups v2, pivot root (runs in WSL) | argon-high |  | TODO |
| T52 | P3 | vug | images as cairn packages, `run`, `ps`, `kill` | hydrogen | T51 T47 | TODO |
| T53 | P3 | crust | shell language: parser, expansion, globbing | hydrogen-high |  | PASS |
| T57 | P3 | crust | executor, pipes, redirects, builtins, job control | hydrogen | T53 | PASS |
| T54 | P3 | slate | buffer (piece table), undo tree, multi-cursor, own regex engine | hydrogen-high |  | PASS |
| T55 | P3 | slate | terminal interface, key maps, files, Mica highlighting | hydrogen | T54 T09 | TODO |
| T58 | P4 | pigment | framebuffer, lines, rectangles, anti-aliased polygons, blending | haiku |  | PASS |
| T59 | P4 | pigment | inflate, deflate, PNG read and write | hydrogen-high | T58 | PASS |
| T60 | P4 | pigment | own bitmap font, text layout, wrapping | hydrogen | T58 | PASS |
| T61 | P4 | glint | widget tree, flex layout, events, focus, themes | argon | T60 | TODO |
| T62 | P4 | glint | widgets (button, field, list, scroll, tabs, menu), spring and ease animation | hydrogen | T61 | TODO |
| T63 | P4 | facet | compositor: windows, tiling and floating, damage, input, animated moves | argon | T62 | TODO |
| T64 | P4 | facet | client protocol + apps: terminal (crust), editor (slate), files | hydrogen | T63 T57 T55 | TODO |
| T65 | P4 | spelunk | HTML and CSS parsers | hydrogen |  | PASS |
| T66 | P4 | spelunk | cascade, block and inline layout, flex subset | argon-high | T65 T60 | TODO |
| T67 | P4 | spelunk | paint, fetch over HTTP, page scripts in Mica, browser shell in glint | argon | T66 T62 T18 T37 | TODO |
| T68 | P4 | tectonic | entity system, fixed time step, maths | haiku |  | PASS |
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
| T82 | P6 | ci | GitHub Actions workflow for the gate | hydrogen |  | PASS |
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
