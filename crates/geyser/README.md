# geyser

geyser is a deterministic, multi-CPU fair scheduler core, with a small work-stealing thread pool built on top of it. The scheduler is pure: the caller passes in the time, so the same sequence of calls always gives the same decisions. It exists so that other Strata crates can reason about CPU sharing without a real clock or real threads.

## Features

- `Scheduler` with 1 to 64 CPUs (values outside the range are clamped), one run queue per CPU.
- Three scheduling classes: `Realtime { prio }` (higher prio first), `Normal { nice }` (weighted fair, nice clamped to -20..=19 with the standard CFS weight table), and `Idle` (runs only when nothing else is ready).
- CPU affinity as a bitmask; `0` means any CPU.
- Time slices: normal tasks get `max(min_slice_us, latency_us / ready normal tasks)`; realtime and idle tasks get `min_slice_us`.
- `pick` steals ready tasks from other CPU queues when the local queue has nothing to run or a better class waits elsewhere.
- Task lifecycle: `spawn`, `pick`, `block`, `wake`, `exit`, `set_class`, with `stats` and `runnable` for inspection.
- The core is `no_std` and uses `alloc`. It has no external dependencies.
- `pool` module (feature `std`, on by default): a work-stealing thread pool. Each worker has a local deque, jobs from outside go to a shared injector, and each worker has a core task pinned to its own CPU index.
- The pool caps workers at 64, supports nested spawns through `pool::current()`, and reports `PoolStats` (spawned, completed, stolen).
- Jobs that panic are caught and still counted as completed.

## Public API

| Item | What it does |
| --- | --- |
| `Scheduler::new(cpus, Config)` | Create a scheduler for 1..=64 CPUs |
| `Scheduler::spawn(class, affinity, now_us)` | Create a task, return its `TaskId` |
| `Scheduler::pick(cpu, now_us)` | Charge the running task, return a `Decision` (task and slice) |
| `Scheduler::block` / `wake` / `exit` | Move a task between Ready, Running, Blocked and Dead |
| `Scheduler::set_class(task, class)` | Change the class of a live task |
| `Scheduler::stats(task)` / `runnable()` | Read a `Stats` snapshot or count runnable tasks |
| `Config`, `Class`, `State`, `Decision`, `Stats`, `Error` | Supporting types |
| `pool::Pool::new(threads)` | Start a pool (1..=64 workers) |
| `Pool::spawn`, `wait_idle`, `stats`, `shutdown` | Queue jobs, wait for them, read counters, stop |
| `pool::Handle::spawn`, `pool::current()` | Spawn from inside a running job |

## Demo

The demo steps the core scheduler by hand, then runs 100 jobs on the pool. It reads no clock and uses no network, so its output is stable.

```
cargo run -p geyser --example demo
```

Real output:

```
core scheduler: 2 CPUs, latency 6000 us, min slice 750 us
spawned rt=0 fast=1 slow=2 idle=3, runnable=4
t=   0 us  cpu0 runs Some(0) for 750 us
t=   0 us  cpu1 runs Some(1) for 6000 us
t=1000 us  cpu0 runs Some(0) for 750 us
t=1000 us  cpu1 runs Some(1) for 6000 us
t=2000 us  cpu0 runs Some(0) for 750 us
t=2000 us  cpu1 runs Some(1) for 6000 us
slow blocked: state Blocked
slow woken: state Ready
rt exited: state Dead
fast: runtime 2000 us, switches 3, runnable now 3
thread pool: 4 workers
sum of squares 1..=100 plus child bonus = 339350
jobs spawned=101 completed=101
done
```

## Tests

```
cargo test -p geyser
```

The test suite includes unit tests beside the scheduler and pool, plus integration tests in `crates/geyser/tests/`. Property checks use the `assay` dev-dependency.

## Gate

```
bash tools/check.sh geyser
```

The gate runs format, clippy with warnings as errors, the tests, and the zero-dependency check.
