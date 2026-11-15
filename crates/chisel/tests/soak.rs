//! Fuzz soak tests for chisel's public parsers, assembler, and ELF reader/loader.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use chisel::assemble;
use chisel::elf::{load, read, write, Image, Segment, PF_R, PF_W, PF_X};
use silicon::machine::{Config, Machine, RAM_BASE};

const DEFAULT_SEED: u64 = 0x05EE_D793;
const DEFAULT_MS: u64 = 300;
const MAX_CASE_BYTES: usize = 64 * 1024;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(&hex.replace('_', ""), 16).unwrap_or(DEFAULT_SEED)
            } else {
                s.replace('_', "").parse::<u64>().unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

fn mutate_bytes(rng: &mut Rng, corpus: &[Vec<u8>]) -> Vec<u8> {
    let base = rng.pick(corpus);
    let mut out = base.clone();
    if out.is_empty() {
        return rng.bytes(256);
    }
    let ops = 1 + rng.below(4);
    for _ in 0..ops {
        if out.is_empty() {
            break;
        }
        match rng.below(5) {
            // Bit flip
            0 => {
                let idx = rng.below(out.len());
                let bit = 1u8 << rng.below(8);
                out[idx] ^= bit;
            }
            // Byte overwrite
            1 => {
                let idx = rng.below(out.len());
                out[idx] = rng.u64() as u8;
            }
            // Truncation
            2 => {
                let new_len = rng.below(out.len() + 1);
                out.truncate(new_len);
            }
            // Splice with another corpus item
            3 => {
                let other = rng.pick(corpus);
                if !other.is_empty() {
                    let cut1 = rng.below(out.len() + 1);
                    let cut2 = rng.below(other.len());
                    out.truncate(cut1);
                    let take = (other.len() - cut2).min(MAX_CASE_BYTES.saturating_sub(out.len()));
                    out.extend_from_slice(&other[cut2..cut2 + take]);
                }
            }
            // Repeated chunk
            _ => {
                let start = rng.below(out.len());
                let end = start + rng.below(out.len() - start + 1).min(256);
                let chunk = out[start..end].to_vec();
                let reps = 1 + rng.below(4);
                for _ in 0..reps {
                    if out.len() + chunk.len() > MAX_CASE_BYTES {
                        break;
                    }
                    out.extend_from_slice(&chunk);
                }
            }
        }
    }
    out
}

fn random_text(rng: &mut Rng, max_len: usize) -> String {
    let len = rng.below(max_len + 1);
    let mut s = String::with_capacity(len);
    const TOKENS: &[&str] = &[
        "add ", "addi ", "sub ", "ld ", "sd ", "lw ", "sw ", "li ", "la ", "j ", "jal ", "jalr ",
        "beq ", "bne ", "ecall", "ebreak", "c.addi ", "c.nop", "c.li ", "c.mv ", ".text", ".data",
        ".globl ", ".byte ", ".half ", ".word ", ".dword ", ".zero ", ".space ", ".skip ",
        ".align ", ".org ", ".equ ", ".rept ", ".endr", ".macro ", ".endm", ".asciz ", "x0", "x1",
        "sp", "ra", "t0", "a0", "s0", "0x", "-1", "100", "_start:", "L0:", ",", "(", ")", "+", "-",
        "*", "/", "<<", ">>", "|", "&", "^", "\n", "\t", " ", "#", "\"abc\"",
    ];
    if rng.chance(1, 2) {
        // Purely random UTF-8 lossy bytes
        let raw = rng.bytes(len);
        return String::from_utf8_lossy(&raw).into_owned();
    }
    while s.len() < len {
        if rng.chance(3, 4) {
            s.push_str(rng.pick(TOKENS));
        } else {
            let ch = (0x20 + (rng.u64() % 95) as u8) as char;
            s.push(ch);
        }
    }
    s.truncate(s.floor_char_boundary(len));
    s
}

fn valid_asm_corpus() -> Vec<&'static str> {
    vec![
        ".text\n.globl _start\n_start:\n  addi a0, x0, 42\n  ecall\n",
        ".text\nmain:\n  li t0, 0x80001000\n  ld t1, 0(t0)\n  c.addi t1, 1\n  sd t1, 0(t0)\n  ret\n",
        ".data\nmsg:\n  .asciz \"hello\"\n  .align 3\n  .dword 0x123456789abcdef0\n  .zero 16\n",
        ".equ N, 4\n.text\n  addi a0, zero, N\n  .byte 1, 2, 3, 4\n",
        ".macro inc reg\n  addi \\reg, \\reg, 1\n.endm\n.text\n  inc a0\n  inc a1\n",
        ".text\nloop:\n  beq a0, zero, done\n  addi a0, a0, -1\n  j loop\ndone:\n  ebreak\n",
    ]
}

