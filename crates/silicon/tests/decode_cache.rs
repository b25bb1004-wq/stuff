//! Lockstep tests verifying `decode_cache: true` matches `decode_cache: false` on every step.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use assay::forall;
use silicon::inst::{decode, encode, AmoOp, Inst};
use silicon::machine::{Config, Machine, RAM_BASE};
use silicon::Step;

fn lockstep_step(on: &mut Machine, off: &mut Machine) -> (Step, Step) {
    let s_on = on.step();
    let s_off = off.step();
    assert_eq!(s_on, s_off, "step mismatch at pc={:#x}", off.cpu.pc);
    assert_eq!(on.cpu, off.cpu, "cpu mismatch at pc={:#x}", off.cpu.pc);
    (s_on, s_off)
}

fn run_lockstep_prog(prog: &[u8], disk: Vec<u8>, ram_size: usize, steps: usize) {
    let mut on = Machine::new(Config {
        ram_size,
        disk: disk.clone(),
        decode_cache: true,
        ..Config::default()
    });
    let mut off = Machine::new(Config {
        ram_size,
        disk,
        decode_cache: false,
        ..Config::default()
    });
    on.load(RAM_BASE, prog).unwrap();
    off.load(RAM_BASE, prog).unwrap();
    for _ in 0..steps {
        lockstep_step(&mut on, &mut off);
    }
    assert_eq!(on.board.take_uart_output(), off.board.take_uart_output());
    assert_eq!(on.board.framebuffer(), off.board.framebuffer());
    assert_eq!(on.board.disk(), off.board.disk());
}

fn asm(insts: &[Inst]) -> Vec<u8> {
    let mut out = Vec::with_capacity(insts.len() * 4);
    for &inst in insts {
        out.extend_from_slice(&encode(inst).to_le_bytes());
    }
    out
}

fn li32(rd: u8, val: u32) -> [Inst; 2] {
    let lo = (val & 0xFFF) as i32;
    let lo_sext = (lo << 20) >> 20;
    let hi = val.wrapping_sub(lo_sext as u32) as i32 as i64;
    [
        Inst::Lui { rd, imm: hi },
        Inst::Addi {
            rd,
            rs1: rd,
            imm: i64::from(lo_sext),
        },
    ]
}

#[test]
fn lockstep_all_fixtures() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for sub in ["tests/fixtures/machine", "tests/fixtures/ac"] {
        let dir = manifest.join(sub);
        if !dir.exists() {
            continue;
        }
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "bin"))
            .collect();
        entries.sort();
        for path in entries {
            let bin = std::fs::read(&path).unwrap();
            let mut on = Machine::new(Config {
                decode_cache: true,
                ..Config::default()
            });
            let mut off = Machine::new(Config {
                decode_cache: false,
                ..Config::default()
            });
            on.load(RAM_BASE, &bin).unwrap();
            off.load(RAM_BASE, &bin).unwrap();
            for _ in 0..200_000 {
                let (s_on, _) = lockstep_step(&mut on, &mut off);
                if matches!(s_on, Step::Wfi) {
                    break;
                }
            }
            assert_eq!(
                on.board.take_uart_output(),
                off.board.take_uart_output(),
                "uart mismatch on {}",
                path.display()
            );
            assert_eq!(on.board.framebuffer(), off.board.framebuffer());
            assert_eq!(on.board.disk(), off.board.disk());
        }
    }
}

/// Self-modifying code test: calls routine at `target_off`, patches it using `patch_insts`,
/// then calls it again.
fn smc_test(patch_insts: &[Inst], target_val: u32) {
    let mut prog = Vec::new();
    // 0: jal ra, +32 (calls routine, initially ADDI x10, x0, 1; JALR x0, ra, 0)
    // We'll place the routine after the main body.
    prog.extend_from_slice(&[Inst::Jal { rd: 1, imm: 0 }]); // placeholder 0
    let [a, b] = li32(5, (RAM_BASE + 128) as u32);
    prog.push(a);
    prog.push(b);
    let [c, d] = li32(6, target_val);
    prog.push(c);
    prog.push(d);
    prog.extend_from_slice(patch_insts);
    prog.push(Inst::Jal { rd: 1, imm: 0 }); // placeholder 1
    prog.push(Inst::Jal { rd: 0, imm: 0 }); // spin

    let mut bytes = asm(&prog);
    let routine_off = 128usize;
    bytes.resize(routine_off + 16, 0);
    // Patch jal offsets
    let call1 = encode(Inst::Jal {
        rd: 1,
        imm: routine_off as i64,
    });
    bytes[0..4].copy_from_slice(&call1.to_le_bytes());
    let call2_idx = (prog.len() - 2) * 4;
    let call2 = encode(Inst::Jal {
        rd: 1,
        imm: (routine_off - call2_idx) as i64,
    });
    bytes[call2_idx..call2_idx + 4].copy_from_slice(&call2.to_le_bytes());

    let init = encode(Inst::Addi {
        rd: 10,
        rs1: 0,
        imm: 1,
    });
    let ret = encode(Inst::Jalr {
        rd: 0,
        rs1: 1,
        imm: 0,
    });
    bytes[routine_off..routine_off + 4].copy_from_slice(&init.to_le_bytes());
    bytes[routine_off + 4..routine_off + 8].copy_from_slice(&ret.to_le_bytes());

    run_lockstep_prog(&bytes, Vec::new(), 4096, 40);
}

