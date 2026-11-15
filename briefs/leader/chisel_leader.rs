//! Leader check for T21 (chisel assembler). Copied to crates/chisel/tests/chisel_leader.rs.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use chisel::assemble;
use silicon::inst::{encode, AmoOp, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use std::time::Instant;

const BASE: u64 = 0x8000_0000;
const CORPUS: &str = include_str!("fixtures/corpus.s");
const CORPUS_BIN: &[u8] = include_bytes!("fixtures/corpus.bin");
const MAP: &str = include_str!("fixtures/corpus.map");
const SYMS: &str = include_str!("fixtures/corpus.syms");
const C_PAIRS: &str = include_str!("../../silicon/tests/fixtures/ac/c_pairs.txt");
const AMO: &str = include_str!("../../silicon/tests/fixtures/ac/amo.txt");
const COMMON: &str = include_str!("../../silicon/tests/fixtures/machine/common.inc");

const ABI: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

fn reg(s: &str) -> Option<u8> {
    if s == "fp" {
        return Some(8);
    }
    if let Some(n) = s.strip_prefix('x') {
        return n.parse().ok().filter(|&n: &u8| n < 32);
    }
    ABI.iter().position(|&a| a == s).map(|i| i as u8)
}

fn asm_at(src: &str, base: u64) -> Vec<u8> {
    match assemble(src, base) {
        Ok(o) => o.bytes,
        Err(e) => panic!("assemble failed for {src:?}: {e}"),
    }
}

fn asm(src: &str) -> Vec<u8> {
    asm_at(src, BASE)
}

/// Statement text of a corpus line: labels and comment removed.
fn body(line: &str) -> &str {
    let mut s = line.split('#').next().unwrap().trim();
    while let Some(i) = s.find(':') {
        let head = &s[..i];
        if !head.is_empty()
            && head
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
        {
            s = s[i + 1..].trim_start();
        } else {
            break;
        }
    }
    s
}

fn parse_li(text: &str) -> Option<(u8, i64)> {
    let rest = text.strip_prefix("li ")?;
    let (r, v) = rest.split_once(',')?;
    let v = v.trim();
    let (neg, mag) = v.strip_prefix('-').map_or((false, v), |m| (true, m));
    let x = match mag.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok()? as i64,
        None => mag.parse::<i64>().ok()?,
    };
    Some((reg(r.trim())?, if neg { x.wrapping_neg() } else { x }))
}

/// Runs straight-line 4-byte code from zeroed registers; returns the register file.
fn run_regs(code: &[u8]) -> [u64; 32] {
    assert!(code.len().is_multiple_of(4), "expected 4-byte instructions");
    let mut m = Machine::new(Config {
        ram_size: 64 * 1024,
        ..Config::default()
    });
    m.load(RAM_BASE, code).unwrap();
    assert_eq!(m.run(code.len() as u64 / 4), Exit::OutOfSteps);
    assert_eq!(
        m.cpu.pc,
        RAM_BASE + code.len() as u64,
        "code did not run straight"
    );
    m.cpu.x
}

