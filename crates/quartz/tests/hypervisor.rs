//! Hypervisor extension (H) tests: CSR masks, VS redirection, virtual-instruction traps,
//! two-stage translation (Sv39 + Sv39x4) against a reference model, HLV/HLVX/HSV, trap and
//! interrupt routing to VS/HS/M, xRET with V, and a guest running in VS under Sv39x4.
#![allow(clippy::unwrap_used, clippy::too_many_arguments)]

use assay::{forall, Rng};
use quartz::inst::{decode, encode, Inst};
use quartz::machine::{Config, Machine, RAM_BASE};
use quartz::{Access, Bus, Cpu, Mode, Ram, Step, Trap};

const SIE: u64 = 1 << 1;
const MIE: u64 = 1 << 3;
const SPIE: u64 = 1 << 5;
const MPIE: u64 = 1 << 7;
const SPP: u64 = 1 << 8;
const MPP_S: u64 = 1 << 11;
const MPP_M: u64 = 3 << 11;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;
const TVM: u64 = 1 << 20;
const TW: u64 = 1 << 21;
const TSR: u64 = 1 << 22;
const MSTATUS_GVA: u64 = 1 << 38;
const MPV: u64 = 1 << 39;

const HSTATUS_GVA: u64 = 1 << 6;
const SPV: u64 = 1 << 7;
const SPVP: u64 = 1 << 8;
const HU: u64 = 1 << 9;
const VTVM: u64 = 1 << 20;
const VTW: u64 = 1 << 21;
const VTSR: u64 = 1 << 22;

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;
const PPN_MASK: u64 = (1 << 44) - 1;

fn place(ram: &mut Ram, addr: u64, insts: &[Inst]) {
    for (i, inst) in insts.iter().enumerate() {
        let bytes = encode(*inst).to_le_bytes();
        ram.load(addr + 4 * i as u64, &bytes).unwrap();
    }
}

fn step_one(cpu: &mut Cpu, inst: Inst) -> Result<(), Trap> {
    let mut ram = Ram::new(0, 0x1000);
    place(&mut ram, 0, &[inst]);
    cpu.pc = 0;
    cpu.step(&mut ram)
}

#[test]
fn h_instructions_encode_and_decode_to_spec_words() {
    let cases = [
        (
            0x6000_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 1,
                unsigned: false,
                exec: false,
            },
        ),
        (
            0x6010_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 1,
                unsigned: true,
                exec: false,
            },
        ),
        (
            0x6400_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 2,
                unsigned: false,
                exec: false,
            },
        ),
        (
            0x6410_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 2,
                unsigned: true,
                exec: false,
            },
        ),
        (
            0x6430_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 2,
                unsigned: true,
                exec: true,
            },
        ),
        (
            0x6800_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 4,
                unsigned: false,
                exec: false,
            },
        ),
        (
            0x6810_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 4,
                unsigned: true,
                exec: false,
            },
        ),
        (
            0x6830_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 4,
                unsigned: true,
                exec: true,
            },
        ),
        (
            0x6c00_4073,
            Inst::Hlv {
                rd: 0,
                rs1: 0,
                width: 8,
                unsigned: false,
                exec: false,
            },
        ),
        (
            0x6220_c073,
            Inst::Hsv {
                rs1: 1,
                rs2: 2,
                width: 1,
            },
        ),
        (
            0x6620_c073,
            Inst::Hsv {
                rs1: 1,
                rs2: 2,
                width: 2,
            },
        ),
        (
            0x6a20_c073,
            Inst::Hsv {
                rs1: 1,
                rs2: 2,
                width: 4,
            },
        ),
        (
            0x6e20_c073,
            Inst::Hsv {
                rs1: 1,
                rs2: 2,
                width: 8,
            },
        ),
        (0x2220_8073, Inst::HfenceVvma { rs1: 1, rs2: 2 }),
        (0x6220_8073, Inst::HfenceGvma { rs1: 1, rs2: 2 }),
    ];
    for (word, inst) in cases {
        assert_eq!(decode(word), Some(inst), "decode {word:#010x}");
        assert_eq!(encode(inst), word, "encode {inst:?}");
    }
    // Non-zero rd on HSV or HFENCE is rejected.
    assert_eq!(decode(0x6220_c0f3), None);
    assert_eq!(decode(0x2220_80f3), None);
    assert_eq!(decode(0x6220_80f3), None);
}

