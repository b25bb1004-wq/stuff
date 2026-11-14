# Strata: the complete plan
Written by Claude Fable on 2026-11-14 for whoever steers next (Claude Opus, or the Gemini
Orchestrator). Read order for a fresh session: `LOOP.md` (state), this file (what and how),
`AGENTS.md` (rules), then only the brief you are about to dispatch. `docs/SPEC.md` holds the
rules of the stack and the names of the 25 systems; do not re-plan it.

## 1. What is being built
All 25 "build your own X" systems as ONE stack, written in Rust with zero outside crates:
a RISC-V machine (Quartz) that runs our kernels (Flint in assembly, Mantle in Rust) under our
hypervisor (Bedrock), with our file system, scheduler and allocator inside, our language (Mica)
compiled by our compiler (Sinter) and run by our VM (Geode), our network protocol, web server,
database, version control, package manager, blockchain, containers, shell, editor, graphics
stack, browser and game engine on top, and a browser IDE (Lapidary) plus a site that shows it
all running live as WebAssembly. Each system must work alone and inside the stack.

## 2. Who does what (operator rules, 2026-11-14)
| Lane | Model | Used for | How to call |
|---|---|---|---|
| Architect | Claude Fable | This plan, language and protocol designs (section 7). Not for steering | operator switches `/model` |
| Leader | Claude Opus (Claude Code) | Steers the loop: writes briefs, judges, integrates, commits | `/model` then the start prompt in section 9 |
| Co-leader | Gemini Orchestrator (Argon) | Same job when Claude is out of tokens, or second judge | `agy --model gemini-pro-orchestrator-g1-pro` in this folder |
| Hard worker | Gemini Argon | Hard tasks, second reviews | `bash tools/lane.sh argon <id>` (or `argon-high`) |
| Default worker | Gemini Hydrogen | Most tasks | `bash tools/lane.sh hydrogen <id>` (retry: `hydrogen-high`) |
| Cheapest | Gemini Flash 3.8 | Docs, tables, corpus files | `bash tools/lane.sh flash <id>` |
| Text helper | Nemotron Ultra, MiniMax M3.1 Flash (NVIDIA key in WSL) | Draft prose only, no file access | `tools/probe_models.py` shows the call |
| Fallback | Claude Haiku (medium), Claude Sonnet | Only after a Gemini lane failed the same check twice | Agent tool, one at a time |
| Operator | Arnav | Keys, money, making anything public, deleting | ask and stop |

Facts checked on 2026-11-14 16:05: `agy` 4-series models answer from this PC (Argon, Hydrogen,
Flash, Orchestrator). There is no model called Carbon in `agy models`; Flash is the cheapest
lane. If Carbon appears later, add one line to `tools/lane.sh`. Kimi, GLM, DeepSeek, Gemma,
GPT-OSS and Qwen on the NVIDIA key time out or return 404.

Hard rules for the leader:
1. **One worker at a time.** `tools/lane.sh` holds a lock (`.loop-local/BUSY`). Finish, judge,
   commit, then start the next. No parallel sub-agents.
2. **Exactly one leader at a time.** The name in `LOOP.md` State is the leader. The other
   model may only do worker or review tasks handed to it.
3. **The leader writes no feature code.** It writes briefs and its own independent check.
   A fix under about 20 lines is cheaper done by the leader than re-dispatched.
4. **Never PASS on a report.** Run the gate yourself, run your own check, read the diff.
5. **Save Claude tokens.** Do not read worker transcripts; read `reports/<id>.md`, the gate
   output and `git diff --stat`. Read files with `head`, `grep`, `sed -n`. Clear the session
   after every 3 to 4 tasks; `LOOP.md` carries the state.

## 3. The loop for one task
1. Pick the first `TODO` row in `LOOP.md` whose `needs` are all `PASS` (section 5 order).
2. If `briefs/<id>.md` is missing, write it in the shape of `briefs/T05.md`: files it may
   touch, exact public API, behaviour, tests required, the leader's extra check. Fix the API in
   the brief; later crates are written against it.
3. Set the row to `RUNNING`, then `bash tools/lane.sh <lane> <id>`.
4. Judge: `bash tools/check.sh <crate>`, then your own check (section 6), then the diff.
5. Verdict in `LOOP.md` round log with the real clock time (`date`): `PASS`, `RETRY` (append
   one specific defect to the brief; same lane at `-high`, then Argon, then Claude fallback;
   two retries at most) or `ESCALATE`.
