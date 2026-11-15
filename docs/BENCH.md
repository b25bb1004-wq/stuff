# Benchmarks

## silicon

Command:

```sh
cargo run --release -p silicon --example bench
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

## services: mercury, iridium, xenon, aurum (T78)

Command:

```sh
cargo run --release -p bench -- --full          # all suites, 2 s per benchmark
cargo run --release -p bench -- --only xenon    # one suite, 200 ms per benchmark
```

Every benchmark checks its own results (a lookup returns the value written, a response has the right
status and length, the bytes received equal the bytes sent, a proof verifies); a wrong result exits 1.
In-process and over loopback on one machine, no containers.

Measured 2026-11-16 on the same PC (AMD Ryzen 5 7600X, 12 threads, Windows 11), release build, `--full`.
A second run was within 8% on every row.

| suite | benchmark | rate | unit |
|---|---|---|---|
| mercury | parse_request | 785k | req/s |
| mercury | write_response | 3.28M | resp/s |
| mercury | server_keepalive | 122k | req/s |
| iridium | seq_insert | 87.3k | keys/s |
| iridium | rand_insert | 78.5k | keys/s |
| iridium | lookup_hit | 39.3k | keys/s |
| iridium | range_scan | 5.30M | keys/s |
| iridium | file_insert | 55.1k | inserts/s |
| xenon | bulk_transfer | 84.3 | MB/s |
| xenon | round_trip | 37.6k | rt/s |
| aurum | merkle_root | 1.13k | roots/s |
| aurum | merkle_proof | 1.14k | proofs/s |
| aurum | tx_verify | 3.93k | tx/s |
| aurum | chain_blocks | 19.6 | blocks/s |

Notes:
- `server_keepalive` is 4 client threads against one mercury server over loopback TCP, keep-alive.
- iridium point lookups (about 39k/s) are slower than inserts (about 87k/s). Not profiled yet; the likely
  cause is that each `get` reads every page on its path through the pager, while inserts work on pages
  already dirty in memory. Worth a profile before adding a clean-page cache.
- xenon `bulk_transfer` is 4 MiB x 10 over loopback UDP with encryption and acks; `MB/s` is 10^6 bytes.
- aurum `merkle_proof` builds the whole tree for each proof, so it runs at the `merkle_root` rate.
- `chain_blocks` applies blocks of 100 signed transfers, so about 1.9k verified transfers/s.
