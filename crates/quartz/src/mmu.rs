//! Address translation: Bare mode, Sv39 one-stage walk, and VS + G two-stage (Sv39 + Sv39x4)
//! walk, with hardware A/D updates and load/store/HLV/HSV memory helpers.
use crate::bus::Bus;
use crate::csr::{HU, MPRV, MXR, SPVP, SUM};
use crate::{Access, Cpu, Mode, Trap};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;
/// The 44-bit physical page number field of a PTE (and of satp/vsatp/hgatp).
const PPN_MASK: u64 = (1 << 44) - 1;

/// Sign or zero extend the low `size` bytes of `raw`.
pub(crate) fn extend(raw: u64, size: u8, signed: bool) -> u64 {
    let shift = 64 - 8 * u32::from(size);
    if signed {
        (((raw << shift) as i64) >> shift) as u64
    } else {
        (raw << shift) >> shift
    }
}

/// The page fault for an access kind.
fn page_fault(access: Access, vaddr: u64) -> Trap {
    match access {
        Access::Fetch => Trap::InstPageFault(vaddr),
        Access::Load => Trap::LoadPageFault(vaddr),
        Access::Store => Trap::StorePageFault(vaddr),
    }
}

/// The guest-page fault for an access kind.
fn guest_page_fault(access: Access, gva: u64, gpa: u64) -> Trap {
    match access {
        Access::Fetch => Trap::InstGuestPageFault { gva, gpa },
        Access::Load => Trap::LoadGuestPageFault { gva, gpa },
        Access::Store => Trap::StoreGuestPageFault { gva, gpa },
    }
}

impl Cpu {
    /// True when instruction fetches use Bare (physical) addressing.
    #[inline]
    pub(crate) fn is_fetch_bare(&self) -> bool {
        if self.virt {
            self.csr.vsatp >> 60 != 8 && self.csr.hgatp >> 60 != 8
        } else {
            self.mode == Mode::Machine || self.csr.satp >> 60 != 8
        }
    }