6. On PASS: `bash tools/check.sh && git add -A && git commit && git push` chained with `&&`.
   Commit message ends with the `Claude-Session:` line when Claude commits.
7. Update `heartbeat` in `LOOP.md`. Every 5 tasks update the vault note
   `C:\Users\ADMIN\spidermonkey-vault\10 Projects\Strata.md`.
Stop and write the reason in `LOOP.md` State when: a usage-limit warning appears, two tasks in
a row end in ESCALATE, or a task needs keys, money, publishing or deleting.

## 4. Where things stand (2026-11-14 16:10)
PASS: T00 assay, T01 onyx (SHA-2, HMAC, HKDF, ChaCha20-Poly1305), T02 quarry, T03 quartz
RV64IM, T06 shale core, T08 Mica and bytecode designs, T11 geode module and verifier.
Half done: T09 mica parser (lexer finished with 18 tests, parser tests missing; the Claude
worker stopped at the usage limit). RUNNING: T05 geyser on Hydrogen, the first agy trial.
Claude worker tokens spent: 948k. Leader files kept ready: `briefs/leader/asym_leader.rs`
(Python-made vectors for T04).

## 5. Every task, in order
Lane: H hydrogen, A argon, A+ argon-high, F flash, D design by the architect (section 7).
"Check" is the leader's own check besides the gate. Run tasks top to bottom; a row may start
when its needs are PASS.

### P1 Cores
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T05 | geyser | fair scheduler core + simulator | H | | own simulator, new seed |
| T09 | mica | finish parser, printer, corpus, fuzz (files exist: read them first) | H | T08 | programs written from `docs/MICA.md`; fuzz with new seed |
| T04 | onyx | X25519, Ed25519 (brief written) | A | T01 | copy `briefs/leader/asym_leader.rs` to `crates/onyx/tests/` |
| T17 | geode | interpreter, heap, mark-sweep GC, fuel, `Host` trait | A | T11 | hand-built modules; fuel and heap limits hold; fuzzed valid modules never panic |
| T10 | mica | type checker, layer checker, IR | A+ | T09 | reject-corpus: one program per rule in `docs/MICA.md` |
| T18 | sinter | Mica IR to Geode bytecode; `mica run`, `mica test` commands | A | T10 T17 | example programs give expected output; `test` and `prop` blocks run |
| T07 | shale | journal, fsck, crash injection at every write | A | T06 | cut power at each block write: mount always gives old or new state |
| T12 | quartz | Zicsr, M/S/U modes, traps, Sv39 paging | A+ | T03 | LLVM-assembled privilege tests (generator in section 6) |
| T13 | quartz | A and C extensions, official riscv-tests as data | H | T12 | all rv64ui, um, ua, uc, mi, si tests pass |
| T16 | quarry | const constructor, global allocator, big alignments, `check()` | H | T02 | shadow model re-run; Miri-style stress under threads |
| T15 | geyser | work-stealing thread pool on std threads, driven by the core | H | T05 | 1M tasks, no loss, no deadlock, clean shutdown |
| T14 | onyx | Cinnabar: own cipher from `docs/CINNABAR.md`, labelled experimental | H | T01 D1 | avalanche and bias statistics; never the default |
| T20 | onyx | ChaCha20 random generator, sealed-box helper | H | T04 | known-answer vectors from Python |

