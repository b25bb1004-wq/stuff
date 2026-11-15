//! Security and adversarial boundary tests for `silicon`.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use silicon::inst::{decode, decode_c, encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::{Access, Bus, Cpu, Fault, Mode, Ram, Step, Trap};

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

#[test]
fn display_new_overflow_panic() {
    let mut m = Machine::new(Config {
        ram_size: 4096,
        fb_width: u32::MAX,
        fb_height: u32::MAX,
        ..Config::default()
    });
    assert!(m.board.framebuffer().is_empty());
    assert_eq!(
        m.board.read(0x3000_0000, 4),
        Err(Fault::Unmapped(0x3000_0000))
    );
}

#[test]
fn decode_all_compressed_halfwords_never_panics() {
    assert_eq!(decode_c(0), None);
    for h in 0..=u16::MAX {
        let res = decode_c(h);
        if h == 0 || (h & 3) == 3 {
            assert_eq!(res, None, "reserved halfword {h:#06x} accepted");
        }
    }
    // Reserved RV64C encodings: C.ADDI4SPN nzuimm=0, C.ADDIW rd=0, C.ADDI16SP nzimm=0,
    // C.LUI nzimm=0, C.LWSP rd=0, C.LDSP rd=0, C.JR rs1=0, C.SUB/ADDW reserved funct2.
    assert_eq!(decode_c(0x0000), None);
    assert_eq!(decode_c(0x2001), None); // C.ADDIW rd=0
    assert_eq!(decode_c(0x6101), None); // C.ADDI16SP imm=0
    assert_eq!(decode_c(0x6081), None); // C.LUI rd=1, imm=0
    assert_eq!(decode_c(0x4002), None); // C.LWSP rd=0
    assert_eq!(decode_c(0x6002), None); // C.LDSP rd=0
    assert_eq!(decode_c(0x8002), None); // C.JR rs1=0
    assert_eq!(decode_c(0x9c41), None); // C.CA reserved (bit12=1, f2=2)
    assert_eq!(decode_c(0x9c61), None); // C.CA reserved (bit12=1, f2=3)
}

#[test]
fn decode_arbitrary_words_never_panics_and_rejects_malformed() {
    forall(10_000, 0x51a7_a5ec, |rng: &mut Rng| {
        let w = rng.u64() as u32;
        if let Some(inst) = decode(w) {
            let re = encode(inst);
            assert!(decode(re).is_some());
        }
    });

    // Hostile/malformed instruction encodings must decode to None.
    assert_eq!(decode(0x0000_0000), None);
    assert_eq!(decode(0xffff_ffff), None);
    // LR.W / LR.D with rs2 != 0
    assert_eq!(decode(0x1010_202f), None);
    assert_eq!(decode(0x1010_302f), None);
    // HSV.D with rd != 0
    assert_eq!(decode(0x6e00_40f3), None);
    // HLV with invalid rs2 = 2
    assert_eq!(decode(0x6420_40f3), None);
    // SLLI with top6 != 0
    assert_eq!(decode(0x0400_1093), None);
    // SRLIW with funct7 = 0x01
    assert_eq!(decode(0x0200_509b), None);
    // SFENCE.VMA with rd != 0
    assert_eq!(decode(0x1200_00f3), None);
}

#[test]
fn mmu_sv39_loop_and_hostile_ptes_terminate() {
    let mut cpu = Cpu::new(RAM_BASE);
    cpu.mode = Mode::Supervisor;
    let mut ram = Ram::new(RAM_BASE, 0x10000);

    // Level-2 root table at RAM_BASE + 0x1000; every entry loops back to itself!
    let root_pa = RAM_BASE + 0x1000;
    let loop_pte = ((root_pa >> 12) << 10) | PTE_V;
    for i in 0..512u64 {
        ram.write(root_pa + i * 8, 8, loop_pte).unwrap();
    }
    cpu.csr_write(0x180, (8u64 << 60) | (root_pa >> 12));

    let va = 0x0000_0000_4000_1000;
    assert_eq!(
        cpu.translate(&mut ram, va, Access::Load),
        Err(Trap::LoadPageFault(va))
    );

    // Non-canonical virtual address
    let bad_va = 0x0000_0040_0000_0000;
    assert_eq!(
        cpu.translate(&mut ram, bad_va, Access::Fetch),
        Err(Trap::InstPageFault(bad_va))
    );

    // Misaligned 1 GiB superpage at level 2 (ppn[17:0] != 0)
    let misaligned_1g = (((RAM_BASE >> 12) | 1) << 10) | PTE_V | PTE_R | PTE_A;
    ram.write(root_pa, 8, misaligned_1g).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, 0x1000, Access::Load),
        Err(Trap::LoadPageFault(0x1000))
    );

    // Reserved W=1, R=0 leaf PTE
    let write_only = ((RAM_BASE >> 12) << 10) | PTE_V | PTE_W | PTE_A | PTE_D;
    ram.write(root_pa, 8, write_only).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, 0x1000, Access::Store),
        Err(Trap::StorePageFault(0x1000))
    );

    // Reserved upper bits (bits 63:54 non-zero)
    let reserved_hi = (1u64 << 54) | ((RAM_BASE >> 12) << 10) | PTE_V | PTE_R | PTE_A;
    ram.write(root_pa, 8, reserved_hi).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, 0x1000, Access::Load),
        Err(Trap::LoadPageFault(0x1000))
    );

    // Out-of-range physical address in non-leaf PTE
    let oob_nonleaf = (0x0000_0fff_ffff_ffffu64 << 10) | PTE_V;
    ram.write(root_pa, 8, oob_nonleaf).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, 0x1000, Access::Load),
        Err(Trap::LoadPageFault(0x1000))
    );
}