#[test]
fn leader_corpus_equals_llvm() {
    let out = assemble(CORPUS, BASE).unwrap_or_else(|e| panic!("corpus: {e}"));
    let lines: Vec<&str> = CORPUS.lines().collect();
    let map: Vec<(usize, usize)> = MAP
        .lines()
        .map(|l| {
            let mut it = l.split_whitespace();
            (
                it.next().unwrap().parse().unwrap(),
                it.next().unwrap().parse().unwrap(),
            )
        })
        .collect();
    let n = CORPUS_BIN.len();
    let mut masked = vec![false; n];
    let mut li2 = Vec::new();
    for (k, &(off, ln)) in map.iter().enumerate() {
        let end = map.get(k + 1).map_or(n, |m| m.0);
        if let Some((rd, v)) = parse_li(body(lines[ln - 1])) {
            if end - off == 8 {
                masked[off..end].iter_mut().for_each(|b| *b = true);
                li2.push((off, ln, rd, v));
            }
        }
    }
    for i in 0..n.min(out.bytes.len()) {
        if !masked[i] && out.bytes[i] != CORPUS_BIN[i] {
            let (off, ln) = map[map.partition_point(|m| m.0 <= i) - 1];
            panic!(
                "corpus differs at +{i:#x}, line {ln}: {:?}\n llvm   {:02x?}\n chisel {:02x?}",
                lines[ln - 1],
                &CORPUS_BIN[off..(off + 8).min(n)],
                &out.bytes[off..(off + 8).min(out.bytes.len())]
            );
        }
    }
    assert_eq!(out.bytes.len(), n, "corpus size");
    assert!(li2.len() > 20, "too few li cases: {}", li2.len());
    for (off, ln, rd, v) in li2 {
        let x = run_regs(&out.bytes[off..off + 8]);
        for (r, &val) in x.iter().enumerate() {
            let want = if r == rd as usize && rd != 0 {
                v as u64
            } else {
                0
            };
            assert_eq!(val, want, "line {ln}: {:?} register x{r}", lines[ln - 1]);
        }
    }
    for l in SYMS.lines() {
        let (name, addr) = l.split_once(' ').unwrap();
        let addr = u64::from_str_radix(addr, 16).unwrap();
        let got = out.symbols.iter().find(|s| s.0 == name);
        assert_eq!(got.map(|s| s.1), Some(addr), "symbol {name}");
    }
}

#[test]
fn leader_llvm_encoding_tables() {
    let mut n = 0;
    for l in C_PAIRS.lines() {
        let h = u16::from_str_radix(&l[0..4], 16).unwrap();
        let w = u32::from_str_radix(&l[5..13], 16).unwrap();
        let (c, e) = l[14..].split_once(" | ").unwrap();
        assert_eq!(asm(c), h.to_le_bytes(), "{c}");
        assert_eq!(asm(e), w.to_le_bytes(), "{e}");
        n += 1;
    }
    assert_eq!(n, 2500);
    for l in AMO.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let word = u32::from_str_radix(f[0], 16).unwrap();
        let r = |i: usize| ABI[f[i].parse::<usize>().unwrap()];
        let suf = match (f[6], f[7]) {
            ("0", "0") => "",
            ("1", "0") => ".aq",
            ("0", "1") => ".rl",
            _ => ".aqrl",
        };
        let src = match f[1] {
            "lr" => format!("lr.{}{suf} {}, ({})", f[2], r(3), r(4)),
            "sc" => format!("sc.{}{suf} {}, {}, ({})", f[2], r(3), r(5), r(4)),
            op => format!("amo{op}.{}{suf} {}, {}, ({})", f[2], r(3), r(5), r(4)),
        };
        assert_eq!(asm(&src), word.to_le_bytes(), "{src}");
    }
}

type Ctor3 = fn(u8, u8, i64) -> Inst;
type CtorR = fn(u8, u8, u8) -> Inst;
type CtorCsr = fn(u8, u8, u16) -> Inst;

fn rn(r: &mut Rng) -> u8 {
    r.below(32) as u8
}

fn nm(x: u8, r: &mut Rng) -> String {
    if r.chance(1, 2) {
        format!("x{x}")
    } else {
        ABI[x as usize].to_string()
    }
}

fn simm(r: &mut Rng, bits: u32) -> i64 {
    r.range(0, 1 << bits) as i64 - (1 << (bits - 1))
}

fn suffix(aq: bool, rl: bool) -> &'static str {
    match (aq, rl) {
        (false, false) => "",
        (true, false) => ".aq",
        (false, true) => ".rl",
        (true, true) => ".aqrl",
    }
}

