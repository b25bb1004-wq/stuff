//! Leader check for T15: seeded soak of the pool with nested spawns and panics, where the exact
//! set of jobs is known in advance; repeated create/drop while jobs still spawn; stealing.
#![allow(clippy::unwrap_used)]
use kairos::pool::{current, Pool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn watchdog(secs: u64, what: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("WATCHDOG: {what} still running after {secs} s");
        std::process::abort();
    });
}

fn quiet_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let old = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !info.to_string().contains("leader-planned") {
                old(info);
            }
        }));
    });
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// How many children job `key` at `depth` spawns (same rule in the pool and the model).
fn kids(key: u64, depth: u32, max_depth: u32) -> u64 {
    if depth >= max_depth {
        0
    } else {
        mix(key) % 4
    }
}

fn model(key: u64, depth: u32, max_depth: u32, out: &mut Vec<u64>) {
    out.push(key);
    for k in 0..kids(key, depth, max_depth) {
        model(mix(key ^ (k + 1)), depth + 1, max_depth, out);
    }
}

struct Seen {
    shards: Vec<Mutex<HashMap<u64, u32>>>,
}

impl Seen {
    fn new() -> Arc<Seen> {
        Arc::new(Seen {
            shards: (0..64).map(|_| Mutex::new(HashMap::new())).collect(),
        })
    }
    fn hit(&self, key: u64) {
        *self.shards[(key % 64) as usize]
            .lock()
            .unwrap()
            .entry(key)
            .or_insert(0) += 1;
    }
    fn all(&self) -> HashMap<u64, u32> {
        let mut m = HashMap::new();
        for s in &self.shards {
            m.extend(s.lock().unwrap().iter().map(|(k, v)| (*k, *v)));
        }
        m
    }
}

fn job(key: u64, depth: u32, max_depth: u32, seen: Arc<Seen>, panic_every: u64) {
    seen.hit(key);
    let h = current().expect("current() inside a job");
    for k in 0..kids(key, depth, max_depth) {
        let child = mix(key ^ (k + 1));
        let s = Arc::clone(&seen);
        h.spawn(move || job(child, depth + 1, max_depth, s, panic_every));
    }
    if panic_every > 0 && key.is_multiple_of(panic_every) {
        panic!("leader-planned panic in job {key:#x}");
    }
}

fn expected(roots: &[u64], max_depth: u32) -> HashMap<u64, u32> {
    let mut v = Vec::new();
    for r in roots {
        model(*r, 0, max_depth, &mut v);
    }
    let mut m = HashMap::new();
    for k in v {
        *m.entry(k).or_insert(0) += 1;
    }
    m
}

#[test]
fn leader_seeded_soak_runs_exactly_the_planned_jobs() {
    watchdog(240, "soak");
    quiet_panics();
    let seed = env("STRATA_SEED", 0x15_2026_1114);
    let pool = Pool::new(8);
    let seen = Seen::new();
    let roots: Vec<u64> = (0..4000).map(|i| mix(seed ^ i)).collect();
    for r in &roots {
        let s = Arc::clone(&seen);
        let r = *r;
        pool.spawn(move || job(r, 0, 7, s, 97));
    }
    pool.wait_idle();
    let want = expected(&roots, 7);
    let total: u32 = want.values().sum();
    let st = pool.stats();
    eprintln!("soak: {total} jobs, stats {st:?}");
    assert!(total > 20_000, "soak too small: {total}");
    assert_eq!(seen.all(), want, "some job ran zero or two times");
    assert_eq!(st.spawned, u64::from(total));
    assert_eq!(st.completed, st.spawned);
    pool.wait_idle(); // a second wait on an idle pool returns at once
    pool.shutdown();
}

#[test]
fn leader_drop_while_jobs_still_spawn() {
    watchdog(240, "create/drop loop");
    quiet_panics();
    let seed = env("STRATA_SEED", 0x15_2026_1114) ^ 0xd0;
    for round in 0..150u64 {
        let threads = 1 + (round % 8) as usize;
        let pool = Pool::new(threads);
        let seen = Seen::new();
        let roots: Vec<u64> = (0..1 + round % 20)
            .map(|i| mix(seed ^ round ^ (i << 20)))
            .collect();
        for r in &roots {
            let s = Arc::clone(&seen);
            let r = *r;
            pool.spawn(move || job(r, 0, 5, s, 0));
        }
        if round % 2 == 0 {
            drop(pool);
        } else {
            pool.shutdown();
        }
        assert_eq!(
            seen.all(),
            expected(&roots, 5),
            "round {round} ({threads} threads): queued or child jobs lost at shutdown"
        );
    }
}

#[test]
fn leader_waiters_and_stealing() {
    watchdog(120, "waiters and stealing");
    let pool = Arc::new(Pool::new(4));
    let done = Arc::new(AtomicU64::new(0));
    let d = Arc::clone(&done);
    // One job floods its own deque; the other workers can only get work by stealing.
    pool.spawn(move || {
        let h = current().unwrap();
        for _ in 0..3000 {
            let d = Arc::clone(&d);
            h.spawn(move || {
                std::thread::sleep(Duration::from_micros(30));
                d.fetch_add(1, SeqCst);
            });
        }
    });
    let waiters: Vec<_> = (0..3)
        .map(|_| {
            let p = Arc::clone(&pool);
            let d = Arc::clone(&done);
            std::thread::spawn(move || {
                p.wait_idle();
                d.load(SeqCst)
            })
        })
        .collect();
    for w in waiters {
        assert_eq!(w.join().unwrap(), 3000, "wait_idle returned early");
    }
    let st = pool.stats();
    eprintln!("stealing: {st:?}");
    assert!(st.stolen > 100, "other workers did not steal: {st:?}");
    assert!(current().is_none(), "current() off the pool");
}
