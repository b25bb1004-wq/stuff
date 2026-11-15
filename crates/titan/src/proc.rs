//! Hardware-independent process table logic: pids, states, wait/exit/kill, orphans, ps.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::uaccess::Errno;

/// Process identifier (valid PIDs start at `1`).
pub type Pid = u32;

/// Maximum number of concurrent processes (including unreaped zombies).
pub const MAX_PROCS: usize = 64;

/// Reason a process is currently blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// Blocked in `wait` for the specified child PID.
    Wait(Pid),
}

/// Lifecycle state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    /// Ready to run when picked by the scheduler.
    Runnable,
    /// Currently executing on the CPU.
    Running,
    /// Blocked waiting on an event.
    Blocked(BlockReason),
    /// Terminated with an exit code, awaiting `wait` by its parent.
    Zombie,
}

impl ProcState {
    /// Short lowercase status name displayed by `ps`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runnable => "runnable",
            Self::Running => "running",
            Self::Blocked(_) => "blocked",
            Self::Zombie => "zombie",
        }
    }
}

/// Result of [`Table::wait`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitStatus {
    /// Target child had exited; reaped with its exit code.
    Exited(i32),
    /// Target child is still alive; caller transitioned to `Blocked(Wait)`.
    WouldBlock,
}

/// One row of [`Table::ps`] output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsRow {
    /// Process identifier.
    pub pid: Pid,
    /// Current lifecycle state.
    pub state: ProcState,
    /// Program name.
    pub name: String,
}

impl fmt::Display for PsRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.pid, self.state.as_str(), self.name)
    }
}

#[derive(Debug, Clone)]
struct Slot {
    pid: Pid,
    parent: Option<Pid>,
    state: ProcState,
    exit_code: i32,
    name: String,
}

/// Fixed-capacity process table tracking parent links, states, zombies, and orphans.
#[derive(Debug, Clone)]
pub struct Table {
    slots: Vec<Option<Slot>>,
    next_pid: Pid,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// Create an empty process table whose first spawned process receives PID `1`.
    pub fn new() -> Self {
        Self {
            slots: vec![None; MAX_PROCS],
            next_pid: 1,
        }
    }

    /// Spawn a new process under optional `parent` with `name`.
    pub fn spawn(&mut self, parent: Option<Pid>, name: &str) -> Result<Pid, Errno> {
        if let Some(ppid) = parent {
            let p = self.slot(ppid).ok_or(Errno::BadArg)?;
            if p.state == ProcState::Zombie {
                return Err(Errno::BadArg);
            }
        } else if self.slot(1).is_some() {
            return Err(Errno::BadArg);
        }

        let free_idx = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(Errno::NoMem)?;
        let pid = self.alloc_pid();
        self.slots[free_idx] = Some(Slot {
            pid,
            parent,
            state: ProcState::Runnable,
            exit_code: 0,
            name: String::from(name),
        });
        Ok(pid)
    }

    /// Transition a `Runnable` process to `Running`.
    pub fn set_running(&mut self, pid: Pid) -> Result<(), Errno> {
        let s = self.slot_mut(pid).ok_or(Errno::BadArg)?;
        if s.state != ProcState::Runnable {
            return Err(Errno::BadArg);
        }
        s.state = ProcState::Running;
        Ok(())
    }

    /// Transition a `Running` process back to `Runnable`.
    pub fn set_runnable(&mut self, pid: Pid) -> Result<(), Errno> {
        let s = self.slot_mut(pid).ok_or(Errno::BadArg)?;
        if s.state != ProcState::Running {
            return Err(Errno::BadArg);
        }
        s.state = ProcState::Runnable;
        Ok(())
    }

