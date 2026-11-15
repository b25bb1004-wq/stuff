//! Soak test for the geyser scheduler and pool: random op sequences checked against a
//! model, plus a 1 s watchdog per case. Budget per target: `STRATA_SOAK_MS` (default 300).
//! Seed: `STRATA_SOAK_SEED`, decimal or `0x` hex (default `0x5EED_79D`).
//! geyser has no parsers or decoders, so the byte-input half of the soak does not apply.
#![allow(clippy::unwrap_used)]

use std::panic::resume_unwind;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use assay::Rng;
use geyser::pool::{self, Pool};
use geyser::{Class, Config, Error, Scheduler, State};

/// The brief's default seed, 0x5EED_79D, written with even digit groups.
const DEFAULT_SEED: u64 = 0x05EE_D79D;
const DEFAULT_MS: u64 = 300;
const CASE_LIMIT: Duration = Duration::from_secs(1);
/// Caps the work per case, which also bounds allocation (no allocator hook; sizes are the check).
const MAX_TASKS: usize = 300;

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Err(_) => default,
        Ok(text) => {
            let t = text.trim();
            let parsed = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(hex) => u64::from_str_radix(hex, 16),
                None => t.parse::<u64>(),
            };
            parsed.unwrap_or_else(|_| panic!("{name} is not a u64: {text:?}"))
        }
    }
}

/// Prints the failing case seed when a case panics.
struct CaseReport {
    target: &'static str,
    seed: u64,
}

impl Drop for CaseReport {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!(
                "soak: {} failed on case seed {:#018x}",
                self.target, self.seed
            );
        }
    }
}

/// Run `case` on derived seeds until the budget is spent; returns the case count.
fn run_budget(target: &'static str, seed: u64, ms: u64, case: impl Fn(u64)) -> u64 {
    let budget = Duration::from_millis(ms);
    let start = Instant::now();
    let mut master = Rng::new(seed);
    let mut cases = 0u64;
    let mut slowest = Duration::ZERO;
    loop {
        let case_seed = master.u64();
        let _report = CaseReport {
            target,
            seed: case_seed,
        };
        let began = Instant::now();
        case(case_seed);
        let took = began.elapsed();
        slowest = slowest.max(took);
        assert!(
            took <= CASE_LIMIT,
            "soak: {target} case {case_seed:#018x} took over 1 s"
        );
        cases += 1;
        if start.elapsed() >= budget {
            break;
        }
    }
    println!(
        "soak: {target} seed={seed:#018x} cases={cases} budget_ms={ms} slowest_case_ms={}",
        slowest.as_millis()
    );
    cases
}

/// Run `f` on a helper thread; fail if it does not finish within 1 s (deadlock watchdog).
/// `diag` is read on the timeout path so the report says how far the case got.
fn within_limit(what: &str, f: impl FnOnce() + Send + 'static, diag: impl FnOnce() -> String) {
    let (tx, rx) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    match rx.recv_timeout(CASE_LIMIT) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
        Err(RecvTimeoutError::Timeout) => {
            panic!(
                "soak: {what} did not finish within 1 s (deadlock?): {}",
                diag()
            )
        }
    }
    if let Err(payload) = worker.join() {
        resume_unwind(payload);
    }
}

// ---------------------------------------------------------------- scheduler

#[derive(Clone, Copy, Debug)]
struct Mt {
    state: State,
    class: Class,
    /// Effective affinity: the mask with `0` expanded and out-of-range bits removed.
    aff: u64,
    cpu: Option<usize>,
}

fn clamp_class(c: Class) -> Class {
    match c {
        Class::Normal { nice } => Class::Normal {
            nice: nice.clamp(-20, 19),
        },
        other => other,
    }
}

/// The scheduler model: expected task states, with each op checked against the real one.
struct Soak {
    s: Scheduler,
    m: Vec<Mt>,
    cur: Vec<Option<usize>>,
    cpus: usize,
    mask: u64,
    min_slice: u64,
    now: u64,
}