/// A random valid 4-byte instruction (no control transfer when `straight`) and its text.
fn gen_inst(r: &mut Rng, straight: bool) -> (Inst, String) {
    let ialu: [(&str, Ctor3); 7] = [
        ("addi", |rd, rs1, imm| Inst::Addi { rd, rs1, imm }),
        ("slti", |rd, rs1, imm| Inst::Slti { rd, rs1, imm }),
        ("sltiu", |rd, rs1, imm| Inst::Sltiu { rd, rs1, imm }),
        ("xori", |rd, rs1, imm| Inst::Xori { rd, rs1, imm }),
        ("ori", |rd, rs1, imm| Inst::Ori { rd, rs1, imm }),
        ("andi", |rd, rs1, imm| Inst::Andi { rd, rs1, imm }),
        ("addiw", |rd, rs1, imm| Inst::Addiw { rd, rs1, imm }),
    ];
    let shifts: [(&str, Ctor3, u64); 6] = [
        ("slli", |rd, rs1, imm| Inst::Slli { rd, rs1, imm }, 64),
        ("srli", |rd, rs1, imm| Inst::Srli { rd, rs1, imm }, 64),
        ("srai", |rd, rs1, imm| Inst::Srai { rd, rs1, imm }, 64),
        ("slliw", |rd, rs1, imm| Inst::Slliw { rd, rs1, imm }, 32),
        ("srliw", |rd, rs1, imm| Inst::Srliw { rd, rs1, imm }, 32),
        ("sraiw", |rd, rs1, imm| Inst::Sraiw { rd, rs1, imm }, 32),
    ];
    let loads: [(&str, Ctor3); 7] = [
        ("lb", |rd, rs1, imm| Inst::Lb { rd, rs1, imm }),
        ("lh", |rd, rs1, imm| Inst::Lh { rd, rs1, imm }),
        ("lw", |rd, rs1, imm| Inst::Lw { rd, rs1, imm }),
        ("ld", |rd, rs1, imm| Inst::Ld { rd, rs1, imm }),
        ("lbu", |rd, rs1, imm| Inst::Lbu { rd, rs1, imm }),
        ("lhu", |rd, rs1, imm| Inst::Lhu { rd, rs1, imm }),
        ("lwu", |rd, rs1, imm| Inst::Lwu { rd, rs1, imm }),
    ];
    let stores: [(&str, Ctor3); 4] = [
        ("sb", |rs1, rs2, imm| Inst::Sb { rs1, rs2, imm }),
        ("sh", |rs1, rs2, imm| Inst::Sh { rs1, rs2, imm }),
        ("sw", |rs1, rs2, imm| Inst::Sw { rs1, rs2, imm }),
        ("sd", |rs1, rs2, imm| Inst::Sd { rs1, rs2, imm }),
    ];
    let rr: [(&str, CtorR); 28] = [
        ("add", |rd, rs1, rs2| Inst::Add { rd, rs1, rs2 }),
        ("sub", |rd, rs1, rs2| Inst::Sub { rd, rs1, rs2 }),
        ("sll", |rd, rs1, rs2| Inst::Sll { rd, rs1, rs2 }),
        ("slt", |rd, rs1, rs2| Inst::Slt { rd, rs1, rs2 }),
        ("sltu", |rd, rs1, rs2| Inst::Sltu { rd, rs1, rs2 }),
        ("xor", |rd, rs1, rs2| Inst::Xor { rd, rs1, rs2 }),
        ("srl", |rd, rs1, rs2| Inst::Srl { rd, rs1, rs2 }),
        ("sra", |rd, rs1, rs2| Inst::Sra { rd, rs1, rs2 }),
        ("or", |rd, rs1, rs2| Inst::Or { rd, rs1, rs2 }),
        ("and", |rd, rs1, rs2| Inst::And { rd, rs1, rs2 }),
        ("addw", |rd, rs1, rs2| Inst::Addw { rd, rs1, rs2 }),
        ("subw", |rd, rs1, rs2| Inst::Subw { rd, rs1, rs2 }),
        ("sllw", |rd, rs1, rs2| Inst::Sllw { rd, rs1, rs2 }),
        ("srlw", |rd, rs1, rs2| Inst::Srlw { rd, rs1, rs2 }),
        ("sraw", |rd, rs1, rs2| Inst::Sraw { rd, rs1, rs2 }),
        ("mul", |rd, rs1, rs2| Inst::Mul { rd, rs1, rs2 }),
        ("mulh", |rd, rs1, rs2| Inst::Mulh { rd, rs1, rs2 }),
        ("mulhsu", |rd, rs1, rs2| Inst::Mulhsu { rd, rs1, rs2 }),
        ("mulhu", |rd, rs1, rs2| Inst::Mulhu { rd, rs1, rs2 }),
        ("div", |rd, rs1, rs2| Inst::Div { rd, rs1, rs2 }),
        ("divu", |rd, rs1, rs2| Inst::Divu { rd, rs1, rs2 }),
        ("rem", |rd, rs1, rs2| Inst::Rem { rd, rs1, rs2 }),
        ("remu", |rd, rs1, rs2| Inst::Remu { rd, rs1, rs2 }),
        ("mulw", |rd, rs1, rs2| Inst::Mulw { rd, rs1, rs2 }),
        ("divw", |rd, rs1, rs2| Inst::Divw { rd, rs1, rs2 }),
        ("divuw", |rd, rs1, rs2| Inst::Divuw { rd, rs1, rs2 }),
        ("remw", |rd, rs1, rs2| Inst::Remw { rd, rs1, rs2 }),
        ("remuw", |rd, rs1, rs2| Inst::Remuw { rd, rs1, rs2 }),
    ];
    let branches: [(&str, Ctor3); 6] = [
        ("beq", |rs1, rs2, imm| Inst::Beq { rs1, rs2, imm }),
        ("bne", |rs1, rs2, imm| Inst::Bne { rs1, rs2, imm }),
        ("blt", |rs1, rs2, imm| Inst::Blt { rs1, rs2, imm }),
        ("bge", |rs1, rs2, imm| Inst::Bge { rs1, rs2, imm }),
        ("bltu", |rs1, rs2, imm| Inst::Bltu { rs1, rs2, imm }),
        ("bgeu", |rs1, rs2, imm| Inst::Bgeu { rs1, rs2, imm }),
    ];
    let csrs: [(&str, CtorCsr); 6] = [
        ("csrrw", |rd, x, csr| Inst::Csrrw { rd, rs1: x, csr }),
        ("csrrs", |rd, x, csr| Inst::Csrrs { rd, rs1: x, csr }),
        ("csrrc", |rd, x, csr| Inst::Csrrc { rd, rs1: x, csr }),
        ("csrrwi", |rd, x, csr| Inst::Csrrwi { rd, uimm: x, csr }),
        ("csrrsi", |rd, x, csr| Inst::Csrrsi { rd, uimm: x, csr }),
        ("csrrci", |rd, x, csr| Inst::Csrrci { rd, uimm: x, csr }),
    ];
    let amo = [
        (AmoOp::Swap, "swap"),
        (AmoOp::Add, "add"),
        (AmoOp::Xor, "xor"),
        (AmoOp::And, "and"),
        (AmoOp::Or, "or"),
        (AmoOp::Min, "min"),
        (AmoOp::Max, "max"),
        (AmoOp::Minu, "minu"),
        (AmoOp::Maxu, "maxu"),
    ];
    let (a, b, c) = (rn(r), rn(r), rn(r));
    let (na, nb, nc) = (nm(a, r), nm(b, r), nm(c, r));
    loop {
        let k = r.below(12);
        if straight && (k == 6 || k == 7) {
            continue;
        }
        return match k {
            0 => {
                let u = r.below(1 << 20) as i64;
                let imm = i64::from((u << 12) as u32 as i32);
                if r.chance(1, 2) {
                    (Inst::Lui { rd: a, imm }, format!("lui {na}, {u}"))
                } else {
                    (Inst::Auipc { rd: a, imm }, format!("auipc {na}, {u}"))
                }
            }
            1 => {
                let (op, f) = *r.pick(&ialu);
                let imm = simm(r, 12);
                (f(a, b, imm), format!("{op} {na}, {nb}, {imm}"))
            }
            2 => {
                let (op, f, lim) = *r.pick(&shifts);
                let imm = r.range(0, lim) as i64;
                (f(a, b, imm), format!("{op} {na}, {nb}, {imm}"))
            }
            3 => {
                let (op, f) = *r.pick(&loads);
                let imm = simm(r, 12);
                (f(a, b, imm), format!("{op} {na}, {imm}({nb})"))
            }
            4 => {
                let (op, f) = *r.pick(&stores);
                let imm = simm(r, 12);
                (f(b, a, imm), format!("{op} {na}, {imm}({nb})"))
            }
            5 => {
                let (op, f) = *r.pick(&rr);
                (f(a, b, c), format!("{op} {na}, {nb}, {nc}"))
            }
            6 => {
                let (op, f) = *r.pick(&branches);
                let imm = simm(r, 12) * 2;
                (f(a, b, imm), format!("{op} {na}, {nb}, {imm}"))
            }
            7 => {
                if r.chance(1, 2) {
                    let imm = simm(r, 20) * 2;
                    (Inst::Jal { rd: a, imm }, format!("jal {na}, {imm}"))
                } else {
                    let imm = simm(r, 12);
                    (
                        Inst::Jalr { rd: a, rs1: b, imm },
                        format!("jalr {na}, {imm}({nb})"),
                    )
                }
            }
            8 => {
                let (op, f) = *r.pick(&csrs);
                let csr = r.below(4096) as u16;
                let x = if op.ends_with('i') {
                    (b, b.to_string())
                } else {
                    (b, nb.clone())
                };
                (f(a, x.0, csr), format!("{op} {na}, {csr}, {}", x.1))
            }
            9 => r
                .pick(&[
                    (Inst::Ecall, "ecall".to_string()),
                    (Inst::Ebreak, "ebreak".to_string()),
                    (Inst::Mret, "mret".to_string()),
                    (Inst::Sret, "sret".to_string()),
                    (Inst::Wfi, "wfi".to_string()),
                    (Inst::FenceI, "fence.i".to_string()),
                    (
                        Inst::SfenceVma { rs1: a, rs2: b },
                        format!("sfence.vma {na}, {nb}"),
                    ),
                ])
                .clone(),
            10 => {
                let (aq, rl) = (r.chance(1, 2), r.chance(1, 2));
                let s = suffix(aq, rl);
                match r.below(4) {
                    0 => (
                        Inst::LrW {
                            rd: a,
                            rs1: b,
                            aq,
                            rl,
                        },
                        format!("lr.w{s} {na}, ({nb})"),
                    ),
                    1 => (
                        Inst::LrD {
                            rd: a,
                            rs1: b,
                            aq,
                            rl,
                        },
                        format!("lr.d{s} {na}, ({nb})"),
                    ),
                    2 => (
                        Inst::ScW {
                            rd: a,
                            rs1: b,
                            rs2: c,
                            aq,
                            rl,
                        },
                        format!("sc.w{s} {na}, {nc}, ({nb})"),
                    ),
                    _ => (
                        Inst::ScD {
                            rd: a,
                            rs1: b,
                            rs2: c,
                            aq,
                            rl,
                        },
                        format!("sc.d{s} {na}, {nc}, ({nb})"),
                    ),
                }
            }
            _ => {
                let (aq, rl) = (r.chance(1, 2), r.chance(1, 2));
                let (op, o) = *r.pick(&amo);
                let s = suffix(aq, rl);
                if r.chance(1, 2) {
                    (
                        Inst::AmoW {
                            op,
                            rd: a,
                            rs1: b,
                            rs2: c,
                            aq,
                            rl,
                        },
                        format!("amo{o}.w{s} {na}, {nc}, ({nb})"),
                    )
                } else {
                    (
                        Inst::AmoD {
                            op,
                            rd: a,
                            rs1: b,
                            rs2: c,
                            aq,
                            rl,
                        },
                        format!("amo{o}.d{s} {na}, {nc}, ({nb})"),
                    )
                }
            }
        };
    }
}

