//! Leader-run check: an independent model over the public API only.
//! `STRATA_SEED` and `STRATA_CASES` change the run without editing the file.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use geyser::{Class, Config, Scheduler, State, TaskId};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn allowed(aff: u64, cpu: usize) -> bool {
    aff == 0 || (aff >> cpu) & 1 == 1
}

fn random_class(rng: &mut Rng) -> Class {
    match rng.below(10) {
        0 => Class::Realtime {
            prio: rng.below(4) as u8,
        },
        1 => Class::Idle,
        _ => Class::Normal {
            nice: rng.range(0, 60) as i8 - 30,
        },
    }
}

struct Model {
    s: Scheduler,
    ids: Vec<(TaskId, u64)>,
    cur: Vec<Option<(TaskId, u64)>>,
    busy: u64,
}

impl Model {
    fn close(&mut self, task: TaskId, now: u64) {
        for slot in self.cur.iter_mut() {
            if let Some((t, at)) = *slot {
                if t == task {
                    self.busy += now.saturating_sub(at);
                    *slot = None;
                }
            }
        }
    }

    fn state(&self, id: TaskId) -> Option<State> {
        self.s.stats(id).ok().map(|st| st.state)
    }

    fn check(&self, ctx: &str) {
        let mut runnable = 0;
        let mut running = Vec::new();
        for &(id, _) in &self.ids {
            if let Ok(st) = self.s.stats(id) {
                match st.state {
                    State::Ready => runnable += 1,
                    State::Running => {
                        runnable += 1;
                        let cpu = st
                            .cpu
                            .unwrap_or_else(|| panic!("{ctx}: running {id} has no cpu"));
                        running.push((id, cpu));
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(runnable, self.s.runnable(), "{ctx}: runnable() mismatch");
        let mut cpus: Vec<usize> = running.iter().map(|r| r.1).collect();
        cpus.sort_unstable();
        cpus.dedup();
        assert_eq!(cpus.len(), running.len(), "{ctx}: two tasks share a cpu");
        for (id, cpu) in running {
            assert_eq!(
                self.cur.get(cpu).copied().flatten().map(|c| c.0),
                Some(id),
                "{ctx}: task {id} Running on cpu {cpu} but that cpu was not given it"
            );
        }
    }
}

/// Random call sequences: structure, affinity, dead tasks, work conservation, runtime sum.
fn random_case(rng: &mut Rng, monotonic: bool) -> Vec<String> {
    let cpus = 1 + rng.below(6);
    let mut m = Model {
        s: Scheduler::new(cpus, Config::default()),
        ids: Vec::new(),
        cur: vec![None; cpus],
        busy: 0,
    };
    let mut log = Vec::new();
    let mut now = 0u64;
    for step in 0..600 {
        if monotonic || rng.chance(9, 10) {
            now += rng.range(0, 3000);
        } else {
            now = now.saturating_sub(rng.range(0, 5000));
        }
        let pick_id = |rng: &mut Rng, ids: &Vec<(TaskId, u64)>| -> TaskId {
            if ids.is_empty() || rng.chance(1, 12) {
                rng.u64() as u32
            } else {
                rng.pick(ids).0
            }
        };
        let ctx = format!("step {step}");
        match rng.below(9) {
            0 | 1 => {
                let aff = if rng.chance(1, 2) {
                    0
                } else {
                    rng.u64() & ((1u64 << cpus) - 1)
                };
                if let Ok(id) = m.s.spawn(random_class(rng), aff, now) {
                    assert!(m.ids.iter().all(|x| x.0 != id), "{ctx}: id {id} reused");
                    m.ids.push((id, aff));
                }
            }
            2..=4 => {
                let cpu = if rng.chance(1, 15) {
                    cpus + rng.below(3)
                } else {
                    rng.below(cpus)
                };
                let r = m.s.pick(cpu, now);
                if cpu >= cpus {
                    assert!(r.is_err(), "{ctx}: bad cpu {cpu} accepted");
                    continue;
                }
                let d = r.unwrap();
                if let Some((_, at)) = m.cur[cpu] {
                    m.busy += now.saturating_sub(at);
                }
                m.cur[cpu] = d.task.map(|t| (t, now));
                log.push(format!("{cpu}:{:?}:{}", d.task, d.slice_us));
                match d.task {
                    Some(t) => {
                        let aff = m.ids.iter().find(|x| x.0 == t).map(|x| x.1);
                        let aff = aff.unwrap_or_else(|| panic!("{ctx}: unknown task {t} picked"));
                        assert!(allowed(aff, cpu), "{ctx}: affinity broken for {t} on {cpu}");
                        assert_eq!(
                            m.state(t),
                            Some(State::Running),
                            "{ctx}: picked task not Running"
                        );
                        assert!(d.slice_us > 0, "{ctx}: zero slice");
                    }
                    None => {
                        for &(id, aff) in &m.ids {
                            if allowed(aff, cpu) {
                                assert_ne!(
                                    m.state(id),
                                    Some(State::Ready),
                                    "{ctx}: cpu {cpu} idles while {id} is ready for it"
                                );
                            }
                        }
                    }
                }
            }
            5 => {
                let id = pick_id(rng, &m.ids);
                if m.s.block(id, now).is_ok() {
                    m.close(id, now);
                }
            }
            6 => {
                let id = pick_id(rng, &m.ids);
                let before = m.state(id);
                let r = m.s.wake(id, now);
                if before != Some(State::Blocked) {
                    assert!(r.is_err(), "{ctx}: wake of {before:?} accepted");
                }
            }
            7 => {
                let id = pick_id(rng, &m.ids);
                if m.s.exit(id, now).is_ok() {
                    m.close(id, now);
                    assert!(matches!(m.state(id), None | Some(State::Dead)));
                }
            }
            _ => {
                let id = pick_id(rng, &m.ids);
                let _ = m.s.set_class(id, random_class(rng));
            }
        }
        m.check(&ctx);
    }
    if monotonic {
        let total: u64 = m
            .ids
            .iter()
            .filter_map(|&(id, _)| m.s.stats(id).ok())
            .map(|st| st.runtime_us)
            .sum();
        // Still-running tasks are charged only at their next pick; charge them now through pick.
        let mut open = 0;
        for c in m.cur.iter().flatten() {
            open += now.saturating_sub(c.1);
        }
        assert_eq!(
            total, m.busy,
            "runtime sum != closed busy time (open {open})"
        );
    }
    log
}

#[test]
fn leader_random_sequences() {
    let seed = env("STRATA_SEED", 0x6e7_2026_1114);
    let cases = env("STRATA_CASES", 400) as u32;
    forall(cases, seed, |rng| {
        random_case(rng, true);
    });
    forall(cases, seed ^ 0xbad, |rng| {
        random_case(rng, false);
    });
}

#[test]
fn leader_determinism() {
    let seed = env("STRATA_SEED", 0x6e7_2026_1114) ^ 0xd37;
    forall(50, seed, |rng| {
        let start = rng.u64();
        let a = random_case(&mut Rng::new(start), false);
        let b = random_case(&mut Rng::new(start), false);
        assert_eq!(a, b);
    });
}

/// One CPU, CPU-bound tasks, the CPU always takes the full slice.
fn run_one_cpu(classes: &[Class], until_us: u64) -> (Scheduler, Vec<TaskId>, u64) {
    let mut s = Scheduler::new(1, Config::default());
    let ids: Vec<TaskId> = classes.iter().map(|c| s.spawn(*c, 0, 0).unwrap()).collect();
    let mut last_run = vec![0u64; ids.len()];
    let mut now = 0;
    let mut max_slice = 0;
    while now < until_us {
        let d = s.pick(0, now).unwrap();
        let t = d.task.expect("cpu idles with ready tasks");
        let i = ids.iter().position(|x| *x == t).unwrap();
        let normals = classes
            .iter()
            .filter(|c| matches!(c, Class::Normal { .. }))
            .count() as u64;
        if matches!(classes[i], Class::Normal { .. }) {
            let gap = now - last_run[i];
            let bound = Config::default().latency_us * (normals + 1);
            assert!(gap <= bound, "task {t} waited {gap} us > {bound}");
        }
        for (j, lr) in last_run.iter_mut().enumerate() {
            if j == i {
                *lr = now + d.slice_us;
            }
        }
        max_slice = max_slice.max(d.slice_us);
        now += d.slice_us;
    }
    s.pick(0, now).unwrap();
    (s, ids, max_slice)
}

#[test]
fn leader_fairness() {
    let mut rng = Rng::new(env("STRATA_SEED", 0x6e7_2026_1114) ^ 0xfa1);
    for _ in 0..20 {
        let n = 2 + rng.below(12);
        let nice = rng.range(0, 40) as i8 - 20;
        let classes = vec![Class::Normal { nice }; n];
        let (s, ids, slice) = run_one_cpu(&classes, 5_000_000);
        let rt: Vec<u64> = ids
            .iter()
            .map(|i| s.stats(*i).unwrap().runtime_us)
            .collect();
        let (lo, hi) = (rt.iter().min().unwrap(), rt.iter().max().unwrap());
        assert!(
            hi - lo <= slice,
            "n {n} nice {nice}: spread {} > slice {slice}",
            hi - lo
        );
    }
    let (s, ids, _) = run_one_cpu(
        &[Class::Normal { nice: 0 }, Class::Normal { nice: 5 }],
        20_000_000,
    );
    let a = s.stats(ids[0]).unwrap().runtime_us as f64;
    let b = s.stats(ids[1]).unwrap().runtime_us as f64;
    let want = 1024.0 / 335.0;
    assert!(
        ((a / b) / want - 1.0).abs() < 0.05,
        "nice 0/5 ratio {}",
        a / b
    );
}

#[test]
fn leader_classes() {
    let (s, ids, _) = run_one_cpu(
        &[
            Class::Normal { nice: 0 },
            Class::Idle,
            Class::Realtime { prio: 1 },
            Class::Realtime { prio: 1 },
            Class::Realtime { prio: 0 },
        ],
        1_000_000,
    );
    let rt: Vec<u64> = ids
        .iter()
        .map(|i| s.stats(*i).unwrap().runtime_us)
        .collect();
    assert_eq!(rt[0], 0, "normal ran beside ready realtime");
    assert_eq!(rt[1], 0, "idle ran beside ready realtime");
    assert_eq!(rt[4], 0, "lower realtime prio ran");
    assert!(
        rt[2] > 0 && rt[3] > 0,
        "realtime round robin broken: {rt:?}"
    );
    let (s, ids, _) = run_one_cpu(&[Class::Idle, Class::Normal { nice: 19 }], 1_000_000);
    assert_eq!(
        s.stats(ids[0]).unwrap().runtime_us,
        0,
        "idle ran beside normal"
    );
}