fn mutate_text(rng: &mut Rng, corpus: &[&str]) -> String {
    let byte_corpus: Vec<Vec<u8>> = corpus.iter().map(|s| s.as_bytes().to_vec()).collect();
    let mut bytes = mutate_bytes(rng, &byte_corpus);
    if rng.chance(1, 3) {
        // Also splice in a random directive/token
        let extra = random_text(rng, 64);
        let pos = rng.below(bytes.len() + 1);
        bytes.splice(pos..pos, extra.into_bytes());
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn random_valid_image(rng: &mut Rng) -> Image {
    let n_seg = rng.below(5);
    let mut segments = Vec::with_capacity(n_seg);
    let mut next_vaddr = RAM_BASE + (rng.u64() & 0xfff);
    for _ in 0..n_seg {
        let data_len = rng.below(512);
        let data = rng.bytes(data_len);
        let extra_bss = rng.below(256) as u64;
        let mem_size = data.len() as u64 + extra_bss;
        let flags = match rng.below(4) {
            0 => PF_R,
            1 => PF_R | PF_X,
            2 => PF_R | PF_W,
            _ => PF_R | PF_W | PF_X,
        };
        segments.push(Segment {
            vaddr: next_vaddr,
            flags,
            data,
            mem_size,
        });
        next_vaddr = next_vaddr.saturating_add(mem_size.max(1) + 0x100 + (rng.u64() & 0xfff));
    }
    let n_sym = rng.below(8);
    let mut symbols = Vec::with_capacity(n_sym);
    for i in 0..n_sym {
        symbols.push((format!("sym_{i}_{}", rng.u64() & 0xff), rng.u64()));
    }
    Image {
        entry: RAM_BASE,
        segments,
        symbols,
    }
}

#[test]
fn soak_assemble() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0xA55E_1111);
    let corpus = valid_asm_corpus();

    // Round-trip / determinism check on valid corpus
    for &src in &corpus {
        let out1 = assemble(src, RAM_BASE).unwrap();
        let out2 = assemble(src, RAM_BASE).unwrap();
        assert_eq!(out1, out2);
        let img = Image {
            entry: RAM_BASE,
            segments: vec![Segment {
                vaddr: RAM_BASE,
                flags: PF_R | PF_X,
                data: out1.bytes,
                mem_size: 4096,
            }],
            symbols: out1.symbols,
        };
        assert_eq!(read(&write(&img)), Ok(img));
    }

    let start = Instant::now();
    let mut cases: u64 = 0;
    while start.elapsed() < budget {
        let base = if rng.chance(1, 2) {
            RAM_BASE
        } else {
            rng.u64()
        };
        let src = if cases.is_multiple_of(2) {
            random_text(&mut rng, 1024)
        } else {
            mutate_text(&mut rng, &corpus)
        };
        let t0 = Instant::now();
        let _ = assemble(&src, base);
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "assemble took > 1s on seed {seed:#x} case {cases}"
        );
        cases += 1;
    }
    println!("soak_assemble: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_elf_read() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0xE1F0_2222);

    let mut valid_corpus = Vec::new();
    for _ in 0..16 {
        let img = random_valid_image(&mut rng);
        let bytes = write(&img);
        assert_eq!(read(&bytes), Ok(img));
        valid_corpus.push(bytes);
    }

    let start = Instant::now();
    let mut cases: u64 = 0;
    while start.elapsed() < budget {
        let input = if cases.is_multiple_of(2) {
            let mut b = rng.bytes(2048);
            if b.len() >= 4 && rng.chance(1, 3) {
                b[0..4].copy_from_slice(b"\x7fELF");
            }
            b
        } else {
            mutate_bytes(&mut rng, &valid_corpus)
        };
        let t0 = Instant::now();
        let _ = read(&input);
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "elf::read took > 1s on seed {seed:#x} case {cases}"
        );
        cases += 1;
    }
    println!("soak_elf_read: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_elf_load() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x10AD_3333);

    let mut valid_corpus = Vec::new();
    for _ in 0..16 {
        valid_corpus.push(write(&random_valid_image(&mut rng)));
    }

    let mut machine = Machine::new(Config::default());
    let start = Instant::now();
    let mut cases: u64 = 0;
    while start.elapsed() < budget {
        let img = if cases.is_multiple_of(2) {
            // Random/mutated Image with bounded data size
            let n_seg = rng.below(4);
            let mut segments = Vec::with_capacity(n_seg);
            for _ in 0..n_seg {
                let data = rng.bytes(1024);
                let mem_size = if rng.chance(1, 4) {
                    rng.u64()
                } else {
                    data.len() as u64 + (rng.u64() & 0xfff)
                };
                let vaddr = if rng.chance(1, 2) {
                    RAM_BASE.wrapping_add(rng.u64() & 0xffff)
                } else {
                    rng.u64()
                };
                segments.push(Segment {
                    vaddr,
                    flags: rng.u64() as u32,
                    data,
                    mem_size,
                });
            }
            Image {
                entry: rng.u64(),
                segments,
                symbols: Vec::new(),
            }
        } else {
            let mutated = mutate_bytes(&mut rng, &valid_corpus);
            match read(&mutated) {
                Ok(img) => img,
                Err(_) => random_valid_image(&mut rng),
            }
        };

        let t0 = Instant::now();
        let _ = load(&img, &mut machine);
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "elf::load took > 1s on seed {seed:#x} case {cases}"
        );
        cases += 1;
    }
    println!("soak_elf_load: seed={seed:#x} cases={cases}");
}

/// Regression (leader, T79c): repeated capped directives once summed to a 48 GiB allocation.
#[test]
fn total_output_is_bounded() {
    let aligns = ".byte 1
.p2align 30
"
    .repeat(4);
    assert!(chisel::assemble(&aligns, 0x8000_0000).is_err());
}

/// Regression (leader, T79c): nested macros doubled at each of 64 levels (48 GiB allocation).
#[test]
fn macro_fan_out_is_bounded() {
    let mut src = String::from(".macro m0\nnop\n.endm\n");
    for i in 1..40 {
        src.push_str(&format!(".macro m{i}\nm{p}\nm{p}\n.endm\n", p = i - 1));
    }
    src.push_str("m39\n");
    assert!(chisel::assemble(&src, 0x8000_0000).is_err());
    // arguments that double at every level
    let mut src = String::from(".macro w0 x\n.byte 0\n.endm\n");
    for i in 1..40 {
        src.push_str(&format!(".macro w{i} x\nw{p} \\x\\x\n.endm\n", p = i - 1));
    }
    src.push_str("w39 1\n");
    assert!(chisel::assemble(&src, 0x8000_0000).is_err());
}
