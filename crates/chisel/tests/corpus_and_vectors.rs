//! Corpus, compressed-pair, AMO, and 64-bit li verification tests.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chisel::assemble;
use quartz::inst::{decode, Inst};

fn manifest_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn reg_name(r: u8) -> String {
    format!("x{r}")
}

fn sim_li_sequence(words: &[u32], rd: u8) -> i64 {
    let mut regs = [0i64; 32];
    for &w in words {
        let inst = decode(w).unwrap();
        match inst {
            Inst::Lui { rd: d, imm } => {
                assert_eq!(d, rd);
                regs[d as usize] = imm;
            }
            Inst::Addi { rd: d, rs1, imm } => {
                assert_eq!(d, rd);
                assert!(rs1 == 0 || rs1 == rd);
                regs[d as usize] = regs[rs1 as usize].wrapping_add(imm);
            }
            Inst::Addiw { rd: d, rs1, imm } => {
                assert_eq!(d, rd);
                assert!(rs1 == 0 || rs1 == rd);
                let sum = (regs[rs1 as usize] as i32).wrapping_add(imm as i32);
                regs[d as usize] = i64::from(sum);
            }
            Inst::Slli { rd: d, rs1, imm } => {
                assert_eq!(d, rd);
                assert_eq!(rs1, rd);
                regs[d as usize] = ((regs[rs1 as usize] as u64) << (imm as u32)) as i64;
            }
            other => panic!("unexpected instruction in li sequence: {other:?}"),
        }
        regs[0] = 0;
    }
    regs[rd as usize]
}

#[test]
fn corpus_matches_llvm_bin_and_syms() {
    let src = std::fs::read_to_string(manifest_path("tests/fixtures/corpus.s")).unwrap();
    let expected = std::fs::read(manifest_path("tests/fixtures/corpus.bin")).unwrap();
    let syms_txt = std::fs::read_to_string(manifest_path("tests/fixtures/corpus.syms")).unwrap();
    let map_txt = std::fs::read_to_string(manifest_path("tests/fixtures/corpus.map")).unwrap();

    let out = assemble(&src, 0x8000_0000).unwrap();
    assert_eq!(
        out.bytes.len(),
        expected.len(),
        "corpus total byte length mismatch"
    );

    let mut off_to_line = Vec::new();
    for line in map_txt.lines() {
        let mut it = line.split_whitespace();
        let off: usize = it.next().unwrap().parse().unwrap();
        let lno: usize = it.next().unwrap().parse().unwrap();
        off_to_line.push((off, lno));
    }
    let src_lines: Vec<&str> = src.lines().collect();

    let mut li_ranges: HashSet<usize> = HashSet::new();
    let mut li_spans: Vec<(usize, usize, u8)> = Vec::new();
    for (idx, &(off, lno)) in off_to_line.iter().enumerate() {
        let text = src_lines[lno - 1].trim();
        let after_lbl = text.rsplit(':').next().unwrap().trim();
        if let Some(rest) = after_lbl.strip_prefix("li ") {
            let next_off = off_to_line.get(idx + 1).map_or(expected.len(), |&(o, _)| o);
            for b in off..next_off {
                li_ranges.insert(b);
            }
            let reg_tok = rest.split(',').next().unwrap().trim();
            let rd = match reg_tok {
                "zero" | "x0" => 0,
                "ra" | "x1" => 1,
                "sp" | "x2" => 2,
                "gp" | "x3" => 3,
                "tp" | "x4" => 4,
                "t0" | "x5" => 5,
                "t1" | "x6" => 6,
                "t2" | "x7" => 7,
                "s0" | "fp" | "x8" => 8,
                "s1" | "x9" => 9,
                "a0" | "x10" => 10,
                "a1" | "x11" => 11,
                "a2" | "x12" => 12,
                "a3" | "x13" => 13,
                "a4" | "x14" => 14,
                "a5" | "x15" => 15,
                "a6" | "x16" => 16,
                "a7" | "x17" => 17,
                "s2" | "x18" => 18,
                "s3" | "x19" => 19,
                "s4" | "x20" => 20,
                "s5" | "x21" => 21,
                "s6" | "x22" => 22,
                "s7" | "x23" => 23,
                "s8" | "x24" => 24,
                "s9" | "x25" => 25,
                "s10" | "x26" => 26,
                "s11" | "x27" => 27,
                "t3" | "x28" => 28,
                "t4" | "x29" => 29,
                "t5" | "x30" => 30,
                "t6" | "x31" => 31,
                other => other.strip_prefix('x').unwrap().parse().unwrap(),
            };
            li_spans.push((off, next_off, rd));
        }
    }

    for (i, &exp_b) in expected.iter().enumerate() {
        if out.bytes[i] != exp_b && !li_ranges.contains(&i) {
            let lno = off_to_line
                .iter()
                .rev()
                .find(|&&(o, _)| o <= i)
                .map_or(0, |&(_, l)| l);
            panic!(
                "byte mismatch at offset {i:#x} (line {lno}: {:?}): got {:02x}, expected {exp_b:02x}",
                src_lines.get(lno.saturating_sub(1)),
                out.bytes[i],
            );
        }
    }

    for (off, next_off, rd) in li_spans {
        let got_words: Vec<u32> = out.bytes[off..next_off]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| u32::from_le_bytes(c))
            .collect();
        let exp_words: Vec<u32> = expected[off..next_off]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| u32::from_le_bytes(c))
            .collect();
        assert_eq!(
            sim_li_sequence(&got_words, rd),
            sim_li_sequence(&exp_words, rd)
        );
    }

    let sym_map: HashMap<&str, u64> = out.symbols.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    for line in syms_txt.lines() {
        let mut it = line.split_whitespace();
        let name = it.next().unwrap();
        let addr = u64::from_str_radix(it.next().unwrap(), 16).unwrap();
        assert_eq!(sym_map.get(name).copied(), Some(addr), "symbol {name}");
    }
}

