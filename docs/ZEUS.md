# Zeus v1: a type-1 hypervisor on Silicon's H extension

Zeus runs in HS-mode on one Silicon hart. It hosts up to 3 guests that were written for bare Silicon and boot in
M-mode (lithium, titan, test guests), time-sliced, each in its own guest-physical space. Nothing sits lower than zeus
except a tiny M-mode shim. Written by the leader on 2026-11-16; T32 implements it.

## Pieces
- `crates/zeus/src/bin/zeus.rs`: the hypervisor, `no_std`/`no_main` on `target_os = "none"` (host: stub `main`),
  linked at `0x8000_0000` by `zeus.ld` (same pattern as `crates/titan/src/bin/kernel.rs` and `kernel.ld`).
- `crates/zeus/src/lib.rs` (host, std): builds a Silicon `Machine` with zeus and the guests loaded, and splits the
  console. Tests build the zeus ELF with a nested `cargo build --target riscv64gc-unknown-none-elf` exactly like
  `crates/titan/tests/boot.rs` builds the titan kernel.

## Host physical memory (machine RAM 64 MiB at `0x8000_0000`)
| Range | Use |
|---|---|
| `0x8000_0000..0x8040_0000` | zeus text, data, stacks, G-stage tables (static, 16 KiB aligned roots) |
| `0x8040_0000` (4 KiB) | boot table written by the host |
| `0x8100_0000 + i * 0x0100_0000` | guest slot `i` (0, 1, 2), 16 MiB each |

Boot table (little endian): `b"ZEUSBOOT"`, `u64 count` (1..=3), then per guest 48 bytes: `name: [u8; 16]` (NUL padded,
ASCII `a-z0-9-`), `slot: u64`, `size: u64` (bytes of RAM given, at most 16 MiB, multiple of 2 MiB), `entry: u64`
(guest-physical), `a1: u64` (value of `a1` at guest entry; titan wants its RAM size). A bad table: zeus prints
`zeus: bad boot table` and fails the machine.

## Guest-physical space (same for every guest)
- RAM: GPA `0x8000_0000 .. 0x8000_0000 + size` maps to the guest's slot, through an Sv39x4 G-stage table (`hgatp`
  mode 8, a VMID per guest = slot + 1) using 2 MiB leaves with R W X U set. Nothing else is mapped, so every other GPA
  raises a guest-page fault (cause 20, 21 or 23) in HS with `htval = GPA >> 2`.
- Emulated devices (on those faults; anything else is an access fault for the guest, see below):

| GPA | Device | Behaviour |
|---|---|---|
| `0x1000_0000` (8 B) | UART | write offset 0: byte to the guest's console; read offset 5 (LSR): `0x60`; other reads 0 |
| `0x0200_0000` (64 KiB) | CLINT | `mtimecmp` at `+0x4000` (8 B, per guest), `mtime` at `+0xBFF8` (real mtime); `msip` reads 0, writes ignored |
| `0x0010_0000` (4 B) | finisher | `0x5555`: guest exits 0; `(code << 16) \| 0x3333`: guest exits `code`; other values ignored |
| `0x1000_1000`, `0x1000_2000`, `0x1000_3000`, `0x0c00_0000` (4 KiB each, PLIC 64 MiB) | block, keys, display, PLIC | reads 0 (so the disk has capacity 0), writes ignored |

- `htinst` is always 0 on Silicon, so zeus decodes the faulting instruction itself: read it at the guest pc with
  `hlvx.hu` (two halves for a 32-bit one), handle `lb lh lw ld lbu lhu lwu sb sh sw sd` and the compressed
  `c.lw c.ld c.sw c.sd c.lwsp c.ldsp c.swsp c.sdsp`; advance the guest pc by 2 or 4. Any other instruction faulting on
  a device GPA is an access fault for the guest.

## Virtual console
Each guest has a line buffer. On `\n` (or when 200 bytes are buffered) zeus writes `[<name>] <line>\n` to the real UART
in one piece, so lines from different guests never mix. Zeus's own lines start with `zeus: `.

## Guest modes and the M-mode emulation
Guests think they run in M-mode. Zeus keeps a virtual hart per guest: the 31 registers, pc, guest mode (M, S or U),
and emulated M state: `mstatus` (MIE, MPIE, MPP only), `mtvec` (direct mode only), `mepc`, `mcause`, `mtval`,
`mscratch`, `mie`, `mip` (MTIP from the virtual timer; SSIP, STIP, SEIP as written), `medeleg`, `mideleg`. Reads of
`mhartid`, `mvendorid`, `marchid`, `mimpid` give 0, `misa` gives RV64 + IMACSU, `pmpcfg*`/`pmpaddr*`/`menvcfg`/
`mcounteren` read 0 and ignore writes.