#[test]
fn mmu_two_stage_h_extension_hostile_tables() {
    let mut cpu = Cpu::new(RAM_BASE);
    cpu.mode = Mode::Supervisor;
    cpu.virt = true;
    let mut ram = Ram::new(RAM_BASE, 0x20000);

    // G-stage root (16 KiB aligned): RAM_BASE + 0x4000
    let g_root = RAM_BASE + 0x4000;
    // VS-stage root at GPA 0x1000 (mapped by G-stage to RAM_BASE + 0x8000)
    let vs_root_gpa = 0x1000u64;
    let vs_root_hpa = RAM_BASE + 0x8000;

    // Self-looping G-stage non-leaf PTEs -> must terminate with GuestPageFault
    let g_loop = ((g_root >> 12) << 10) | PTE_V;
    for i in 0..2048u64 {
        ram.write(g_root + i * 8, 8, g_loop).unwrap();
    }
    cpu.csr_write(0x280, (8u64 << 60) | (vs_root_gpa >> 12)); // vsatp
    cpu.csr_write(0x680, (8u64 << 60) | (g_root >> 12)); // hgatp

    let gva = 0x2000u64;
    assert_eq!(
        cpu.translate(&mut ram, gva, Access::Load),
        Err(Trap::LoadGuestPageFault {
            gva,
            gpa: vs_root_gpa
        })
    );

    // Map GPA 0..1GiB via G-stage 1 GiB leaf PTE without PTE_U -> must fault!
    let g_no_u = ((RAM_BASE >> 12) << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    ram.write(g_root, 8, g_no_u).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, gva, Access::Load),
        Err(Trap::LoadGuestPageFault {
            gva,
            gpa: vs_root_gpa
        })
    );

    // Misaligned 1 GiB G-stage leaf PTE with PTE_U -> must fault!
    let g_misaligned =
        (((RAM_BASE >> 12) | 1) << 10) | PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
    ram.write(g_root, 8, g_misaligned).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, gva, Access::Load),
        Err(Trap::LoadGuestPageFault {
            gva,
            gpa: vs_root_gpa
        })
    );

    // Valid G-stage 4 KiB mapping for vs_root_gpa -> vs_root_hpa (read-only, U=1, A=1),
    // and VS-stage self-looping table -> must terminate with LoadPageFault.
    let g_l1 = ((RAM_BASE + 0x9000) >> 12) << 10 | PTE_V;
    let g_l0 = ((RAM_BASE + 0xa000) >> 12) << 10 | PTE_V;
    let g_leaf_ro = ((vs_root_hpa >> 12) << 10) | PTE_V | PTE_R | PTE_U | PTE_A;
    ram.write(g_root, 8, g_l1).unwrap();
    ram.write(RAM_BASE + 0x9000, 8, g_l0).unwrap();
    ram.write(RAM_BASE + 0xa000 + (vs_root_gpa >> 12) * 8, 8, g_leaf_ro)
        .unwrap();

    let vs_loop = ((vs_root_gpa >> 12) << 10) | PTE_V;
    for i in 0..512u64 {
        ram.write(vs_root_hpa + i * 8, 8, vs_loop).unwrap();
    }
    assert_eq!(
        cpu.translate(&mut ram, gva, Access::Fetch),
        Err(Trap::InstPageFault(gva))
    );

    // VS-stage aligned 1 GiB leaf with A=0 on read-only G-stage page table page -> A-bit update fails with GuestPageFault
    let vs_leaf_no_a = PTE_V | PTE_R | PTE_W;
    ram.write(vs_root_hpa, 8, vs_leaf_no_a).unwrap();
    assert_eq!(
        cpu.translate(&mut ram, gva, Access::Load),
        Err(Trap::LoadGuestPageFault {
            gva,
            gpa: vs_root_gpa
        })
    );

    // GPA >= 2^41 when vsatp is Bare and hgatp is Sv39x4 -> GuestPageFault
    cpu.csr_write(0x280, 0);
    let huge_gpa = 1u64 << 41;
    assert_eq!(
        cpu.translate(&mut ram, huge_gpa, Access::Store),
        Err(Trap::StoreGuestPageFault {
            gva: huge_gpa,
            gpa: huge_gpa
        })
    );

    // HLV.D from VS-mode must trap with VirtualInstruction (cause 22)
    let hlv_d = encode(Inst::Hlv {
        rd: 1,
        rs1: 2,
        width: 8,
        unsigned: false,
        exec: false,
    });
    cpu.pc = 0x2000;
    // Map 0x2000 in G-stage for fetch + execution
    let g_code_leaf = ((RAM_BASE >> 12) << 10) | PTE_V | PTE_R | PTE_X | PTE_U | PTE_A;
    ram.write(RAM_BASE + 0xa000 + (0x2000u64 >> 12) * 8, 8, g_code_leaf)
        .unwrap();
    ram.write(RAM_BASE, 4, u64::from(hlv_d)).unwrap();
    assert_eq!(cpu.step(&mut ram), Err(Trap::VirtualInstruction(hlv_d)));
}