impl Soak {
    fn spawn(&mut self, class: Class, aff: u64) {
        let eff = if aff == 0 { self.mask } else { aff & self.mask };
        match self.s.spawn(class, aff, self.now) {
            Ok(id) => {
                assert!(
                    eff != 0,
                    "soak: spawn accepted affinity {aff:#x} that names no CPU (task never runs)"
                );
                assert_eq!(id as usize, self.m.len(), "soak: task ids not sequential");
                self.m.push(Mt {
                    state: State::Ready,
                    class: clamp_class(class),
                    aff: eff,
                    cpu: None,
                });
            }
            Err(e) => assert!(
                eff == 0 && e == Error::BadCpu,
                "soak: spawn({aff:#x}) returned {e:?}"
            ),
        }
    }

    fn pick(&mut self, cpu: usize) {
        if cpu >= self.cpus {
            assert_eq!(self.s.pick(cpu, self.now), Err(Error::BadCpu));
            return;
        }
        if let Some(prev) = self.cur[cpu].take() {
            self.m[prev].state = State::Ready;
            self.m[prev].cpu = None;
        }
        let d = self.s.pick(cpu, self.now).unwrap();
        match d.task {
            Some(tid) => {
                let t = tid as usize;
                let Some(task) = self.m.get(t).copied() else {
                    panic!("soak: pick returned unknown task {t}")
                };
                assert_eq!(
                    task.state,
                    State::Ready,
                    "soak: pick returned task {t} in state {:?} (double run or lost wakeup)",
                    task.state
                );
                assert!(
                    task.aff & (1u64 << cpu) != 0,
                    "soak: task {t} picked on cpu {cpu} outside its affinity"
                );
                match task.class {
                    Class::Normal { .. } => assert!(
                        d.slice_us >= self.min_slice,
                        "soak: slice {} below min {}",
                        d.slice_us,
                        self.min_slice
                    ),
                    _ => assert_eq!(d.slice_us, self.min_slice, "soak: fixed slice mismatch"),
                }
                self.m[t].state = State::Running;
                self.m[t].cpu = Some(cpu);
                self.cur[cpu] = Some(t);
            }
            None => {
                assert_eq!(d.slice_us, 0, "soak: idle decision with a slice");
                let bit = 1u64 << cpu;
                let work = self
                    .m
                    .iter()
                    .any(|x| x.state == State::Ready && x.aff & bit != 0);
                assert!(!work, "soak: cpu {cpu} idled while a ready task allowed it");
            }
        }
    }

    fn block(&mut self, t: usize) {
        let res = self.s.block(t as u32, self.now);
        let Some(task) = self.m.get(t).copied() else {
            assert_eq!(res, Err(Error::NoSuchTask));
            return;
        };
        match task.state {
            State::Running | State::Ready => {
                assert_eq!(res, Ok(()), "soak: block({t}) failed");
                if let Some(c) = task.cpu {
                    self.cur[c] = None;
                }
                self.m[t].state = State::Blocked;
                self.m[t].cpu = None;
            }
            _ => assert_eq!(
                res,
                Err(Error::BadState),
                "soak: block({t}) on {:?}",
                task.state
            ),
        }
    }

    fn wake(&mut self, t: usize) {
        let res = self.s.wake(t as u32, self.now);
        let Some(task) = self.m.get(t).copied() else {
            assert_eq!(res, Err(Error::NoSuchTask));
            return;
        };
        if task.state == State::Blocked {
            assert_eq!(res, Ok(()), "soak: wake({t}) failed");
            self.m[t].state = State::Ready;
        } else {
            assert_eq!(
                res,
                Err(Error::BadState),
                "soak: wake({t}) on {:?}",
                task.state
            );
        }
    }

    fn exit(&mut self, t: usize) {
        let res = self.s.exit(t as u32, self.now);
        let Some(task) = self.m.get(t).copied() else {
            assert_eq!(res, Err(Error::NoSuchTask));
            return;
        };
        if task.state == State::Dead {
            assert_eq!(res, Err(Error::BadState));
            return;
        }
        assert_eq!(res, Ok(()), "soak: exit({t}) failed");
        if let Some(c) = task.cpu {
            self.cur[c] = None;
        }
        self.m[t].state = State::Dead;
        self.m[t].cpu = None;
    }

