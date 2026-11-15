# Benchmarks

## quartz

Command:

```sh
cargo run --release -p quartz --example bench
```

Measured 2026-11-15 on an AMD Ryzen 5 7600X, Windows 11, release build, one run (other runs within 3%):

| Workload | cache on | cache off | speed-up |
|---|---|---|---|
| `smoke.bin` (rustc riscv64gc program), repeated to 20 M instructions | 45 MIPS | 31 MIPS | 1.45x |
| tight loop, 10 M instructions | 61 MIPS | 34 MIPS | 1.8x |
| total | 50 MIPS | 32 MIPS | 1.55x |

Before the decode cache existed (commit 68624e6) the same bench ran at 34 MIPS. So the cache is
about 1.5x faster than the old emulator, and the cache-off path is about 6% slower than the old
one (cache off is now a test and comparison mode; the default is on).
