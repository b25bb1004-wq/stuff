//! S-mode trap cause (`scause`) decoder and human-readable display names.

use core::fmt;

/// Most-significant bit of `scause`, indicating an asynchronous interrupt when set.
pub const INTERRUPT_BIT: u64 = 1u64 << 63;

/// Decoded RISC-V `scause` value (interrupt or synchronous exception).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// Supervisor software interrupt (interrupt code 1).
    SupervisorSoft,
    /// Supervisor timer interrupt (interrupt code 5).
    SupervisorTimer,
    /// Supervisor external interrupt (interrupt code 9).
    SupervisorExternal,
    /// Any other asynchronous interrupt code.
    UnknownInterrupt(u64),
    /// Instruction address misaligned (exception code 0).
    InstructionMisaligned,
    /// Instruction access fault (exception code 1).
    InstructionAccessFault,
    /// Illegal instruction (exception code 2).
    IllegalInstruction,
    /// Breakpoint (`ebreak`, exception code 3).
    Breakpoint,
    /// Load address misaligned (exception code 4).
    LoadMisaligned,
    /// Load access fault (exception code 5).
    LoadAccessFault,
    /// Store/AMO address misaligned (exception code 6).
    StoreMisaligned,
    /// Store/AMO access fault (exception code 7).
    StoreAccessFault,
    /// Environment call from U-mode (exception code 8).
    UserEcall,
    /// Environment call from S-mode (exception code 9).
    SupervisorEcall,
    /// Instruction page fault (exception code 12).
    InstructionPageFault,
    /// Load page fault (exception code 13).
    LoadPageFault,
    /// Store/AMO page fault (exception code 15).
    StorePageFault,
    /// Any other synchronous exception code.
    UnknownException(u64),
}

impl Cause {
    /// Decode a raw `scause` register value into a [`Cause`].
    pub const fn decode(scause: u64) -> Self {
        let is_int = (scause & INTERRUPT_BIT) != 0;
        let code = scause & !INTERRUPT_BIT;
        if is_int {
            match code {
                1 => Self::SupervisorSoft,
                5 => Self::SupervisorTimer,
                9 => Self::SupervisorExternal,
                other => Self::UnknownInterrupt(other),
            }
        } else {
            match code {
                0 => Self::InstructionMisaligned,
                1 => Self::InstructionAccessFault,
                2 => Self::IllegalInstruction,
                3 => Self::Breakpoint,
                4 => Self::LoadMisaligned,
                5 => Self::LoadAccessFault,
                6 => Self::StoreMisaligned,
                7 => Self::StoreAccessFault,
                8 => Self::UserEcall,
                9 => Self::SupervisorEcall,
                12 => Self::InstructionPageFault,
                13 => Self::LoadPageFault,
                15 => Self::StorePageFault,
                other => Self::UnknownException(other),
            }
        }
    }

    /// Alias for [`Cause::decode`].
    pub const fn from_scause(scause: u64) -> Self {
        Self::decode(scause)
    }

    /// Return true if this cause represents an asynchronous interrupt.
    pub const fn is_interrupt(self) -> bool {
        matches!(
            self,
            Self::SupervisorSoft
                | Self::SupervisorTimer
                | Self::SupervisorExternal
                | Self::UnknownInterrupt(_)
        )
    }

    /// Return true if this cause represents a synchronous exception.
    pub const fn is_exception(self) -> bool {
        !self.is_interrupt()
    }

    /// Return true if this cause is a page fault (instruction 12, load 13, or store/AMO 15).
    pub const fn is_page_fault(self) -> bool {
        matches!(
            self,
            Self::InstructionPageFault | Self::LoadPageFault | Self::StorePageFault
        )
    }

