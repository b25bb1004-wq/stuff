# Strata loop board (the single source of truth; re-read this, not the chat)

## State
- LEADER: Claude Fable
- heartbeat: 2026-11-14 16:30
- phase: P1 Cores, round 1 running
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
10. No secret, key or personal data in the repository; nothing pushed without the operator.

## Budget
- Whole programme, honest estimate: 150 to 250 worker tasks, 25M to 40M worker tokens, many
  sessions. It cannot finish inside one usage window.
- Round budget (this session): 2.0M worker tokens. Concurrency 3. Retries 2 per task.
- Per task: 150k (Haiku), 250k (Sonnet).
- Spent so far: 0.

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
| T00 | P0 | testkit, tools | workspace, gate, test kit, loop files | leader | | PASS |
| T01 | P1 | crypto | SHA-2, HMAC, HKDF, ChaCha20-Poly1305 | haiku | | RUNNING |
| T02 | P1 | mem | buddy + slab allocator, GlobalAlloc | haiku | | RUNNING |
| T03 | P1 | cpu | RV64IM decode, encode, execute | haiku | | RUNNING |
| T04 | P1 | crypto | X25519 + Ed25519 (RFC 7748, RFC 8032 vectors) | sonnet | T01 | TODO |
| T05 | P1 | sched | fair scheduler core + simulator tests | haiku | | TODO |
| T06 | P1 | fs | block layer, on-disk format, mkfs, read/write path | haiku | | TODO |
| T07 | P1 | fs | journal, fsck, crash-injection tests | haiku | T06 | TODO |
| T08 | P1 | mica | language design doc `docs/MICA.md` | leader | | TODO |
| T09 | P1 | mica | lexer + parser + AST | haiku | T08 | TODO |
| T10 | P1 | mica | type checker + IR | sonnet | T09 | TODO |
| T11 | P1 | vm | bytecode, interpreter, garbage collector | haiku | T08 | TODO |
| T12 | P1 | cpu | Zicsr, M/S/U modes, traps, Sv39 paging | sonnet | T03 | TODO |
| T13 | P1 | cpu | A + C extensions, official RISC-V tests | haiku | T12 | TODO |
| T14 | P1 | crypto | `strand`: our own cipher, design doc + statistical tests (experimental label) | leader + haiku | T01 | TODO |
| T15 | P1 | sched | work-stealing thread pool | haiku | T05 | TODO |

Later phases get their task rows when P1 is green (see the build order in `docs/SPEC.md`):
P2 asm, micac, cpu devices, akern, os, hv. P3 net, httpd, db, vcs, pkg, chain, cage, sh, edit.
P4 gfx, gui, wm, browser, engine. P5 web front, IDE, containers, load tests. P6 hardening.

## Round log (append only: date time | task | worker | tokens | verdict | note)
- 2026-11-14 16:25 | T00 | leader | n/a | PASS | scaffold gate green, 24 crates, test kit 3 tests
