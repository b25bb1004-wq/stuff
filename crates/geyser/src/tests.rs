#![allow(clippy::unwrap_used)]

use assay::forall;

use crate::{Class, Config, Decision, Error, Scheduler, State, TaskId};

/// Simple discrete-event simulator for driving `Scheduler` across multiple CPUs.
struct SimTask {
    id: TaskId,
    burst_us: u64,
    sleep_us: u64,
    ran_in_burst: u64,
    wake_at: Option<u64>,
}

struct SimCpu {
    current: Option<TaskId>,
    started_at: u64,
    next_event_at: u64,
}

struct Simulator {
    sched: Scheduler,
    now_us: u64,
    cpus: Vec<SimCpu>,
    tasks: Vec<SimTask>,
}

impl Simulator {
    fn new(num_cpus: usize, config: Config) -> Self {
        let sched = Scheduler::new(num_cpus, config);
        let actual_cpus = num_cpus.clamp(1, 64);
        let cpus = (0..actual_cpus)
            .map(|_| SimCpu {
                current: None,
                started_at: 0,
                next_event_at: u64::MAX,
            })
            .collect();
        Self {
            sched,
            now_us: 0,
            cpus,
            tasks: Vec::new(),
        }
    }

    fn add_task(&mut self, class: Class, affinity: u64, burst_us: u64, sleep_us: u64) -> TaskId {
        let id = self.sched.spawn(class, affinity, self.now_us).unwrap();
        self.tasks.push(SimTask {
            id,
            burst_us,
            sleep_us,
            ran_in_burst: 0,
            wake_at: None,
        });
        id
    }

    fn pick_idle_cpus(&mut self) {
        for c in 0..self.cpus.len() {
            if self.cpus[c].current.is_none() {
                self.dispatch(c);
            }
        }
    }

    fn dispatch(&mut self, cpu: usize) {
        let dec = self.sched.pick(cpu, self.now_us).unwrap();
        self.cpus[cpu].current = dec.task;
        self.cpus[cpu].started_at = self.now_us;
        if let Some(tid) = dec.task {
            let t = &self.tasks[tid as usize];
            if t.burst_us == u64::MAX {
                self.cpus[cpu].next_event_at = self.now_us.saturating_add(dec.slice_us);
            } else {
                let rem_burst = t.burst_us.saturating_sub(t.ran_in_burst);
                let step = rem_burst.min(dec.slice_us);
                self.cpus[cpu].next_event_at = self.now_us.saturating_add(step);
            }
        } else {
            self.cpus[cpu].next_event_at = u64::MAX;
        }
    }

    fn step_until(&mut self, end_us: u64) {
        self.pick_idle_cpus();
        while self.now_us < end_us {
            let mut next_t = end_us;
            for c in &self.cpus {
                if c.current.is_some() {
                    next_t = next_t.min(c.next_event_at);
                }
            }
            for t in &self.tasks {
                if let Some(w) = t.wake_at {
                    next_t = next_t.min(w);
                }
            }
            self.now_us = next_t;

            // Wake any sleeping tasks whose wake_at <= now_us.
            for t in &mut self.tasks {
                if let Some(w) = t.wake_at {
                    if w <= self.now_us {
                        t.wake_at = None;
                        t.ran_in_burst = 0;
                        self.sched.wake(t.id, self.now_us).unwrap();
                    }
                }
            }

            // Handle CPU events (burst completion or slice expiry).
            for c in 0..self.cpus.len() {
                if let Some(tid) = self.cpus[c].current {
                    if self.cpus[c].next_event_at <= self.now_us {
                        let elapsed = self.now_us.saturating_sub(self.cpus[c].started_at);
                        let t = &mut self.tasks[tid as usize];
                        if t.burst_us != u64::MAX {
                            t.ran_in_burst = t.ran_in_burst.saturating_add(elapsed);
                            if t.ran_in_burst >= t.burst_us {
                                self.sched.block(tid, self.now_us).unwrap();
                                t.wake_at = Some(self.now_us.saturating_add(t.sleep_us));
                                self.cpus[c].current = None;
                                self.dispatch(c);
                                continue;
                            }
                        }
                        self.dispatch(c);
                    }
                }
            }
            self.pick_idle_cpus();
        }
    }
}