| Guest mode | Runs as | Notes |
|---|---|---|
| M | VS, `vsatp = 0` while in M (the guest's own `vsatp` is saved and put back on `mret`), `hedeleg = 0`, `hvip = 0`, no VS interrupt enabled in `hie` | every M CSR access, `mret` and `wfi` traps to zeus (illegal instruction, cause 2) and is emulated, then pc += 4 |
| S | VS | guest S CSRs are the real `vs*` CSRs, native; `hedeleg` = guest `medeleg` masked to the bits HS may delegate; `hideleg` = guest `mideleg` shifted to the VS bits (1→2, 5→6, 9→10) |
| U | VU | same delegation as S |

- `mret` (emulated): MIE = MPIE, MPIE = 1, mode = MPP, MPP = U, pc = `mepc`. Entering S or U: restore the guest's
  `vsatp`, set `hedeleg`/`hideleg`/`hvip` from the guest state, `hstatus.SPV = 1`, `sstatus.SPP` = (target is S),
  `sepc = mepc`, `sret`.
- Trap into guest M (emulated): `mepc` = guest pc, `mcause`, `mtval`, MPP = guest mode, MPIE = MIE, MIE = 0,
  mode = M, pc = `mtvec`. Used for: exceptions from guest M; exceptions from guest S/U that reach zeus (not delegated by
  the guest, or not delegable: ecall from VS becomes cause 9, ecall from VU cause 8; a guest-page fault on a non-device
  GPA becomes access fault 1, 5 or 7 with `mtval` = the guest address); and the virtual timer.
- Virtual timer: MTIP is pending when `mtime >= mtimecmp` of that guest. It is delivered (trap into guest M with
  cause `1 << 63 | 7`) when `mie.MTIE` is set and (guest mode is below M, or `mstatus.MIE` is set). Zeus checks this
  every time it is about to resume a guest.
- Guest writes to `mip.SSIP/STIP/SEIP` set the matching `hvip` bits (2, 6, 10) whenever the guest is in S or U, so a
  titan-style shim that forwards the timer with `csrs mip, 2` works, and guest S clears it natively with `csrc sip, 2`.
- `wfi` in guest M is emulated as "idle until an interrupt of this guest is pending"; in guest S/U, `hstatus.VTW = 1`
  makes it a virtual-instruction trap (cause 22) handled the same way. Other virtual-instruction traps (a guest
  touching `h*`/`vs*` CSRs, `hfence`, `hlv`) become illegal-instruction traps into the guest.
- A guest that traps into guest M while its `mtvec` is 0, or nests M traps more than 64 times without an `mret`, is
  killed: `zeus: <name> killed: <reason>`. Other guests keep running.

## Zeus itself
- M-mode shim (assembly, about 40 lines): stack, clear `.bss`, `medeleg` = all exceptions HS may take (0..=15 except
  ecall from M, plus 20, 21, 22, 23), `mideleg` = SSI STI SEI, `mcounteren`/`hcounteren` = time, arm the real
  `mtimecmp` every `QUANTUM = 10_000` mtime ticks, forward the M timer as SSIP (the titan pattern), `mret` to
  `zeus_main` in HS.
- Scheduler: round robin over live guests on every SSIP tick, and when the running guest idles. All guests idle:
  zeus `wfi`s. Before resuming a guest, deliver its pending virtual timer.
- A guest exits through the finisher: `zeus: <name> exited <code>`. When none is left: `zeus: all guests done`, then
  the real finisher gets `0x5555` if every guest exited 0, else `(1 << 16) | 0x3333`.

## Isolation argument (what the tests check)
A guest's only mapped memory is its own slot; zeus's memory and the other slots have no G-stage mapping, `hgatp` is
switched (with `hfence.gvma`) on every guest switch, VMIDs differ, and guests cannot touch `h*` CSRs or `hgatp`
(virtual-instruction trap). Device access is emulated per guest; the CLINT timer compare is per guest.

## Out of scope in v1
More than one hart, SBI, real interrupts from devices to guests, guest input, guest disks, vector or floating point,
`mtvec` vectored mode, PMP enforcement, a guest switching to `satp` modes other than Bare/Sv39 (guest's choice; zeus
does not care).
