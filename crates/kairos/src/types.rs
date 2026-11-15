//! Public types and weight table for the kairos fair scheduler.

/// Unique identifier for a spawned task.
pub type TaskId = u32;

/// Scheduler configuration parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Target scheduling latency in microseconds.
    pub latency_us: u64,
    /// Minimum time slice in microseconds.
    pub min_slice_us: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            latency_us: 6000,
            min_slice_us: 750,
        }
    }
}

/// Scheduling class of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Fixed-priority realtime task (higher `prio` runs first).
    Realtime {
        /// Realtime priority level.
        prio: u8,
    },
    /// Weighted fair normal task (`nice` is clamped to `-20..=19`).
    Normal {
        /// Niceness value in `-20..=19`.
        nice: i8,
    },
    /// Background idle task that runs only when nothing else is ready.
    Idle,
}

/// Lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Runnable and waiting in a CPU run queue.
    Ready,
    /// Currently executing on a CPU.
    Running,
    /// Waiting on an external event and not runnable.
    Blocked,
    /// Exited and permanently retired.
    Dead,
}

/// Errors returned by scheduler operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The given `TaskId` does not exist.
    NoSuchTask,
    /// The given CPU index is out of range.
    BadCpu,
    /// The task is not in a valid state for this operation.
    BadState,
    /// The live task limit (65 536) or ID space has been reached.
    TooManyTasks,
}

/// Result of `Scheduler::pick` for a CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Chosen task to run, or `None` if the CPU should idle.
    pub task: Option<TaskId>,
    /// Recommended time slice in microseconds.
    pub slice_us: u64,
}

/// Snapshot of a task's scheduling statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Current lifecycle state.
    pub state: State,
    /// Current scheduling class.
    pub class: Class,
    /// Total accumulated CPU runtime in microseconds.
    pub runtime_us: u64,
    /// Number of times the task was picked to run.
    pub switches: u64,
    /// CPU the task is currently running on, if `State::Running`.
    pub cpu: Option<usize>,
}

/// Standard CFS weight table for nice values `-20..=19` (index `nice + 20`).
pub(crate) const NICE_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

/// Clamp `Normal { nice }` to `-20..=19`.
pub(crate) fn clamp_class(class: Class) -> Class {
    match class {
        Class::Normal { nice } => Class::Normal {
            nice: nice.clamp(-20, 19),
        },
        other => other,
    }
}

/// Return the weight for a `Normal` nice value (already clamped or clamped here).
pub(crate) fn nice_weight(nice: i8) -> u64 {
    let idx = (nice.clamp(-20, 19) as i32 + 20) as usize;
    NICE_TO_WEIGHT.get(idx).copied().unwrap_or(1024)
}