#[test]
fn unit_basic_errors_and_clamping() {
    let mut s = Scheduler::new(0, Config::default());
    assert!(s.pick(0, 0).is_ok());
    assert_eq!(s.pick(1, 0), Err(Error::BadCpu));
    assert_eq!(s.block(99, 0), Err(Error::NoSuchTask));
    assert_eq!(s.wake(99, 0), Err(Error::NoSuchTask));
    assert_eq!(s.exit(99, 0), Err(Error::NoSuchTask));
    assert_eq!(s.stats(99), Err(Error::NoSuchTask));
    assert_eq!(s.set_class(99, Class::Idle), Err(Error::NoSuchTask));

    let id = s.spawn(Class::Normal { nice: -100 }, 0, 0).unwrap();
    assert_eq!(s.stats(id).unwrap().class, Class::Normal { nice: -20 });
    s.set_class(id, Class::Normal { nice: 100 }).unwrap();
    assert_eq!(s.stats(id).unwrap().class, Class::Normal { nice: 19 });

    // Wake on Ready is BadState.
    assert_eq!(s.wake(id, 10), Err(Error::BadState));
    s.block(id, 10).unwrap();
    assert_eq!(s.block(id, 10), Err(Error::BadState));
    s.wake(id, 20).unwrap();
    s.exit(id, 30).unwrap();
    assert_eq!(s.stats(id).unwrap().state, State::Dead);
    assert_eq!(s.exit(id, 40), Err(Error::BadState));
    assert_eq!(s.set_class(id, Class::Idle), Err(Error::BadState));
}

#[test]
fn unit_realtime_round_robin_and_priority() {
    let mut s = Scheduler::new(1, Config::default());
    let low = s.spawn(Class::Realtime { prio: 1 }, 0, 0).unwrap();
    let hi1 = s.spawn(Class::Realtime { prio: 10 }, 0, 0).unwrap();
    let hi2 = s.spawn(Class::Realtime { prio: 10 }, 0, 0).unwrap();

    assert_eq!(s.pick(0, 0).unwrap().task, Some(hi1));
    assert_eq!(s.pick(0, 750).unwrap().task, Some(hi2));
    assert_eq!(s.pick(0, 1500).unwrap().task, Some(hi1));
    assert_eq!(s.pick(0, 2250).unwrap().task, Some(hi2));

    s.exit(hi1, 2500).unwrap();
    s.exit(hi2, 2500).unwrap();
    assert_eq!(s.pick(0, 2500).unwrap().task, Some(low));
}

#[test]
fn prop_fairness_equal_nice_and_weighted_nice() {
    forall(30, 0x1234_5678, |rng| {
        let n = rng.range(2, 9) as usize;
        let mut sim = Simulator::new(1, Config::default());
        let mut ids = Vec::new();
        for _ in 0..n {
            ids.push(sim.add_task(Class::Normal { nice: 0 }, 0, u64::MAX, 0));
        }
        sim.step_until(200_000);
        // Final pick to flush running task's runtime.
        sim.sched.pick(0, sim.now_us).unwrap();

        let runtimes: Vec<u64> = ids
            .iter()
            .map(|&id| sim.sched.stats(id).unwrap().runtime_us)
            .collect();
        let min_r = *runtimes.iter().min().unwrap();
        let max_r = *runtimes.iter().max().unwrap();
        let slice = Config::default()
            .min_slice_us
            .max(Config::default().latency_us / n as u64);
        assert!(
            max_r - min_r <= slice,
            "unfairness: max={max_r}, min={min_r}, slice={slice}"
        );
    });

    // Nice 0 vs nice 5: weight ratio is 1024 / 335 ~= 3.0567.
    let mut sim = Simulator::new(1, Config::default());
    let t0 = sim.add_task(Class::Normal { nice: 0 }, 0, u64::MAX, 0);
    let t5 = sim.add_task(Class::Normal { nice: 5 }, 0, u64::MAX, 0);
    sim.step_until(500_000);
    sim.sched.pick(0, sim.now_us).unwrap();

    let r0 = sim.sched.stats(t0).unwrap().runtime_us as f64;
    let r5 = sim.sched.stats(t5).unwrap().runtime_us as f64;
    let ratio = r0 / r5;
    let expected = 1024.0 / 335.0;
    let rel_err = (ratio - expected).abs() / expected;
    assert!(
        rel_err < 0.05,
        "ratio={ratio:.4}, expected={expected:.4}, rel_err={rel_err:.4}"
    );
}

