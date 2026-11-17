# Own machine for a model: plan (leader, 2026-11-17)

Operator, 2026-11-17: "i want it to be a personal computer, not just programs"; "like there's linux, but u
created ur own OS, so optimising that os for AI only on a new machine and lets create it to run both online
and offline"; "give my model a complete machine to work on FOR FREE along with all the tools we created";
"everything optimised to run faster, such that i get myself a claude pro, and hand it the new machine, it can
run things fast and smooth in there".

**Goal:** one command gives any model (the local Qwen, or Claude through a Pro plan) its own complete machine:
our kernel (Titan) on our machine (Silicon), with the Monolith tools inside, working with or without the
internet, always on, free, and rewindable.

**What exists (checked 2026-11-17):** Silicon, a RISC-V machine in software (5494 lines, 201 tests; console,
disk, keys, display, snapshot and restore; 32 to 50 million instructions per second). Titan, a kernel for it
(2967 lines, 37 tests; paging, processes, 16 system calls, the Hades file system on the disk, a small shell
with ls, cat, echo, mkdir, rm, ps). The harness in `crates/monolith` (computers in a host folder, journal,
agent loop, memory, exams; Qwen 27B passes 35 of 44 at level 1). The 25 tools run on Windows only.

**What is missing:** the tools do not run on Titan (they use the Rust standard library, Titan has none); no
network and no model access from inside the machine; no door for an outside model such as Claude; nothing
keeps the machine and its agent alive; the machine is about 100 times slower than the PC.

## Global constraints
- Zero external crates. Gate per crate: `bash tools/check.sh <crate>` ends `CHECK PASS`.
- No downloads without the operator's yes. Nothing in this plan needs one (the RISC-V build target is installed).
- The model never runs inside the machine: an interpreted machine cannot run a 27B model, and no from-scratch
  kernel can drive the RTX 5070. The model is a device the machine talks to.
- One exam suite is the yardstick at every stage: `suites/*.tasks`, same checks, host computer versus machine.

## Stages (each ends with something that runs and a number)
| Stage | Delivers | Gate | Workers |
|---|---|---|---|
| S0 Door | `monolith mcp NAME`: a standard tool server on stdin and stdout (run, read, write, snapshot, rewind) so Claude Code or any other client can drive a computer; works on today's host computers at once | a scripted client lists the tools and finishes 3 exam tasks through the door; the operator attaches Claude and does one task | 2 |
| S1 Machine as computer | `monolith new NAME --kind vm`: boots Titan on Silicon with a persistent disk; `exec` goes through the console; `rewind` and `fork` use machine snapshots; the journal is unchanged | the shell commands Titan already has work through `monolith exec`; rewind restores disk and memory byte for byte; boot time and command round trip measured | 3 |
| S2 Tools inside | a small runtime crate for Titan programs (arguments, files, output, heap, exit) and the tools built on it, in order of exam use: calliope, sphinx, krypton, hades, iridium, chronos, then the rest | spike first: one tool (sphinx) ported and timed, then decide the route; per tool, its exam tasks replay clean inside the machine; at the end the 44-task selftest passes inside the machine | 1 spike, then 6 to 10 |
| S3 Model and network as devices | two new Silicon devices backed by the host: a model port (prompt in, reply out; any OpenAI-style address, local or cloud) and a net port (connect, send, receive, governed by an allow list); `--offline` removes the net port | inside the machine, a program asks the model a question and fetches a page; with `--offline` the fetch is refused and the local model still answers | 4 |
| S4 Always on | `monolith serve NAME`: keeps the machine and its agent running, takes tasks from an inbox, wakes on a schedule, writes reports to an outbox, survives a restart from the last snapshot | a task dropped in the inbox is done and reported with no one at the keyboard; kill and restart loses nothing committed | 3 |
| S5 Speed | measure where time goes on the exam (model, emulator, tools); then make Silicon faster (translate whole blocks, not single instructions) and cut tool start-up cost | exam wall time inside the machine within 1.5 times the host computer's; Silicon at 3 times today's speed on `docs/BENCH.md` | 3 |
| S6 Real hardware speed (later) | Titan for x86-64 under the Windows hypervisor, so the machine runs at the PC's own speed | same suite passes; needs the operator's yes for one Rust build target and a Windows feature | not planned yet |

AI-only choices made along the way (not a separate stage): the machine boots straight into an agent console
with no login and no human shell; every tool answers in the one structured format; the kernel journals each
action; snapshot, rewind and fork are system features; the agent's reach (files, network) is a written allow
list; tokens and steps are budgeted like processor time.

## Order and budget
Wave A: S0, S1 and the S2 spike (about 900k Haiku tokens). Then S2 proper, S3, S4, S5 in that order.
Whole plan: about 3.5 to 4.5 million Haiku tokens over several sessions, cap 3 workers, at most 2 retries per
task, leader reruns every gate. Exams on the local GPU cost nothing.

## Review focus (most likely to bite, each gets a test in the owning stage)
1. A command that never returns inside the machine: `exec` must time out and leave the machine usable.
2. A snapshot taken mid-write: rewind must never yield a file system that fails `fsck`.
3. The model or network device being absent: programs get a clear error, not a hang.
4. Output larger than the console buffer: nothing is silently cut.
5. Two clients on one machine at once through the door: refused or queued, never interleaved.

## Limits stated up front
- Speed: an interpreted machine stays slower than the PC until S6. For agent work the model is the slow part
  (seconds per step against milliseconds per tool), so this should be hard to notice on small tasks and plain
  on heavy ones. S5 measures it instead of guessing.
- S2 is the unknown: porting 25 tools off the standard library may cost more than estimated. The spike decides.
- "Free" means no rent: the machine is local. Claude itself is still the operator's paid plan.
