# Titan P1 (thin slice) implementation plan

> For agentic workers: one task per brief (`briefs/T140.md` and on). Follow `briefs/_RULES.md`.

**Goal:** one exam task runs inside the machine end to end: the model talks to the door, hermes runs
calliope inside Titan on Silicon, and the answer comes back as an envelope.

**Architecture:** new Silicon device `door` (queues inside the device, DMA like the block device);
Titan gets a door driver and calls 16, 22, 23, 25; new `atlas` runtime crate (host and titan
backends); calliope moves its edges onto atlas; `hermes` daemon inside; monolith gains a titan backend.

**Spec:** `docs/superpowers/specs/2026-11-17-ai-computer-design.md` (read sections 5 and 10).

## Global constraints
- Zero external crates; `tools/check.sh` must pass for every touched crate.
- No OS-specific code in silicon, titan, atlas, hermes.
- Rust target for the inside: `riscv64gc-unknown-none-elf` (installed). User programs link with
  `crates/titan/user.ld` at `0x1_0000`, built by a nested `cargo build` the way `crates/titan/build.rs`
  builds `userland`.
- Existing Silicon default RAM stays 16 MiB.

## Contracts (fixed; every task codes against these)

### C1. Door device (Silicon), MMIO window `0x1000_4000`, size `0x1000`, PLIC source 12
Registers are 64-bit, also readable and writable as 32-bit (low half).

| Off | Name | Access | Meaning |
|---|---|---|---|
| 0x00 | TX_ADDR | rw | guest physical address of the frame to send |
| 0x08 | TX_LEN | rw | frame length in bytes, 1 to 1 MiB |
| 0x10 | TX_CHAN | rw | channel 0 to 255 |
| 0x18 | TX_GO | w | copy TX_LEN bytes at TX_ADDR to the outbox as (TX_CHAN, bytes); STATUS bit 1 set if the range is not inside RAM or the length is bad, else cleared |
| 0x20 | RX_ADDR | rw | guest buffer for the next incoming frame |
| 0x28 | RX_CAP | rw | that buffer's size |
| 0x30 | STATUS | r | bit 0 RX_READY (inbox not empty), bit 1 TX_ERR, bit 2 RX_TOO_BIG (front frame longer than RX_CAP) |
| 0x38 | RX_LEN | r | length of the front frame, 0 if none |
| 0x40 | RX_CHAN | r | channel of the front frame |
| 0x48 | RX_TAKE | w | copy the front frame to RX_ADDR and pop it; if longer than RX_CAP or outside RAM set bit 2 and do not pop |
| 0x50 | IRQ_ENABLE | rw | when non-zero, PLIC source 12 is raised while RX_READY |

Host API on `Machine` (public): `pub fn door_push_to_guest(&mut self, channel: u8, bytes: &[u8])`
and `pub fn door_take_from_guest(&mut self) -> Vec<(u8, Vec<u8>)>` (drains the outbox in order).
Inbox holds at most 64 frames; a push beyond that is dropped and counted (`pub fn door_dropped(&self) -> u64`).
The door state is part of `Snapshot` (queues and registers round-trip).

### C2. Titan calls (added to `docs/TITAN.md` numbering)
| No | Call | Result |
|---|---|---|
| 16 | `result(ptr, len)` | 0; stores up to 64 KiB of UTF-8 as the process's result (last call wins); -1 if too long or not UTF-8 |
| 22 | `door_send(chan, ptr, len)` | 0, -1 bad args, -7 busy; only for a process started with the door right (P1: the process named `hermes`) else -1 |
| 23 | `door_recv(ptr, cap, chan_out_ptr)` | frame length; writes the channel byte to `chan_out_ptr`; -7 if no frame (non-blocking); -1 if longer than `cap` (frame stays queued); same right as 22 |
| 25 | `child_result(pid, ptr, cap)` | length copied; -2 if that child has no result or was not waited; a child's result is kept until its parent fetches it or exits |