#[test]
fn self_modifying_all_store_kinds() {
    let patched = encode(Inst::Addi {
        rd: 10,
        rs1: 0,
        imm: 42,
    });

    // SW
    smc_test(
        &[Inst::Sw {
            rs1: 5,
            rs2: 6,
            imm: 0,
        }],
        patched,
    );

    // Two SH
    smc_test(
        &[
            Inst::Sh {
                rs1: 5,
                rs2: 6,
                imm: 0,
            },
            Inst::Srli {
                rd: 7,
                rs1: 6,
                imm: 16,
            },
            Inst::Sh {
                rs1: 5,
                rs2: 7,
                imm: 2,
            },
        ],
        patched,
    );

    // Four SB
    smc_test(
        &[
            Inst::Sb {
                rs1: 5,
                rs2: 6,
                imm: 0,
            },
            Inst::Srli {
                rd: 7,
                rs1: 6,
                imm: 8,
            },
            Inst::Sb {
                rs1: 5,
                rs2: 7,
                imm: 1,
            },
            Inst::Srli {
                rd: 7,
                rs1: 6,
                imm: 16,
            },
            Inst::Sb {
                rs1: 5,
                rs2: 7,
                imm: 2,
            },
            Inst::Srli {
                rd: 7,
                rs1: 6,
                imm: 24,
            },
            Inst::Sb {
                rs1: 5,
                rs2: 7,
                imm: 3,
            },
        ],
        patched,
    );

    // SD
    smc_test(
        &[Inst::Sd {
            rs1: 5,
            rs2: 6,
            imm: 0,
        }],
        patched,
    );

    // AMOSWAP.W
    smc_test(
        &[Inst::AmoW {
            op: AmoOp::Swap,
            rd: 7,
            rs1: 5,
            rs2: 6,
            aq: false,
            rl: false,
        }],
        patched,
    );

    // LR.W + SC.W
    smc_test(
        &[
            Inst::LrW {
                rd: 7,
                rs1: 5,
                aq: false,
                rl: false,
            },
            Inst::ScW {
                rd: 8,
                rs1: 5,
                rs2: 6,
                aq: false,
                rl: false,
            },
        ],
        patched,
    );
}

#[test]
fn self_modifying_compressed_store_and_routine() {
    // C.SW x11, 0(x10) is 0xC10C
    let c_sw: u16 = 0xC10C;
    assert_eq!(
        silicon::inst::decode_c(c_sw),
        Some(Inst::Sw {
            rs1: 10,
            rs2: 11,
            imm: 0
        })
    );
    // C.ADDI x12, 1 is 0x0605; C.ADDI x12, 9 is 0x0625; C.JR ra is 0x8082
    let c_addi_1: u16 = 0x0605;
    let c_addi_9: u16 = 0x0625;
    let c_jr_ra: u16 = 0x8082;

    let mut prog = Vec::new();
    // Call routine at offset 64
    prog.extend_from_slice(&encode(Inst::Jal { rd: 1, imm: 64 }).to_le_bytes());
    let [a, b] = li32(10, (RAM_BASE + 64) as u32);
    prog.extend_from_slice(&encode(a).to_le_bytes());
    prog.extend_from_slice(&encode(b).to_le_bytes());
    let patched_word = u32::from(c_addi_9) | (u32::from(c_jr_ra) << 16);
    let [c, d] = li32(11, patched_word);
    prog.extend_from_slice(&encode(c).to_le_bytes());
    prog.extend_from_slice(&encode(d).to_le_bytes());
    prog.extend_from_slice(&c_sw.to_le_bytes());
    let call2_off = prog.len();
    prog.extend_from_slice(
        &encode(Inst::Jal {
            rd: 1,
            imm: 64 - call2_off as i64,
        })
        .to_le_bytes(),
    );
    prog.extend_from_slice(&encode(Inst::Jal { rd: 0, imm: 0 }).to_le_bytes());
    prog.resize(64, 0);
    prog.extend_from_slice(&c_addi_1.to_le_bytes());
    prog.extend_from_slice(&c_jr_ra.to_le_bytes());

    run_lockstep_prog(&prog, Vec::new(), 4096, 40);
}