#[test]
fn prop_classes_priority_order() {
    forall(40, 0xCAFE_BABE, |rng| {
        let cpus = rng.range(1, 5) as usize;
        let mut s = Scheduler::new(cpus, Config::default());
        let mut rt_ids = Vec::new();
        let mut norm_ids = Vec::new();
        let mut idle_ids = Vec::new();

        for _ in 0..rng.range(1, 6) {
            let prio = rng.range(0, 10) as u8;
            rt_ids.push(s.spawn(Class::Realtime { prio }, 0, 0).unwrap());
        }
        for _ in 0..rng.range(1, 6) {
            let nice = rng.range(0, 40) as i8 - 20;
            norm_ids.push(s.spawn(Class::Normal { nice }, 0, 0).unwrap());
        }
        for _ in 0..rng.range(1, 4) {
            idle_ids.push(s.spawn(Class::Idle, 0, 0).unwrap());
        }

        let mut now = 0u64;
        for _ in 0..100 {
            let cpu = rng.below(cpus);
            now += rng.range(100, 1000);
            let dec = s.pick(cpu, now).unwrap();
            if let Some(tid) = dec.task {
                let c = s.stats(tid).unwrap().class;
                let any_ready_rt = rt_ids
                    .iter()
                    .any(|&id| s.stats(id).unwrap().state == State::Ready);
                let any_ready_norm = norm_ids
                    .iter()
                    .any(|&id| s.stats(id).unwrap().state == State::Ready);
                if matches!(c, Class::Normal { .. }) {
                    assert!(!any_ready_rt, "normal picked while RT was ready");
                }
                if matches!(c, Class::Idle) {
                    assert!(!any_ready_rt, "idle picked while RT was ready");
                    assert!(!any_ready_norm, "idle picked while Normal was ready");
                }
            }
        }
    });
}

#[test]
fn prop_no_starvation_normal_tasks() {
    forall(40, 0xDEAD_BEEF, |rng| {
        let n = rng.range(2, 12) as usize;
        let cfg = Config::default();
        let mut s = Scheduler::new(1, cfg);
        let mut ids = Vec::new();
        for _ in 0..n {
            ids.push(s.spawn(Class::Normal { nice: 0 }, 0, 0).unwrap());
        }
        let mut last_ran = vec![0u64; n];
        let mut now = 0u64;
        let bound = cfg.latency_us * (n as u64 + 1);
        for _ in 0..(n * 20) {
            let dec = s.pick(0, now).unwrap();
            let tid = dec.task.unwrap();
            let wait = now - last_ran[tid as usize];
            assert!(
                wait <= bound,
                "task {tid} starved for {wait}us > bound {bound}us"
            );
            last_ran[tid as usize] = now;
            now += dec.slice_us;
        }
    });
}

#[test]
fn prop_affinity_and_work_conservation() {
    forall(60, 0xA11_CE01, |rng| {
        let cpus = rng.range(2, 9) as usize;
        let mask_all = (1u64 << cpus) - 1;
        let mut s = Scheduler::new(cpus, Config::default());
        let mut affinities = Vec::new();
        let num_tasks = rng.range(1, 20) as usize;
        for _ in 0..num_tasks {
            let mut aff = rng.u64() & mask_all;
            if aff == 0 {
                aff = mask_all;
            }
            let nice = rng.range(0, 40) as i8 - 20;
            s.spawn(Class::Normal { nice }, aff, 0).unwrap();
            affinities.push(aff);
        }

        let mut now = 0u64;
        for _ in 0..200 {
            let cpu = rng.below(cpus);
            now += rng.range(50, 1500);
            let dec = s.pick(cpu, now).unwrap();
            if let Some(tid) = dec.task {
                assert_ne!(
                    affinities[tid as usize] & (1u64 << cpu),
                    0,
                    "affinity violated: task {tid} on cpu {cpu}"
                );
            } else {
                // Work conservation: no Ready task may be allowed on `cpu`.
                for (tid, &aff) in affinities.iter().enumerate() {
                    let st = s.stats(tid as TaskId).unwrap().state;
                    if st == State::Ready {
                        assert_eq!(
                            aff & (1u64 << cpu),
                            0,
                            "cpu {cpu} idled while ready task {tid} could run on it"
                        );
                    }
                }
            }
        }
    });
}

