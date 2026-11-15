//! Leader check for T33 (decoded-instruction cache). Copied to crates/silicon/tests/cache_leader.rs.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use silicon::inst::{decode, decode_c, encode, AmoOp, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::Bus;

const T1: u8 = 6;
const T2: u8 = 7;
const S0: u8 = 8;
const S1: u8 = 9;
const A0: u8 = 10;
const T3: u8 = 28;
const T4: u8 = 29;
const T5: u8 = 30;
const T6: u8 = 31;

fn pair(bin: &[u8], disk: Vec<u8>) -> (Machine, Machine) {
    let mk = |cache: bool| {
        let mut m = Machine::new(Config {
            decode_cache: cache,
            disk: disk.clone(),
            ..Config::default()
        });
        m.load(RAM_BASE, bin).unwrap();
        m
    };
    (mk(true), mk(false))
}

fn lockstep(a: &mut Machine, b: &mut Machine, max: u64, what: &str) -> Exit {
    for i in 0..max {
        let ea = a.run(1);
        let eb = b.run(1);
        assert_eq!(ea, eb, "{what}: exit differs at step {i}");
        assert!(
            a.cpu == b.cpu,
            "{what}: cpu differs at step {i}:\n on  {:?}\n off {:?}",
            a.cpu,
            b.cpu
        );
        if ea != Exit::OutOfSteps {
            return ea;
        }
    }
    Exit::OutOfSteps
}

fn same_world(a: &mut Machine, b: &mut Machine, ram_window: u64, what: &str) {
    assert_eq!(
        a.board.take_uart_output(),
        b.board.take_uart_output(),
        "{what}: uart"
    );
    assert!(
        a.board.framebuffer() == b.board.framebuffer(),
        "{what}: framebuffer"
    );
    assert!(a.board.disk() == b.board.disk(), "{what}: disk");
    for off in (0..ram_window).step_by(8) {
        let x = a.board.read(RAM_BASE + off, 8).unwrap();
        let y = b.board.read(RAM_BASE + off, 8).unwrap();
        assert_eq!(x, y, "{what}: RAM at +{off:#x}");
    }
}

#[test]
fn leader_fixtures_lockstep() {
    let fx: &[(&str, &[u8], u64)] = &[
        (
            "hello",
            include_bytes!("fixtures/machine/hello.bin"),
            200_000,
        ),
        (
            "timer",
            include_bytes!("fixtures/machine/timer.bin"),
            400_000,
        ),
        (
            "echo_irq",
            include_bytes!("fixtures/machine/echo_irq.bin"),
            200_000,
        ),
        (
            "block",
            include_bytes!("fixtures/machine/block.bin"),
            400_000,
        ),
        ("fb", include_bytes!("fixtures/machine/fb.bin"), 400_000),
        (
            "keys_s",
            include_bytes!("fixtures/machine/keys_s.bin"),
            200_000,
        ),
        ("msi", include_bytes!("fixtures/machine/msi.bin"), 200_000),
        ("rvc", include_bytes!("fixtures/ac/rvc.bin"), 200_000),
        (
            "amo_sem",
            include_bytes!("fixtures/ac/amo_sem.bin"),
            200_000,
        ),
        ("smoke", include_bytes!("fixtures/ac/smoke.bin"), 5_000_000),
    ];
    for (name, bin, max) in fx {
        let disk: Vec<u8> = (0..16 * 512u32)
            .map(|i| (i.wrapping_mul(7) + 3) as u8)
            .collect();
        let (mut a, mut b) = pair(bin, disk);
        for m in [&mut a, &mut b] {
            m.board.uart_input(b"hi\n");
            m.board.press_key(0x1E);
        }
        let e = lockstep(&mut a, &mut b, *max, name);
        same_world(&mut a, &mut b, 0x2_0000, name);
        if ["hello", "rvc", "amo_sem", "smoke"].contains(name) {
            assert_eq!(e, Exit::Pass, "{name} should pass");
        }
    }
}