#[test]
fn csr_masks_and_interrupt_aliasing() {
    let mut cpu = Cpu::new(0);
    assert!(!cpu.virt);
    assert_ne!(cpu.csr_read(0x301).unwrap() & (1 << 7), 0, "misa.H is set");

    cpu.csr_write(0x600, u64::MAX);
    assert_eq!(
        cpu.csr_read(0x600),
        Some((2 << 32) | VTSR | VTW | VTVM | HU | SPVP | SPV | HSTATUS_GVA)
    );

    cpu.csr_write(0x602, u64::MAX);
    assert_eq!(cpu.csr_read(0x602), Some(0xb1ff));

    cpu.csr_write(0x603, u64::MAX);
    assert_eq!(cpu.csr_read(0x603), Some(0x444));

    // vsstatus has the same writable bits and UXL as sstatus.
    cpu.csr_write(0x200, u64::MAX);
    assert_eq!(
        cpu.csr_read(0x200),
        Some((2 << 32) | SIE | SPIE | SPP | SUM | MXR)
    );

    // vsepc forces bit 0 to 0; vstvec, vsscratch, vscause, vstval, htval, htinst, mtval2, mtinst,
    // hcounteren, henvcfg are plain storage.
    cpu.csr_write(0x241, 0x1235);
    assert_eq!(cpu.csr_read(0x241), Some(0x1234));
    for addr in [
        0x205, 0x240, 0x242, 0x243, 0x34a, 0x34b, 0x606, 0x60a, 0x643, 0x64a,
    ] {
        cpu.csr_write(addr, 0xdead_beef_cafe_babe);
        assert_eq!(cpu.csr_read(addr), Some(0xdead_beef_cafe_babe), "{addr:#x}");
    }
    assert_eq!(cpu.csr_read(0x607), Some(0));
    cpu.csr_write(0x607, u64::MAX);
    assert_eq!(cpu.csr_read(0x607), Some(0));
    assert_eq!(cpu.csr_read(0xe12), Some(0));

    // vsatp and hgatp modes.
    cpu.csr_write(0x280, (8 << 60) | 0x123);
    assert_eq!(cpu.csr_read(0x280), Some((8 << 60) | 0x123));
    cpu.csr_write(0x280, (9 << 60) | 0x456);
    assert_eq!(cpu.csr_read(0x280), Some((8 << 60) | 0x123));

    cpu.csr_write(0x680, u64::MAX);
    assert_eq!(cpu.csr_read(0x680), Some(0));
    cpu.csr_write(0x680, (8 << 60) | (0x3fff << 44) | 0x1237);
    assert_eq!(
        cpu.csr_read(0x680),
        Some((8 << 60) | 0x1234),
        "VMID and PPN[1:0] read 0"
    );

    // hvip, hip, mip, vsip, hie, mie, vsie aliasing.
    let mut cpu = Cpu::new(0);
    assert_eq!(
        cpu.csr_read(0x303),
        Some(0x1444),
        "mideleg always has H_INT"
    );
    cpu.csr_write(0x645, u64::MAX);
    assert_eq!(cpu.csr_read(0x645), Some(0x444));
    assert_eq!(cpu.csr_read(0x644), Some(0x444));
    assert_eq!(cpu.csr_read(0x344), Some(0x444));
    assert_eq!(cpu.csr_read(0x144), Some(0), "sip never shows VS bits");
    assert_eq!(cpu.csr_read(0x244), Some(0), "hideleg is 0 so vsip reads 0");

    cpu.csr_write(0x603, (1 << 2) | (1 << 10));
    assert_eq!(cpu.csr_read(0x244), Some((1 << 1) | (1 << 9)));
    // Clearing bit 1 of vsip clears bit 2 of hvip/hip/mip, while bits 5/9 are read-only in vsip.
    cpu.csr_write(0x244, 0);
    assert_eq!(cpu.csr_read(0x645), Some((1 << 6) | (1 << 10)));
    // Writing hip only changes bit 2 (VSSIP).
    cpu.csr_write(0x644, 1 << 2);
    assert_eq!(cpu.csr_read(0x645), Some(0x444));

    // hie and vsie.
    cpu.csr_write(0x604, u64::MAX);
    assert_eq!(cpu.csr_read(0x604), Some(0x1444));
    assert_eq!(cpu.csr_read(0x304), Some(0x1444));
    assert_eq!(cpu.csr_read(0x104), Some(0));
    assert_eq!(cpu.csr_read(0x204), Some((1 << 1) | (1 << 9)));
    cpu.csr_write(0x204, 0);
    assert_eq!(cpu.csr_read(0x604), Some((1 << 6) | (1 << 12)));
}

#[test]
fn vs_csr_redirection_and_virtual_instruction_table() {
    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::Supervisor;
    cpu.virt = true;

    let pairs = [
        (0x100, 0x200, (2 << 32) | SIE | SUM, SIE | SUM),
        (0x105, 0x205, 0x4000, 0x4000),
        (0x140, 0x240, 0x1111, 0x1111),
        (0x141, 0x241, 0x2222, 0x2222),
        (0x142, 0x242, 0x3333, 0x3333),
        (0x143, 0x243, 0x4444, 0x4444),
        (0x180, 0x280, (8 << 60) | 7, (8 << 60) | 7),
    ];
    for (s_addr, vs_addr, expected, write_val) in pairs {
        cpu.x[2] = write_val;
        let inst = Inst::Csrrw {
            rd: 1,
            rs1: 2,
            csr: s_addr,
        };
        assert_eq!(step_one(&mut cpu, inst), Ok(()));
        assert_eq!(cpu.csr_read(vs_addr), Some(expected));
        // Real HS CSR was not modified (except sstatus has UXL).
        if s_addr != 0x100 {
            assert_eq!(cpu.csr_read(s_addr), Some(0));
        }
    }

    // VTVM traps satp (0x180) in VS as VirtualInstruction.
    cpu.csr_write(0x280, 0);
    cpu.csr_write(0x600, VTVM);
    let satp_rw = Inst::Csrrw {
        rd: 1,
        rs1: 2,
        csr: 0x180,
    };
    assert_eq!(
        step_one(&mut cpu, satp_rw),
        Err(Trap::VirtualInstruction(encode(satp_rw)))
    );
    cpu.csr_write(0x600, 0);

    // Accessing HS/VS CSRs (0x2xx, 0x6xx, 0xe12) directly from VS or VU is VirtualInstruction;
    // from U (virt=false) is IllegalInstruction; M-level CSRs from VS/VU are IllegalInstruction.
    for csr in [0x200, 0x280, 0x600, 0x680, 0xe12] {
        let rd_inst = Inst::Csrrs { rd: 1, rs1: 0, csr };
        let w = encode(rd_inst);
        cpu.virt = true;
        cpu.mode = Mode::Supervisor;
        assert_eq!(
            step_one(&mut cpu, rd_inst),
            Err(Trap::VirtualInstruction(w))
        );
        cpu.mode = Mode::User;
        assert_eq!(
            step_one(&mut cpu, rd_inst),
            Err(Trap::VirtualInstruction(w))
        );
        cpu.virt = false;
        cpu.mode = Mode::User;
        assert_eq!(
            step_one(&mut cpu, rd_inst),
            Err(Trap::IllegalInstruction(w))
        );
        cpu.mode = Mode::Supervisor;
        assert_eq!(step_one(&mut cpu, rd_inst), Ok(()));
    }

    let m_inst = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x300,
    };
    let mw = encode(m_inst);
    cpu.virt = true;
    for mode in [Mode::Supervisor, Mode::User] {
        cpu.mode = mode;
        assert_eq!(
            step_one(&mut cpu, m_inst),
            Err(Trap::IllegalInstruction(mw))
        );
    }

    // From VU, any S-level CSR is VirtualInstruction.
    cpu.virt = true;
    cpu.mode = Mode::User;
    for csr in [
        0x100, 0x104, 0x105, 0x106, 0x140, 0x141, 0x142, 0x143, 0x144, 0x180,
    ] {
        let inst = Inst::Csrrs { rd: 1, rs1: 0, csr };
        assert_eq!(
            step_one(&mut cpu, inst),
            Err(Trap::VirtualInstruction(encode(inst)))
        );
    }

    // Counter CSRs from VS/VU.
    let rdcycle = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0xc00,
    };
    let cw = encode(rdcycle);
    cpu.csr_write(0x306, 0);
    cpu.csr_write(0x606, 7);
    cpu.csr_write(0x106, 7);
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    assert_eq!(
        step_one(&mut cpu, rdcycle),
        Err(Trap::IllegalInstruction(cw))
    );
    cpu.csr_write(0x306, 1);
    cpu.csr_write(0x606, 0);
    assert_eq!(
        step_one(&mut cpu, rdcycle),
        Err(Trap::VirtualInstruction(cw))
    );
    cpu.mode = Mode::User;
    assert_eq!(
        step_one(&mut cpu, rdcycle),
        Err(Trap::VirtualInstruction(cw))
    );
    cpu.csr_write(0x606, 1);
    cpu.csr_write(0x106, 0);
    cpu.mode = Mode::Supervisor;
    assert_eq!(step_one(&mut cpu, rdcycle), Ok(()));
    cpu.mode = Mode::User;
    assert_eq!(
        step_one(&mut cpu, rdcycle),
        Err(Trap::VirtualInstruction(cw))
    );
    cpu.csr_write(0x106, 1);
    assert_eq!(step_one(&mut cpu, rdcycle), Ok(()));
}