    fn set_class(&mut self, t: usize, class: Class) {
        let res = self.s.set_class(t as u32, class);
        let Some(task) = self.m.get(t).copied() else {
            assert_eq!(res, Err(Error::NoSuchTask));
            return;
        };
        if task.state == State::Dead {
            assert_eq!(res, Err(Error::BadState));
        } else {
            assert_eq!(res, Ok(()), "soak: set_class({t}) failed");
            self.m[t].class = clamp_class(class);
        }
    }

    fn runnable_model(&self) -> usize {
        self.m
            .iter()
            .filter(|x| matches!(x.state, State::Ready | State::Running))
            .count()
    }

    /// Compare every task's stats and the runnable count against the model.
    fn check_all(&self) {
        assert_eq!(
            self.s.runnable(),
            self.runnable_model(),
            "soak: runnable() mismatch"
        );
        for (t, mt) in self.m.iter().enumerate() {
            let st = self.s.stats(t as u32).unwrap();
            assert_eq!(st.state, mt.state, "soak: task {t} state");
            assert_eq!(st.class, mt.class, "soak: task {t} class");
            assert_eq!(st.cpu, mt.cpu, "soak: task {t} cpu");
        }
        assert_eq!(
            self.s.stats(self.m.len() as u32),
            Err(Error::NoSuchTask),
            "soak: unknown id did not report NoSuchTask"
        );
    }
}

fn rand_class(r: &mut Rng) -> Class {
    match r.below(3) {
        0 => Class::Realtime {
            prio: r.u64() as u8,
        },
        1 => Class::Normal {
            nice: *r.pick(&[i8::MIN, -21, -20, -1, 0, 1, 19, 20, i8::MAX]),
        },
        _ => Class::Idle,
    }
}

/// Affinity masks: any-CPU, one valid CPU, random bits, and bits that may name no CPU.
fn rand_aff(r: &mut Rng, cpus: usize) -> u64 {
    match r.below(5) {
        0 => 0,
        1 => 1u64 << r.below(cpus),
        2 => r.u64(),
        3 => 1u64 << r.below(64),
        _ => r.u64() & r.u64(),
    }
}

fn sched_case(seed: u64) {
    let mut r = Rng::new(seed);
    let cpus_req = *r.pick(&[0usize, 1, 2, 3, 4, 8, 63, 64, 65, 1000]);
    let cpus = cpus_req.clamp(1, 64);
    let config = Config {
        latency_us: *r.pick(&[0u64, 1, 6000, u64::MAX]),
        min_slice_us: *r.pick(&[0u64, 750, u64::MAX]),
    };
    let mut soak = Soak {
        s: Scheduler::new(cpus_req, config),
        m: Vec::new(),
        cur: vec![None; cpus],
        cpus,
        mask: if cpus == 64 {
            u64::MAX
        } else {
            (1u64 << cpus) - 1
        },
        min_slice: config.min_slice_us,
        now: 0,
    };
    for _ in 0..r.range(1, 400) {
        soak.now = if r.chance(1, 20) {
            r.u64()
        } else {
            soak.now.saturating_add(r.range(0, 5000))
        };
        let op = r.below(100);
        if op <= 24 {
            if soak.m.len() < MAX_TASKS {
                let class = rand_class(&mut r);
                let aff = rand_aff(&mut r, cpus);
                soak.spawn(class, aff);
            }
        } else if op <= 44 {
            let cpu = r.below(cpus + 1);
            soak.pick(cpu);
        } else if op <= 54 {
            let t = r.below(soak.m.len() + 2);
            soak.block(t);
        } else if op <= 64 {
            let t = r.below(soak.m.len() + 2);
            soak.wake(t);
        } else if op <= 72 {
            let t = r.below(soak.m.len() + 2);
            soak.exit(t);
        } else if op <= 80 {
            let t = r.below(soak.m.len() + 2);
            let class = rand_class(&mut r);
            soak.set_class(t, class);
        }
        soak.check_all();
    }
    // Drain: wake the blocked, then pick and retire until nothing is runnable.
    for t in 0..soak.m.len() {
        if soak.m[t].state == State::Blocked {
            soak.wake(t);
        }
    }
    for _ in 0..(2 * soak.m.len() + 8) {
        if soak.runnable_model() == 0 {
            break;
        }
        for cpu in 0..cpus {
            soak.pick(cpu);
            if let Some(t) = soak.cur[cpu] {
                soak.exit(t);
            }
        }
    }
    let left = soak.runnable_model();
    assert_eq!(left, 0, "soak: {left} task(s) never ran to exit (lost)");
    soak.check_all();
}

