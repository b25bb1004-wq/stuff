//! Address translation: Bare mode (identity) and the Sv39 three-level page walk, with the
//! accessed and dirty bits set by the hardware.
use crate::bus::Bus;
use crate::csr::{MPRV, MXR, SUM};
use crate::{Access, Cpu, Mode, Trap};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;
/// The 44-bit physical page number field of a PTE (and of satp).
const PPN_MASK: u64 = (1 << 44) - 1;

/// The page fault for an access kind.
fn page_fault(access: Access, vaddr: u64) -> Trap {
    match access {
        Access::Fetch => Trap::InstPageFault(vaddr),
        Access::Load => Trap::LoadPageFault(vaddr),
        Access::Store => Trap::StorePageFault(vaddr),
    }
}

impl Cpu {
    /// Translate `vaddr` for an access of kind `access` in the current mode. MPRV in M-mode
    /// makes loads and stores translate as the mode in MPP.
    pub fn translate(
        &mut self,
        bus: &mut impl Bus,
        vaddr: u64,
        access: Access,
    ) -> Result<u64, Trap> {
        self.translate_with(bus, vaddr, access)
    }

    /// Translation used by the executor and by `translate`.
    pub(crate) fn translate_with(
        &self,
        bus: &mut impl Bus,
        vaddr: u64,
        access: Access,
    ) -> Result<u64, Trap> {
        let st = self.csr.mstatus;
        let eff = if access != Access::Fetch && (st & MPRV) != 0 && self.mode == Mode::Machine {
            Mode::from_field(st >> 11)
        } else {
            self.mode
        };
        if eff == Mode::Machine || self.csr.satp >> 60 != 8 {
            return Ok(vaddr);
        }
        // Bits 63:38 must all equal bit 38.
        let top = vaddr >> 38;
        if top != 0 && top != 0x3ff_ffff {
            return Err(page_fault(access, vaddr));
        }
        let mut table = (self.csr.satp & PPN_MASK) << 12;
        for level in (0..3u32).rev() {
            let index = (vaddr >> (12 + 9 * level)) & 0x1ff;
            let pte_addr = table.wrapping_add(index << 3);
            let pte = bus
                .read(pte_addr, 8)
                .map_err(|_| page_fault(access, vaddr))?;
            let reserved = pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0);
            if pte & PTE_V == 0 || reserved {
                return Err(page_fault(access, vaddr));
            }
            if pte & (PTE_R | PTE_X) == 0 {
                // Pointer to the next table; a pointer at the last level is a fault.
                if level == 0 {
                    return Err(page_fault(access, vaddr));
                }
                table = ((pte >> 10) & PPN_MASK) << 12;
                continue;
            }
            return self.leaf(bus, vaddr, access, eff, (pte, pte_addr, level));
        }
        Err(page_fault(access, vaddr))
    }

    /// Check a leaf PTE and return the physical address, setting A (and D on stores).
    fn leaf(
        &self,
        bus: &mut impl Bus,
        vaddr: u64,
        access: Access,
        eff: Mode,
        (pte, pte_addr, level): (u64, u64, u32),
    ) -> Result<u64, Trap> {
        let fault = page_fault(access, vaddr);
        let ppn = (pte >> 10) & PPN_MASK;
        // A superpage must have its lower physical page number fields zero.
        let low = (1u64 << (9 * level)) - 1;
        if ppn & low != 0 {
            return Err(fault);
        }
        let st = self.csr.mstatus;
        let allowed = match access {
            Access::Fetch => pte & PTE_X != 0,
            Access::Load => pte & PTE_R != 0 || (st & MXR != 0 && pte & PTE_X != 0),
            Access::Store => pte & PTE_W != 0,
        };
        if !allowed {
            return Err(fault);
        }
        let user = pte & PTE_U != 0;
        let privilege_ok = match eff {
            Mode::User => user,
            Mode::Supervisor => !user || (access != Access::Fetch && (st & SUM) != 0),
            Mode::Machine => true,
        };
        if !privilege_ok {
            return Err(fault);
        }
        let mut new = pte | PTE_A;
        if access == Access::Store {
            new |= PTE_D;
        }
        if new != pte {
            bus.write(pte_addr, 8, new).map_err(|_| fault)?;
        }
        let mask = (1u64 << (12 + 9 * level)) - 1;
        Ok(((ppn << 12) & !mask) | (vaddr & mask))
    }
}