#[test]
fn privileged_and_hypervisor_instruction_traps() {
    let mut cpu = Cpu::new(0);
    // WFI
    let wfi = Inst::Wfi;
    let ww = encode(wfi);
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.csr_write(0x300, 0);
    assert_eq!(step_one(&mut cpu, wfi), Err(Trap::VirtualInstruction(ww)));
    cpu.csr_write(0x300, TW);
    assert_eq!(step_one(&mut cpu, wfi), Err(Trap::IllegalInstruction(ww)));
    cpu.mode = Mode::Supervisor;
    assert_eq!(step_one(&mut cpu, wfi), Err(Trap::IllegalInstruction(ww)));
    cpu.csr_write(0x300, 0);
    cpu.csr_write(0x600, VTW);
    assert_eq!(step_one(&mut cpu, wfi), Err(Trap::VirtualInstruction(ww)));
    cpu.csr_write(0x600, 0);
    assert_eq!(step_one(&mut cpu, wfi), Ok(()));

    // SFENCE.VMA
    let sf = Inst::SfenceVma { rs1: 0, rs2: 0 };
    let sfw = encode(sf);
    cpu.virt = true;
    cpu.mode = Mode::User;
    assert_eq!(step_one(&mut cpu, sf), Err(Trap::VirtualInstruction(sfw)));
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x600, VTVM);
    assert_eq!(step_one(&mut cpu, sf), Err(Trap::VirtualInstruction(sfw)));
    cpu.csr_write(0x600, 0);
    assert_eq!(step_one(&mut cpu, sf), Ok(()));

    // HFENCE.VVMA and HFENCE.GVMA
    let vvma = Inst::HfenceVvma { rs1: 0, rs2: 0 };
    let gvma = Inst::HfenceGvma { rs1: 0, rs2: 0 };
    cpu.virt = true;
    for mode in [Mode::Supervisor, Mode::User] {
        cpu.mode = mode;
        assert_eq!(
            step_one(&mut cpu, vvma),
            Err(Trap::VirtualInstruction(encode(vvma)))
        );
        assert_eq!(
            step_one(&mut cpu, gvma),
            Err(Trap::VirtualInstruction(encode(gvma)))
        );
    }
    cpu.virt = false;
    cpu.mode = Mode::User;
    assert_eq!(
        step_one(&mut cpu, vvma),
        Err(Trap::IllegalInstruction(encode(vvma)))
    );
    assert_eq!(
        step_one(&mut cpu, gvma),
        Err(Trap::IllegalInstruction(encode(gvma)))
    );
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x300, TVM);
    assert_eq!(step_one(&mut cpu, vvma), Ok(()));
    assert_eq!(
        step_one(&mut cpu, gvma),
        Err(Trap::IllegalInstruction(encode(gvma)))
    );
    cpu.csr_write(0x300, 0);
    assert_eq!(step_one(&mut cpu, gvma), Ok(()));

    // SRET from VU is VirtualInstruction; from VS with VTSR is VirtualInstruction; from VS with TSR=1, VTSR=0 succeeds.
    let sret = Inst::Sret;
    let srw = encode(sret);
    cpu.virt = true;
    cpu.mode = Mode::User;
    assert_eq!(step_one(&mut cpu, sret), Err(Trap::VirtualInstruction(srw)));
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x600, VTSR);
    assert_eq!(step_one(&mut cpu, sret), Err(Trap::VirtualInstruction(srw)));
    cpu.csr_write(0x600, 0);
    cpu.csr_write(0x300, TSR);
    assert_eq!(step_one(&mut cpu, sret), Ok(()));
    cpu.csr_write(0x300, 0);
}