#[test]
fn leader_random_instructions_match_quartz_encode() {
    let mut r = Rng::new(0x21_2026_1115);
    for _ in 0..3000 {
        let (inst, text) = gen_inst(&mut r, false);
        assert_eq!(asm(&text), encode(inst).to_le_bytes(), "{text}");
    }
}

#[test]
fn leader_labels_equal_numeric_offsets() {
    let mut r = Rng::new(0x21_1abe_1500);
    for case in 0..200 {
        let n = 20 + r.below(60);
        let label_at: Vec<usize> = (0..6).map(|_| r.below(n + 1)).collect();
        let mut with_labels = String::new();
        let mut with_offsets = String::new();
        for i in 0..=n {
            for (k, &p) in label_at.iter().enumerate() {
                if p == i {
                    with_labels.push_str(&format!("T{k}:\n"));
                }
            }
            if i == n {
                break;
            }
            if r.chance(1, 3) {
                let k = r.below(6);
                let off = (label_at[k] as i64 - i as i64) * 4;
                let (a, b) = (rn(&mut r), rn(&mut r));
                let (op, label_text, num_text) = match r.below(4) {
                    0 => (
                        "bne",
                        format!("{}, {}, T{k}", ABI[a as usize], ABI[b as usize]),
                        format!("{}, {}, {off}", ABI[a as usize], ABI[b as usize]),
                    ),
                    1 => (
                        "jal",
                        format!("{}, T{k}", ABI[a as usize]),
                        format!("{}, {off}", ABI[a as usize]),
                    ),
                    2 => (
                        "bgeu",
                        format!("{}, {}, T{k}", ABI[a as usize], ABI[b as usize]),
                        format!("{}, {}, {off}", ABI[a as usize], ABI[b as usize]),
                    ),
                    _ => (
                        "blt",
                        format!("{}, {}, T{k}", ABI[a as usize], ABI[b as usize]),
                        format!("{}, {}, {off}", ABI[a as usize], ABI[b as usize]),
                    ),
                };
                with_labels.push_str(&format!("{op} {label_text}\n"));
                with_offsets.push_str(&format!("{op} {num_text}\n"));
            } else {
                let (_, t) = gen_inst(&mut r, true);
                with_labels.push_str(&format!("{t}\n"));
                with_offsets.push_str(&format!("{t}\n"));
            }
        }
        let a = assemble(&with_labels, 0x1000).unwrap_or_else(|e| panic!("case {case}: {e}"));
        let b = asm_at(&with_offsets, 0x1000);
        assert_eq!(a.bytes, b, "case {case}:\n{with_labels}");
        assert_eq!(a.bytes.len(), n * 4);
        let mut want: Vec<(String, u64)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for i in 0..=n {
            for (k, &p) in label_at.iter().enumerate() {
                if p == i && seen.insert(k) {
                    want.push((format!("T{k}"), 0x1000 + 4 * i as u64));
                }
            }
        }
        assert_eq!(a.symbols, want, "case {case} symbols");
    }
}