### P2 Machine
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T21 | chisel | RV64IMAC assembler: text, labels, directives | H | T13 | every word equal to LLVM for a 2000-line corpus |
| T22 | chisel | ELF64 writer and loader | H | T21 | `llvm-readelf` accepts it; Quartz boots it |
| T23 | quartz | devices: UART, timer, interrupt controller, block, framebuffer, keys; `Machine` | A | T12 | device register tests; a ROM that echoes UART |
| T24 | flint | kernel in pure assembly: boot, traps, console, timer, 2 tasks, 3 syscalls | A | T22 T23 | golden boot transcript, 1M instructions deterministic |
| T25 | mantle | Rust kernel: boot shim, traps, Sv39, heap (quarry), console | A+ | T23 T16 | boots to a banner on Quartz; page-fault tests |
| T26 | mantle | processes, user mode, syscalls, scheduler (geyser), ELF loading | A+ | T25 T05 | 3 user programs run and exit; kill and wait work |
| T27 | mantle | block driver, shale mounted, file syscalls | A | T26 T07 | write, reboot, read back; power cut mid-write |
| T28 | mantle | userland: init, shell (crust), ls cat echo mkdir rm ps | H | T27 T57 | scripted session transcript |
| T29 | sinter | Mica IR to RV64 code, linear-scan registers, ELF out | A+ | T10 T22 | same program, same output on Geode and on Quartz (quality bar 7) |
| T30 | sinter | constant folding, dead code, copy propagation | H | T29 | differential test: optimised equals unoptimised on 5000 random programs |
| T31 | quartz | hypervisor extension: HS, VS, VU, two-stage paging | A+ | T13 | LLVM-assembled H tests |
| T32 | bedrock | hypervisor: 2 guests (flint + mantle), virtual console and timer, isolation | A+ | T31 T24 T26 | both guests run; one guest cannot read the other's memory |
| T33 | quartz | speed: decoded-block cache, benchmark | H | T23 | same trace with cache on and off; MIPS in `docs/BENCH.md` |

### P3 Services
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T34 | tremor | protocol core from `docs/TREMOR.md`, no I/O inside | A+ | T04 T20 D2 | simulator: loss, reorder, duplicate, corruption up to 30 percent |
| T35 | tremor | UDP transport, echo and file-transfer tools | H | T34 | 100 MB transfer over a lossy local link, hash equal |
| T36 | outcrop | HTTP/1.1 parser and writer, no I/O inside | H | | request corpus + fuzz with new seed |
| T37 | outcrop | server: thread pool, keep-alive, static files, routes, limits, graceful stop | H | T36 T15 | own load tool: 10k connections, no leak, slow-client attack bounded |
| T38 | outcrop | WebSocket, ranges, chunked bodies | H | T37 | protocol vectors; fuzz |
| T39 | varve | pager, write-ahead log, crash recovery | A+ | D3 | crash at every write: committed data survives, uncommitted is gone |
| T40 | varve | B+tree with range scans | H | T39 | against `BTreeMap` shadow, 1M operations |
| T41 | varve | SQL: parser, planner, executor, indexes, transactions | A+ | T40 | 300 queries compared with SQLite by the leader (Python) |
| T42 | varve | server, client tool, one writer and many readers | H | T41 T37 | concurrent bank-transfer test, sum is constant |
| T43 | drillcore | objects, refs, index, commit, log, branch, checkout | H | T01 | round trip of a 2000-file tree |
| T44 | drillcore | diff (Myers), 3-way merge, status | A | T43 | merge cases compared with `git merge-file` |
| T45 | drillcore | push and pull over HTTP (outcrop) | H | T44 T37 | clone, change both sides, sync |
| T46 | cairn | manifest, versions, dependency solver, lock file | A | | solver against brute force on small random graphs |
| T47 | cairn | registry on outcrop, signed packages (Ed25519), install, remove | H | T46 T04 T37 | tampered package is refused |
| T48 | seam | blocks, Merkle tree, signed transactions, state, validation | A | T04 | invalid-block corpus all rejected |
| T49 | seam | contracts: Mica on Geode, fuel as gas, `chain` layer | H | T48 T18 | same contract, same state root on 3 nodes |
| T50 | seam | node: gossip over tremor, sync, fork choice | A+ | T49 T35 | 7-node simulation with partitions converges |
| T51 | vug | Linux container runtime: namespaces, cgroups v2, pivot root (runs in WSL) | A+ | | rootless run in WSL: own PID 1, no host files visible |
| T52 | vug | images as cairn packages, `run`, `ps`, `kill` | H | T51 T47 | start 50 containers, stop them, nothing left behind |
| T53 | crust | shell language: parser, expansion, globbing | H | | script corpus compared with `bash` output |
| T57 | crust | executor, pipes, redirects, builtins, job control | H | T53 | scripted sessions on Windows, WSL and (later) Mantle |
| T54 | slate | buffer (piece table), undo tree, multi-cursor, own regex engine | A | | regex against Python `re` on 5000 cases; edit shadow model |
| T55 | slate | terminal interface, key maps, files, Mica highlighting | H | T54 T09 | scripted key sessions give golden screens |