### C3. atlas API (crate `crates/atlas`, `no_std` plus `alloc`; backend `host` uses std when `not(target_os = "none")`, backend `titan` uses `ecall` when `target_os = "none"`)
```rust
pub enum Error { BadArg, NotFound, NoMem, BadFd, Exists, NotDir, Busy, TooBig, Io(String) }
pub type Result<T> = core::result::Result<T, Error>;
pub mod args { pub fn args() -> Vec<String>; }                         // argv including argv[0]
pub mod io {
    pub fn out(bytes: &[u8]);  pub fn err(bytes: &[u8]);
    pub fn read_stdin() -> Result<Vec<u8>>;                                 // until EOF
}
pub mod fs {
    pub struct DirEntry { pub name: String, pub is_dir: bool }
    pub fn read(path: &str) -> Result<Vec<u8>>;
    pub fn read_to_string(path: &str) -> Result<String>;
    pub fn write(path: &str, data: &[u8]) -> Result<()>;                   // create or truncate
    pub fn append(path: &str, data: &[u8]) -> Result<()>;
    pub fn read_dir(path: &str) -> Result<Vec<DirEntry>>;
    pub fn create_dir(path: &str) -> Result<()>;
    pub fn create_dir_all(path: &str) -> Result<()>;
    pub fn remove_file(path: &str) -> Result<()>;
    pub fn is_dir(path: &str) -> bool;
    pub fn exists(path: &str) -> bool;
}
pub mod proc {
    pub fn exit(code: i32) -> !;
    pub fn spawn(path: &str, args: &[&str], stdin_fd: Option<i32>, stdout_fd: Option<i32>) -> Result<u64>; // pid
    pub fn wait(pid: u64) -> Result<i32>;
    pub fn result(envelope: &str) -> Result<()>;                           // host backend: prints it as the last stdout line
    pub fn child_result(pid: u64) -> Result<Option<String>>;               // host backend: Ok(None)
}
pub mod time { pub fn uptime_ms() -> u64; }
#[macro_export] macro_rules! outln { ($($t:tt)*) => { $crate::io::out(format!($($t)*).as_bytes()); $crate::io::out(b"\n") } }
#[macro_export] macro_rules! main { ($f:path) => { /* host: fn main() { std::process::exit($f()) }; titan: _start, allocator, panic handler, argv from a0/a1, exit($f()) */ } }
```
Program shape on both targets: `#![cfg_attr(target_os = "none", no_std, no_main)]`, `extern crate alloc;`,
`fn run() -> i32 { ... }`, `atlas::main!(run);`. On titan the panic handler writes the message to
stderr and exits 101; the allocator is midas or a bump allocator over `sbrk`, the worker's choice.
`proc` on the host backend uses `std::process::Command` with the given fds ignored (host tests do not
pass fds).

### C4. hermes frames (channel 0, JSON, one object per frame)
Request: `{"id":N,"op":"run","argv":["calliope","grep","x","/w/a.txt"]}` |
`{"id":N,"op":"read","path":"/w/a.txt"}` | `{"id":N,"op":"write","path":"/w/a.txt","text":"..."}`.
Reply: `{"id":N,"ok":true,"exit":0,"stdout":"...","result":<envelope or null>}` or
`{"id":N,"ok":false,"error":"..."}`. stdout is cut to 256 KiB with a `[cut N bytes]` marker.
hermes runs each tool with stdout redirected to `/tmp/hermes-<id>.out`, waits, reads it back,
fetches `child_result`, removes the file. Programs found in the kernel's built-in table by name.

## Tasks
| ID | Task | Depends | Files | Gate |
|---|---|---|---|---|
| T140 | Door device (C1) | none | `crates/silicon/src/machine/door.rs` (new), `machine/mod.rs`, `machine/snapshot.rs` | `tools/check.sh silicon`; tests: TX and RX round trip through a tiny RV64 program or direct bus writes, bad range sets TX_ERR, too-big frame stays queued, snapshot round trip, 64-frame cap |
| T141 | atlas host backend + titan backend that compiles (C3) | none | `crates/atlas/**` (new), root `Cargo.toml` members line (leader adds) | `tools/check.sh atlas`; host tests for every fs and proc function; `cargo build -p atlas --target riscv64gc-unknown-none-elf` succeeds |
| T142 | Titan door driver and calls 16, 22, 23, 25 (C2) | T140 | `crates/titan/src/bin/kernel/*`, `docs/TITAN.md` call table | `tools/check.sh titan`; kernel tests inside Silicon: result round trip via child_result, door frames both ways, refusal for a non-hermes process |
| T143 | calliope edges onto atlas, builds for titan | T141 | `crates/calliope/**` | `tools/check.sh calliope` (all old tests pass); `cargo build -p calliope --bin calliope --target riscv64gc-unknown-none-elf` with user.ld succeeds |
| T144 | hermes (C4) embedded in Titan, init starts it; calliope embedded | T142, T143 | `crates/hermes/**` (new), `crates/titan/build.rs`, `crates/titan/src/bin/kernel/progs.rs`, init | `tools/check.sh titan`; test: push a `run` frame for `calliope grep`, take the reply frame, exit 0 and expected stdout |
| T145 | monolith `--machine titan` backend | T144 | `crates/monolith/src/computer.rs` (+ new `titan_machine.rs`) | `tools/check.sh monolith`; one calliope exam task passes inside with the real model (leader runs it) |

Rounds: T140 and T141 in parallel; then T142 and T143; then T144; then T145.