#[test]
fn leader_li_64bit_runs() {
    let mut r = Rng::new(0x21_0011_6464);
    let mut vals: Vec<i64> = vec![
        0,
        -1,
        i64::MIN,
        i64::MAX,
        0x8000_0000,
        0xffff_ffff,
        -0x8000_0001,
        0x1_0000_0000,
        0x7fff_ffff_ffff_f800,
        0x0123_4567_89ab_cdef,
        -0x0123_4567_89ab_cdef,
        0x8000_0000_0000_0800u64 as i64,
        0xfff,
        0x7ff_ffff_ffff,
        1 << 40,
        (1 << 52) + 1,
    ];
    for _ in 0..2000 {
        let v = match r.below(5) {
            0 => r.u64() as i64,
            1 => (r.u64() >> r.below(64)) as i64,
            2 => ((r.u64() >> r.below(64)) as i64).wrapping_neg(),
            3 => ((r.u64() & 0xfff) << r.below(52)) as i64,
            _ => (r.u64() as i64) >> 31 << r.below(12),
        };
        vals.push(v);
    }
    for (i, &v) in vals.iter().enumerate() {
        let rd = 1 + r.below(31) as u8;
        let text = if i % 2 == 0 {
            format!("li {}, {v}", ABI[rd as usize])
        } else {
            format!("li {}, {:#x}", ABI[rd as usize], v as u64)
        };
        let code = asm(&text);
        assert!(
            !code.is_empty() && code.len() <= 32,
            "{text}: {} bytes",
            code.len()
        );
        if (-2048..2048).contains(&v) || (v & 0xfff == 0 && i32::try_from(v).is_ok()) {
            assert_eq!(code.len(), 4, "{text}");
        } else if i32::try_from(v).is_ok() {
            assert_eq!(code.len(), 8, "{text}");
        }
        let x = run_regs(&code);
        for (k, &val) in x.iter().enumerate() {
            let want = if k == rd as usize { v as u64 } else { 0 };
            assert_eq!(val, want, "{text}: x{k}");
        }
    }
}