    /// Mark `pid` as `Zombie` with `code`, re-parent its children to PID `1`,
    /// and return `Some(parent)` if its parent was `Blocked(Wait(pid))` (waking it).
    pub fn exit(&mut self, pid: Pid, code: i32) -> Result<Option<Pid>, Errno> {
        let parent_pid = {
            let s = self.slot_mut(pid).ok_or(Errno::BadArg)?;
            if s.state == ProcState::Zombie {
                return Err(Errno::BadArg);
            }
            s.state = ProcState::Zombie;
            s.exit_code = code;
            s.parent
        };

        let new_parent = if pid == 1 { None } else { Some(1) };
        for slot in self.slots.iter_mut().flatten() {
            if slot.parent == Some(pid) {
                slot.parent = new_parent;
            }
        }

        if let Some(ppid) = parent_pid {
            if let Some(pslot) = self.slot_mut(ppid) {
                if pslot.state == ProcState::Blocked(BlockReason::Wait(pid)) {
                    pslot.state = ProcState::Runnable;
                    return Ok(Some(ppid));
                }
            }
        }
        Ok(None)
    }

    /// Kill `pid` with exit code `-1`.
    pub fn kill(&mut self, pid: Pid) -> Result<Option<Pid>, Errno> {
        self.exit(pid, -1)
    }

    /// Kill `pid` with exit code `-1` only if `pid`'s parent is `caller`.
    pub fn kill_child(&mut self, caller: Pid, pid: Pid) -> Result<Option<Pid>, Errno> {
        let s = self.slot(pid).ok_or(Errno::BadArg)?;
        if s.parent != Some(caller) {
            return Err(Errno::BadArg);
        }
        self.exit(pid, -1)
    }

    /// Wait for child `pid` from `parent`: reap zombie and return exit code, or block parent.
    pub fn wait(&mut self, parent: Pid, pid: Pid) -> Result<WaitStatus, Errno> {
        let parent_idx = self.slot_index(parent).ok_or(Errno::BadArg)?;
        let parent_state = self.slots[parent_idx]
            .as_ref()
            .map(|s| s.state)
            .ok_or(Errno::BadArg)?;
        if !matches!(parent_state, ProcState::Runnable | ProcState::Running) {
            return Err(Errno::BadArg);
        }

        let child_idx = self.slot_index(pid).ok_or(Errno::BadArg)?;
        let (child_parent, child_state, child_code) = {
            let c = self.slots[child_idx].as_ref().ok_or(Errno::BadArg)?;
            (c.parent, c.state, c.exit_code)
        };
        if child_parent != Some(parent) {
            return Err(Errno::BadArg);
        }

        if child_state == ProcState::Zombie {
            self.slots[child_idx] = None;
            return Ok(WaitStatus::Exited(child_code));
        }

        if let Some(p) = self.slots[parent_idx].as_mut() {
            p.state = ProcState::Blocked(BlockReason::Wait(pid));
        }
        Ok(WaitStatus::WouldBlock)
    }

    /// Return current processes sorted by PID ascending.
    pub fn ps(&self) -> Vec<PsRow> {
        let mut rows: Vec<PsRow> = self
            .slots
            .iter()
            .flatten()
            .map(|s| PsRow {
                pid: s.pid,
                state: s.state,
                name: s.name.clone(),
            })
            .collect();
        rows.sort_by_key(|r| r.pid);
        rows
    }

    /// Format [`Self::ps`] rows as newline-terminated `"<pid> <state> <name>\n"` lines.
    pub fn format_ps(&self) -> String {
        let mut out = String::new();
        for row in self.ps() {
            out.push_str(&format!("{row}\n"));
        }
        out
    }

    /// Return the current state of `pid`, if it exists in the table.
    pub fn state(&self, pid: Pid) -> Option<ProcState> {
        self.slot(pid).map(|s| s.state)
    }

    /// Return the parent PID of `pid`, if it exists and has a parent.
    pub fn parent(&self, pid: Pid) -> Option<Pid> {
        self.slot(pid).and_then(|s| s.parent)
    }