/// Reference model for G-stage translation (Bare or Sv39x4).
fn ref_translate_g(
    ram: &mut Ram,
    hgatp: u64,
    mstatus: u64,
    gva: u64,
    gpa: u64,
    orig: Access,
    stage: Access,
    hlvx: bool,
) -> Result<u64, Trap> {
    let fault = match orig {
        Access::Fetch => Trap::InstGuestPageFault { gva, gpa },
        Access::Load => Trap::LoadGuestPageFault { gva, gpa },
        Access::Store => Trap::StoreGuestPageFault { gva, gpa },
    };
    if hgatp >> 60 != 8 {
        return Ok(gpa);
    }
    if gpa >> 41 != 0 {
        return Err(fault);
    }
    let mut table = (hgatp & PPN_MASK & !3) << 12;
    for level in (0..3u32).rev() {
        let idx = if level == 2 {
            (gpa >> 30) & 0x7ff
        } else {
            (gpa >> (12 + 9 * level)) & 0x1ff
        };
        let pte_pa = table.wrapping_add(idx << 3);
        let pte = ram.read(pte_pa, 8).map_err(|_| fault)?;
        if pte & PTE_V == 0 || pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0) {
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
        if ppn & ((1u64 << (9 * level)) - 1) != 0 || pte & PTE_U == 0 {
            return Err(fault);
        }
        let ok = if hlvx {
            pte & PTE_X != 0
        } else {
            match stage {
                Access::Fetch => pte & PTE_X != 0,
                Access::Load => pte & PTE_R != 0 || ((mstatus & MXR) != 0 && pte & PTE_X != 0),
                Access::Store => pte & PTE_W != 0,
            }
        };
        if !ok {
            return Err(fault);
        }
        let mut new = pte | PTE_A;
        if stage == Access::Store {
            new |= PTE_D;
        }
        if new != pte {
            ram.write(pte_pa, 8, new).map_err(|_| fault)?;
        }
        let mask = (1u64 << (12 + 9 * level)) - 1;
        return Ok(((ppn << 12) & !mask) | (gpa & mask));
    }
    Err(fault)
}