struct Fx {
    name: &'static str,
    src: &'static str,
    bin: &'static [u8],
    extra: &'static str,
    max: u64,
}

#[test]
fn leader_quartz_fixtures_run_the_same() {
    let fx = [
        Fx {
            name: "hello",
            src: include_str!("../../silicon/tests/fixtures/machine/hello.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/hello.bin"),
            extra: "",
            max: 200_000,
        },
        Fx {
            name: "timer",
            src: include_str!("../../silicon/tests/fixtures/machine/timer.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/timer.bin"),
            extra: "",
            max: 400_000,
        },
        Fx {
            name: "echo_irq",
            src: include_str!("../../silicon/tests/fixtures/machine/echo_irq.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/echo_irq.bin"),
            extra: "",
            max: 200_000,
        },
        Fx {
            name: "block",
            src: include_str!("../../silicon/tests/fixtures/machine/block.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/block.bin"),
            extra: "",
            max: 400_000,
        },
        Fx {
            name: "fb",
            src: include_str!("../../silicon/tests/fixtures/machine/fb.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/fb.bin"),
            extra: "",
            max: 400_000,
        },
        Fx {
            name: "keys_s",
            src: include_str!("../../silicon/tests/fixtures/machine/keys_s.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/keys_s.bin"),
            extra: "",
            max: 200_000,
        },
        Fx {
            name: "msi",
            src: include_str!("../../silicon/tests/fixtures/machine/msi.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/machine/msi.bin"),
            extra: "",
            max: 200_000,
        },
        Fx {
            name: "rvc",
            src: include_str!("../../silicon/tests/fixtures/ac/rvc.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/ac/rvc.bin"),
            extra: ".option rvc\n.option arch, +a\n",
            max: 200_000,
        },
        Fx {
            name: "amo_sem",
            src: include_str!("../../silicon/tests/fixtures/ac/amo_sem.S"),
            bin: include_bytes!("../../silicon/tests/fixtures/ac/amo_sem.bin"),
            extra: "",
            max: 200_000,
        },
    ];
    for f in &fx {
        let src = format!("{COMMON}\n{}{}", f.extra, f.src);
        let ours = assemble(&src, RAM_BASE).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        let run = |bin: &[u8]| {
            let disk: Vec<u8> = (0..16 * 512u32)
                .map(|i| (i.wrapping_mul(7) + 3) as u8)
                .collect();
            let mut m = Machine::new(Config {
                disk,
                ..Config::default()
            });
            m.load(RAM_BASE, bin).unwrap();
            m.board.uart_input(b"hi\n");
            m.board.press_key(0x1E);
            let e = m.run(f.max);
            let fb_sum: u64 = m.board.framebuffer().iter().map(|&p| u64::from(p)).sum();
            (
                e,
                m.board.take_uart_output(),
                fb_sum,
                m.board.disk().to_vec(),
            )
        };
        let want = run(f.bin);
        let got = run(&ours.bytes);
        assert_eq!(got.0, want.0, "{}: exit", f.name);
        assert_eq!(got.1, want.1, "{}: uart", f.name);
        assert_eq!(got.2, want.2, "{}: framebuffer", f.name);
        assert!(got.3 == want.3, "{}: disk", f.name);
        if ["hello", "rvc", "amo_sem"].contains(&f.name) {
            assert_eq!(got.0, Exit::Pass, "{} should pass", f.name);
        }
    }
}