    /// Return the number of occupied slots (including unreaped zombies).
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    fn slot_index(&self, pid: Pid) -> Option<usize> {
        if pid == 0 {
            return None;
        }
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|slot| slot.pid == pid))
    }

    fn slot(&self, pid: Pid) -> Option<&Slot> {
        let idx = self.slot_index(pid)?;
        self.slots.get(idx).and_then(|s| s.as_ref())
    }

    fn slot_mut(&mut self, pid: Pid) -> Option<&mut Slot> {
        let idx = self.slot_index(pid)?;
        self.slots.get_mut(idx).and_then(|s| s.as_mut())
    }

    fn alloc_pid(&mut self) -> Pid {
        loop {
            let candidate = self.next_pid;
            self.next_pid = if candidate == u32::MAX {
                1
            } else {
                candidate + 1
            };
            if candidate != 0 && self.slot_index(candidate).is_none() {
                return candidate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::collections::BTreeMap;
    use assay::forall;

    #[test]
    fn spawn_wait_kill_and_orphan_reparenting() {
        let mut t = Table::new();
        let p1 = t.spawn(None, "init").unwrap();
        assert_eq!(p1, 1);
        t.set_running(p1).unwrap();
        assert_eq!(t.format_ps(), "1 running init\n");

        let p2 = t.spawn(Some(p1), "parent").unwrap();
        let p3 = t.spawn(Some(p2), "orphan").unwrap();
        assert_eq!(t.parent(p3), Some(p2));

        // Non-parent cannot wait or kill_child.
        assert_eq!(t.wait(p1, p3), Err(Errno::BadArg));
        assert_eq!(t.kill_child(p3, p1), Err(Errno::BadArg));

        // Wait blocks while child is alive, wakes on child exit.
        assert_eq!(t.wait(p1, p2), Ok(WaitStatus::WouldBlock));
        assert_eq!(t.state(p1), Some(ProcState::Blocked(BlockReason::Wait(p2))));

        // p2 exits: wakes p1 and reparents p3 to pid 1.
        assert_eq!(t.exit(p2, 42), Ok(Some(p1)));
        assert_eq!(t.state(p1), Some(ProcState::Runnable));
        assert_eq!(t.parent(p3), Some(1));

        // Now p1 can reap p2 and wait/kill p3.
        assert_eq!(t.wait(p1, p2), Ok(WaitStatus::Exited(42)));
        assert_eq!(t.state(p2), None);
        assert_eq!(t.kill_child(p1, p3), Ok(None));
        assert_eq!(t.wait(p1, p3), Ok(WaitStatus::Exited(-1)));
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn slot_reuse_after_reaping() {
        let mut t = Table::new();
        let init = t.spawn(None, "init").unwrap();
        for i in 0..(MAX_PROCS * 3) {
            let c = t.spawn(Some(init), "worker").unwrap();
            t.exit(c, i as i32).unwrap();
            assert_eq!(t.wait(init, c), Ok(WaitStatus::Exited(i as i32)));
        }
        assert_eq!(t.count(), 1);
    }

    #[derive(Clone, Debug)]
    struct ShadowProc {
        parent: Option<Pid>,
        state: ProcState,
        exit_code: i32,
        name: String,
    }

    #[test]
    fn shadow_model_forall() {
        forall(80, 0x990C_57A7, |rng| {
            let mut t = Table::new();
            let mut shadow: BTreeMap<Pid, ShadowProc> = BTreeMap::new();

            let p1 = t.spawn(None, "init").unwrap();
            assert_eq!(p1, 1);
            shadow.insert(
                p1,
                ShadowProc {
                    parent: None,
                    state: ProcState::Runnable,
                    exit_code: 0,
                    name: String::from("init"),
                },
            );

            for _ in 0..250 {
                let pids: Vec<Pid> = shadow.keys().copied().collect();
                match rng.below(5) {
                    0 | 1 => {
                        // Spawn under a random existing process.
                        let ppid = *rng.pick(&pids);
                        let p_zombie = shadow[&ppid].state == ProcState::Zombie;
                        let res = t.spawn(Some(ppid), "p");
                        if p_zombie {
                            assert_eq!(res, Err(Errno::BadArg));
                        } else if shadow.len() == MAX_PROCS {
                            assert_eq!(res, Err(Errno::NoMem));
                        } else {
                            let child = res.unwrap();
                            assert!(!shadow.contains_key(&child));
                            shadow.insert(
                                child,
                                ShadowProc {
                                    parent: Some(ppid),
                                    state: ProcState::Runnable,
                                    exit_code: 0,
                                    name: String::from("p"),
                                },
                            );
                        }
                    }
                    2 => {
                        // Exit or kill a non-init process.
                        let target = *rng.pick(&pids);
                        if target == 1 {
                            continue;
                        }
                        let is_kill = rng.chance(1, 2);
                        let code = if is_kill {
                            -1
                        } else {
                            (rng.u64() % 200) as i32 - 50
                        };
                        let res = if is_kill {
                            t.kill(target)
                        } else {
                            t.exit(target, code)
                        };
                        if shadow[&target].state == ProcState::Zombie {
                            assert_eq!(res, Err(Errno::BadArg));
                        } else {
                            let ppid = shadow[&target].parent;
                            let mut woke = None;
                            if let Some(s) = shadow.get_mut(&target) {
                                s.state = ProcState::Zombie;
                                s.exit_code = code;
                            }
                            for s in shadow.values_mut() {
                                if s.parent == Some(target) {
                                    s.parent = Some(1);
                                }
                            }
                            if let Some(p) = ppid {
                                if let Some(ps) = shadow.get_mut(&p) {
                                    if ps.state == ProcState::Blocked(BlockReason::Wait(target)) {
                                        ps.state = ProcState::Runnable;
                                        woke = Some(p);
                                    }
                                }
                            }
                            assert_eq!(res, Ok(woke));
                        }
                    }
                    3 => {
                        // Wait on a random pair.
                        let parent = *rng.pick(&pids);
                        let child = *rng.pick(&pids);
                        let res = t.wait(parent, child);
                        let p_ok = matches!(
                            shadow[&parent].state,
                            ProcState::Runnable | ProcState::Running
                        );
                        let is_child = shadow[&child].parent == Some(parent);
                        if !p_ok || !is_child {
                            assert_eq!(res, Err(Errno::BadArg));
                        } else if shadow[&child].state == ProcState::Zombie {
                            let code = shadow.remove(&child).unwrap().exit_code;
                            assert_eq!(res, Ok(WaitStatus::Exited(code)));
                        } else {
                            shadow.get_mut(&parent).unwrap().state =
                                ProcState::Blocked(BlockReason::Wait(child));
                            assert_eq!(res, Ok(WaitStatus::WouldBlock));
                        }
                    }
                    _ => {
                        // Toggle Running <-> Runnable on a random process.
                        let target = *rng.pick(&pids);
                        if rng.chance(1, 2) {
                            let res = t.set_running(target);
                            if shadow[&target].state == ProcState::Runnable {
                                shadow.get_mut(&target).unwrap().state = ProcState::Running;
                                assert_eq!(res, Ok(()));
                            } else {
                                assert_eq!(res, Err(Errno::BadArg));
                            }
                        } else {
                            let res = t.set_runnable(target);
                            if shadow[&target].state == ProcState::Running {
                                shadow.get_mut(&target).unwrap().state = ProcState::Runnable;
                                assert_eq!(res, Ok(()));
                            } else {
                                assert_eq!(res, Err(Errno::BadArg));
                            }
                        }
                    }
                }

                // Compare ps() and count() against shadow map on every step.
                assert_eq!(t.count(), shadow.len());
                let ps = t.ps();
                assert_eq!(ps.len(), shadow.len());
                for row in ps {
                    let s = &shadow[&row.pid];
                    assert_eq!(row.state, s.state);
                    assert_eq!(row.name, s.name);
                    assert_eq!(t.parent(row.pid), s.parent);
                }
            }
        });
    }
}