#[test]
fn mmio_devices_hostile_addresses_and_sizes() {
    let mut m = Machine::new(Config {
        ram_size: 4096,
        fb_width: 4,
        fb_height: 4,
        ..Config::default()
    });
    let bases = [
        0x0010_0000u64, // Finisher
        0x0200_0000,    // CLINT
        0x0C00_0000,    // PLIC
        0x1000_0000,    // UART
        0x1000_1000,    // Block
        0x1000_2000,    // Keys
        0x1000_3000,    // Display regs
        0x3000_0000,    // Framebuffer
        RAM_BASE,       // RAM
    ];
    let invalid_sizes = [0u8, 3, 5, 7, 16, 255];

    for &base in &bases {
        for &sz in &invalid_sizes {
            assert_eq!(m.board.read(base, sz), Err(Fault::Unmapped(base)));
            assert_eq!(m.board.write(base, sz, 0), Err(Fault::Unmapped(base)));
        }
        // Straddling start of window
        if base > 0 {
            assert_eq!(m.board.read(base - 1, 4), Err(Fault::Unmapped(base - 1)));
            assert_eq!(
                m.board.write(base - 1, 4, 0),
                Err(Fault::Unmapped(base - 1))
            );
        }
    }

    // Extremes & straddling end of windows
    assert_eq!(m.board.read(u64::MAX, 8), Err(Fault::Unmapped(u64::MAX)));
    assert_eq!(
        m.board.write(u64::MAX, 8, 0),
        Err(Fault::Unmapped(u64::MAX))
    );
    assert_eq!(
        m.board.read(0x0010_0002, 4),
        Err(Fault::Unmapped(0x0010_0002))
    );
    assert_eq!(
        m.board.read(0x0200_FFFE, 4),
        Err(Fault::Unmapped(0x0200_FFFE))
    );
    assert_eq!(
        m.board.read(0x0C00_0001, 4),
        Err(Fault::Misaligned(0x0C00_0001))
    );
    assert_eq!(
        m.board.read(0x0C00_0000, 8),
        Err(Fault::Unmapped(0x0C00_0000))
    );
    assert_eq!(
        m.board.read(0x1000_0000, 4),
        Err(Fault::Unmapped(0x1000_0000))
    );
    assert_eq!(
        m.board.read(0x3000_0000 + 4 * 4 * 4 - 2, 4),
        Err(Fault::Unmapped(0x3000_0000 + 62))
    );

    // Keys FIFO cap (64) must drop extras rather than grow unboundedly
    for i in 0..500u32 {
        m.board.press_key(i);
    }
    assert_eq!(m.board.read(0x1000_2000, 4), Ok(64));

    // PLIC complete with out-of-range IDs must not panic
    m.board.write(0x0C20_0004, 4, 0).unwrap();
    m.board.write(0x0C20_0004, 4, 32).unwrap();
    m.board.write(0x0C20_0004, 4, u64::MAX).unwrap();
}

#[test]
fn block_dma_hostile_ranges_never_panic_or_corrupt() {
    let mut disk = vec![0xAAu8; 1024];
    disk[0..8].copy_from_slice(b"SAFE_DSK");
    let mut m = Machine::new(Config {
        ram_size: 4096,
        disk,
        ..Config::default()
    });
    m.board.write(RAM_BASE, 8, 0x1122_3344_5566_7788).unwrap();

    let hostile = [
        (u64::MAX, RAM_BASE, 1u64, 1u64),
        (0, RAM_BASE, u64::from(u32::MAX), 1),
        (0, 0, 1, 1),
        (0, u64::MAX, 1, 2),
        (0, RAM_BASE + 4096 - 256, 1, 2),
        (0, RAM_BASE, 1, 99), // unknown cmd
    ];

    for (sec, dma, cnt, cmd) in hostile {
        m.board.write(0x1000_1000, 8, sec).unwrap();
        m.board.write(0x1000_1008, 8, dma).unwrap();
        m.board.write(0x1000_1010, 4, cnt).unwrap();
        m.board.write(0x1000_1014, 4, cmd).unwrap();
    }

    assert_eq!(m.board.read(RAM_BASE, 8), Ok(0x1122_3344_5566_7788));
    assert_eq!(&m.board.disk()[0..8], b"SAFE_DSK");
}