#[test]
fn dma_overwrites_executed_code() {
    let mut disk = vec![0u8; 512];
    let new_inst = encode(Inst::Addi {
        rd: 10,
        rs1: 0,
        imm: 99,
    });
    let ret = encode(Inst::Jalr {
        rd: 0,
        rs1: 1,
        imm: 0,
    });
    disk[0..4].copy_from_slice(&new_inst.to_le_bytes());
    disk[4..8].copy_from_slice(&ret.to_le_bytes());

    let mut prog = Vec::new();
    prog.push(Inst::Jal { rd: 1, imm: 512 });
    // Trigger block DMA read of sector 0 to RAM_BASE + 512
    let [b0, b1] = li32(5, 0x1000_1000);
    prog.extend_from_slice(&[b0, b1]);
    let [d0, d1] = li32(6, (RAM_BASE + 512) as u32);
    prog.extend_from_slice(&[d0, d1]);
    prog.push(Inst::Sd {
        rs1: 5,
        rs2: 0,
        imm: 0x00,
    }); // sector = 0
    prog.push(Inst::Sd {
        rs1: 5,
        rs2: 6,
        imm: 0x08,
    }); // dma_addr
    prog.push(Inst::Addi {
        rd: 7,
        rs1: 0,
        imm: 1,
    });
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 7,
        imm: 0x10,
    }); // count = 1
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 7,
        imm: 0x14,
    }); // cmd = 1 (disk -> RAM)
    let call2_off = (prog.len() * 4) as i64;
    prog.push(Inst::Jal {
        rd: 1,
        imm: 512 - call2_off,
    });
    prog.push(Inst::Jal { rd: 0, imm: 0 });

    let mut bytes = asm(&prog);
    bytes.resize(520, 0);
    let old_inst = encode(Inst::Addi {
        rd: 10,
        rs1: 0,
        imm: 1,
    });
    bytes[512..516].copy_from_slice(&old_inst.to_le_bytes());
    bytes[516..520].copy_from_slice(&ret.to_le_bytes());

    run_lockstep_prog(&bytes, disk, 8192, 60);
}

#[test]
fn code_straddling_page_boundary() {
    let mut bytes = vec![0u8; 8192];
    let jmp = encode(Inst::Jal { rd: 0, imm: 0xFFE });
    bytes[0..4].copy_from_slice(&jmp.to_le_bytes());
    let straddle = encode(Inst::Addi {
        rd: 10,
        rs1: 10,
        imm: 7,
    });
    let spin = encode(Inst::Jal { rd: 0, imm: 0 });
    bytes[0xFFE..0x1002].copy_from_slice(&straddle.to_le_bytes());
    bytes[0x1002..0x1006].copy_from_slice(&spin.to_le_bytes());

    run_lockstep_prog(&bytes, Vec::new(), 8192, 20);
}