### P4 Pixels
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T58 | pigment | framebuffer, lines, rectangles, anti-aliased polygons, blending | H | | golden images, pixel exact |
| T59 | pigment | inflate, deflate, PNG read and write | A | T58 | files open in Python PIL; PIL files open in Pigment |
| T60 | pigment | own bitmap font, text layout, wrapping | H | T58 | golden images |
| T61 | glint | widget tree, flex layout, events, focus, themes | A | T60 | layout cases against expected boxes |
| T62 | glint | widgets (button, field, list, scroll, tabs, menu), spring and ease animation | H | T61 | golden frames at fixed times |
| T63 | facet | compositor: windows, tiling and floating, damage, input, animated moves | A | T62 | damage equals changed pixels; scripted sessions |
| T64 | facet | client protocol + apps: terminal (crust), editor (slate), files | H | T63 T57 T55 | scripted desktop session, golden frames |
| T65 | spelunk | HTML and CSS parsers | H | | fuzz; tree dumps for a page corpus |
| T66 | spelunk | cascade, block and inline layout, flex subset | A+ | T65 T60 | layout boxes for 60 test pages |
| T67 | spelunk | paint, fetch over HTTP, page scripts in Mica, browser shell in glint | A | T66 T62 T18 T37 | loads the Strata docs site served by Outcrop |
| T68 | tectonic | entity system, fixed time step, maths | H | | determinism: same inputs, same world hash |
| T69 | tectonic | 2D renderer, collisions and impulse physics | A | T68 T58 | energy bound, no tunnelling in stress scene |
| T70 | tectonic | audio mixer, scenes, Mica scripting, one original small game | H | T69 T18 | recorded input replays to the same end state |

### P5 Front
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T71 | web | WebAssembly builds with our own glue (no wasm-bindgen): Quartz + Mantle on a canvas, Mica playground | A | T28 T18 | boots to the shell in Chrome; same transcript as native |
| T72 | lapidary | IDE shell: editor, file tree, terminal, run, test | Claude | T71 T55 | section 8 design pass, screenshots desktop and mobile |
| T73 | lapidary | language service: errors, hover, go to definition, format | H | T10 T72 | scripted editor sessions |
| T74 | lapidary | debugger for Geode: breakpoints, step, inspect | H | T17 T72 | scripted debug session |
| T75 | site | the Strata site: 25 systems, each with a live demo, scroll-driven motion | Claude | T71 | section 8; reduced-motion and mobile pass |
| T76 | site | accessibility, performance, cross-browser pass | Claude | T75 | Lighthouse numbers recorded |

### P6 Scale and hardening
| Id | System | Deliverable | Lane | Needs | Check |
|---|---|---|---|---|---|
| T77 | deploy | static binaries, container images, compose file, health checks | H | T42 T50 T47 | images start, stop cleanly on SIGTERM |
| T78 | bench | load tools and numbers for outcrop, varve, tremor, seam in `docs/BENCH.md` | H | T77 | leader re-runs one number of each |
| T79 | fuzz | one-hour soak of every parser and decoder with new seeds | H | all | findings fixed, seeds added as tests |
| T80 | review | security review of every crate, one report each | A | all | leader reads and files fixes |
| T81 | docs | README and runnable example per system, architecture book | F | all | every example runs in the gate |
| T82 | ci | GitHub Actions workflow for the gate | H | | OPERATOR decides: uses paid minutes on a private repo |
| T83 | release | version tag, change log, demo video | leader | all | OPERATOR publishes |

## 6. The leader's own checks (reusable tools)
- Crypto and codecs: generate vectors with Python (`hashlib`, `cryptography`, `zlib`, `PIL`,
  `sqlite3`, `re`) and commit them as a test file `crates/<x>/tests/*_leader.rs`.
- CPU and assembler: write assembly, build it with
  `rustc --target riscv64gc-unknown-none-elf --emit obj` using `global_asm!(include_str!(..))`,
  extract with `llvm-objcopy -O binary -j .text`, compare words. See
  `crates/quartz/tests/llvm_leader.rs`.