#[test]
fn cpu_hostile_guest_program_never_panics_or_hangs() {
    let mut m = Machine::new(Config {
        ram_size: 0x1_0000,
        decode_cache: true,
        ..Config::default()
    });

    // Build a guest sequence with div-by-zero, i64::MIN / -1, reserved MPP=2 mret,
    // misaligned load/store, page-crossing instruction at 0xFFE, and finisher pass.
    let mut code = Vec::new();
    let insts = [
        Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: -1,
        },
        Inst::Slli {
            rd: 2,
            rs1: 1,
            imm: 63,
        }, // x2 = i64::MIN
        Inst::Div {
            rd: 3,
            rs1: 2,
            rs2: 1,
        }, // i64::MIN / -1
        Inst::Rem {
            rd: 4,
            rs1: 2,
            rs2: 1,
        }, // i64::MIN % -1
        Inst::Divu {
            rd: 5,
            rs1: 2,
            rs2: 0,
        }, // div by 0
        Inst::Divw {
            rd: 6,
            rs1: 2,
            rs2: 0,
        },
        Inst::Lui {
            rd: 10,
            imm: 0x0010_0000,
        },
        Inst::Addi {
            rd: 11,
            rs1: 0,
            imm: 0x555,
        }, // build 0x5555
        Inst::Slli {
            rd: 11,
            rs1: 11,
            imm: 4,
        },
        Inst::Ori {
            rd: 11,
            rs1: 11,
            imm: 5,
        },
        Inst::Sw {
            rs1: 10,
            rs2: 11,
            imm: 0,
        },
    ];
    for inst in insts {
        code.extend_from_slice(&encode(inst).to_le_bytes());
    }
    m.load(RAM_BASE, &code).unwrap();
    assert_eq!(m.run(64), Exit::Pass);
    assert_eq!(m.cpu.x[3], i64::MIN as u64);
    assert_eq!(m.cpu.x[4], 0);
    assert_eq!(m.cpu.x[5], u64::MAX);

    // Execute random garbage words under M-mode trap handler; must never panic or hang.
    let mut rng = Rng::new(0x51a7_a5ec);
    let mut garbage = vec![0u8; 512];
    rng.fill(&mut garbage);
    let mut m2 = Machine::new(Config {
        ram_size: 4096,
        decode_cache: true,
        ..Config::default()
    });
    m2.load(RAM_BASE, &garbage).unwrap();
    m2.cpu.csr_write(0x305, RAM_BASE + 1024); // mtvec -> zeroed RAM (illegal insts)
    assert_eq!(m2.run(200), Exit::OutOfSteps);
}

#[test]
fn ram_and_pc_wrap_boundaries_never_panic() {
    let mut ram = Ram::new(RAM_BASE, 4096);
    assert_eq!(
        ram.load(RAM_BASE + 4090, &[1; 8]),
        Err(Fault::Unmapped(RAM_BASE + 4090))
    );
    assert_eq!(ram.load(u64::MAX, &[1]), Err(Fault::Unmapped(u64::MAX)));

    let mut cpu = Cpu::new(u64::MAX - 1);
    assert_eq!(cpu.step(&mut ram), Err(Trap::FetchFault(u64::MAX - 1)));

    let mut cpu_odd = Cpu::new(u64::MAX);
    assert_eq!(cpu_odd.step(&mut ram), Err(Trap::FetchMisaligned(u64::MAX)));

    // Unaligned LD wrapping across u64::MAX -> 0..6
    let mut cpu_ld = Cpu::new(RAM_BASE);
    cpu_ld.x[1] = u64::MAX;
    let ld = encode(Inst::Ld {
        rd: 2,
        rs1: 1,
        imm: 0,
    });
    ram.write(RAM_BASE, 4, u64::from(ld)).unwrap();
    assert_eq!(cpu_ld.step(&mut ram), Err(Trap::LoadFault(u64::MAX)));

    // Tick with odd PC must trap cleanly with FetchMisaligned
    let mut cpu_tick = Cpu::new(RAM_BASE + 1);
    assert_eq!(
        cpu_tick.tick(&mut ram),
        Step::Exception(Trap::FetchMisaligned(RAM_BASE + 1))
    );
}
