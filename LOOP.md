# Strata loop board (the single source of truth; re-read this, not the chat)

## State
- LEADER: Claude Fable
- heartbeat: 2026-11-14 15:19
- phase: P1 Cores, round 2 running (T11 PASS; T06, T09 with workers)
- stop reason: none
- Stand-in: Gemini Argon. Open this folder in Antigravity, choose Argon, say
  "You are the stand-in leader. Follow AGENTS.md." (The agy command is not installed on this PC
  and the Mac is offline, so takeover is one manual step for now.)

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
- Whole programme, honest estimate: 150 to 250 worker tasks, 25M to 40M worker tokens, many
  sessions. It cannot finish inside one usage window.
- Round budget (this session): 2.0M worker tokens. Concurrency 3. Retries 2 per task.
- Per task: 150k (Haiku), 250k (Sonnet).
- Spent so far: 708k (worker tokens only; leader tokens are not counted here).

## Workers checked on 2026-11-14
| Worker | Status |
|---|---|
| Claude Haiku / Sonnet subagents | available |
| Nemotron Ultra (WSL `~/faith/tools/nv.py`) | PONG ok; bulk text only |
| agy / Argon | NOT reachable from here: Mac offline 19 days, no agy command on the PC |
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
| T04 | P1 | onyx | X25519 + Ed25519 (RFC 7748, RFC 8032 vectors) | sonnet | T01 | TODO |
| T05 | P1 | geyser | fair scheduler core + simulator tests | haiku | | TODO |
| T06 | P1 | shale | block layer, on-disk format, mkfs, read/write path | haiku | | RUNNING |
| T07 | P1 | shale | journal, fsck, crash-injection tests | haiku | T06 | TODO |
| T08 | P1 | mica | language design `docs/MICA.md` + bytecode `docs/MBC.md` | leader | | PASS |
| T09 | P1 | mica | lexer + parser + AST | haiku | T08 | RUNNING |
| T10 | P1 | mica | type checker + IR | sonnet | T09 | TODO |
| T11 | P1 | geode | module, verifier, binary encode/decode (`docs/MBC.md`) | haiku | T08 | PASS |
| T17 | P1 | geode | interpreter, heap, garbage collector, fuel | haiku | T11 | TODO |
| T12 | P1 | quartz | Zicsr, M/S/U modes, traps, Sv39 paging | sonnet | T03 | TODO |
| T13 | P1 | quartz | A + C extensions, official RISC-V tests | haiku | T12 | TODO |
| T14 | P1 | onyx | `strand`: our own cipher, design doc + statistical tests (experimental label) | leader + haiku | T01 | TODO |
| T15 | P1 | geyser | work-stealing thread pool | haiku | T05 | TODO |
| T16 | P2 | quarry | const constructor for a static global allocator; `check()` also counts live slots; alignments above one page | haiku | T02 | TODO |

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