#[test]
fn sv39_remap_with_sfence_vma() {
    // Set up Sv39 identity map for RAM_BASE (0x8000_0000 -> 0x8000_0000, gigapage)
    // and remap virtual page 0x1000_0000 from PA 0x8001_0000 to PA 0x8001_1000.
    let l2_pa = RAM_BASE + 0x4000;
    let l1_pa = RAM_BASE + 0x5000;
    let l0_pa = RAM_BASE + 0x6000;
    let code_a = RAM_BASE + 0x1_0000;
    let code_b = RAM_BASE + 0x1_1000;

    let mut mem = vec![0u8; 0x2_0000];
    let write_u64 = |buf: &mut [u8], pa: u64, val: u64| {
        let off = (pa - RAM_BASE) as usize;
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    };
    let write_u32 = |buf: &mut [u8], pa: u64, val: u32| {
        let off = (pa - RAM_BASE) as usize;
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };

    // L2[2] (VA 0x8000_0000): 1 GiB leaf RWXAD
    let pte_ram = ((RAM_BASE >> 12) << 10) | 0xCF;
    write_u64(&mut mem, l2_pa + 2 * 8, pte_ram);
    // L2[0] -> L1
    write_u64(&mut mem, l2_pa, ((l1_pa >> 12) << 10) | 0x01);
    // L1[128] (VA 0x1000_0000) -> L0
    write_u64(&mut mem, l1_pa + 128 * 8, ((l0_pa >> 12) << 10) | 0x01);
    // L0[0] -> code_a (RWXAD)
    let pte_a = ((code_a >> 12) << 10) | 0xCF;
    let pte_b = ((code_b >> 12) << 10) | 0xCF;
    write_u64(&mut mem, l0_pa, pte_a);

    let ret = encode(Inst::Jalr {
        rd: 0,
        rs1: 1,
        imm: 0,
    });
    write_u32(
        &mut mem,
        code_a,
        encode(Inst::Addi {
            rd: 10,
            rs1: 0,
            imm: 11,
        }),
    );
    write_u32(&mut mem, code_a + 4, ret);
    write_u32(
        &mut mem,
        code_b,
        encode(Inst::Addi {
            rd: 10,
            rs1: 0,
            imm: 22,
        }),
    );
    write_u32(&mut mem, code_b + 4, ret);

    // M-mode setup -> drop to S-mode at RAM_BASE + 0x1000
    let satp_val = (8u64 << 60) | (l2_pa >> 12);
    let mut m_prog = Vec::new();
    let [s0, s1] = li32(5, (satp_val & 0xFFFF_FFFF) as u32);
    m_prog.extend_from_slice(&[s0, s1]);
    let [s2, s3] = li32(6, (satp_val >> 32) as u32);
    m_prog.extend_from_slice(&[s2, s3]);
    m_prog.push(Inst::Slli {
        rd: 6,
        rs1: 6,
        imm: 32,
    });
    m_prog.push(Inst::Add {
        rd: 5,
        rs1: 5,
        rs2: 6,
    });
    m_prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x180,
    }); // satp
        // MPP = 1 (S-mode) in mstatus (0x800)
    m_prog.push(Inst::Lui { rd: 5, imm: 0x1000 });
    m_prog.push(Inst::Addi {
        rd: 5,
        rs1: 5,
        imm: -0x800,
    });
    m_prog.push(Inst::Csrrs {
        rd: 0,
        rs1: 5,
        csr: 0x300,
    });
    let [e0, e1] = li32(5, (RAM_BASE + 0x1000) as u32);
    m_prog.extend_from_slice(&[e0, e1]);
    m_prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x341,
    }); // mepc
    m_prog.push(Inst::Mret);

    let m_bytes = asm(&m_prog);
    mem[0..m_bytes.len()].copy_from_slice(&m_bytes);

    // S-mode program at RAM_BASE + 0x1000:
    // Call VA 0x1000_0000, remap L0[0] to code_b, sfence.vma, call VA 0x1000_0000 again.
    let mut s_prog = Vec::new();
    let [v0, v1] = li32(8, 0x1000_0000);
    s_prog.extend_from_slice(&[v0, v1]);
    s_prog.push(Inst::Jalr {
        rd: 1,
        rs1: 8,
        imm: 0,
    });
    let [p0, p1] = li32(9, l0_pa as u32);
    s_prog.extend_from_slice(&[p0, p1]);
    let [pb0, pb1] = li32(11, pte_b as u32);
    s_prog.extend_from_slice(&[pb0, pb1]);
    s_prog.push(Inst::Sd {
        rs1: 9,
        rs2: 11,
        imm: 0,
    });
    s_prog.push(Inst::SfenceVma { rs1: 0, rs2: 0 });
    s_prog.push(Inst::Jalr {
        rd: 1,
        rs1: 8,
        imm: 0,
    });
    s_prog.push(Inst::Jal { rd: 0, imm: 0 });

    let s_bytes = asm(&s_prog);
    mem[0x1000..0x1000 + s_bytes.len()].copy_from_slice(&s_bytes);

    run_lockstep_prog(&mem, Vec::new(), 0x2_0000, 100);
}

#[test]
fn random_self_modifying_streams() {
    forall(60, 0xCA00_5EED, |rng| {
        let mut prog = vec![0u8; 1024];
        for i in 0..256 {
            let word = loop {
                let w = rng.u64() as u32;
                if let Some(inst) = decode(w) {
                    if !matches!(
                        inst,
                        Inst::Ecall | Inst::Ebreak | Inst::Wfi | Inst::Mret | Inst::Sret
                    ) {
                        break w;
                    }
                }
            };
            prog[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut on = Machine::new(Config {
            ram_size: 4096,
            decode_cache: true,
            ..Config::default()
        });
        let mut off = Machine::new(Config {
            ram_size: 4096,
            decode_cache: false,
            ..Config::default()
        });
        on.load(RAM_BASE, &prog).unwrap();
        off.load(RAM_BASE, &prog).unwrap();
        for r in 1..32 {
            let addr = RAM_BASE + rng.range(0, 512);
            on.cpu.x[r] = addr;
            off.cpu.x[r] = addr;
        }
        for _ in 0..400 {
            lockstep_step(&mut on, &mut off);
        }
    });
}
