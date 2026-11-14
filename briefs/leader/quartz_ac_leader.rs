//! Leader check for T13a (A and C extensions). Oracles: LLVM 19 via rustc (fixtures/ac, built by
//! briefs/leader/build_quartz_ac.sh): 2500 compressed halfwords with the 32-bit word LLVM gives
//! for the same instruction, 1200 A-extension words; two self-checking programs; a rustc-built
//! Rust program (compressed code and atomics from LLVM and core) whose answer the host computes
//! from the same source. Plus a reference model for random AMOs and fetch at the end of memory.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use quartz::inst::{decode, decode_c, encode, AmoOp, Inst};
use quartz::machine::{Config, Exit, Machine, RAM_BASE};
use quartz::{Bus, Cpu, Fault, Ram, Trap};

mod smoke {
    include!("fixtures/ac/smoke_core.rs");
}

#[test]
fn leader_compressed_decode_matches_llvm() {
    let text = include_str!("fixtures/ac/c_pairs.txt");
    let mut bad = Vec::new();
    let mut n = 0;
    for line in text.lines() {
        let h = u16::from_str_radix(&line[0..4], 16).unwrap();
        let w = u32::from_str_radix(&line[5..13], 16).unwrap();
        let want = decode(w);
        assert!(want.is_some(), "LLVM word not decoded: {line}");
        if decode_c(h) != want {
            bad.push(format!(
                "{line}\n   got {:?}\n  want {:?}",
                decode_c(h),
                want
            ));
        }
        n += 1;
    }
    assert_eq!(n, 2500);
    assert!(
        bad.is_empty(),
        "{} of {n} differ:\n{}",
        bad.len(),
        bad[..bad.len().min(8)].join("\n")
    );
}

#[test]
fn leader_compressed_reserved_and_fp_forms_are_none() {
    for h in [
        0x0000u16, // all zero
        0x0004,    // c.addi4spn, nzuimm 0
        0x4002,    // c.lwsp rd 0
        0x6002,    // c.ldsp rd 0
        0x8002,    // c.jr rs1 0
        0x6101,    // c.addi16sp, nzimm 0
        0x6281,    // c.lui x5, nzimm 0
        0x2001,    // c.addiw rd 0
        0x2005,    // c.addiw rd 0
        0x8000,    // reserved quadrant 0 funct3 100
        0x2000,    // c.fld (no D)
        0xa000,    // c.fsd
        0x2002,    // c.fldsp
        0xa002,    // c.fsdsp
        0x0003,    // low bits 11: not compressed
        0xffff,
    ] {
        assert_eq!(decode_c(h), None, "{h:#06x}");
    }
}

