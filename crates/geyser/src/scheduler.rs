//! Core fair scheduler implementation.

use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::types::{
    clamp_class, nice_weight, Class, Config, Decision, Error, State, Stats, TaskId,
};

/// Maximum number of concurrent live tasks allowed.
const MAX_LIVE_TASKS: usize = 65_536;

/// Internal per-task scheduling state.
#[derive(Debug, Clone)]
struct Task {
    id: TaskId,
    class: Class,
    state: State,
    affinity_mask: u64,
    vruntime: u64,
    runtime_us: u64,
    switches: u64,
    cpu: Option<usize>,
    rt_seq: u64,
}

/// Per-CPU run queue and currently executing task.
#[derive(Debug, Clone, Default)]
struct CpuState {
    queue: Vec<TaskId>,
    current: Option<TaskId>,
    start_us: u64,
}

/// Pure, deterministic multi-CPU fair scheduler.
#[derive(Debug, Clone)]
pub struct Scheduler {
    config: Config,
    cpus: Vec<CpuState>,
    all_cpus_mask: u64,
    tasks: Vec<Task>,
    live_tasks: usize,
    next_seq: u64,
}

impl Scheduler {
    /// Create a new scheduler for `cpus` (clamped to `1..=64`) with `config`.
    pub fn new(cpus: usize, config: Config) -> Self {
        let count = cpus.clamp(1, 64);
        let all_cpus_mask = if count == 64 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
        let mut cpu_vec = Vec::with_capacity(count);
        cpu_vec.resize_with(count, CpuState::default);
        Self {
            config,
            cpus: cpu_vec,
            all_cpus_mask,
            tasks: Vec::new(),
            live_tasks: 0,
            next_seq: 0,
        }
    }

    /// Spawn a new task with `class` and CPU `affinity` bitmask (`0` = any CPU).
    pub fn spawn(&mut self, class: Class, affinity: u64, _now_us: u64) -> Result<TaskId, Error> {
        if self.live_tasks >= MAX_LIVE_TASKS || self.tasks.len() > u32::MAX as usize {
            return Err(Error::TooManyTasks);
        }
        let id = self.tasks.len() as TaskId;
        let class = clamp_class(class);
        let affinity_mask = if affinity == 0 {
            self.all_cpus_mask
        } else {
            affinity & self.all_cpus_mask
        };
        let target_cpu = self.find_target_cpu(affinity_mask);
        let vruntime = match class {
            Class::Normal { .. } => self.min_vruntime_on_cpu(target_cpu).unwrap_or(0),
            _ => 0,
        };
        let rt_seq = self.alloc_seq();
        self.tasks.push(Task {
            id,
            class,
            state: State::Ready,
            affinity_mask,
            vruntime,
            runtime_us: 0,
            switches: 0,
            cpu: None,
            rt_seq,
        });
        self.live_tasks = self.live_tasks.saturating_add(1);
        if let Some(c) = self.cpus.get_mut(target_cpu) {
            c.queue.push(id);
        }
        Ok(id)
    }

    /// Charge the currently running task on `cpu` and pick the next task to run.
    pub fn pick(&mut self, cpu: usize, now_us: u64) -> Result<Decision, Error> {
        if cpu >= self.cpus.len() {
            return Err(Error::BadCpu);
        }

        // Charge and requeue the currently running task on this CPU, if any.
        if let Some(prev_id) = self.charge_cpu(cpu, now_us) {
            let seq = self.alloc_seq();
            if let Some(t) = self.tasks.get_mut(prev_id as usize) {
                t.state = State::Ready;
                t.cpu = None;
                t.rt_seq = seq;
            }
            if let Some(c) = self.cpus.get_mut(cpu) {
                c.queue.push(prev_id);
            }
        }

        // Pick from local queue, or steal if local queue is empty or has a lower class
        // than a ready task allowed on `cpu` in another queue.
        let picked = self.pick_or_steal_for_cpu(cpu);

        let Some(tid) = picked else {
            return Ok(Decision {
                task: None,
                slice_us: 0,
            });
        };

        let class = if let Some(t) = self.tasks.get_mut(tid as usize) {
            t.state = State::Running;
            t.cpu = Some(cpu);
            t.switches = t.switches.saturating_add(1);
            t.class
        } else {
            return Ok(Decision {
                task: None,
                slice_us: 0,
            });
        };

        if let Some(c) = self.cpus.get_mut(cpu) {
            c.current = Some(tid);
            c.start_us = now_us;
        }

        let slice_us = match class {
            Class::Normal { .. } => {
                let normal_count = 1u64.saturating_add(self.ready_normal_count(cpu));
                let fair = self.config.latency_us / normal_count.max(1);
                self.config.min_slice_us.max(fair)
            }
            Class::Realtime { .. } | Class::Idle => self.config.min_slice_us,
        };

        Ok(Decision {
            task: Some(tid),
            slice_us,
        })
    }

