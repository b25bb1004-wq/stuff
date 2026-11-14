//! Tests for the thread pool. Each test runs under a 60 s watchdog.
#![allow(clippy::unwrap_used)]

use super::*;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Run `body` on its own thread; abort the process if it takes longer than 60 s.
fn watchdog(name: &str, body: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        body();
        let _ = tx.send(());
    });
    match rx.recv_timeout(Duration::from_secs(60)) {
        Err(RecvTimeoutError::Timeout) => {
            eprintln!("WATCHDOG: {name} did not finish within 60 s");
            std::process::abort();
        }
        _ => {
            if let Err(payload) = handle.join() {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

fn counters(n: usize) -> Arc<Vec<AtomicU32>> {
    Arc::new((0..n).map(|_| AtomicU32::new(0)).collect())
}

/// Visit node `id` of an implicit binary tree and spawn its two children.
fn visit(id: usize, total: usize, hits: Arc<Vec<AtomicU32>>) {
    if let Some(h) = hits.get(id) {
        h.fetch_add(1, SeqCst);
    }
    if let Some(handle) = current() {
        for child in [2 * id + 1, 2 * id + 2] {
            if child < total {
                let hits = Arc::clone(&hits);
                handle.spawn(move || visit(child, total, hits));
            }
        }
    }
}

#[test]
fn current_is_none_off_worker_threads() {
    assert!(current().is_none());
}

#[test]
fn zero_threads_clamps_to_one_worker() {
    watchdog("zero_threads", || {
        let pool = Pool::new(0);
        let ran = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&ran);
        pool.spawn(move || {
            r.fetch_add(1, SeqCst);
        });
        pool.wait_idle();
        assert_eq!(ran.load(SeqCst), 1);
        assert_eq!(pool.stats().completed, 1);
    });
}

#[test]
fn million_tiny_jobs_run_exactly_once() {
    watchdog("million_tiny_jobs", || {
        const N: usize = 1_000_000;
        let flags = counters(N);
        let pool = Pool::new(8);
        for i in 0..N {
            let flags = Arc::clone(&flags);
            pool.spawn(move || {
                if let Some(f) = flags.get(i) {
                    f.fetch_add(1, SeqCst);
                }
            });
        }
        pool.wait_idle();
        assert!(flags.iter().all(|f| f.load(SeqCst) == 1));
        let st = pool.stats();
        assert_eq!(st.spawned, N as u64);
        assert_eq!(st.completed, st.spawned);
    });
}

#[test]
fn recursive_tree_of_100k_jobs_all_run_before_wait_idle_returns() {
    watchdog("recursive_tree", || {
        const N: usize = 100_000;
        let hits = counters(N);
        let pool = Pool::new(8);
        let root = Arc::clone(&hits);
        pool.spawn(move || visit(0, N, root));
        pool.wait_idle();
        assert!(hits.iter().all(|h| h.load(SeqCst) == 1));
        assert_eq!(pool.stats().completed, N as u64);
    });
}

#[test]
fn stealing_happens_when_one_worker_gets_all_the_jobs() {
    watchdog("stealing", || {
        let pool = Pool::new(4);
        let done = Arc::new(AtomicUsize::new(0));
        let d = Arc::clone(&done);
        pool.spawn(move || {
            if let Some(handle) = current() {
                for _ in 0..10_000 {
                    let d = Arc::clone(&d);
                    handle.spawn(move || {
                        thread::sleep(Duration::from_micros(50));
                        d.fetch_add(1, SeqCst);
                    });
                }
            }
        });
        pool.wait_idle();
        assert_eq!(done.load(SeqCst), 10_000);
        assert!(pool.stats().stolen > 0, "no job was stolen");
    });
}

#[test]
fn panicking_job_does_not_stop_other_jobs_or_shutdown() {
    watchdog("panic", || {
        let pool = Pool::new(2);
        let ok = Arc::new(AtomicUsize::new(0));
        for i in 0..100 {
            let ok = Arc::clone(&ok);
            pool.spawn(move || {
                if i % 10 == 0 {
                    panic!("deliberate test panic {i}");
                }
                ok.fetch_add(1, SeqCst);
            });
        }
        pool.wait_idle();
        assert_eq!(ok.load(SeqCst), 90);
        assert_eq!(pool.stats().completed, 100);
        pool.shutdown();
    });
}

#[test]
fn shutdown_while_busy_runs_every_queued_job() {
    for run in 0..50 {
        watchdog("shutdown_while_busy", move || {
            let count = Arc::new(AtomicUsize::new(0));
            let pool = Pool::new(4);
            for _ in 0..100 {
                let c = Arc::clone(&count);
                pool.spawn(move || {
                    let child = Arc::clone(&c);
                    if let Some(handle) = current() {
                        handle.spawn(move || {
                            child.fetch_add(1, SeqCst);
                        });
                    }
                    c.fetch_add(1, SeqCst);
                });
            }
            pool.shutdown();
            assert_eq!(count.load(SeqCst), 200, "run {run} lost jobs");
        });
    }
}