    /// Return the low 63-bit cause code without the interrupt bit.
    pub const fn code(self) -> u64 {
        match self {
            Self::SupervisorSoft => 1,
            Self::SupervisorTimer => 5,
            Self::SupervisorExternal => 9,
            Self::UnknownInterrupt(c) => c & !INTERRUPT_BIT,
            Self::InstructionMisaligned => 0,
            Self::InstructionAccessFault => 1,
            Self::IllegalInstruction => 2,
            Self::Breakpoint => 3,
            Self::LoadMisaligned => 4,
            Self::LoadAccessFault => 5,
            Self::StoreMisaligned => 6,
            Self::StoreAccessFault => 7,
            Self::UserEcall => 8,
            Self::SupervisorEcall => 9,
            Self::InstructionPageFault => 12,
            Self::LoadPageFault => 13,
            Self::StorePageFault => 15,
            Self::UnknownException(c) => c & !INTERRUPT_BIT,
        }
    }

    /// Encode this [`Cause`] back into a raw 64-bit `scause` value.
    pub const fn scause(self) -> u64 {
        let bit = if self.is_interrupt() {
            INTERRUPT_BIT
        } else {
            0
        };
        bit | self.code()
    }
}

impl From<u64> for Cause {
    fn from(scause: u64) -> Self {
        Self::decode(scause)
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SupervisorSoft => write!(f, "supervisor software interrupt"),
            Self::SupervisorTimer => write!(f, "supervisor timer interrupt"),
            Self::SupervisorExternal => write!(f, "supervisor external interrupt"),
            Self::UnknownInterrupt(c) => write!(f, "unknown interrupt ({c})"),
            Self::InstructionMisaligned => write!(f, "instruction address misaligned"),
            Self::InstructionAccessFault => write!(f, "instruction access fault"),
            Self::IllegalInstruction => write!(f, "illegal instruction"),
            Self::Breakpoint => write!(f, "breakpoint"),
            Self::LoadMisaligned => write!(f, "load address misaligned"),
            Self::LoadAccessFault => write!(f, "load access fault"),
            Self::StoreMisaligned => write!(f, "store/AMO address misaligned"),
            Self::StoreAccessFault => write!(f, "store/AMO access fault"),
            Self::UserEcall => write!(f, "environment call from U-mode"),
            Self::SupervisorEcall => write!(f, "environment call from S-mode"),
            Self::InstructionPageFault => write!(f, "instruction page fault"),
            Self::LoadPageFault => write!(f, "load page fault"),
            Self::StorePageFault => write!(f, "store/AMO page fault"),
            Self::UnknownException(c) => write!(f, "unknown exception ({c})"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::string::ToString;
    use assay::forall;

    #[test]
    fn known_causes_decode_and_display() {
        assert_eq!(Cause::decode(INTERRUPT_BIT | 1), Cause::SupervisorSoft);
        assert_eq!(Cause::decode(INTERRUPT_BIT | 5), Cause::SupervisorTimer);
        assert_eq!(Cause::decode(INTERRUPT_BIT | 9), Cause::SupervisorExternal);
        assert_eq!(Cause::decode(2), Cause::IllegalInstruction);
        assert_eq!(Cause::decode(8), Cause::UserEcall);
        assert_eq!(Cause::decode(12), Cause::InstructionPageFault);
        assert_eq!(Cause::decode(13), Cause::LoadPageFault);
        assert_eq!(Cause::decode(15), Cause::StorePageFault);

        assert!(Cause::decode(12).is_page_fault());
        assert!(Cause::decode(13).is_page_fault());
        assert!(Cause::decode(15).is_page_fault());
        assert!(!Cause::decode(2).is_page_fault());
        assert!(!Cause::decode(INTERRUPT_BIT | 12).is_page_fault());

        assert_eq!(Cause::LoadPageFault.to_string(), "load page fault");
        assert_eq!(Cause::StorePageFault.to_string(), "store/AMO page fault");
        assert_eq!(
            Cause::InstructionPageFault.to_string(),
            "instruction page fault"
        );
    }

    #[test]
    fn roundtrip_scause_property() {
        forall(200, 0xCA05_E123, |rng| {
            let raw = rng.u64();
            let c = Cause::decode(raw);
            assert_eq!(c.is_interrupt(), (raw & INTERRUPT_BIT) != 0);
            assert_eq!(c.code(), raw & !INTERRUPT_BIT);
            assert_eq!(c.scause(), raw);
            assert!(!c.to_string().is_empty());
        });
    }
}
