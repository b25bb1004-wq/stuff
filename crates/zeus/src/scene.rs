//! Stepwise host scene: boots zeus with guests, runs it in slices, and exposes consoles and guest page faults.

use crate::{build_machine, split_console, Guest};
use silicon::machine::Machine;

pub use silicon::machine::Exit;

/// Source of the spy guest (`tests/fixtures/spy.s`), embedded so other crates can build the same scene.
pub const SPY_SRC: &str = include_str!("../tests/fixtures/spy.s");

/// Guest page-fault causes watched by `Scene::last_fault`: instruction (20), load (21), store (23).
const FAULT_CAUSES: [u64; 3] = [20, 21, 23];

/// A guest page fault seen during the most recent step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    /// Exception cause code (20, 21 or 23).
    pub cause: u64,
    /// Cumulative count of this cause since the scene was built.
    pub count: u64,
}

/// A booted zeus machine that can be run in slices and inspected between them.
pub struct Scene {
    machine: Machine,
    out: Vec<u8>,
    last: Option<Fault>,
}

impl Scene {
    /// Boot zeus with `guests` (see `build_machine`) without running it yet.
    pub fn new(zeus_elf: &[u8], guests: &[Guest]) -> Result<Scene, String> {
        Ok(Scene {
            machine: build_machine(zeus_elf, guests)?,
            out: Vec::new(),
            last: None,
        })
    }

    /// Run at most `max` steps, collecting console output and the guest page faults seen.
    ///
    /// If any of causes 20, 21 or 23 rose during this call, `last_fault` reports the one that
    /// rose most (ties go to the higher cause code).
    pub fn step(&mut self, max: u64) -> Exit {
        let before = self.machine.probe().exceptions;
        let exit = self.machine.run(max);
        let after = self.machine.probe().exceptions;
        self.out.extend(self.machine.board.take_uart_output());

        let count_of = |counts: &[u64; 32], cause: u64| {
            usize::try_from(cause)
                .ok()
                .and_then(|i| counts.get(i).copied())
                .unwrap_or(0)
        };
        let mut best: Option<(u64, u64)> = None; // (rise, cause)
        for cause in FAULT_CAUSES {
            let rise = count_of(&after, cause).saturating_sub(count_of(&before, cause));
            if rise > 0 && !matches!(best, Some(b) if b > (rise, cause)) {
                best = Some((rise, cause));
            }
        }
        if let Some((_, cause)) = best {
            self.last = Some(Fault {
                cause,
                count: count_of(&after, cause),
            });
        }
        exit
    }

    /// Console lines written by `guest` so far (zeus's own lines appear under `"zeus"`).
    pub fn console(&self, guest: &str) -> Vec<String> {
        let mut lines = split_console(&self.out);
        lines.remove(guest).unwrap_or_default()
    }

    /// The most recent guest page fault seen during a step, if any.
    pub fn last_fault(&self) -> Option<Fault> {
        self.last
    }
}
