//! quartz: RV64I + M unprivileged core (decoder, encoder and executor).
#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod alu;
mod bus;
mod exec;
pub mod inst;

pub use bus::{Bus, Fault, Ram};

/// Why an instruction did not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// The word is not a supported instruction.
    IllegalInstruction(u32),
    /// ECALL was executed.
    Ecall,
    /// EBREAK was executed.
    Ebreak,
    /// Instruction fetch hit unmapped memory.
    FetchFault(u64),
    /// Load hit unmapped memory.
    LoadFault(u64),
    /// Store hit unmapped memory.
    StoreFault(u64),
    /// Jump or taken branch to a pc that is not a multiple of 4.
    FetchMisaligned(u64),
}

/// An RV64IM hart: 32 integer registers, a program counter and a retired-instruction count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    /// Integer registers; `x[0]` always reads as zero.
    pub x: [u64; 32],
    /// Program counter.
    pub pc: u64,
    /// Number of instructions retired so far.
    pub instret: u64,
}

impl Cpu {
    /// A cpu with all registers zero, starting at `pc`.
    pub fn new(pc: u64) -> Cpu {
        Cpu {
            x: [0; 32],
            pc,
            instret: 0,
        }
    }

    /// Execute instructions until `max_steps` have run; `Ok` if none trapped.
    pub fn run(&mut self, bus: &mut impl Bus, max_steps: u64) -> Result<(), Trap> {
        for _ in 0..max_steps {
            self.step(bus)?;
        }
        Ok(())
    }
}