    /// Current `(satp_or_vsatp, hgatp)` pair for ITLB tagging.
    #[inline]
    pub(crate) fn itlb_keys(&self) -> (u64, u64) {
        if self.virt {
            (self.csr.vsatp, self.csr.hgatp)
        } else {
            (self.csr.satp, 0)
        }
    }

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
        self.translate_inner(bus, vaddr, access, None)
    }

    /// Translation for cached instruction fetch, recording page-table page numbers in `pt_mask`.
    pub(crate) fn translate_fetch_tracked(
        &self,
        bus: &mut impl Bus,
        vaddr: u64,
        pt_mask: &mut u64,
    ) -> Result<u64, Trap> {
        self.translate_inner(bus, vaddr, Access::Fetch, Some(pt_mask))
    }

    /// Shared translation implementation.
    fn translate_inner(
        &self,
        bus: &mut impl Bus,
        vaddr: u64,
        access: Access,
        mut pt_mask: Option<&mut u64>,
    ) -> Result<u64, Trap> {
        if self.virt {
            return self.translate_two_stage(bus, vaddr, access, self.mode, false, pt_mask);
        }
        let st = self.csr.mstatus;
        let eff = if access != Access::Fetch && (st & MPRV) != 0 && self.mode == Mode::Machine {
            Mode::from_field(st >> 11)
        } else {
            self.mode
        };
        if eff == Mode::Machine || self.csr.satp >> 60 != 8 {
            return Ok(vaddr);
        }
        let top = vaddr >> 38;
        if top != 0 && top != 0x3ff_ffff {
            return Err(page_fault(access, vaddr));
        }
        let mut table = (self.csr.satp & PPN_MASK) << 12;
        for level in (0..3u32).rev() {
            let index = (vaddr >> (12 + 9 * level)) & 0x1ff;
            let pte_addr = table.wrapping_add(index << 3);
            if let Some(mask) = pt_mask.as_deref_mut() {
                *mask |= 1u64 << ((pte_addr >> 12) & 63);
            }
            let pte = bus
                .read(pte_addr, 8)
                .map_err(|_| page_fault(access, vaddr))?;
            let reserved = pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0);
            if pte & PTE_V == 0 || reserved {
                return Err(page_fault(access, vaddr));
            }
            if pte & (PTE_R | PTE_X) == 0 {
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

    /// Check a one-stage leaf PTE and return the physical address, setting A (and D on stores).
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

    /// Two-stage translation (VS-stage followed by G-stage) as `eff` (VS or VU).
    pub(crate) fn translate_two_stage(
        &self,
        bus: &mut impl Bus,
        gva: u64,
        access: Access,
        eff: Mode,
        hlvx: bool,
        mut pt_mask: Option<&mut u64>,
    ) -> Result<u64, Trap> {
        let gpa = self.translate_vs(bus, gva, access, eff, hlvx, pt_mask.as_deref_mut())?;
        self.translate_g(bus, gva, gpa, access, access, hlvx, pt_mask)
    }

    /// VS-stage translation from `gva` to `gpa`.
    fn translate_vs(
        &self,
        bus: &mut impl Bus,
        gva: u64,
        access: Access,
        eff: Mode,
        hlvx: bool,
        mut pt_mask: Option<&mut u64>,
    ) -> Result<u64, Trap> {
        if self.csr.vsatp >> 60 != 8 {
            return Ok(gva);
        }
        let fault = page_fault(access, gva);
        let top = gva >> 38;
        if top != 0 && top != 0x3ff_ffff {
            return Err(fault);
        }
        let mut table = (self.csr.vsatp & PPN_MASK) << 12;
        for level in (0..3u32).rev() {
            let index = (gva >> (12 + 9 * level)) & 0x1ff;
            let pte_gpa = table.wrapping_add(index << 3);
            let pte_pa = self.translate_g(
                bus,
                gva,
                pte_gpa,
                access,
                Access::Load,
                false,
                pt_mask.as_deref_mut(),
            )?;
            if let Some(mask) = pt_mask.as_deref_mut() {
                *mask |= 1u64 << ((pte_pa >> 12) & 63);
            }
            let pte = bus.read(pte_pa, 8).map_err(|_| fault)?;
            let reserved = pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0);
            if pte & PTE_V == 0 || reserved {
                return Err(fault);
            }
            if pte & (PTE_R | PTE_X) == 0 {
                if level == 0 {
                    return Err(fault);
                }
                table = ((pte >> 10) & PPN_MASK) << 12;
                continue;
            }
            let ppn = (pte >> 10) & PPN_MASK;
            let low = (1u64 << (9 * level)) - 1;
            if ppn & low != 0 {
                return Err(fault);
            }
            let mxr = ((self.csr.vsstatus | self.csr.mstatus) & MXR) != 0;
            let allowed = if hlvx {
                pte & PTE_X != 0
            } else {
                match access {
                    Access::Fetch => pte & PTE_X != 0,
                    Access::Load => pte & PTE_R != 0 || (mxr && pte & PTE_X != 0),
                    Access::Store => pte & PTE_W != 0,
                }
            };
            if !allowed {
                return Err(fault);
            }
            let user = pte & PTE_U != 0;
            let privilege_ok = match eff {
                Mode::User => user,
                Mode::Supervisor => {
                    !user || (access != Access::Fetch && (self.csr.vsstatus & SUM) != 0)
                }
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
                let w_pa = self.translate_g(
                    bus,
                    gva,
                    pte_gpa,
                    access,
                    Access::Store,
                    false,
                    pt_mask.as_deref_mut(),
                )?;
                bus.write(w_pa, 8, new).map_err(|_| fault)?;
            }
            let mask = (1u64 << (12 + 9 * level)) - 1;
            return Ok(((ppn << 12) & !mask) | (gva & mask));
        }
        Err(fault)
    }

    /// G-stage translation from `gpa` to host physical address (Bare or Sv39x4).
    #[allow(clippy::too_many_arguments)]
    fn translate_g(
        &self,
        bus: &mut impl Bus,
        gva: u64,
        gpa: u64,
        orig_access: Access,
        stage_access: Access,
        hlvx: bool,
        mut pt_mask: Option<&mut u64>,
    ) -> Result<u64, Trap> {
        if self.csr.hgatp >> 60 != 8 {
            return Ok(gpa);
        }
        let fault = guest_page_fault(orig_access, gva, gpa);
        if gpa >> 41 != 0 {
            return Err(fault);
        }
        let mut table = (self.csr.hgatp & PPN_MASK & !3) << 12;
        for level in (0..3u32).rev() {
            let index = if level == 2 {
                (gpa >> 30) & 0x7ff
            } else {
                (gpa >> (12 + 9 * level)) & 0x1ff
            };
            let pte_addr = table.wrapping_add(index << 3);
            if let Some(mask) = pt_mask.as_deref_mut() {
                *mask |= 1u64 << ((pte_addr >> 12) & 63);
            }
            let pte = bus.read(pte_addr, 8).map_err(|_| fault)?;
            let reserved = pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0);
            if pte & PTE_V == 0 || reserved {
                return Err(fault);
            }
            if pte & (PTE_R | PTE_X) == 0 {
                if level == 0 {
                    return Err(fault);
                }
                table = ((pte >> 10) & PPN_MASK) << 12;
                continue;
            }
            let ppn = (pte >> 10) & PPN_MASK;
            let low = (1u64 << (9 * level)) - 1;
            if ppn & low != 0 || pte & PTE_U == 0 {
                return Err(fault);
            }
            let allowed = if hlvx {
                pte & PTE_X != 0
            } else {
                match stage_access {
                    Access::Fetch => pte & PTE_X != 0,
                    Access::Load => {
                        pte & PTE_R != 0 || ((self.csr.mstatus & MXR) != 0 && pte & PTE_X != 0)
                    }
                    Access::Store => pte & PTE_W != 0,
                }
            };
            if !allowed {
                return Err(fault);
            }
            let mut new = pte | PTE_A;
            if stage_access == Access::Store {
                new |= PTE_D;
            }
            if new != pte {
                bus.write(pte_addr, 8, new).map_err(|_| fault)?;
            }
            let mask = (1u64 << (12 + 9 * level)) - 1;
            return Ok(((ppn << 12) & !mask) | (gpa & mask));
        }
        Err(fault)
    }

    /// Execute HLV / HLVX.
    pub(crate) fn hlv(
        &self,
        bus: &mut impl Bus,
        word: u32,
        rs1: u8,
        width: u8,
        unsigned: bool,
        exec: bool,
    ) -> Result<u64, Trap> {
        if self.virt {
            return Err(Trap::VirtualInstruction(word));
        }
        if !matches!(width, 1 | 2 | 4 | 8)
            || (self.mode == Mode::User && (self.csr.hstatus & HU) == 0)
        {
            return Err(Trap::IllegalInstruction(word));
        }
        let eff = if (self.csr.hstatus & SPVP) != 0 {
            Mode::Supervisor
        } else {
            Mode::User
        };
        let addr = self.reg(rs1);
        let signed = !unsigned && !exec;
        if addr & (u64::from(width) - 1) == 0 {
            let pa = self.translate_two_stage(bus, addr, Access::Load, eff, exec, None)?;
            let raw = bus.read(pa, width).map_err(|_| Trap::LoadFault(addr))?;
            return Ok(extend(raw, width, signed));
        }
        let mut pas = [0u64; 8];
        for (i, pa_slot) in pas.iter_mut().take(usize::from(width)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            *pa_slot = self.translate_two_stage(bus, va, Access::Load, eff, exec, None)?;
        }
        let mut raw = 0u64;
        for (i, &pa) in pas.iter().take(usize::from(width)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            let byte = bus.read(pa, 1).map_err(|_| Trap::LoadFault(va))?;
            raw |= (byte & 0xff) << (8 * i);
        }
        Ok(extend(raw, width, signed))
    }

    /// Execute HSV.
    pub(crate) fn hsv(
        &self,
        bus: &mut impl Bus,
        word: u32,
        rs1: u8,
        rs2: u8,
        width: u8,
    ) -> Result<(), Trap> {
        if self.virt {
            return Err(Trap::VirtualInstruction(word));
        }
        if !matches!(width, 1 | 2 | 4 | 8)
            || (self.mode == Mode::User && (self.csr.hstatus & HU) == 0)
        {
            return Err(Trap::IllegalInstruction(word));
        }
        let eff = if (self.csr.hstatus & SPVP) != 0 {
            Mode::Supervisor
        } else {
            Mode::User
        };
        let addr = self.reg(rs1);
        if addr & (u64::from(width) - 1) == 0 {
            let pa = self.translate_two_stage(bus, addr, Access::Store, eff, false, None)?;
            return bus
                .write(pa, width, self.reg(rs2))
                .map_err(|_| Trap::StoreFault(addr));
        }
        let mut pas = [0u64; 8];
        for (i, pa_slot) in pas.iter_mut().take(usize::from(width)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            *pa_slot = self.translate_two_stage(bus, va, Access::Store, eff, false, None)?;
        }
        let val = self.reg(rs2);
        for (i, &pa) in pas.iter().take(usize::from(width)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            let byte = (val >> (8 * i)) & 0xff;
            bus.write(pa, 1, byte).map_err(|_| Trap::StoreFault(va))?;
        }
        Ok(())
    }
}
