//! Work-stealing thread pool on std threads, driven by the core `Scheduler`.
//!
//! Each worker owns a local deque: its own jobs push and pop at the back,
//! other workers steal from the front. Jobs spawned from outside the pool go
//! to a shared injector. Each worker has one core task pinned to its own CPU;
//! the core's `pick`, `block` and `wake` decide when that worker runs and sleeps.

use std::boxed::Box;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::format;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::thread_local;
use std::time::Instant;
use std::vec::Vec;

use crate::{Class, Config, Scheduler, TaskId};

#[cfg(test)]
mod tests;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// The core scheduler supports at most 64 CPUs, so the pool does too.
/// shortcut: 64 workers maximum, upgrade by widening the core's CPU mask.
const MAX_THREADS: usize = 64;

/// Golden-ratio constant used to seed each worker's steal generator.
const SEED_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// Counters reported by `Pool::stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
    /// Jobs accepted by `Pool::spawn` or `Handle::spawn`.
    pub spawned: u64,
    /// Jobs that finished, including ones that panicked.
    pub completed: u64,
    /// Jobs taken from another worker's deque.
    pub stolen: u64,
}

/// A pool of worker threads. Dropping it runs `shutdown`.
pub struct Pool {
    shared: Arc<Shared>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

/// A cloneable handle that spawns jobs on the pool a running job belongs to.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
}

/// Wake-up epoch and shutdown flag, guarded by one mutex.
struct Signal {
    epoch: u64,
    closing: bool,
}

struct Shared {
    start: Instant,
    sched: Mutex<Scheduler>,
    /// Core task of worker `i`, pinned to CPU `i`.
    tokens: Vec<Option<TaskId>>,
    queues: Vec<Mutex<VecDeque<Job>>>,
    injector: Mutex<VecDeque<Job>>,
    signal: Mutex<Signal>,
    wake: Condvar,
    idle_mx: Mutex<()>,
    idle: Condvar,
    /// Jobs spawned and not yet finished.
    outstanding: AtomicU64,
    spawned: AtomicU64,
    completed: AtomicU64,
    stolen: AtomicU64,
}

thread_local! {
    /// Set on worker threads: the pool they serve and their worker index.
    static CURRENT: RefCell<Option<(Arc<Shared>, usize)>> = const { RefCell::new(None) };
}

impl Pool {
    /// Start a pool with `threads` workers (clamped to 1..=64).
    pub fn new(threads: usize) -> Pool {
        let threads = threads.clamp(1, MAX_THREADS);
        let shared = Arc::new(Shared::new(threads));
        let mut handles = Vec::with_capacity(threads);
        for idx in 0..threads {
            let worker_shared = Arc::clone(&shared);
            let built = thread::Builder::new()
                .name(format!("kairos-{idx}"))
                .spawn(move || worker(worker_shared, idx));
            // If the OS refuses a thread, the pool runs with fewer workers.
            if let Ok(handle) = built {
                handles.push(handle);
            }
        }
        Pool {
            shared,
            handles: Mutex::new(handles),
        }
    }