#[test]
fn prop_random_call_sequences_invariants() {
    forall(300, 0x5EED_0505, |rng| {
        let cpus = rng.range(1, 9) as usize;
        let mut s = Scheduler::new(cpus, Config::default());
        let mut now = 1000u64;

        let mut running_on: Vec<Option<(TaskId, u64)>> = vec![None; cpus];
        let mut dead: Vec<bool> = Vec::new();
        let mut total_busy = 0u64;

        for _ in 0..500 {
            // Occasionally move time backwards!
            if rng.chance(1, 20) {
                now = now.saturating_sub(rng.range(1, 500));
            } else {
                now = now.saturating_add(rng.range(0, 1000));
            }

            let op = rng.range(0, 7);
            match op {
                0 => {
                    // spawn
                    let class = match rng.range(0, 3) {
                        0 => Class::Realtime {
                            prio: rng.range(0, 8) as u8,
                        },
                        1 => Class::Normal {
                            nice: rng.range(0, 50) as i8 - 25,
                        },
                        _ => Class::Idle,
                    };
                    let aff = if rng.chance(1, 2) { 0 } else { rng.u64() };
                    if let Ok(id) = s.spawn(class, aff, now) {
                        assert_eq!(id as usize, dead.len());
                        dead.push(false);
                    }
                }
                1 | 2 => {
                    // pick (including bad cpu)
                    let cpu = if rng.chance(1, 15) {
                        cpus + rng.below(4)
                    } else {
                        rng.below(cpus)
                    };
                    let res = s.pick(cpu, now);
                    if cpu >= cpus {
                        assert_eq!(res, Err(Error::BadCpu));
                    } else {
                        let dec = res.unwrap();
                        if let Some((_, start)) = running_on[cpu].take() {
                            total_busy += now.saturating_sub(start);
                        }
                        if let Some(tid) = dec.task {
                            assert!(!dead[tid as usize], "dead task {tid} picked");
                            // Must not be running on any other CPU.
                            for (other_c, slot) in running_on.iter().enumerate() {
                                if let Some((other_t, _)) = slot {
                                    assert_ne!(
                                        *other_t, tid,
                                        "task {tid} running on {other_c} and {cpu}"
                                    );
                                }
                            }
                            running_on[cpu] = Some((tid, now));
                        }
                    }
                }
                3 => {
                    // block (including bad id)
                    let tid = if dead.is_empty() || rng.chance(1, 10) {
                        dead.len() as u32 + 5
                    } else {
                        rng.below(dead.len()) as u32
                    };
                    if s.block(tid, now).is_ok() {
                        for slot in &mut running_on {
                            if let Some((rt, start)) = *slot {
                                if rt == tid {
                                    total_busy += now.saturating_sub(start);
                                    *slot = None;
                                }
                            }
                        }
                    }
                }
                4 => {
                    // wake
                    let tid = if dead.is_empty() || rng.chance(1, 10) {
                        dead.len() as u32 + 5
                    } else {
                        rng.below(dead.len()) as u32
                    };
                    let _ = s.wake(tid, now);
                }
                5 => {
                    // exit
                    let tid = if dead.is_empty() || rng.chance(1, 10) {
                        dead.len() as u32 + 5
                    } else {
                        rng.below(dead.len()) as u32
                    };
                    if s.exit(tid, now).is_ok() {
                        dead[tid as usize] = true;
                        for slot in &mut running_on {
                            if let Some((rt, start)) = *slot {
                                if rt == tid {
                                    total_busy += now.saturating_sub(start);
                                    *slot = None;
                                }
                            }
                        }
                    }
                }
                _ => {
                    // set_class
                    let tid = if dead.is_empty() || rng.chance(1, 10) {
                        dead.len() as u32 + 5
                    } else {
                        rng.below(dead.len()) as u32
                    };
                    let _ = s.set_class(
                        tid,
                        Class::Normal {
                            nice: rng.range(0, 40) as i8 - 20,
                        },
                    );
                }
            }

            // Check invariants after each call:
            let mut counted_runnable = 0usize;
            let mut sum_runtime = 0u64;
            for (idx, &is_dead) in dead.iter().enumerate() {
                let st = s.stats(idx as TaskId).unwrap();
                if is_dead {
                    assert_eq!(st.state, State::Dead);
                    assert_eq!(st.cpu, None);
                }
                if matches!(st.state, State::Ready | State::Running) {
                    counted_runnable += 1;
                }
                if st.state == State::Running {
                    let c = st.cpu.unwrap();
                    assert_eq!(running_on[c].map(|x| x.0), Some(idx as TaskId));
                } else {
                    assert_eq!(st.cpu, None);
                }
                sum_runtime += st.runtime_us;
            }
            assert_eq!(s.runnable(), counted_runnable);
            assert_eq!(sum_runtime, total_busy);
        }
    });
}