#[test]
fn leader_errors_report_their_line() {
    let cases: &[(&str, usize)] = &[
        ("nop\nfoo a0, a1\n", 2),
        ("addi x32, x0, 1\n", 1),
        ("nop\nnop\nadd a0, a1\n", 3),
        ("addi a0, a0, 2048\n", 1),
        ("nop\nslli a0, a0, 64\n", 2),
        ("slliw a0, a0, 32\n", 1),
        ("nop\nj nowhere\n", 2),
        ("a:\nnop\na:\n", 3),
        ("beq a0, a1, far\n.zero 5000\nfar:\n", 1),
        ("c.lw ra, 0(a0)\n", 1),
        ("nop\nc.addi a0, 0\n", 2),
        ("nop\n.bogus 3\n", 2),
        ("nop\n.macro M a\nnop\n", 2),
        ("j 3f\n3:\nj 3f\n", 3),
        ("auipc a0, 0\naddi a0, a0, %pcrel_lo(x)\nx: nop\n", 2),
        (".byte 256\n", 1),
        ("nop\n.half 65536\n", 2),
        (".ascii \"abc\n", 1),
        (".macro M r\naddi \\r, \\r, 5000\n.endm\nnop\nM a0\n", 5),
        ("beq a0, a1, 3\n", 1),
        ("c.beqz a0, 300\n", 1),
        (".equ A, B + 1\n", 1),
        ("nop\nli a0, later\nlater:\n", 2),
        ("csrrwi a0, mstatus, 32\n", 1),
        ("lui a0, 0x100000\n", 1),
    ];
    for &(src, line) in cases {
        match assemble(src, BASE) {
            Ok(o) => panic!("{src:?} assembled to {:02x?}, expected an error", o.bytes),
            Err(e) => assert_eq!(e.line, line, "{src:?}: {e}"),
        }
    }
}