/// Tiny program builder over silicon's own encoder.
struct B {
    b: Vec<u8>,
}

impl B {
    fn new() -> B {
        B { b: Vec::new() }
    }
    fn at(&self) -> usize {
        self.b.len()
    }
    fn i(&mut self, x: Inst) -> usize {
        let p = self.b.len();
        self.b.extend_from_slice(&encode(x).to_le_bytes());
        p
    }
    fn h(&mut self, h: u16) {
        self.b.extend_from_slice(&h.to_le_bytes());
    }
    fn pad_to(&mut self, off: usize) {
        assert!(self.b.len() <= off, "layout overflow at {off:#x}");
        self.b.resize(off, 0);
    }
    fn put(&mut self, at: usize, x: Inst) {
        self.b[at..at + 4].copy_from_slice(&encode(x).to_le_bytes());
    }
    fn data64(&mut self, at: usize, v: u64) {
        if self.b.len() < at + 8 {
            self.b.resize(at + 8, 0);
        }
        self.b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    /// rd = v using only addi/slli (any u64).
    fn li(&mut self, rd: u8, v: u64) {
        let mut chunks = Vec::new();
        let mut x = v;
        loop {
            chunks.push(x & 0x7ff);
            x >>= 11;
            if x == 0 {
                break;
            }
        }
        chunks.reverse();
        self.i(Inst::Addi {
            rd,
            rs1: 0,
            imm: chunks[0] as i64,
        });
        for c in &chunks[1..] {
            self.i(Inst::Slli {
                rd,
                rs1: rd,
                imm: 11,
            });
            if *c != 0 {
                self.i(Inst::Addi {
                    rd,
                    rs1: rd,
                    imm: *c as i64,
                });
            }
        }
    }
    /// rd = s0 + off
    fn addr(&mut self, rd: u8, off: u64) {
        self.li(rd, off);
        self.i(Inst::Add {
            rd,
            rs1: rd,
            rs2: S0,
        });
    }
    fn pass(&mut self) {
        self.li(T3, 0x10_0000);
        self.li(T4, 0x5555);
        self.i(Inst::Sw {
            rs1: T3,
            rs2: T4,
            imm: 0,
        });
        self.i(Inst::Jal { rd: 0, imm: 0 });
    }
    fn bne_to(&mut self, rs1: u8, rs2: u8, target: usize) {
        let here = self.at() as i64;
        self.i(Inst::Bne {
            rs1,
            rs2,
            imm: target as i64 - here,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Sw,
    Sh2,
    Sb4,
    Sd,
    Amo,
    LrSc,
    Dma,
    Compressed,
    Straddle,
}

fn addi_a0(imm: i64) -> Inst {
    Inst::Addi {
        rd: A0,
        rs1: A0,
        imm,
    }
}
const RET: Inst = Inst::Jalr {
    rd: 0,
    rs1: 1,
    imm: 0,
};

/// A routine at TGT is called 3 times; after the first call the program patches it.
fn smc(kind: Kind) -> (Vec<u8>, Vec<u8>, u64) {
    let tgt = if kind == Kind::Straddle { 0xFFE } else { 0x400 };
    let (old, new) = (encode(addi_a0(1)), encode(addi_a0(100)));
    let mut p = B::new();
    p.i(Inst::Auipc { rd: S0, imm: 0 });
    p.i(Inst::Addi {
        rd: A0,
        rs1: 0,
        imm: 0,
    });
    p.i(Inst::Addi {
        rd: S1,
        rs1: 0,
        imm: 3,
    });
    p.i(Inst::Ld {
        rd: T1,
        rs1: S0,
        imm: 0x200,
    });
    p.addr(T2, tgt as u64);
    let top = p.i(Inst::Jalr {
        rd: 1,
        rs1: T2,
        imm: 0,
    });
    match kind {
        Kind::Sw => {
            p.i(Inst::Sw {
                rs1: T2,
                rs2: T1,
                imm: 0,
            });
        }
        Kind::Sh2 => {
            p.i(Inst::Sh {
                rs1: T2,
                rs2: T1,
                imm: 0,
            });
            p.i(Inst::Srli {
                rd: T4,
                rs1: T1,
                imm: 16,
            });
            p.i(Inst::Sh {
                rs1: T2,
                rs2: T4,
                imm: 2,
            });
        }
        Kind::Sb4 => {
            p.i(Inst::Sb {
                rs1: T2,
                rs2: T1,
                imm: 0,
            });
            for k in 1..4 {
                p.i(Inst::Srli {
                    rd: T4,
                    rs1: T1,
                    imm: 8 * k,
                });
                p.i(Inst::Sb {
                    rs1: T2,
                    rs2: T4,
                    imm: k,
                });
            }
        }
        Kind::Sd => {
            p.i(Inst::Sd {
                rs1: T2,
                rs2: T1,
                imm: 0,
            });
        }
        Kind::Amo => {
            p.i(Inst::AmoW {
                op: AmoOp::Swap,
                rd: 0,
                rs1: T2,
                rs2: T1,
                aq: false,
                rl: false,
            });
        }
        Kind::LrSc => {
            p.i(Inst::LrW {
                rd: T5,
                rs1: T2,
                aq: false,
                rl: false,
            });
            p.i(Inst::ScW {
                rd: T5,
                rs1: T2,
                rs2: T1,
                aq: false,
                rl: false,
            });
        }
        Kind::Dma => {
            p.li(T5, 0x1000_1000);
            p.i(Inst::Sd {
                rs1: T5,
                rs2: 0,
                imm: 0,
            });
            p.i(Inst::Sd {
                rs1: T5,
                rs2: T2,
                imm: 8,
            });
            p.i(Inst::Addi {
                rd: T6,
                rs1: 0,
                imm: 1,
            });
            p.i(Inst::Sw {
                rs1: T5,
                rs2: T6,
                imm: 0x10,
            });
            p.i(Inst::Sw {
                rs1: T5,
                rs2: T6,
                imm: 0x14,
            });
        }
        Kind::Compressed => {
            p.i(Inst::Sh {
                rs1: T2,
                rs2: T1,
                imm: 0,
            });
        }
        Kind::Straddle => {
            p.i(Inst::Sh {
                rs1: T2,
                rs2: T1,
                imm: 2,
            });
        }
    }
    p.i(Inst::Addi {
        rd: S1,
        rs1: S1,
        imm: -1,
    });
    p.bne_to(S1, 0, top);
    p.pass();
    let nv: u64 = match kind {
        Kind::Sd => u64::from(new) | (u64::from(encode(RET)) << 32),
        Kind::Compressed => 0x051d,
        Kind::Straddle => u64::from(new >> 16),
        _ => u64::from(new),
    };
    p.data64(0x200, nv);
    p.pad_to(tgt);
    if kind == Kind::Compressed {
        assert_eq!(decode_c(0x0505), Some(addi_a0(1)));
        assert_eq!(decode_c(0x051d), Some(addi_a0(7)));
        p.h(0x0505);
    } else {
        assert_eq!(old & 0xFFFF, new & 0xFFFF);
        p.i(addi_a0(1));
    }
    p.i(RET);
    let mut disk = vec![0u8; 512];
    disk[..4].copy_from_slice(&new.to_le_bytes());
    disk[4..8].copy_from_slice(&encode(RET).to_le_bytes());
    let want = if kind == Kind::Compressed {
        1 + 2 * 7
    } else {
        1 + 2 * 100
    };
    (p.b, disk, want)
}

#[test]
fn leader_self_modifying_code() {
    use Kind::*;
    for kind in [Sw, Sh2, Sb4, Sd, Amo, LrSc, Dma, Compressed, Straddle] {
        let (img, disk, want) = smc(kind);
        let (mut a, mut b) = pair(&img, disk);
        let what = format!("{kind:?}");
        assert_eq!(
            lockstep(&mut a, &mut b, 10_000, &what),
            Exit::Pass,
            "{what}"
        );
        assert_eq!(
            a.cpu.x[A0 as usize], want,
            "{what}: routine result (cache on)"
        );
        same_world(&mut a, &mut b, 0x2000, &what);
    }
}

#[test]
fn leader_remap_and_physical_rewrite() {
    remap(false);
    remap(true);
}

/// The S-mode code page is remapped after the first ecall and rewritten physically after the second.
fn remap(via_satp: bool) {
    let (v, r, w, x, a, d) = (1u64, 2u64, 4u64, 8u64, 64u64, 128u64);
    let pte = |pa: u64, flags: u64| ((pa >> 12) << 10) | flags;
    let mut p = B::new();
    p.i(Inst::Auipc { rd: S0, imm: 0 });
    p.addr(5, 0x100);
    p.i(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x305,
    });
    p.li(5, (8 << 60) | ((RAM_BASE + 0x1000) >> 12));
    p.i(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x180,
    });
    p.li(T1, 3 << 11);
    p.i(Inst::Csrrc {
        rd: 0,
        rs1: T1,
        csr: 0x300,
    });
    p.li(5, 1 << 11);
    p.i(Inst::Csrrs {
        rd: 0,
        rs1: 5,
        csr: 0x300,
    });
    p.i(Inst::Csrrw {
        rd: 0,
        rs1: 0,
        csr: 0x341,
    });
    p.i(Inst::Addi {
        rd: A0,
        rs1: 0,
        imm: 0,
    });
    p.i(Inst::Addi {
        rd: S1,
        rs1: 0,
        imm: 0,
    });
    p.i(Inst::Mret);
    p.pad_to(0x100);
    // M-mode handler for the S-mode ecalls
    p.i(Inst::Addi {
        rd: S1,
        rs1: S1,
        imm: 1,
    });
    p.i(Inst::Addi {
        rd: 5,
        rs1: 0,
        imm: 1,
    });
    let b1 = p.i(Inst::Bne {
        rs1: S1,
        rs2: 5,
        imm: 0,
    });
    if via_satp {
        // switch to a second root table, no sfence.vma: the uncached path has no TLB
        p.li(T1, (8 << 60) | ((RAM_BASE + 0x6000) >> 12));
        p.i(Inst::Csrrw {
            rd: 0,
            rs1: T1,
            csr: 0x180,
        });
    } else {
        p.li(T1, pte(RAM_BASE + 0x5000, v | r | x | a));
        p.addr(T2, 0x3000);
        p.i(Inst::Sd {
            rs1: T2,
            rs2: T1,
            imm: 0,
        });
        p.i(Inst::SfenceVma { rs1: 0, rs2: 0 });
    }
    p.i(Inst::Csrrw {
        rd: 0,
        rs1: 0,
        csr: 0x341,
    });
    p.i(Inst::Mret);
    let second = p.at();
    p.put(
        b1,
        Inst::Bne {
            rs1: S1,
            rs2: 5,
            imm: (second - b1) as i64,
        },
    );
    p.i(Inst::Addi {
        rd: 5,
        rs1: 0,
        imm: 2,
    });
    let b2 = p.i(Inst::Bne {
        rs1: S1,
        rs2: 5,
        imm: 0,
    });
    p.li(T1, u64::from(encode(addi_a0(1000))));
    p.addr(T2, 0x5000);
    p.i(Inst::Sw {
        rs1: T2,
        rs2: T1,
        imm: 0,
    });
    p.i(Inst::Csrrw {
        rd: 0,
        rs1: 0,
        csr: 0x341,
    });
    p.i(Inst::Mret);
    let third = p.at();
    p.put(
        b2,
        Inst::Bne {
            rs1: S1,
            rs2: 5,
            imm: (third - b2) as i64,
        },
    );
    p.pass();
    // page tables: root at +0x1000, L1 at +0x2000, L0 at +0x3000
    p.data64(0x1000, pte(RAM_BASE + 0x2000, v));
    p.data64(0x1000 + 2 * 8, pte(0x8000_0000, v | r | w | x | a | d));
    p.data64(0x2000, pte(RAM_BASE + 0x3000, v));
    p.data64(0x3000, pte(RAM_BASE + 0x4000, v | r | x | a));
    p.pad_to(0x4000);
    p.i(addi_a0(1));
    p.i(Inst::Ecall);
    p.pad_to(0x5000);
    p.i(addi_a0(100));
    p.i(Inst::Ecall);
    p.data64(0x6000, pte(RAM_BASE + 0x7000, v));
    p.data64(0x7000, pte(RAM_BASE + 0x8000, v));
    p.data64(0x8000, pte(RAM_BASE + 0x5000, v | r | x | a));
    let what = format!("remap via_satp={via_satp}");
    let (mut ma, mut mb) = pair(&p.b, Vec::new());
    assert_eq!(lockstep(&mut ma, &mut mb, 20_000, &what), Exit::Pass);
    assert_eq!(ma.cpu.x[A0 as usize], 1101, "{what}: S-mode saw stale code");
    same_world(&mut ma, &mut mb, 0x9000, &what);
}

#[test]
fn leader_random_self_modifying_streams() {
    let mut r = Rng::new(0x33_0000_0001);
    let mut changed = 0;
    for prog in 0..300 {
        let mut img = vec![0u8; 0x1000];
        let mut off = 0;
        while off + 2 <= img.len() {
            if r.chance(1, 4) || off + 4 > img.len() {
                let h = loop {
                    let h = r.u64() as u16;
                    if h & 3 != 3 && decode_c(h).is_some() {
                        break h;
                    }
                };
                img[off..off + 2].copy_from_slice(&h.to_le_bytes());
                off += 2;
            } else {
                let w = if r.chance(1, 6) {
                    let (rs1, rs2) = (1 + r.below(31) as u8, r.below(32) as u8);
                    let imm = r.range(0, 128) as i64 - 64;
                    encode(match r.below(4) {
                        0 => Inst::Sb { rs1, rs2, imm },
                        1 => Inst::Sh { rs1, rs2, imm },
                        2 => Inst::Sw { rs1, rs2, imm },
                        _ => Inst::Sd { rs1, rs2, imm },
                    })
                } else {
                    loop {
                        let w = r.u64() as u32 | 3;
                        if decode(w).is_some() {
                            break w;
                        }
                    }
                };
                img[off..off + 4].copy_from_slice(&w.to_le_bytes());
                off += 4;
            }
        }
        let (mut a, mut b) = pair(&img, Vec::new());
        let mtvec = RAM_BASE + 4 * r.below(0x400) as u64;
        let mut regs = [0u64; 32];
        for x in regs.iter_mut().skip(1) {
            let o = r.below(0x1000) as u64;
            *x = RAM_BASE + if r.chance(1, 2) { o & !7 } else { o };
        }
        for m in [&mut a, &mut b] {
            m.cpu.csr_write(0x305, mtvec);
            m.cpu.x = regs;
        }
        let what = format!("random program {prog}");
        lockstep(&mut a, &mut b, 3000, &what);
        same_world(&mut a, &mut b, 0x2000, &what);
        let now: Vec<u8> = (0..0x1000u64)
            .map(|o| a.board.read(RAM_BASE + o, 1).unwrap() as u8)
            .collect();
        if now != img {
            changed += 1;
        }
    }
    assert!(
        changed > 100,
        "only {changed} programs rewrote their own code"
    );
}