/// Reference model for full two-stage translation.
fn ref_translate_two_stage(
    ram: &mut Ram,
    vsatp: u64,
    hgatp: u64,
    mstatus: u64,
    vsstatus: u64,
    mode: Mode,
    gva: u64,
    access: Access,
    hlvx: bool,
) -> Result<u64, Trap> {
    let fault = match access {
        Access::Fetch => Trap::InstPageFault(gva),
        Access::Load => Trap::LoadPageFault(gva),
        Access::Store => Trap::StorePageFault(gva),
    };
    let gpa = if vsatp >> 60 != 8 {
        gva
    } else {
        let top = gva >> 38;
        if top != 0 && top != 0x3ff_ffff {
            return Err(fault);
        }
        let mut table = (vsatp & PPN_MASK) << 12;
        let mut leaf_gpa = None;
        for level in (0..3u32).rev() {
            let idx = (gva >> (12 + 9 * level)) & 0x1ff;
            let pte_gpa = table.wrapping_add(idx << 3);
            let pte_pa = ref_translate_g(
                ram,
                hgatp,
                mstatus,
                gva,
                pte_gpa,
                access,
                Access::Load,
                false,
            )?;
            let pte = ram.read(pte_pa, 8).map_err(|_| fault)?;
            if pte & PTE_V == 0 || pte >> 54 != 0 || (pte & PTE_W != 0 && pte & PTE_R == 0) {
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
            if ppn & ((1u64 << (9 * level)) - 1) != 0 {
                return Err(fault);
            }
            let mxr = ((vsstatus | mstatus) & MXR) != 0;
            let ok = if hlvx {
                pte & PTE_X != 0
            } else {
                match access {
                    Access::Fetch => pte & PTE_X != 0,
                    Access::Load => pte & PTE_R != 0 || (mxr && pte & PTE_X != 0),
                    Access::Store => pte & PTE_W != 0,
                }
            };
            if !ok {
                return Err(fault);
            }
            let user = pte & PTE_U != 0;
            let priv_ok = match mode {
                Mode::User => user,
                Mode::Supervisor => !user || (access != Access::Fetch && (vsstatus & SUM) != 0),
                Mode::Machine => true,
            };
            if !priv_ok {
                return Err(fault);
            }
            let mut new = pte | PTE_A;
            if access == Access::Store {
                new |= PTE_D;
            }
            if new != pte {
                let w_pa = ref_translate_g(
                    ram,
                    hgatp,
                    mstatus,
                    gva,
                    pte_gpa,
                    access,
                    Access::Store,
                    false,
                )?;
                ram.write(w_pa, 8, new).map_err(|_| fault)?;
            }
            let mask = (1u64 << (12 + 9 * level)) - 1;
            leaf_gpa = Some(((ppn << 12) & !mask) | (gva & mask));
            break;
        }
        leaf_gpa.ok_or(fault)?
    };
    ref_translate_g(ram, hgatp, mstatus, gva, gpa, access, access, hlvx)
}

fn random_pte(r: &mut Rng, max_ppn: usize, level: u32, is_ptr: bool) -> u64 {
    if r.chance(1, 8) {
        return 0;
    }
    let mut ppn = r.below(max_ppn) as u64;
    if !is_ptr && level > 0 && r.chance(7, 8) {
        ppn &= !((1u64 << (9 * level)) - 1);
    }
    let flags = if is_ptr {
        PTE_V
    } else {
        let mut f = PTE_V;
        if r.chance(3, 4) {
            f |= PTE_R;
        }
        if r.chance(1, 2) {
            f |= PTE_W;
        }
        if r.chance(1, 2) {
            f |= PTE_X;
        }
        if r.chance(3, 4) {
            f |= PTE_U;
        }
        if r.chance(1, 2) {
            f |= PTE_A;
        }
        if r.chance(1, 2) {
            f |= PTE_D;
        }
        f
    };
    let rsvd = if r.chance(1, 32) { 1u64 << 55 } else { 0 };
    rsvd | (ppn << 10) | flags
}

#[test]
fn two_stage_translation_matches_reference_model() {
    forall(400, 0x4859_5031, |r| {
        let mut ram1 = Ram::new(0, 0x20000);
        // Pages 0..4: G-stage root (16 KiB). Page 4: G-stage L1. Page 5: G-stage L0.
        // Page 6: VS-stage L2. Page 7: VS-stage L1. Page 8: VS-stage L0.
        for i in 0..8u64 {
            let pte = if i == 0 && r.chance(3, 4) {
                (4 << 10) | PTE_V
            } else {
                random_pte(r, 32, 2, false)
            };
            ram1.write(i * 8, 8, pte).unwrap();
        }
        for i in 0..8u64 {
            let pte = if i == 0 && r.chance(3, 4) {
                (5 << 10) | PTE_V
            } else {
                random_pte(r, 32, 1, false)
            };
            ram1.write(0x4000 + i * 8, 8, pte).unwrap();
        }
        for i in 0..32u64 {
            let pte = random_pte(r, 32, 0, false);
            ram1.write(0x5000 + i * 8, 8, pte).unwrap();
        }
        for i in 0..8u64 {
            let pte = if i == 0 && r.chance(3, 4) {
                (7 << 10) | PTE_V
            } else {
                random_pte(r, 32, 2, false)
            };
            ram1.write(0x6000 + i * 8, 8, pte).unwrap();
        }
        for i in 0..8u64 {
            let pte = if i == 0 && r.chance(3, 4) {
                (8 << 10) | PTE_V
            } else {
                random_pte(r, 32, 1, false)
            };
            ram1.write(0x7000 + i * 8, 8, pte).unwrap();
        }
        for i in 0..16u64 {
            let pte = random_pte(r, 32, 0, false);
            ram1.write(0x8000 + i * 8, 8, pte).unwrap();
        }
        let mut ram2 = ram1.clone();

        let vsatp = if r.chance(1, 5) { 0 } else { (8 << 60) | 6 };
        let hgatp = if r.chance(1, 5) { 0 } else { 8 << 60 };
        let mstatus = if r.chance(1, 2) { MXR } else { 0 };
        let vsstatus =
            (if r.chance(1, 2) { SUM } else { 0 }) | (if r.chance(1, 2) { MXR } else { 0 });
        let mode = if r.chance(1, 2) {
            Mode::Supervisor
        } else {
            Mode::User
        };

        let mut cpu = Cpu::new(0);
        cpu.virt = true;
        cpu.mode = mode;
        cpu.csr_write(0x280, vsatp);
        cpu.csr_write(0x680, hgatp);
        cpu.csr_write(0x300, mstatus);
        cpu.csr_write(0x200, vsstatus);

        for _ in 0..10 {
            let gva = if r.chance(1, 12) {
                // Non-canonical or wide address
                1u64 << r.range(38, 45)
            } else {
                let l2 = r.below(4) as u64;
                let l1 = r.below(4) as u64;
                let l0 = r.below(16) as u64;
                let off = r.below(0x1000) as u64;
                (l2 << 30) | (l1 << 21) | (l0 << 12) | off
            };
            let access = *r.pick(&[Access::Fetch, Access::Load, Access::Store]);
            let got = cpu.translate(&mut ram1, gva, access);
            let want = ref_translate_two_stage(
                &mut ram2, vsatp, hgatp, mstatus, vsstatus, mode, gva, access, false,
            );
            assert_eq!(got, want, "gva={gva:#x} access={access:?} mode={mode:?}");
        }
    });
}

#[test]
fn hlv_hlvx_hsv_and_trap_gva_fields() {
    let mut ram = Ram::new(0, 0x10000);
    // G-stage root at 0x0000..0x4000, L1 at 0x4000, L0 at 0x5000.
    // VS-stage L2 at 0x6000, L1 at 0x7000, L0 at 0x8000.
    // Guest data page at GPA 0x9000 -> PA 0xa000 (R|W|U|A|D).
    // Guest exec-only page at GPA 0xb000 -> PA 0xc000 (X|U|A).
    ram.write(0, 8, (4 << 10) | PTE_V).unwrap();
    ram.write(0x4000, 8, (5 << 10) | PTE_V).unwrap();
    // Map GPA 0x6000, 0x7000, 0x8000 (VS page tables) with R|W|U.
    for p in [6u64, 7, 8] {
        ram.write(0x5000 + p * 8, 8, (p << 10) | PTE_V | PTE_R | PTE_W | PTE_U)
            .unwrap();
    }
    ram.write(
        0x5000 + 9 * 8,
        8,
        (10 << 10) | PTE_V | PTE_R | PTE_W | PTE_U,
    )
    .unwrap();
    ram.write(0x5000 + 11 * 8, 8, (12 << 10) | PTE_V | PTE_X | PTE_U)
        .unwrap();

    // VS-stage maps GVA 0x1000 -> GPA 0x9000 (R|W|U), GVA 0x2000 -> GPA 0xb000 (X|U).
    ram.write(0x6000, 8, (7 << 10) | PTE_V).unwrap();
    ram.write(0x7000, 8, (8 << 10) | PTE_V).unwrap();
    ram.write(0x8000 + 8, 8, (9 << 10) | PTE_V | PTE_R | PTE_W | PTE_U)
        .unwrap();
    ram.write(0x8000 + 16, 8, (11 << 10) | PTE_V | PTE_X | PTE_U)
        .unwrap();

    ram.write(0xc000, 4, 0xfeed_face).unwrap();

    let mut cpu = Cpu::new(0xf000);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x280, (8 << 60) | 6);
    cpu.csr_write(0x680, 8 << 60);

    // HSV.W and HLV.W / HLV.WU from HS (SPVP = 0 -> VU access).
    cpu.x[1] = 0x1003; // misaligned!
    cpu.x[2] = 0x89ab_cdef;
    place(
        &mut ram,
        0xf000,
        &[
            Inst::Hsv {
                rs1: 1,
                rs2: 2,
                width: 4,
            },
            Inst::Hlv {
                rd: 3,
                rs1: 1,
                width: 4,
                unsigned: false,
                exec: false,
            },
            Inst::Hlv {
                rd: 4,
                rs1: 1,
                width: 4,
                unsigned: true,
                exec: false,
            },
        ],
    );
    cpu.step(&mut ram).unwrap();
    cpu.step(&mut ram).unwrap();
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.x[3], 0xffff_ffff_89ab_cdef);
    assert_eq!(cpu.x[4], 0x0000_0000_89ab_cdef);

    // HLVX.WU on X-only page succeeds, while HLV.WU on X-only page fails with LoadPageFault.
    cpu.x[1] = 0x2000;
    place(
        &mut ram,
        0xf00c,
        &[
            Inst::Hlv {
                rd: 5,
                rs1: 1,
                width: 4,
                unsigned: true,
                exec: true,
            },
            Inst::Hlv {
                rd: 6,
                rs1: 1,
                width: 4,
                unsigned: true,
                exec: false,
            },
        ],
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.x[5], 0xfeed_face);
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadPageFault(0x2000)));

    // U-mode with HU=1 can execute HLV; when it faults on a guest page fault, GVA=1 and htval=gpa>>2.
    cpu.mode = Mode::User;
    cpu.csr_write(0x600, HU);
    cpu.csr_write(0x302, u64::MAX);
    cpu.csr_write(0x105, 0xe000);
    // Map GVA 0x3000 -> GPA 0xd000 (unmapped in G-stage).
    ram.write(0x8000 + 24, 8, (13 << 10) | PTE_V | PTE_R | PTE_U | PTE_A)
        .unwrap();
    cpu.x[1] = 0x3004;
    cpu.pc = 0xf020;
    place(
        &mut ram,
        0xf020,
        &[Inst::Hlv {
            rd: 7,
            rs1: 1,
            width: 4,
            unsigned: false,
            exec: false,
        }],
    );
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::LoadGuestPageFault {
            gva: 0x3004,
            gpa: 0xd004
        })
    );
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert!(!cpu.virt);
    assert_eq!(cpu.csr_read(0x142), Some(21));
    assert_eq!(cpu.csr_read(0x143), Some(0x3004));
    assert_eq!(cpu.csr_read(0x643), Some(0xd004 >> 2));
    assert_ne!(cpu.csr_read(0x600).unwrap() & HSTATUS_GVA, 0);
}