#[test]
fn prop_determinism_replay() {
    #[derive(Clone, Copy)]
    enum Op {
        Spawn(Class, u64, u64),
        Pick(usize, u64),
        Block(TaskId, u64),
        Wake(TaskId, u64),
        Exit(TaskId, u64),
    }

    forall(30, 0x9988_7766, |rng| {
        let cpus = rng.range(1, 5) as usize;
        let mut ops = Vec::with_capacity(200);
        let mut now = 0u64;
        for _ in 0..200 {
            now += rng.range(0, 500);
            let op = match rng.range(0, 5) {
                0 => Op::Spawn(
                    Class::Normal {
                        nice: rng.range(0, 40) as i8 - 20,
                    },
                    rng.u64() & 0xF,
                    now,
                ),
                1 | 2 => Op::Pick(rng.below(cpus), now),
                3 => Op::Block(rng.range(0, 20) as u32, now),
                4 => Op::Wake(rng.range(0, 20) as u32, now),
                _ => Op::Exit(rng.range(0, 20) as u32, now),
            };
            ops.push(op);
        }

        let run = |ops: &[Op]| -> Vec<Result<Decision, Error>> {
            let mut s = Scheduler::new(cpus, Config::default());
            let mut out = Vec::new();
            for &op in ops {
                match op {
                    Op::Spawn(c, a, t) => {
                        let _ = s.spawn(c, a, t);
                    }
                    Op::Pick(c, t) => out.push(s.pick(c, t)),
                    Op::Block(id, t) => {
                        let _ = s.block(id, t);
                    }
                    Op::Wake(id, t) => {
                        let _ = s.wake(id, t);
                    }
                    Op::Exit(id, t) => {
                        let _ = s.exit(id, t);
                    }
                }
            }
            out
        };

        assert_eq!(run(&ops), run(&ops));
    });
}

#[test]
fn unit_sim_sleepers_and_max_tasks() {
    let mut sim = Simulator::new(2, Config::default());
    let hog = sim.add_task(Class::Normal { nice: 0 }, 0, u64::MAX, 0);
    let sleeper = sim.add_task(Class::Normal { nice: 0 }, 0, 500, 10_000);
    sim.step_until(100_000);
    sim.sched.pick(0, sim.now_us).unwrap();
    sim.sched.pick(1, sim.now_us).unwrap();

    let hog_rt = sim.sched.stats(hog).unwrap().runtime_us;
    let sleeper_rt = sim.sched.stats(sleeper).unwrap().runtime_us;
    assert!(hog_rt > 50_000, "hog should get CPU while sleeper sleeps");
    assert!(sleeper_rt >= 4_000, "sleeper should run its bursts");

    let mut s = Scheduler::new(1, Config::default());
    for i in 0..65_536u32 {
        assert_eq!(s.spawn(Class::Idle, 0, 0), Ok(i));
    }
    assert_eq!(s.spawn(Class::Idle, 0, 0), Err(Error::TooManyTasks));
    s.exit(0, 0).unwrap();
    // Dead ID 0 is never reused; next ID is 65_536.
    assert_eq!(s.spawn(Class::Idle, 0, 0), Ok(65_536));
}
