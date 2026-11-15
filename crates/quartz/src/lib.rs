//! quartz: RV64IMAC hart with Zicsr, M/S/U modes, traps, interrupts and Sv39 paging
//! (RISC-V Privileged Specification 20211203).
#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod alu;
mod amo;
mod bus;
mod csr;
mod exec;
pub mod inst;
pub mod machine;
mod mmu;
mod trap;

pub use bus::{Bus, Fault, Ram};

use csr::Csrs;

/// Why an instruction did not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// The word is not a supported instruction, or it is not allowed in the current mode.
    IllegalInstruction(u32),
    /// ECALL was executed; the cause code depends on the mode.
    Ecall,
    /// EBREAK was executed.
    Ebreak,
    /// Instruction fetch hit unmapped memory.
    FetchFault(u64),
    /// Load hit unmapped memory.
    LoadFault(u64),
    /// Store hit unmapped memory.
    StoreFault(u64),
    /// Jump or taken branch to a pc that is not a multiple of 2.
    FetchMisaligned(u64),
    /// Load-reserved address is not a multiple of the access size.
    LoadMisaligned(u64),
    /// Store-conditional or AMO address is not a multiple of the access size.
    StoreMisaligned(u64),
    /// Instruction fetch page fault.
    InstPageFault(u64),
    /// Load page fault.
    LoadPageFault(u64),
    /// Store page fault.
    StorePageFault(u64),
}

/// Privilege mode of the hart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// U-mode.
    User,
    /// S-mode.
    Supervisor,
    /// M-mode.
    Machine,
}

impl Mode {
    /// Privilege encoding as used by the CSR fields: U=0, S=1, M=3.
    pub(crate) fn level(self) -> u64 {
        match self {
            Mode::User => 0,
            Mode::Supervisor => 1,
            Mode::Machine => 3,
        }
    }

    /// Mode named by a two-bit MPP or SPP field. The reserved value 2 never gets here
    /// because the CSR writes keep it out, so it maps to machine mode.
    pub(crate) fn from_field(bits: u64) -> Mode {
        match bits & 3 {
            0 => Mode::User,
            1 => Mode::Supervisor,
            _ => Mode::Machine,
        }
    }
}

/// The kind of memory access being translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Instruction fetch.
    Fetch,
    /// Data load.
    Load,
    /// Data store.
    Store,
}

/// What one call to `Cpu::tick` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// An instruction retired.
    Retired,
    /// An instruction trapped and the trap was taken into the handler.
    Exception(Trap),
    /// An interrupt was taken; the value is its cause code (MEI=11, MSI=3, MTI=7, SEI=9, SSI=1, STI=5).
    Interrupt(u64),
    /// WFI retired and no interrupt is pending; the caller decides how to wait.
    Wfi,
}

/// An RV64IMAC hart with Zicsr, M/S/U privilege and Sv39 paging.
///
/// `x[0]` always reads as zero. The CSR file is private; use `csr_read` and `csr_write`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    /// Integer registers; `x[0]` always reads as zero.
    pub x: [u64; 32],
    /// Program counter.
    pub pc: u64,
    /// Number of instructions retired so far (minstret).
    pub instret: u64,
    /// Current privilege mode.
    pub mode: Mode,
    csr: Csrs,
    reservation: Option<u64>,
}

impl Cpu {
    /// A hart in machine mode with zeroed registers and CSRs, starting at `pc`, paging off.
    pub fn new(pc: u64) -> Cpu {
        Cpu {
            x: [0; 32],
            pc,
            instret: 0,
            mode: Mode::Machine,
            csr: Csrs::default(),
            reservation: None,
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
