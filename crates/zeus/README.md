# zeus

`zeus` is a type-1 hypervisor running on Silicon's RISC-V H extension (`HS` mode), time-slicing up to 3 M-mode guests (such as `lithium` and `titan`) in isolated `Sv39x4` guest-physical spaces.

## What runs

- **M-mode shim** (`src/bin/zeus.rs`): clears `.bss`, configures `medeleg`/`mideleg`/`mcounteren`/`hcounteren`, arms the real CLINT timer every `10_000` ticks, forwards `MTIP` to `SSIP`, and `mret`s into HS mode.
- **Hypervisor** (`src/bin/zeus/{boot,guest,emulate,trap}.rs`): parses the boot table at `0x8040_0000`, builds 2 MiB-page `Sv39x4` G-stage tables per guest, emulates M-mode CSRs (`mstatus`, `mtvec`, `mepc`, `mcause`, `mtval`, `mscratch`, `mie`, `mip`, `medeleg`, `mideleg`, IDs, PMP/envcfg stubs), virtualizes guest `S`/`U` modes natively via `VS`/`VU` and `vs*` CSRs, decodes and emulates device MMIO (UART with per-guest `[<name>]` line buffering, per-guest CLINT `mtimecmp`, finisher, and stubbed block/keys/display/PLIC), and schedules live guests round-robin.

## Host physical memory map (64 MiB at `0x8000_0000`)

| Range | Purpose |
|---|---|
| `0x8000_0000 .. 0x8040_0000` | `zeus` text, data, stacks, and static `Sv39x4` G-stage tables |
| `0x8040_0000 .. 0x8040_1000` | Host-written boot table (`ZEUSBOOT`, guest count, 48-byte guest descriptors) |
| `0x8100_0000 .. 0x8200_0000` | Guest slot 0 (up to 16 MiB) |
| `0x8200_0000 .. 0x8300_0000` | Guest slot 1 (up to 16 MiB) |
| `0x8300_0000 .. 0x8400_0000` | Guest slot 2 (up to 16 MiB) |

## Running the example

```sh
cargo run -p zeus --example two_guests --release
```
