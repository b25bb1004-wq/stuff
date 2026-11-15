# bench

`bench` is the load and throughput harness for the `mercury`, `iridium`, `xenon` and `aurum` crates. Each suite runs its workload for a fixed time budget and reports operations per second. The crate has a small library (`measure`, `Sample`, `Opts`, `table`) and one binary, `bench`, that runs the suites and prints a table. It is not published.

## Features

- `measure` calls a workload repeatedly until a wall-clock budget has elapsed and returns a `Sample` with the total operations and elapsed seconds
- `Sample::rate` converts a sample to operations per second, returning `0.0` when the elapsed time is zero or not finite
- `Opts` sets the time budget and a dataset scale: 200 ms and scale 1 for a quick run, 2000 ms and scale 10 for `--full`
- `table` formats samples as an aligned text table with the suite, name, rate and unit
- One `run(&Opts)` entry point per suite: `mercury::run`, `iridium::run`, `xenon::run` and `aurum::run`

## Public API

| Item | Role |
| --- | --- |
| `Sample` | One measurement: suite, name, unit, total ops, elapsed seconds, and `rate()` |
| `Opts` | Time budget (`budget`) and dataset multiplier (`scale`); `Default` is the quick run |
| `measure` | Run a closure until the budget elapses and return a `Sample` |
| `table` | Format a slice of samples as a text table |
| `mercury::run`, `iridium::run`, `xenon::run`, `aurum::run` | Run one suite and return its samples, or an error string |

## Build and run

Run every suite with the quick budget:

```
cargo run -q --release -p bench --bin bench
```

Run the full budget (2000 ms per benchmark, dataset scale 10):

```
cargo run -q --release -p bench --bin bench -- --full
```

Run selected suites. `--only` can be repeated, and `--only=SUITE` also works:

```
cargo run -q --release -p bench --bin bench -- --only mercury --only aurum
```

Usage line printed on a bad argument:

```
usage: bench [--full] [--only SUITE]...
suites: mercury, iridium, xenon, aurum
```

From Rust code, call a suite directly and format the result:

```rust
let opts = bench::Opts::default();
let samples = bench::mercury::run(&opts)?;
print!("{}", bench::table(&samples));
```

No other crate in the workspace depends on `bench`. It is run from its binary or from its own tests.

## Tests

```
cargo test -p bench
```

The tests check `measure` against its budget and batch-summing behaviour, and check `table` and the suite runners.