#[test]
fn interrupts_xret_and_vs_guest_execution() {
    let mut cpu = Cpu::new(0);
    let mut ram = Ram::new(0, 0x10000);
    place(
        &mut ram,
        0,
        &[Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 1,
        }],
    );

    // VS interrupt (VSTI = 6) delegated in hideleg goes to VS with vscause = (1<<63)|5 and vstvec + 4*5.
    cpu.mode = Mode::Supervisor;
    cpu.virt = true;
    cpu.csr_write(0x603, 1 << 6);
    cpu.csr_write(0x604, 1 << 6);
    cpu.csr_write(0x645, 1 << 6);
    cpu.csr_write(0x205, 0x2001); // vectored vstvec
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Retired,
        "vsstatus.SIE is 0 in VS, so VSTI is masked"
    );
    cpu.pc = 0;
    cpu.csr_write(0x200, SIE);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(6));
    assert!(cpu.virt);
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.pc, 0x2000 + 4 * 5);
    assert_eq!(cpu.csr_read(0x242), Some((1 << 63) | 5));

    // HS interrupt (SEI = 9) preempts VS even when mstatus.SIE is 0.
    cpu.csr_write(0x645, 0);
    cpu.csr_write(0x303, 1 << 9);
    cpu.csr_write(0x304, 1 << 9);
    cpu.csr_write(0x105, 0x3000);
    cpu.set_pending(9, true);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(9));
    assert!(!cpu.virt);
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.pc, 0x3000);
    assert_ne!(cpu.csr_read(0x600).unwrap() & SPV, 0);
    assert_ne!(cpu.csr_read(0x600).unwrap() & SPVP, 0);

    // Full guest in VS under Sv39x4 on Machine (with DecodeCache + ITLB active):
    // Guest writes 0x1234_5678 to GVA 0x4000_1000 and executes ecall -> traps to HS (cause 10).
    let mut m = Machine::new(Config::default());
    // G-stage root at RAM_BASE + 0x10000 (16 KiB aligned: PPN = 0x80010, bits 1..0 == 0).
    let g_root = RAM_BASE + 0x10000;
    let vs_root = RAM_BASE + 0x14000;
    let guest_code_pa = RAM_BASE + 0x18000;
    let guest_data_pa = RAM_BASE + 0x19000;

    // G-stage: 1 GiB superpage at GPA 0x0000_0000 (index 0) -> PA 0x8000_0000 (RAM_BASE).
    m.board
        .write(
            g_root,
            8,
            ((RAM_BASE >> 12) << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U,
        )
        .unwrap();
    // VS-stage (at GPA 0x14000 -> PA vs_root): 1 GiB superpage at GVA 0x4000_0000 (index 1) -> GPA 0x0000_0000.
    m.board
        .write(vs_root + 8, 8, PTE_V | PTE_R | PTE_W | PTE_X)
        .unwrap();

    // Place guest instructions at guest_code_pa (GPA 0x18000, GVA 0x4001_8000):
    // lui x10, 0x40019; addi x11, x0, 0x42; sd x11, 0(x10); ecall
    let guest_prog = [
        Inst::Lui {
            rd: 10,
            imm: 0x4001_9000,
        },
        Inst::Addi {
            rd: 11,
            rs1: 0,
            imm: 0x42,
        },
        Inst::Sd {
            rs1: 10,
            rs2: 11,
            imm: 0,
        },
        Inst::Ecall,
    ];
    for (i, inst) in guest_prog.iter().enumerate() {
        m.load(guest_code_pa + 4 * i as u64, &encode(*inst).to_le_bytes())
            .unwrap();
    }
    // HS entry at RAM_BASE: sret into VS at sepc = 0x4001_8000.
    m.load(RAM_BASE, &encode(Inst::Sret).to_le_bytes()).unwrap();
    m.cpu.mode = Mode::Supervisor;
    m.cpu.csr_write(0x302, 1 << 10); // delegate VS ecall (10) to HS
    m.cpu.csr_write(0x105, RAM_BASE + 0x100); // stvec
    m.cpu.csr_write(0x141, 0x4001_8000); // sepc
    m.cpu.csr_write(0x300, SPP); // return to S
    m.cpu.csr_write(0x600, SPV); // return with V=1 (VS)
    m.cpu.csr_write(0x680, (8 << 60) | (g_root >> 12));
    m.cpu.csr_write(0x280, (8 << 60) | (0x14000 >> 12));

    assert_eq!(m.step(), Step::Retired); // sret
    assert!(m.cpu.virt);
    assert_eq!(m.cpu.mode, Mode::Supervisor);
    assert_eq!(m.step(), Step::Retired); // lui
    assert_eq!(m.step(), Step::Retired); // addi
    assert_eq!(m.step(), Step::Retired); // sd
    assert_eq!(m.step(), Step::Exception(Trap::Ecall)); // ecall -> HS
    assert!(!m.cpu.virt);
    assert_eq!(m.cpu.mode, Mode::Supervisor);
    assert_eq!(m.cpu.pc, RAM_BASE + 0x100);
    assert_eq!(m.cpu.csr_read(0x142), Some(10));
    assert_eq!(m.board.read(guest_data_pa, 8).unwrap(), 0x42);
}