#[test]
fn leader_alignment_expressions_symbols() {
    for k in 0..8usize {
        for dir in [".balign 8", ".p2align 3", ".align 3"] {
            let src = format!("{}{dir}\n.byte 0xaa\n", ".byte 1\n".repeat(k));
            let mut want = vec![1u8; k];
            if k % 8 != 0 {
                let mut at = k;
                if at % 2 == 1 {
                    want.push(0);
                    at += 1;
                }
                if at % 4 == 2 {
                    want.extend([0x01, 0x00]);
                    at += 2;
                }
                while at % 8 != 0 {
                    want.extend([0x13, 0, 0, 0]);
                    at += 4;
                }
            }
            want.push(0xaa);
            assert_eq!(asm(&src), want, "{src:?}");
        }
    }
    let exprs: &[(&str, i32)] = &[
        ("6 & 3 - 1", 1),
        ("2 + 3 * 4", 14),
        ("1 << 2 + 1", 5),
        ("7 - 2 - 1", 4),
        ("100 / 7 % 3", 2),
        ("-8 >> 60", 15),
        ("~0 & 0xff", 255),
        ("5 | 2 ^ 3", 4),
        ("1 + 2 << 3", 17),
        ("12 / 4 * 3", 9),
        ("-7 / 2", -3),
        ("-7 % 2", -1),
        ("'A'", 65),
        ("0b101", 5),
        ("(1 << 9) | 5", 517),
        ("K * 2 + -K", 3),
    ];
    for &(e, v) in exprs {
        let src = format!(".equ K, 3\n.word {e}\n");
        assert_eq!(asm(&src), v.to_le_bytes(), "{e}");
    }
    // %hi/%lo of a label, with the rounding case %lo = -2048
    let got = asm("lui a0, %hi(x)\naddi a0, a0, %lo(x)\n.zero 0x7f8\nx:\n");
    let mut want = encode(Inst::Lui {
        rd: 10,
        imm: i64::from(0x8000_1000u32 as i32),
    })
    .to_le_bytes()
    .to_vec();
    want.extend(
        encode(Inst::Addi {
            rd: 10,
            rs1: 10,
            imm: -2048,
        })
        .to_le_bytes(),
    );
    want.extend(vec![0; 0x7f8]);
    assert_eq!(got, want, "%hi/%lo of a label");
    let o = assemble(
        "start:\nnop\n1: nop\nmid: .word end - start\n.equ K, 3\n.dword mid\nend:\n",
        0x1000,
    )
    .unwrap();
    assert_eq!(
        o.symbols,
        vec![
            ("start".to_string(), 0x1000),
            ("mid".to_string(), 0x1008),
            ("end".to_string(), 0x1014)
        ]
    );
    assert_eq!(&o.bytes[8..12], &20u32.to_le_bytes());
    assert_eq!(&o.bytes[12..20], &0x1008u64.to_le_bytes());
    assert_eq!(
        asm(".asciz \"a\\n\\t\\\\\\\"\\0b\"\n.ascii \"#x\"\n"),
        b"a\n\t\\\"\0b\0#x"
    );
}

#[test]
fn leader_corpus_speed() {
    let t = Instant::now();
    for _ in 0..20 {
        assert!(assemble(CORPUS, BASE).is_ok());
    }
    let dt = t.elapsed().as_secs_f64();
    assert!(dt < 10.0, "corpus x20 took {dt:.1}s");
}