// ---------------------------------------------------------------- pool

fn pool_case(seed: u64) {
    let mut r = Rng::new(seed);
    let threads = *r.pick(&[0usize, 1, 2, 3, 8, 64, 100]);
    let n = r.below(201);
    let child: Vec<bool> = (0..n).map(|_| r.chance(1, 3)).collect();
    let boom: Vec<bool> = (0..n).map(|_| r.chance(1, 40)).collect();
    // Parent job i counts into slot i; its child (if any) counts into a slot after the parents.
    let mut next = n;
    let child_slot: Vec<Option<usize>> = child
        .iter()
        .map(|&c| {
            if c {
                next += 1;
                Some(next - 1)
            } else {
                None
            }
        })
        .collect();
    let slots = next;
    let counts: Arc<Vec<AtomicU32>> = Arc::new((0..slots).map(|_| AtomicU32::new(0)).collect());
    let stage = Arc::new(AtomicU32::new(0));
    let (diag_counts, diag_stage) = (Arc::clone(&counts), Arc::clone(&stage));
    within_limit(
        "pool",
        move || {
            let tp = Pool::new(threads);
            assert!(
                pool::current().is_none(),
                "soak: test thread is not a worker"
            );
            for i in 0..n {
                let counts = Arc::clone(&counts);
                let child_slot = child_slot[i];
                let boom = boom[i];
                tp.spawn(move || {
                    counts[i].fetch_add(1, SeqCst);
                    if let Some(slot) = child_slot {
                        let counts = Arc::clone(&counts);
                        let handle = pool::current().expect("soak: job not on a worker");
                        handle.spawn(move || {
                            counts[slot].fetch_add(1, SeqCst);
                        });
                    }
                    if boom {
                        panic!("soak: planned job panic");
                    }
                });
            }
            stage.store(1, SeqCst);
            tp.wait_idle();
            stage.store(2, SeqCst);
            tp.wait_idle();
            stage.store(3, SeqCst);
            let st = tp.stats();
            let children = child.iter().filter(|&&c| c).count();
            assert_eq!(st.spawned as usize, n + children, "soak: spawned count");
            assert_eq!(st.completed, st.spawned, "soak: jobs lost or unfinished");
            assert!(st.stolen <= st.spawned, "soak: stolen exceeds spawned");
            for (slot, c) in counts.iter().enumerate() {
                assert_eq!(
                    c.load(SeqCst),
                    1,
                    "soak: job slot {slot} did not run exactly once"
                );
            }
            tp.shutdown();
        },
        move || {
            let ran: u32 = diag_counts.iter().map(|c| c.load(SeqCst)).sum();
            format!(
                "stage {} (0 spawning, 1 waiting, 2 waiting again, 3 checking), {ran} of {slots} jobs ran",
                diag_stage.load(SeqCst)
            )
        },
    );
}

#[test]
fn soak_targets() {
    let seed = env_u64("STRATA_SOAK_SEED", DEFAULT_SEED);
    let ms = env_u64("STRATA_SOAK_MS", DEFAULT_MS);
    run_budget("scheduler", seed, ms, sched_case);
    run_budget("pool", seed ^ 0x706F_6F6C, ms, pool_case);
}

#[test]
fn affinity_naming_no_cpu_is_rejected() {
    let mut s = Scheduler::new(4, Config::default());
    assert_eq!(s.spawn(Class::Idle, 1u64 << 40, 0), Err(Error::BadCpu));
    assert_eq!(s.runnable(), 0);
    assert!(s.spawn(Class::Idle, 0b1000, 0).is_ok());
    assert_eq!(s.runnable(), 1);
}
