# assay

assay is the shared test kit for every Strata crate. Strata takes no external crates, so instead of
a property-testing library it ships a small one: a deterministic random generator, a `forall` runner
that reports the seed of a failing case, and hex helpers for published test vectors. Every value comes
from a seed, so a failure found on one machine can be replayed exactly on another.

## Features

- `Rng`, a SplitMix64 generator: the same seed gives the same stream on every machine.
- Bounded draws with `range`, `below` and `chance`, and byte helpers `fill`, `bytes` and `pick`.
- `forall` runs a property on independent streams derived from one master seed.
- When a case panics, `forall` prints that case's seed to stderr.
- `replay` runs one case again from a printed seed.
- `hex` decodes test vectors (spaces, newlines and `:` are ignored); `to_hex` encodes lowercase hex.
- Not included: shrinking of failing inputs.

## Public API

| Item | What it does |
| --- | --- |
| `Rng::new(seed)` | Start a stream |
| `Rng::u64()` | Next 64 random bits |
| `Rng::range(lo, hi)` | Uniform value in `lo..hi`; returns `lo` if the range is empty |
| `Rng::below(n)` | Uniform index in `0..n`; 0 when `n` is 0 |
| `Rng::chance(num, den)` | True with probability `num / den` |
| `Rng::fill(buf)` | Fill a byte slice with random bytes |
| `Rng::bytes(max_len)` | Random `Vec<u8>` with length in `0..=max_len` |
| `Rng::pick(items)` | One item from a slice; panics on an empty slice |
| `forall(cases, seed, prop)` | Run `prop` on `cases` independent streams |
| `replay(case_seed, prop)` | Re-run one case from a printed seed |
| `hex(s)` | Decode hex; panics on bad input (test use only) |
| `to_hex(bytes)` | Encode bytes as lowercase hex |

## Demo

```
cargo run -p assay --example demo
```

Real output:

```
seed 42, first three values: [13679457532755275413, 2949826092126892291, 5139283748462763858]
second generator with seed 42 matches: true
five draws from 1..7: [4, 1, 1, 4, 5]
below(0) returns 0 (an empty range gives its lower bound)
chance(1, 2) returned false
random bytes, length 1: fe
picked name: gamma
range property held on 500 cases
hex("de ad:be:ef") encodes back to deadbeef
replayed case 0x1234 first draw: 6873671191729682568
demo finished
```

## Tests

```
cargo test -p assay
```

The crate gate is `bash tools/check.sh assay` from the repository root. It runs format, clippy with
warnings denied, the tests, and the zero-dependency check.