fn amo_inst(f: &[&str]) -> Inst {
    let n = |i: usize| f[i].parse::<u8>().unwrap();
    let (rd, rs1, rs2, aq, rl) = (n(2), n(3), n(4), f[5] == "1", f[6] == "1");
    let d = f[1] == "d";
    let op = match f[0] {
        "lr" if d => return Inst::LrD { rd, rs1, aq, rl },
        "lr" => return Inst::LrW { rd, rs1, aq, rl },
        "sc" if d => {
            return Inst::ScD {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            }
        }
        "sc" => {
            return Inst::ScW {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            }
        }
        "swap" => AmoOp::Swap,
        "add" => AmoOp::Add,
        "xor" => AmoOp::Xor,
        "and" => AmoOp::And,
        "or" => AmoOp::Or,
        "min" => AmoOp::Min,
        "max" => AmoOp::Max,
        "minu" => AmoOp::Minu,
        "maxu" => AmoOp::Maxu,
        other => panic!("{other}"),
    };
    if d {
        Inst::AmoD {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    } else {
        Inst::AmoW {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    }
}

#[test]
fn leader_amo_encodings_match_llvm() {
    let mut n = 0;
    for line in include_str!("fixtures/ac/amo.txt").lines() {
        let parts: Vec<&str> = line.split(' ').collect();
        let w = u32::from_str_radix(parts[0], 16).unwrap();
        let want = amo_inst(&parts[1..]);
        assert_eq!(decode(w), Some(want), "{line}");
        assert_eq!(encode(want), w, "{line}");
        n += 1;
    }
    assert_eq!(n, 1200);
}

fn boot(bin: &[u8]) -> Machine {
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, bin).unwrap();
    m
}

#[test]
fn leader_compressed_program() {
    let mut m = boot(include_bytes!("fixtures/ac/rvc.bin"));
    assert_eq!(m.run(100_000), Exit::Pass);
}

#[test]
fn leader_atomic_program() {
    let mut m = boot(include_bytes!("fixtures/ac/amo_sem.bin"));
    assert_eq!(m.run(100_000), Exit::Pass);
}

#[test]
fn leader_rustc_built_program() {
    let mut m = boot(include_bytes!("fixtures/ac/smoke.bin"));
    assert_eq!(m.run(5_000_000), Exit::Pass);
    let h = smoke::smoke();
    let want = format!("rust ok {h:016x} {}\n", h % 1000);
    assert_eq!(String::from_utf8_lossy(&m.board.take_uart_output()), want);
}

/// Memory only in [0x1000, 0x2000).
struct Edge(Ram);
impl Bus for Edge {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64, Fault> {
        if addr < 0x1000 || addr + u64::from(size) > 0x2000 {
            return Err(Fault::Unmapped(addr));
        }
        self.0.read(addr, size)
    }
    fn write(&mut self, addr: u64, size: u8, value: u64) -> Result<(), Fault> {
        if addr < 0x1000 || addr + u64::from(size) > 0x2000 {
            return Err(Fault::Unmapped(addr));
        }
        self.0.write(addr, size, value)
    }
}

#[test]
fn leader_fetch_at_the_end_of_memory() {
    // c.addi a0, 1 in the last halfword runs; the next fetch faults at 0x2000.
    let mut bus = Edge(Ram::new(0x1000, 0x1000));
    bus.0.load(0x1ffe, &0x0505u16.to_le_bytes()).unwrap();
    let mut cpu = Cpu::new(0x1ffe);
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_eq!((cpu.x[10], cpu.pc), (1, 0x2000));
    assert_eq!(cpu.step(&mut bus), Err(Trap::FetchFault(0x2000)));
    // A 32-bit instruction whose second half is outside memory: the fault names that half.
    let mut bus = Edge(Ram::new(0x1000, 0x1000));
    bus.0.load(0x1ffe, &[0x13, 0x05]).unwrap(); // low half of addi a0, a0, ...
    let mut cpu = Cpu::new(0x1ffe);
    assert_eq!(cpu.step(&mut bus), Err(Trap::FetchFault(0x2000)));
    assert_eq!(cpu.pc, 0x1ffe);
    // Jumps to 2 mod 4 are fine now; odd targets lose bit 0 (jalr) as before.
    let mut bus = Edge(Ram::new(0x1000, 0x1000));
    let jal = encode(Inst::Jal { rd: 1, imm: 6 });
    bus.0.load(0x1000, &jal.to_le_bytes()).unwrap();
    let mut cpu = Cpu::new(0x1000);
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_eq!((cpu.pc, cpu.x[1]), (0x1006, 0x1004));
    // c.jalr links pc + 2.
    let mut bus = Edge(Ram::new(0x1000, 0x1000));
    bus.0.load(0x1100, &0x9282u16.to_le_bytes()).unwrap(); // c.jalr t0
    let mut cpu = Cpu::new(0x1100);
    cpu.x[5] = 0x1202;
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_eq!((cpu.pc, cpu.x[1]), (0x1202, 0x1102));
}

fn amo_ref(op: AmoOp, d: bool, mem: u64, src: u64) -> u64 {
    let (a, b) = if d {
        (mem, src)
    } else {
        (
            mem as u32 as i32 as i64 as u64,
            src as u32 as i32 as i64 as u64,
        )
    };
    let r = match op {
        AmoOp::Swap => b,
        AmoOp::Add => a.wrapping_add(b),
        AmoOp::Xor => a ^ b,
        AmoOp::And => a & b,
        AmoOp::Or => a | b,
        AmoOp::Min => (a as i64).min(b as i64) as u64,
        AmoOp::Max => (a as i64).max(b as i64) as u64,
        AmoOp::Minu if d => a.min(b),
        AmoOp::Maxu if d => a.max(b),
        AmoOp::Minu => u64::from((a as u32).min(b as u32)),
        AmoOp::Maxu => u64::from((a as u32).max(b as u32)),
    };
    if d {
        r
    } else {
        u64::from(r as u32)
    }
}

#[test]
fn leader_random_amos_match_reference() {
    const OPS: [AmoOp; 9] = [
        AmoOp::Swap,
        AmoOp::Add,
        AmoOp::Xor,
        AmoOp::And,
        AmoOp::Or,
        AmoOp::Min,
        AmoOp::Max,
        AmoOp::Minu,
        AmoOp::Maxu,
    ];
    let mut r = Rng::new(0x13a_a3_2026);
    let special = [
        0,
        1,
        u64::MAX,
        0x8000_0000,
        0x7fff_ffff,
        0xffff_ffff,
        1 << 63,
        (1 << 63) - 1,
    ];
    for i in 0..4000 {
        let mut val = || {
            if r.chance(1, 3) {
                *r.pick(&special)
            } else {
                r.u64()
            }
        };
        let (mem, src, d) = (val(), val(), i % 2 == 0);
        let op = OPS[i % 9];
        let (rd, rs1, rs2) = (10, 11, 12);
        let inst = if d {
            Inst::AmoD {
                op,
                rd,
                rs1,
                rs2,
                aq: i % 3 == 0,
                rl: i % 5 == 0,
            }
        } else {
            Inst::AmoW {
                op,
                rd,
                rs1,
                rs2,
                aq: false,
                rl: true,
            }
        };
        let mut bus = Ram::new(0x1000, 0x1000);
        bus.load(0x1000, &encode(inst).to_le_bytes()).unwrap();
        bus.write(0x1800, 8, mem).unwrap();
        let mut cpu = Cpu::new(0x1000);
        cpu.x[11] = 0x1800;
        cpu.x[12] = src;
        assert_eq!(cpu.step(&mut bus), Ok(()), "{inst:?}");
        let old = if d {
            mem
        } else {
            mem as u32 as i32 as i64 as u64
        };
        let new = amo_ref(op, d, mem, src);
        let want_mem = if d { new } else { (mem & !0xffff_ffff) | new };
        assert_eq!(cpu.x[10], old, "{inst:?} mem={mem:#x} src={src:#x}");
        assert_eq!(
            bus.read(0x1800, 8).unwrap(),
            want_mem,
            "{inst:?} mem={mem:#x} src={src:#x}"
        );
        assert_eq!(cpu.pc, 0x1004);
    }
}

#[test]
fn leader_lr_sc_and_faults() {
    let prog = [
        encode(Inst::LrD {
            rd: 10,
            rs1: 11,
            aq: false,
            rl: false,
        }),
        encode(Inst::ScD {
            rd: 12,
            rs1: 11,
            rs2: 13,
            aq: false,
            rl: false,
        }),
        encode(Inst::ScD {
            rd: 14,
            rs1: 11,
            rs2: 13,
            aq: false,
            rl: false,
        }),
        encode(Inst::AmoW {
            op: AmoOp::Add,
            rd: 10,
            rs1: 15,
            rs2: 13,
            aq: false,
            rl: false,
        }),
        encode(Inst::LrW {
            rd: 10,
            rs1: 15,
            aq: false,
            rl: false,
        }),
        encode(Inst::AmoD {
            op: AmoOp::Swap,
            rd: 10,
            rs1: 16,
            rs2: 13,
            aq: false,
            rl: false,
        }),
        encode(Inst::LrD {
            rd: 10,
            rs1: 16,
            aq: false,
            rl: false,
        }),
    ];
    let mut bus = Ram::new(0x1000, 0x1000);
    for (i, w) in prog.iter().enumerate() {
        bus.load(0x1000 + 4 * i as u64, &w.to_le_bytes()).unwrap();
    }
    bus.write(0x1800, 8, 41).unwrap();
    let mut cpu = Cpu::new(0x1000);
    cpu.x[11] = 0x1800;
    cpu.x[13] = 99;
    cpu.x[15] = 0x1802; // misaligned
    cpu.x[16] = 0x9000; // unmapped
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_eq!(cpu.x[10], 41);
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_eq!((cpu.x[12], bus.read(0x1800, 8).unwrap()), (0, 99));
    assert_eq!(cpu.step(&mut bus), Ok(()));
    assert_ne!(cpu.x[14], 0, "second SC must fail");
    assert_eq!(cpu.step(&mut bus), Err(Trap::StoreMisaligned(0x1802)));
    cpu.pc += 4;
    assert_eq!(cpu.step(&mut bus), Err(Trap::LoadMisaligned(0x1802)));
    cpu.pc += 4;
    assert_eq!(cpu.step(&mut bus), Err(Trap::StoreFault(0x9000)));
    cpu.pc += 4;
    assert_eq!(cpu.step(&mut bus), Err(Trap::LoadFault(0x9000)));
}