    /// Transition a `Running` or `Ready` task to `Blocked`.
    pub fn block(&mut self, task: TaskId, now_us: u64) -> Result<(), Error> {
        let (state, cpu) = {
            let t = self.tasks.get(task as usize).ok_or(Error::NoSuchTask)?;
            (t.state, t.cpu)
        };
        match state {
            State::Running => {
                if let Some(c) = cpu {
                    self.charge_cpu(c, now_us);
                }
            }
            State::Ready => {
                self.remove_from_queues(task);
            }
            State::Blocked | State::Dead => return Err(Error::BadState),
        }
        if let Some(t) = self.tasks.get_mut(task as usize) {
            t.state = State::Blocked;
            t.cpu = None;
        }
        Ok(())
    }

    /// Transition a `Blocked` task to `Ready` and enqueue it on an allowed CPU.
    pub fn wake(&mut self, task: TaskId, _now_us: u64) -> Result<(), Error> {
        let (state, affinity_mask, class) = {
            let t = self.tasks.get(task as usize).ok_or(Error::NoSuchTask)?;
            (t.state, t.affinity_mask, t.class)
        };
        if state != State::Blocked {
            return Err(Error::BadState);
        }
        let target_cpu = self.find_target_cpu(affinity_mask);
        let floor = match class {
            Class::Normal { .. } => self
                .min_vruntime_on_cpu(target_cpu)
                .map(|q_min| q_min.saturating_sub(self.config.latency_us / 2)),
            _ => None,
        };
        let seq = self.alloc_seq();
        if let Some(t) = self.tasks.get_mut(task as usize) {
            t.state = State::Ready;
            t.cpu = None;
            t.rt_seq = seq;
            if let Some(f) = floor {
                t.vruntime = t.vruntime.max(f);
            }
        }
        if let Some(c) = self.cpus.get_mut(target_cpu) {
            c.queue.push(task);
        }
        Ok(())
    }

    /// Transition any live task (`Ready`, `Running`, or `Blocked`) to `Dead`.
    pub fn exit(&mut self, task: TaskId, now_us: u64) -> Result<(), Error> {
        let (state, cpu) = {
            let t = self.tasks.get(task as usize).ok_or(Error::NoSuchTask)?;
            (t.state, t.cpu)
        };
        match state {
            State::Dead => return Err(Error::BadState),
            State::Running => {
                if let Some(c) = cpu {
                    self.charge_cpu(c, now_us);
                }
            }
            State::Ready => {
                self.remove_from_queues(task);
            }
            State::Blocked => {}
        }
        if let Some(t) = self.tasks.get_mut(task as usize) {
            t.state = State::Dead;
            t.cpu = None;
        }
        self.live_tasks = self.live_tasks.saturating_sub(1);
        Ok(())
    }

    /// Change the scheduling class of a live task.
    pub fn set_class(&mut self, task: TaskId, class: Class) -> Result<(), Error> {
        let t = self.tasks.get_mut(task as usize).ok_or(Error::NoSuchTask)?;
        if t.state == State::Dead {
            return Err(Error::BadState);
        }
        t.class = clamp_class(class);
        Ok(())
    }

    /// Return a snapshot of `task`'s scheduling statistics.
    pub fn stats(&self, task: TaskId) -> Result<Stats, Error> {
        let t = self.tasks.get(task as usize).ok_or(Error::NoSuchTask)?;
        Ok(Stats {
            state: t.state,
            class: t.class,
            runtime_us: t.runtime_us,
            switches: t.switches,
            cpu: t.cpu,
        })
    }

