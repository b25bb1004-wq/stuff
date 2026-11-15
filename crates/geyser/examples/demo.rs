//! Deterministic tour of geyser: the core scheduler stepped by hand, then the thread pool.
//! No clock is read and no network is used; the output is the same on every run.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use geyser::pool::Pool;
use geyser::{Class, Config, Error, Scheduler};

fn main() -> Result<(), Error> {
    let config = Config::default();
    println!(
        "core scheduler: 2 CPUs, latency {} us, min slice {} us",
        config.latency_us, config.min_slice_us
    );
    let mut sched = Scheduler::new(2, config);

    // Times are passed in by the caller, so the run is repeatable.
    let rt = sched.spawn(Class::Realtime { prio: 10 }, 0, 0)?;
    let fast = sched.spawn(Class::Normal { nice: -5 }, 0, 0)?;
    let slow = sched.spawn(Class::Normal { nice: 10 }, 0, 0)?;
    let idle = sched.spawn(Class::Idle, 0, 0)?;
    println!(
        "spawned rt={rt} fast={fast} slow={slow} idle={idle}, runnable={}",
        sched.runnable()
    );

    for step in 0..3u64 {
        let now = step * 1000;
        for cpu in 0..2 {
            let decision = sched.pick(cpu, now)?;
            println!(
                "t={now:>4} us  cpu{cpu} runs {:?} for {} us",
                decision.task, decision.slice_us
            );
        }
    }

    sched.block(slow, 3500)?;
    println!("slow blocked: state {:?}", sched.stats(slow)?.state);
    sched.wake(slow, 4000)?;
    println!("slow woken: state {:?}", sched.stats(slow)?.state);
    sched.exit(rt, 4500)?;
    println!("rt exited: state {:?}", sched.stats(rt)?.state);
    let fast_stats = sched.stats(fast)?;
    println!(
        "fast: runtime {} us, switches {}, runnable now {}",
        fast_stats.runtime_us,
        fast_stats.switches,
        sched.runnable()
    );

    println!("thread pool: 4 workers");
    let pool = Pool::new(4);
    let total = Arc::new(AtomicU64::new(0));
    for i in 1..=100u64 {
        let total = Arc::clone(&total);
        pool.spawn(move || {
            total.fetch_add(i * i, Ordering::SeqCst);
            if i == 100 {
                // A job that spawns a job; wait_idle also waits for the child.
                if let Some(handle) = geyser::pool::current() {
                    let child = Arc::clone(&total);
                    handle.spawn(move || {
                        child.fetch_add(1000, Ordering::SeqCst);
                    });
                }
            }
        });
    }
    pool.wait_idle();
    let stats = pool.stats();
    println!(
        "sum of squares 1..=100 plus child bonus = {}",
        total.load(Ordering::SeqCst)
    );
    println!(
        "jobs spawned={} completed={}",
        stats.spawned, stats.completed
    );
    pool.shutdown();
    println!("done");
    Ok(())
}