#[test]
fn trap_routing_to_vs_hs_and_m_sets_all_csr_fields() {
    // 1. Page fault in VU delegated in medeleg and hedeleg -> goes to VS.
    let mut cpu = Cpu::new(0x100);
    let mut ram = Ram::new(0, 0x4000);
    // VS root at 0x1000: map 1 GiB at GVA 0 -> GPA 0 with R|W|X|U.
    ram.write(0x1000, 8, PTE_V | PTE_R | PTE_W | PTE_X | PTE_U)
        .unwrap();
    place(
        &mut ram,
        0x100,
        &[Inst::Lw {
            rd: 1,
            rs1: 2,
            imm: 0,
        }],
    );
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.x[2] = 1u64 << 40; // non-canonical Sv39 -> LoadPageFault(13)
    cpu.csr_write(0x280, (8 << 60) | 1);
    cpu.csr_write(0x302, 1 << 13);
    cpu.csr_write(0x602, 1 << 13);
    cpu.csr_write(0x200, SIE | SPP);
    cpu.csr_write(0x205, 0x500);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::LoadPageFault(1u64 << 40))
    );
    assert!(cpu.virt);
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.pc, 0x500);
    assert_eq!(cpu.csr_read(0x241), Some(0x100));
    assert_eq!(cpu.csr_read(0x242), Some(13));
    assert_eq!(cpu.csr_read(0x243), Some(1u64 << 40));
    let vsst = cpu.csr_read(0x200).unwrap();
    assert_eq!(vsst & SIE, 0);
    assert_eq!(vsst & SPIE, SPIE);
    assert_eq!(vsst & SPP, 0, "came from VU");

    // 2. Guest-page fault in VS delegated in medeleg -> goes to HS (never to VS), sets htval, htinst=0, SPV=1, SPVP=1, GVA=1.
    let mut cpu = Cpu::new(0x200);
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x680, 8 << 60); // G-stage Sv39x4 with empty root at 0 -> InstGuestPageFault
    cpu.csr_write(0x302, 1 << 20);
    cpu.csr_write(0x64a, 0x999);
    cpu.csr_write(0x105, 0x600);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::InstGuestPageFault {
            gva: 0x200,
            gpa: 0x200
        })
    );
    assert!(!cpu.virt);
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.pc, 0x600);
    assert_eq!(cpu.csr_read(0x141), Some(0x200));
    assert_eq!(cpu.csr_read(0x142), Some(20));
    assert_eq!(cpu.csr_read(0x143), Some(0x200));
    assert_eq!(cpu.csr_read(0x643), Some(0x200 >> 2));
    assert_eq!(cpu.csr_read(0x64a), Some(0));
    let hst = cpu.csr_read(0x600).unwrap();
    assert_eq!(hst & (SPV | SPVP | HSTATUS_GVA), SPV | SPVP | HSTATUS_GVA);

    // 3. Guest-page fault in VU not delegated in medeleg -> goes to M, sets mtval2, mtinst=0, MPV=1, MPP=U, GVA=1.
    let mut cpu = Cpu::new(0x204);
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.csr_write(0x680, 8 << 60);
    cpu.csr_write(0x34a, 0x777);
    cpu.csr_write(0x305, 0x700);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::InstGuestPageFault {
            gva: 0x204,
            gpa: 0x204
        })
    );
    assert!(!cpu.virt);
    assert_eq!(cpu.mode, Mode::Machine);
    assert_eq!(cpu.pc, 0x700);
    assert_eq!(cpu.csr_read(0x341), Some(0x204));
    assert_eq!(cpu.csr_read(0x342), Some(20));
    assert_eq!(cpu.csr_read(0x343), Some(0x204));
    assert_eq!(cpu.csr_read(0x34b), Some(0x204 >> 2));
    assert_eq!(cpu.csr_read(0x34a), Some(0));
    let mst = cpu.csr_read(0x300).unwrap();
    assert_eq!(mst & (MPV | MSTATUS_GVA), MPV | MSTATUS_GVA);
    assert_eq!((mst >> 11) & 3, 0);
}