#[test]
fn all_c_pairs_match_llvm() {
    let txt =
        std::fs::read_to_string(manifest_path("../quartz/tests/fixtures/ac/c_pairs.txt")).unwrap();
    for (idx, line) in txt.lines().enumerate() {
        let lno = idx + 1;
        let (left, right) = line.split_once('|').unwrap();
        let mut toks = left.split_whitespace();
        let c_hex = u16::from_str_radix(toks.next().unwrap(), 16).unwrap();
        let w_hex = u32::from_str_radix(toks.next().unwrap(), 16).unwrap();
        let c_asm = toks.collect::<Vec<_>>().join(" ");
        let w_asm = right.trim();

        let out_c =
            assemble(&c_asm, 0).unwrap_or_else(|e| panic!("c_pairs line {lno} '{c_asm}': {e}"));
        assert_eq!(
            out_c.bytes,
            c_hex.to_le_bytes(),
            "c_pairs line {lno} compressed '{c_asm}'"
        );

        let out_w =
            assemble(w_asm, 0).unwrap_or_else(|e| panic!("c_pairs line {lno} '{w_asm}': {e}"));
        assert_eq!(
            out_w.bytes,
            w_hex.to_le_bytes(),
            "c_pairs line {lno} wide '{w_asm}'"
        );
    }
}

#[test]
fn all_amo_vectors_match() {
    let txt =
        std::fs::read_to_string(manifest_path("../quartz/tests/fixtures/ac/amo.txt")).unwrap();
    for (idx, line) in txt.lines().enumerate() {
        let lno = idx + 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        let word = u32::from_str_radix(parts[0], 16).unwrap();
        let op = parts[1];
        let width = parts[2];
        let rd = reg_name(parts[3].parse().unwrap());
        let rs1 = reg_name(parts[4].parse().unwrap());
        let rs2 = reg_name(parts[5].parse().unwrap());
        let aq = parts[6] == "1";
        let rl = parts[7] == "1";
        let suffix = match (aq, rl) {
            (false, false) => "",
            (true, false) => ".aq",
            (false, true) => ".rl",
            (true, true) => ".aqrl",
        };
        let asm = match op {
            "lr" => format!("lr.{width}{suffix} {rd}, ({rs1})"),
            "sc" => format!("sc.{width}{suffix} {rd}, {rs2}, ({rs1})"),
            _ => format!("amo{op}.{width}{suffix} {rd}, {rs2}, ({rs1})"),
        };
        let out = assemble(&asm, 0).unwrap_or_else(|e| panic!("amo line {lno} '{asm}': {e}"));
        assert_eq!(out.bytes, word.to_le_bytes(), "amo line {lno} '{asm}'");
    }
}

#[test]
fn li_64_edge_and_random_values() {
    let mut vals: Vec<i64> = vec![
        0,
        1,
        -1,
        2047,
        -2048,
        2048,
        -2049,
        0x7fff_f800,
        -0x7fff_f800,
        i64::from(i32::MAX),
        i64::from(i32::MIN),
        i64::from(i32::MAX) + 1,
        i64::from(i32::MIN) - 1,
        0x8000_0000_i64,
        i64::MAX,
        i64::MIN,
        i64::MIN + 1,
        i64::MAX - 2048,
        0x7fff_f7ff_ffff_f7ff,
        -0x1234_5678_9abc_def0,
        0x1234_5678_9abc_def0,
    ];
    let mut rng = assay::Rng::new(0xdead_beef_cafe_1234);
    for _ in 0..2000 {
        vals.push(rng.u64() as i64);
    }
    for val in vals {
        let asm = format!("li a0, {val}");
        let out = assemble(&asm, 0).unwrap();
        assert!(
            out.bytes.len().is_multiple_of(4) && out.bytes.len() <= 32,
            "len for {val}"
        );
        let words: Vec<u32> = out
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| u32::from_le_bytes(c))
            .collect();
        let got = sim_li_sequence(&words, 10);
        assert_eq!(got, val, "li a0, {val:#x} produced {got:#x}");
    }
}
