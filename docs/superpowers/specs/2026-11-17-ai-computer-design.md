# Design: Titan, a computer whose user is an AI

Written by the leader on 2026-11-17. On 2026-11-17 the operator handed over the design calls ("take full
accountability and responsibility, and keep working") and allowed downloads. Every call below that he
did not make himself is marked **(leader's call)**.

Replaces the draft roadmap `docs/ai-computer/plans/2026-11-17-own-machine.md` (which assumed Linux
compatibility).

## 1. Intent

The operator wants a personal computer for an AI: free, local, always on, online and offline, fast, with
all the Monolith tools inside, usable by a local model (Qwen) or by Claude. His words: "a new OS for AI",
"why cant we have our own os". So this is a new operating system with its own rules, designed for how a
model works. It must not be a Linux look-alike.

What a model needs that a human OS does not give it:
- **Structured answers, not screens of text.** A model pays for every token it reads. Every program and
  every system call answers in one small, typed envelope (the ACI envelope Monolith already uses).
- **Memory that survives.** The 2026-11-17 exams showed memory is the real gain: recall 0 of 4 with raw
  tools, 4 of 4 with the memory layer, in 2 to 3 steps. So memory is part of the OS, not an add-on.
- **Undo.** Models make mistakes. Trying something, looking at the result and rolling back must be one
  call, not a skill.
- **Fences it cannot talk its way out of.** Permissions are capabilities given per task, and text that
  came from the web is marked as data all the way through the file system.
- **A budget.** Every task runs against a limit on instructions, time and output bytes.

## 2. Goals, non-goals, anti-goals

Goals
1. Titan (our kernel) runs on Silicon (our RISC-V machine in software) on Windows, Linux and Mac,
   driven from outside through one door by any model.
2. The Monolith tools run inside as native Titan programs.
3. The existing exam suites run inside the machine, and score at least what the host harness scores
   at level 2 (train 23 of 24, held-out 12 of 12, memory 8 of 8 on one seed, 2026-11-17).
4. Online and offline: with the net device off, everything except fetching still works.
5. The AI can create software inside: write Logos, check it, test it, run it.
6. Always on: the machine keeps running, saves itself to disk, and resumes where it stopped.

Non-goals (v1)
- The model or the GPU inside the machine. No from-scratch OS can drive the RTX 5070; the model stays
  outside and talks through the door.
- Running Linux programs, or a Linux-shaped system call interface.
- JavaScript in the browser; Chrome inside.
- A Rust compiler inside (too big; Logos is the language inside).
- Several human users.
- pandora (Linux containers), zeus, lithium and the titan booter inside (machines inside the machine).

Anti-goals (would make it a failure even if it ran)
- Secrets, keys or the operator's personal files inside the machine.
- The door listening on anything but localhost.
- Web text treated as instructions.
- A copy of Linux or POSIX with a new name.

## 3. What exists (checked 2026-11-17)

- Silicon (`crates/silicon`, 22.5k lines with tests): RV64 machine with UART, block device, keys,
  display, CLINT, PLIC, snapshot and restore; 32 to 50 million instructions per second.
- Titan (`crates/titan`, 6.9k lines): Sv39 paging, processes, kairos scheduler, ELF loader, 16 system
  calls, Hades file system on the block device, a small multicall userland. Memory map in `docs/TITAN.md`.
- Crates already built to work without `std`: midas, kairos, hades, krypton, chisel, chronos, cobalt,
  elysium, helios, lithium, mercury, neon, sphinx, zeus (all have a `std` feature).
- Logos (`docs/LOGOS.md`): our language; compiles to Elysium bytecode and to RV64 for our OS. Its
  "layers" (`fn save() uses fs, net`) are checked by the compiler.
- Monolith harness (`crates/monolith`): agent loop, actions, journal, memory, exams, a computer type that
  runs tools in a fenced host folder.

## 4. Approaches considered

1. **Linux-compatible inside**: Titan learns the Linux system calls, so the tools recompile unchanged.
   Fastest way to "all tools inside", but the interface is Linux's. Rejected by the operator ("a new OS").
2. **Own interface, shared runtime (chosen)**: one runtime crate gives every program files, processes,
   time, output, results and net through Titan's own calls. Each tool replaces its `std::fs`,
   `std::process` and so on with that crate. More work, nothing borrowed, and the interface is designed
   for a model.
3. **Native speed first**: Titan for x86-64 under the Windows hypervisor. Fastest, but Windows only,
   needs a Windows feature switched on, and a new kernel port. Kept as a later speed option, not v1.

The operator picked the direction of approach 2 on 2026-11-17. Its details are the leader's calls.

## 5. The design

### 5.1 The pieces

```
 outside (host: Windows, Linux or Mac)                inside (Silicon machine)
 ┌────────────────────────────────┐   door device    ┌──────────────────────────────────────┐
 │ model: Qwen (llama-server) or  │   (MMIO frames)  │ Titan kernel                         │
 │ Claude (MCP over stdio)        │◀──────────────▶ │  caps, budgets, journal, tx, labels  │
 │ monolith up  (keeps it running)│                  │ hermes  (door daemon, runs actions)  │
 │ net proxy (allowlist, off =    │                  │ mnemosyne (memory service)           │
 │   offline)                     │                  │ tools built on atlas (runtime crate) │
 └────────────────────────────────┘                  └──────────────────────────────────────┘
```

New crates **(leader's call, names follow the repo's myth naming)**: `atlas` (runtime library for
Titan programs), `hermes` (in-machine door daemon), `mnemosyne` (memory service). New Silicon device:
`door`.

### 5.2 The door device (Silicon)

- MMIO at `0x1000_4000` **(leader's call)**, next to the existing devices. Two rings of frames in guest
  RAM, set up by the kernel: host-to-guest and guest-to-host. A frame is `channel: u8`, `len: u32`,
  then `len` bytes of UTF-8 JSON. Doorbell register for each direction; the kernel polls in v1
  (drivers poll today).
- Channels: `0` control (actions in, results out), `1` net (requests out, responses in), `2` machine
  (snapshot, shutdown, clock sync).
- Host side lives in Silicon behind a trait `DoorHost` so tests can script it.

### 5.3 Kernel rules (Titan v2)

Existing calls 0 to 15 stay. New calls **(leader's call)**:

| No | Call | What it does |
|---|---|---|
| 16 | `result(ptr, len)` | The program's answer: one ACI envelope (JSON). Kernel journals it and returns it to whoever spawned the program. Replaces "parse the last line of stdout". |
| 17 | `spawn_caps(path, argv, caps_ptr)` | Spawn with a capability set. A child can only get a subset of its parent's caps. |
| 18 | `tx_begin()` / 19 `tx_commit(id)` / 20 `tx_abort(id)` | File-system transaction for the calling task: copy-on-write in Hades; abort restores every file the task touched. |
| 21 | `label(fd) -> u32` | Read a file's labels (bit 0 = `web`: content came from the net device). |
| 22 | `door_send(ch, ptr, len)` / 23 `door_recv(ch, ptr, len)` | For hermes only (needs the `door` cap). |
| 24 | `budget() -> left` | Instructions, milliseconds and output bytes left for this task. |

Capabilities: a list of path prefixes with `r` or `rw`, `net` (off or on), `door`, and a budget. Every
file call checks the path against the caller's caps. Logos layers map onto them: a program built
`uses fs` and not `net` is refused the `net` cap at spawn.

Journal: the kernel appends one line per state-changing call and per program run to `/sys/journal`
(append-only, readable by the model).

Labels: any file written by a process that has read net data in this task gets the `web` label
(sticky, file-level, stored in the Hades inode). hermes wraps labelled content as data when it sends
it out of the door.

Memory survives undo: `/mem` is outside every transaction. Rolling back work never erases what the
model learned.

### 5.4 atlas (the runtime crate)

One API, two backends: `titan` (system calls) and `host` (std, so every tool still builds and tests on
Windows exactly as today). Modules: `args`, `io` (stdout, stderr, stdin), `fs` (open, read, write,
readdir, mkdir, unlink, metadata), `proc` (spawn, wait, exit, result), `time` (uptime, monotonic),
`net` (fetch through the door), `tx`, `caps`. Error type maps Titan's negative codes. Global allocator
on titan via midas. `no_std` plus `alloc`.

Porting rule for every tool: core logic stays as it is; only the edges move from `std::*` to `atlas::*`;
the tool's existing tests keep passing on the host backend, and one smoke test runs it inside Silicon.

### 5.5 hermes (the door daemon) and the action set

hermes starts from `init`, holds the `door` cap, and runs the same action set the host harness uses
(`crates/monolith/src/actions.rs`), so prompts and exams carry over unchanged. Each action is spawned
with caps narrowed to the task folder, inside a transaction when the action asks for one. Results go
back as envelopes. Output is capped and truncated with a marker, as on the host.

### 5.6 mnemosyne (memory)

Plain files under `/mem` plus a word index (the research note says files plus search hold up against
vector stores). Calls: `remember(topic, text)`, `recall(query) -> top 5`, `forget(id)`. hermes adds the
top recalls to each task's first observation, as the level 2 harness does now.

### 5.7 The door for models (host)

- Local Qwen: `monolith` gains a backend `--machine titan`; the agent loop is unchanged.
- Claude: `monolith mcp` speaks MCP over stdio (no network listener at all) with a small, CLI-style tool
  surface: `run` (one krypton or Logos script), `read`, `write`, `remember`, `recall`, `undo`. Small
  surface because CLI-style tools cost far fewer tokens than long MCP tool lists.

### 5.8 Online and offline

The net channel is served by a host proxy with an allowlist file and a size cap; off means the channel
answers "offline". Inside, `atlas::net::fetch` returns an envelope with status, headers and body, and
every byte read is `web`-labelled. mercury parses, odyssey turns pages into a tree and text.

### 5.9 Making software inside

cobalt, athena and logos are ported first among the big tools. The model writes Logos, checks it,
tests it (`test` and `prop` are syntax), and runs it on Elysium bytecode inside the machine. Compiling
Logos to RV64 and spawning the result as a Titan program comes after.

### 5.10 Always on

`monolith up` keeps one machine running, takes a Silicon snapshot to disk when idle and on exit, and
resumes from it at start. Tasks queue in a host folder; results land next to them.

### 5.11 Skill shelf

`/skills` holds skill folders in the standard format (`SKILL.md` and friends, as in NVIDIA/skills).
The NVIDIA catalogue is cloned outside the public repo and copied in by the operator's choice. Checking
NVIDIA's signature files needs their format first; until then, skills are loaded only from folders the
operator placed **(leader's call)**.

### 5.12 Speed

Measure first (`bench`). Targets: Silicon from 32 to 50 up to at least 100 million instructions per
second through decode caching and block chaining; exam tasks inside take no more than twice the host
harness's wall time, model time excluded. A just-in-time compiler is out of scope for v1.

## 6. Quality bar for the whole job

1. `tools/check.sh` passes (zero external crates, all tests).
2. Silicon door device: frames round-trip both ways under a scripted `DoorHost` test.
3. Titan new calls 16 to 24 each have a kernel test inside Silicon, including refusal cases (a child
   asking for more caps than its parent, a write outside caps, `door_send` without the cap).
4. A transaction abort restores every touched file; `/mem` is untouched by an abort.
5. A file written after reading net data carries the `web` label.
6. atlas: same test file passes on the host backend and inside Silicon.
7. Each ported tool: its old tests pass on host; one smoke test passes inside.
8. hermes runs every action in `actions.rs` inside the machine.
9. The exam suites run inside with `--machine titan` and score at least the level 2 host numbers.
10. Offline run: with net off, every non-fetch exam task still passes.
11. A Logos program is written, tested and run inside by the model in an exam task.
12. `monolith up` resumes a machine after a host restart with files and memory intact.
13. Door never binds a network port; a test asserts it.
14. CI on ubuntu runs the full gate; no OS-specific code in silicon, titan, atlas, hermes, mnemosyne.

## 7. Phases (each ends in something that runs)

- **P1 thin slice:** door device, `result` call, atlas core (args, io, fs, proc), hermes with three
  actions, one tool (calliope) ported, one exam task passing inside. Proves the whole path.
- **P2 safety:** caps, budgets, journal, transactions, labels.
- **P3 memory:** mnemosyne, recall in observations, the memory exam inside.
- **P4 tools:** port in batches of three: hades, chronos, iridium; cobalt, athena, logos; sphinx,
  krypton, argo; mercury, odyssey, aurum; the rest.
- **P5 online:** net channel, proxy, allowlist, offline mode.
- **P6 software and skills:** Logos inside end to end; skill shelf.
- **P7 models and always on:** `--machine titan` for all suites, `monolith mcp`, `monolith up`.
- **P8 speed:** measure, then decode cache and block chaining.

## 8. What is left to the builder

Ring layout details, frame size limits, the inode bit chosen for labels, the word index format, and how
each tool's edges are split, as long as the contracts above (call numbers, envelope shapes, cap
rules, the atlas API) hold.

## 9. Risks

- Speed: an interpreted machine is slower than the PC; P8 may not reach the target. Mitigation: measure
  in P1 and keep the x86-64 hypervisor route as the fallback.
- Porting cost: about 150k lines of tools. Mitigation: edges only, batches of three, host tests guard
  behaviour.
- Kernel memory: transactions and the journal cost RAM; Silicon's RAM size may need raising.