    /// Count tasks currently in `Ready` or `Running` state.
    pub fn runnable(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, State::Ready | State::Running))
            .count()
    }

    fn alloc_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        s
    }

    fn charge_cpu(&mut self, cpu: usize, now_us: u64) -> Option<TaskId> {
        let c = self.cpus.get_mut(cpu)?;
        let tid = c.current.take()?;
        let elapsed = now_us.saturating_sub(c.start_us);
        c.start_us = now_us;
        if let Some(t) = self.tasks.get_mut(tid as usize) {
            t.runtime_us = t.runtime_us.saturating_add(elapsed);
            if let Class::Normal { nice } = t.class {
                let w = nice_weight(nice);
                let dv = ((elapsed as u128).saturating_mul(1024) / (w as u128))
                    .min(u64::MAX as u128) as u64;
                t.vruntime = t.vruntime.saturating_add(dv);
            }
        }
        Some(tid)
    }

    fn find_target_cpu(&self, affinity_mask: u64) -> usize {
        let mut best_cpu = 0usize;
        let mut best_load = usize::MAX;
        for (idx, c) in self.cpus.iter().enumerate() {
            if (affinity_mask & (1u64 << idx)) == 0 {
                continue;
            }
            let load = c.queue.len() + usize::from(c.current.is_some());
            if load < best_load {
                best_load = load;
                best_cpu = idx;
            }
        }
        best_cpu
    }

    fn min_vruntime_on_cpu(&self, cpu: usize) -> Option<u64> {
        let c = self.cpus.get(cpu)?;
        let mut min_vr: Option<u64> = None;
        for &tid in &c.queue {
            if let Some(t) = self.tasks.get(tid as usize) {
                if matches!(t.class, Class::Normal { .. }) {
                    min_vr = Some(min_vr.map_or(t.vruntime, |m| m.min(t.vruntime)));
                }
            }
        }
        if let Some(tid) = c.current {
            if let Some(t) = self.tasks.get(tid as usize) {
                if matches!(t.class, Class::Normal { .. }) {
                    min_vr = Some(min_vr.map_or(t.vruntime, |m| m.min(t.vruntime)));
                }
            }
        }
        min_vr
    }

    fn ready_normal_count(&self, cpu: usize) -> u64 {
        let Some(c) = self.cpus.get(cpu) else {
            return 0;
        };
        let cpu_bit = 1u64 << cpu;
        c.queue
            .iter()
            .filter(|&&tid| {
                self.tasks.get(tid as usize).is_some_and(|t| {
                    matches!(t.class, Class::Normal { .. }) && (t.affinity_mask & cpu_bit) != 0
                })
            })
            .count() as u64
    }

    fn pick_or_steal_for_cpu(&mut self, cpu: usize) -> Option<TaskId> {
        let local_idx = self.best_index_in_queue(cpu, cpu);
        let local_rank = local_idx
            .and_then(|idx| self.cpus.get(cpu)?.queue.get(idx).copied())
            .and_then(|tid| self.tasks.get(tid as usize))
            .map(|t| class_rank(t.class));

        if local_rank != Some(0) {
            if let Some(stolen) = self.steal_for_cpu(cpu, local_rank) {
                return Some(stolen);
            }
        }

        let idx = local_idx?;
        let c = self.cpus.get_mut(cpu)?;
        Some(c.queue.swap_remove(idx))
    }

    fn steal_for_cpu(&mut self, cpu: usize, max_rank: Option<u8>) -> Option<TaskId> {
        let mut busiest_cpu: Option<usize> = None;
        let mut busiest_len = 0usize;
        let mut best_rank = max_rank.unwrap_or(u8::MAX);
        for other in 0..self.cpus.len() {
            if other == cpu {
                continue;
            }
            let Some(idx) = self.best_index_in_queue(other, cpu) else {
                continue;
            };
            let Some(tid) = self.cpus.get(other).and_then(|c| c.queue.get(idx).copied()) else {
                continue;
            };
            let Some(rank) = self.tasks.get(tid as usize).map(|t| class_rank(t.class)) else {
                continue;
            };
            if max_rank.is_some_and(|mr| rank >= mr) {
                continue;
            }
            let len = self.cpus.get(other).map_or(0, |c| c.queue.len());
            if rank < best_rank || (rank == best_rank && len > busiest_len) {
                best_rank = rank;
                busiest_len = len;
                busiest_cpu = Some(other);
            }
        }
        let src = busiest_cpu?;
        let best_idx = self.best_index_in_queue(src, cpu)?;
        let c = self.cpus.get_mut(src)?;
        Some(c.queue.swap_remove(best_idx))
    }

    fn best_index_in_queue(&self, queue_cpu: usize, target_cpu: usize) -> Option<usize> {
        let q = &self.cpus.get(queue_cpu)?.queue;
        let cpu_bit = 1u64 << target_cpu;
        let mut best: Option<(usize, &Task)> = None;
        for (idx, &tid) in q.iter().enumerate() {
            let Some(t) = self.tasks.get(tid as usize) else {
                continue;
            };
            if (t.affinity_mask & cpu_bit) == 0 {
                continue;
            }
            match best {
                None => best = Some((idx, t)),
                Some((_, cur)) => {
                    if compare_tasks(t, cur) == Ordering::Less {
                        best = Some((idx, t));
                    }
                }
            }
        }
        best.map(|(idx, _)| idx)
    }

    fn remove_from_queues(&mut self, task: TaskId) {
        for c in &mut self.cpus {
            if let Some(pos) = c.queue.iter().position(|&id| id == task) {
                c.queue.swap_remove(pos);
                return;
            }
        }
    }
}

/// Numeric priority rank for scheduling classes (lower runs first).
fn class_rank(class: Class) -> u8 {
    match class {
        Class::Realtime { .. } => 0,
        Class::Normal { .. } => 1,
        Class::Idle => 2,
    }
}

/// Return `Ordering::Less` if `a` should be scheduled before `b`.
fn compare_tasks(a: &Task, b: &Task) -> Ordering {
    class_rank(a.class)
        .cmp(&class_rank(b.class))
        .then_with(|| match (a.class, b.class) {
            (Class::Realtime { prio: pa }, Class::Realtime { prio: pb }) => {
                pb.cmp(&pa).then_with(|| a.rt_seq.cmp(&b.rt_seq))
            }
            (Class::Normal { .. }, Class::Normal { .. }) => a.vruntime.cmp(&b.vruntime),
            (Class::Idle, Class::Idle) => a.rt_seq.cmp(&b.rt_seq),
            _ => Ordering::Equal,
        })
        .then_with(|| a.id.cmp(&b.id))
}