- State machines (allocator, file system, scheduler, database, editor): a small shadow model
  written by the leader that uses only the public API, with a fresh seed. See
  `crates/shale/tests/leader.rs` and `crates/quarry/tests/leader.rs`.
- Hostile input: `forall` with random bytes, bit flips and truncation, then decode and
  re-encode. See `crates/geode/tests/leader.rs`.
- Known trap: a bash heredoc that contains a single quote fails in this harness. Write Rust
  files with the Write tool.

## 7. Designs the architect still owes (ask the operator to switch to Fable for these)
| Id | File | Decides |
|---|---|---|
| D1 | `docs/CINNABAR.md` | the experimental cipher: structure, rounds, test plan |
| D2 | `docs/TREMOR.md` | handshake, packet format, streams, loss recovery, congestion control |
| D3 | `docs/VARVE.md` | page format, log format, SQL subset, isolation level |
| D4 | `docs/MANTLE.md` | memory map, syscall table, process model, driver model |
| D5 | `docs/SEAM.md` | block and transaction format, consensus rule, gas table |
| D6 | `docs/FRONT.md` | look and motion of the site and the IDE |
Opus may write a design itself when Fable is not available; keep it to one page and mark it
"v1, written by the leader".

## 8. Skills to load, by phase
Skills are Claude Code tools; Gemini workers cannot load them, so the leader pastes the two or
three rules that matter into the brief. `docs/SKILLS.md` is the full searched map (made from
the index of every installed skill). Core set:
- Every session: `quality-loop`, `token-budget-discipline`, `verify-before-commit-push`,
  `commit-attribution-check`, `skill-vault-checkpoint`, `log-failure-as-skill`,
  `wsl-from-git-bash`, `ponytail:ponytail`.
- Judging Rust (P1 to P4): `ecc:rust-review`, `ecc:rust-testing`, `ecc:rust-patterns`,
  `superpowers:systematic-debugging`, `superpowers:verification-before-completion`.
- Writing briefs and designs: `agent-skills:spec-driven-development`,
  `agent-skills:api-and-interface-design`, `agent-skills:documentation-and-adrs`.
- Services (P3): `agent-skills:security-and-hardening`, `ecc:security-review`,
  `ecc:api-design`, `agent-skills:performance-optimization`.
- Front (P5): `frontend-design:frontend-design`, `ui-ux-pro-max`, `scroll-craft`,
  `web-animation-integration-gotchas`, `web-design-guidelines`,
  `ecc:make-interfaces-feel-better`, `ecc:accessibility`, `design:accessibility-review`,
  `agent-skills:browser-testing-with-devtools`, `agent-skills:webperf`, and the 21st component
  search for motion references. Animation libraries are allowed only here (SPEC rule 1).
- Deploy (P6): `ecc:docker-patterns`, `ecc:deployment-patterns`,
  `agent-skills:ci-cd-and-automation`, `ecc:benchmark`, `agent-skills:shipping-and-launch`.
- The board in the terminal: `plugin-authoring` (mod `strata-board`, command `/strata`).

## 9. Start prompts
Claude Opus, after `/clear` and `/model`:
> You are the leader of Strata in C:\Users\ADMIN\strata. Read LOOP.md, then docs/PLAN.md, then
> AGENTS.md. Continue the loop from the first open task: one worker at a time through
> tools/lane.sh, judge with your own check, commit and push on PASS. Stop on any stop rule.

Gemini Orchestrator (when Claude is out, or as second judge), run in this folder:
> `agy --model gemini-pro-orchestrator-g1-pro -i "You are the stand-in leader of Strata. Read AGENTS.md, LOOP.md and docs/PLAN.md, write your name in the LEADER line of LOOP.md, and continue the loop."`

## 10. Honest limits
- This is 80 or so tasks. At one worker at a time it is weeks of sessions, not days.
- Not real: TLS (Outcrop speaks plain HTTP; put a proxy in front), a full web platform in
  Spelunk (subset of HTML and CSS, Mica instead of JavaScript), real hardware drivers (the
  machine is our emulator), Windows containers (Vug is Linux only, tested in WSL).
- Cinnabar is a learning cipher. Nothing may depend on it for safety.
- The Gemini lanes are unproven on this code as of this writing; T05 is the first trial.