    /// Queue `f` to run on some worker. Returns without waiting for it to start.
    pub fn spawn<F: FnOnce() + Send + 'static>(&self, f: F) {
        self.shared.submit(Box::new(f), None);
    }

    /// Block until every spawned job, including jobs spawned by jobs, has run.
    pub fn wait_idle(&self) {
        let mut guard = lock(&self.shared.idle_mx);
        while self.shared.outstanding.load(SeqCst) > 0 {
            guard = wait(&self.shared.idle, guard);
        }
    }

    /// Run every queued job, then stop and join all worker threads.
    pub fn shutdown(self) {
        // Dropping `self` at the end of this call does the work.
    }

    /// Counters for jobs spawned, completed and stolen so far.
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            spawned: self.shared.spawned.load(SeqCst),
            completed: self.shared.completed.load(SeqCst),
            stolen: self.shared.stolen.load(SeqCst),
        }
    }

    fn stop(&self) {
        lock(&self.shared.signal).closing = true;
        self.shared.wake.notify_all();
        let handles = std::mem::take(&mut *lock(&self.handles));
        let me = thread::current().id();
        for handle in handles {
            // A job that owns the pool must not join its own thread.
            if handle.thread().id() != me {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Handle {
    /// Queue `f` on this handle's pool. A job spawned from one of that pool's
    /// workers goes to that worker's own deque first.
    pub fn spawn<F: FnOnce() + Send + 'static>(&self, f: F) {
        let local = CURRENT.with(|c| match c.borrow().as_ref() {
            Some((s, idx)) if Arc::ptr_eq(s, &self.shared) => Some(*idx),
            _ => None,
        });
        self.shared.submit(Box::new(f), local);
    }
}

/// A handle to the pool running the current job, or `None` off a worker thread.
pub fn current() -> Option<Handle> {
    CURRENT.with(|c| {
        c.borrow().as_ref().map(|(s, _)| Handle {
            shared: Arc::clone(s),
        })
    })
}

impl Shared {
    fn new(threads: usize) -> Shared {
        let mut sched = Scheduler::new(threads, Config::default());
        let mut tokens = Vec::with_capacity(threads);
        for idx in 0..threads {
            tokens.push(sched.spawn(Class::Normal { nice: 0 }, 1u64 << idx, 0).ok());
        }
        Shared {
            start: Instant::now(),
            sched: Mutex::new(sched),
            tokens,
            queues: (0..threads).map(|_| Mutex::new(VecDeque::new())).collect(),
            injector: Mutex::new(VecDeque::new()),
            signal: Mutex::new(Signal {
                epoch: 0,
                closing: false,
            }),
            wake: Condvar::new(),
            idle_mx: Mutex::new(()),
            idle: Condvar::new(),
            outstanding: AtomicU64::new(0),
            spawned: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            stolen: AtomicU64::new(0),
        }
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn token(&self, idx: usize) -> Option<TaskId> {
        self.tokens.get(idx).copied().flatten()
    }

    /// Ask the core which task runs on CPU `idx` now (marks this worker running).
    fn sched_pick(&self, idx: usize) {
        let now = self.now_us();
        let _ = lock(&self.sched).pick(idx, now);
    }

    /// Mark this worker's core task blocked while it sleeps.
    fn sched_park(&self, idx: usize) {
        if let Some(task) = self.token(idx) {
            let now = self.now_us();
            let _ = lock(&self.sched).block(task, now);
        }
    }

    /// Wake this worker's core task after it sleeps and pick it again.
    fn sched_resume(&self, idx: usize) {
        if let Some(task) = self.token(idx) {
            let now = self.now_us();
            let mut sched = lock(&self.sched);
            let _ = sched.wake(task, now);
            let _ = sched.pick(idx, now);
        }
    }

    /// Queue a job (on a worker's deque or the injector) and wake one sleeper.
    fn submit(&self, job: Job, worker: Option<usize>) {
        self.outstanding.fetch_add(1, SeqCst);
        self.spawned.fetch_add(1, SeqCst);
        let queue = worker
            .and_then(|idx| self.queues.get(idx))
            .unwrap_or(&self.injector);
        lock(queue).push_back(job);
        let mut signal = lock(&self.signal);
        signal.epoch = signal.epoch.wrapping_add(1);
        self.wake.notify_one();
    }

    /// Find a job: own deque, then injector, then steal from another deque.
    fn find_job(&self, idx: usize, rng: &mut u64) -> Option<Job> {
        if let Some(job) = self.queues.get(idx).and_then(|q| take(q, false)) {
            return Some(job);
        }
        if let Some(job) = take(&self.injector, true) {
            return Some(job);
        }
        let n = self.queues.len();
        let start = (next_rand(rng) % (n.max(1) as u64)) as usize;
        for step in 0..n {
            let victim = (start + step) % n;
            if victim == idx {
                continue;
            }
            if let Some(job) = self.queues.get(victim).and_then(|q| take(q, true)) {
                self.stolen.fetch_add(1, SeqCst);
                return Some(job);
            }
        }
        None
    }

    /// Run one job. A panic is caught so the worker and other jobs survive.
    fn run(&self, job: Job) {
        let _ = catch_unwind(AssertUnwindSafe(job));
        self.completed.fetch_add(1, SeqCst);
        if self.outstanding.fetch_sub(1, SeqCst) == 1 {
            let _guard = lock(&self.idle_mx);
            self.idle.notify_all();
        }
    }
}

fn worker(shared: Arc<Shared>, idx: usize) {
    CURRENT.with(|c| *c.borrow_mut() = Some((Arc::clone(&shared), idx)));
    let mut rng = (idx as u64 + 1).wrapping_mul(SEED_MIX);
    shared.sched_pick(idx);
    loop {
        // Read the epoch before scanning, so a push during the scan is never missed.
        let (seen, closing) = {
            let signal = lock(&shared.signal);
            (signal.epoch, signal.closing)
        };
        if let Some(job) = shared.find_job(idx, &mut rng) {
            shared.run(job);
            continue;
        }
        if closing {
            break;
        }
        shared.sched_park(idx);
        {
            let mut signal = lock(&shared.signal);
            while signal.epoch == seen && !signal.closing {
                signal = wait(&shared.wake, signal);
            }
        }
        shared.sched_resume(idx);
    }
    CURRENT.with(|c| *c.borrow_mut() = None);
}

fn take(queue: &Mutex<VecDeque<Job>>, front: bool) -> Option<Job> {
    let mut q = lock(queue);
    if front {
        q.pop_front()
    } else {
        q.pop_back()
    }
}

fn next_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a, T>(cv: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(guard).unwrap_or_else(PoisonError::into_inner)
}