#[test]
fn mret_and_sret_with_virt_transitions() {
    let mut ram = Ram::new(0, 0x1000);
    place(&mut ram, 0, &[Inst::Mret, Inst::Sret, Inst::Sret]);

    // MRET with MPP=S, MPV=1 -> mode=S, virt=true, MPV cleared.
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x300, MPV | MPP_S | MPIE);
    cpu.csr_write(0x341, 4);
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::Supervisor, true, 4));
    assert_eq!(cpu.csr_read(0x300).unwrap() & MPV, 0);
    assert_ne!(cpu.csr_read(0x300).unwrap() & MIE, 0);

    // SRET from VS (at pc=4) -> uses vsstatus/vsepc, keeps virt=true, leaves mstatus/hstatus alone.
    cpu.csr_write(0x200, SPIE); // vsstatus.SPP = 0 (VU)
    cpu.csr_write(0x241, 8);
    cpu.csr_write(0x600, SPV);
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::User, true, 8));
    assert_ne!(cpu.csr_read(0x200).unwrap() & SIE, 0);
    assert_ne!(
        cpu.csr_read(0x600).unwrap() & SPV,
        0,
        "hstatus.SPV untouched by VS sret"
    );

    // MRET with MPP=M, MPV=1 -> mode=M, virt=false, MPV cleared.
    cpu.mode = Mode::Machine;
    cpu.virt = false;
    cpu.pc = 0;
    cpu.csr_write(0x300, MPV | MPP_M);
    cpu.csr_write(0x341, 8);
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::Machine, false, 8));
    assert_eq!(cpu.csr_read(0x300).unwrap() & MPV, 0);

    // SRET from HS with SPV=1, SPP=0 -> mode=U, virt=true, SPV cleared.
    cpu.mode = Mode::Supervisor;
    cpu.virt = false;
    cpu.pc = 4;
    cpu.csr_write(0x600, SPV);
    cpu.csr_write(0x300, SPIE);
    cpu.csr_write(0x141, 0xc);
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::User, true, 0xc));
    assert_eq!(cpu.csr_read(0x600).unwrap() & SPV, 0);
}

#[test]
fn implicit_vs_pte_g_stage_write_permission_and_priority_order() {
    // When VS-stage must set A (or D), the G-stage leaf for the VS page table must have W=1.
    let mut ram = Ram::new(0, 0x10000);
    // G-stage root at 0: 1 GiB leaf at GPA 0 -> PA 0 with R|U (no W!).
    ram.write(0, 8, PTE_V | PTE_R | PTE_U | PTE_A).unwrap();
    // VS-stage root at GPA 0x4000: 1 GiB leaf at GVA 0 -> GPA 0x8000_0000 with R|U and A=0!
    ram.write(0x4000, 8, PTE_V | PTE_R | PTE_U).unwrap();

    let mut cpu = Cpu::new(0);
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.csr_write(0x680, 8 << 60);
    cpu.csr_write(0x280, (8 << 60) | 4);
    // Fails with LoadGuestPageFault on the VS PTE's GPA (0x4000) because A=0 requires G-stage W!
    assert_eq!(
        cpu.translate(&mut ram, 0x123, Access::Load),
        Err(Trap::LoadGuestPageFault {
            gva: 0x123,
            gpa: 0x4000
        })
    );
    // If A is already 1 on the VS PTE, G-stage W is not needed for the VS PTE!
    ram.write(0x4000, 8, PTE_V | PTE_R | PTE_U | PTE_A).unwrap();
    assert_eq!(cpu.translate(&mut ram, 0x123, Access::Load), Ok(0x123));

    // Interrupt priority: VSEI (10) > VSSI (2) > VSTI (6), and non-hideleg goes to HS.
    let mut cpu = Cpu::new(0);
    let mut ram = Ram::new(0, 0x1000);
    place(
        &mut ram,
        0,
        &[Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 1,
        }],
    );
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.csr_write(0x603, (1 << 2) | (1 << 6) | (1 << 10));
    cpu.csr_write(0x604, (1 << 2) | (1 << 6) | (1 << 10));
    cpu.csr_write(0x645, (1 << 2) | (1 << 6) | (1 << 10));
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(10));
    assert_eq!(cpu.csr_read(0x242), Some((1 << 63) | 9));

    // If VSTI (6) is not in hideleg, it goes to HS with scause = (1 << 63) | 6, preempting VSEI (10) in VS.
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.csr_write(0x603, (1 << 2) | (1 << 10)); // 2 and 10 in hideleg; 6 goes to HS
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(6));
    assert!(!cpu.virt);
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.csr_read(0x142), Some((1 << 63) | 6));
}
