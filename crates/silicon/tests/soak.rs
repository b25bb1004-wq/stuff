//! Fuzz soak tests for silicon decoders and machine execution.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use silicon::inst::{decode, decode_c, encode, AmoOp, Inst};
use silicon::machine::{Config, Machine, RAM_BASE};

const DEFAULT_SEED: u64 = 0x05EE_D79C;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let trimmed = s.trim().replace('_', "");
            if let Some(hex) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                u64::from_str_radix(hex, 16).unwrap_or(DEFAULT_SEED)
            } else {
                trimmed.parse::<u64>().unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_millis(ms)
}

fn sext(v: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((v << shift) as i64) >> shift
}

fn gen_amo_op(rng: &mut Rng) -> AmoOp {
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
    *rng.pick(&OPS)
}

fn gen_valid_inst(rng: &mut Rng) -> Inst {
    let rd = rng.below(32) as u8;
    let rs1 = rng.below(32) as u8;
    let rs2 = rng.below(32) as u8;
    let uimm = rng.below(32) as u8;
    let csr = rng.below(4096) as u16;
    let aq = rng.chance(1, 2);
    let rl = rng.chance(1, 2);
    let imm12 = sext(rng.u64(), 12);
    let imm13_even = sext(rng.u64(), 13) & !1;
    let imm21_even = sext(rng.u64(), 21) & !1;
    let imm20_u = ((rng.u64() as i32) as i64) & !0xfff;
    let shamt6 = rng.below(64) as i64;
    let shamt5 = rng.below(32) as i64;

    match rng.below(68) {
        0 => Inst::Lui { rd, imm: imm20_u },
        1 => Inst::Auipc { rd, imm: imm20_u },
        2 => Inst::Jal {
            rd,
            imm: imm21_even,
        },
        3 => Inst::Jalr {
            rd,
            rs1,
            imm: imm12,
        },
        4 => Inst::Beq {
            rs1,
            rs2,
            imm: imm13_even,
        },
        5 => Inst::Bne {
            rs1,
            rs2,
            imm: imm13_even,
        },
        6 => Inst::Blt {
            rs1,
            rs2,
            imm: imm13_even,
        },
        7 => Inst::Bge {
            rs1,
            rs2,
            imm: imm13_even,
        },
        8 => Inst::Bltu {
            rs1,
            rs2,
            imm: imm13_even,
        },
        9 => Inst::Bgeu {
            rs1,
            rs2,
            imm: imm13_even,
        },
        10 => Inst::Lb {
            rd,
            rs1,
            imm: imm12,
        },
        11 => Inst::Lh {
            rd,
            rs1,
            imm: imm12,
        },
        12 => Inst::Lw {
            rd,
            rs1,
            imm: imm12,
        },
        13 => Inst::Ld {
            rd,
            rs1,
            imm: imm12,
        },
        14 => Inst::Lbu {
            rd,
            rs1,
            imm: imm12,
        },
        15 => Inst::Lhu {
            rd,
            rs1,
            imm: imm12,
        },
        16 => Inst::Lwu {
            rd,
            rs1,
            imm: imm12,
        },
        17 => Inst::Sb {
            rs1,
            rs2,
            imm: imm12,
        },
        18 => Inst::Sh {
            rs1,
            rs2,
            imm: imm12,
        },
        19 => Inst::Sw {
            rs1,
            rs2,
            imm: imm12,
        },
        20 => Inst::Sd {
            rs1,
            rs2,
            imm: imm12,
        },
        21 => Inst::Addi {
            rd,
            rs1,
            imm: imm12,
        },
        22 => Inst::Slti {
            rd,
            rs1,
            imm: imm12,
        },
        23 => Inst::Sltiu {
            rd,
            rs1,
            imm: imm12,
        },
        24 => Inst::Xori {
            rd,
            rs1,
            imm: imm12,
        },
        25 => Inst::Ori {
            rd,
            rs1,
            imm: imm12,
        },
        26 => Inst::Andi {
            rd,
            rs1,
            imm: imm12,
        },
        27 => Inst::Slli {
            rd,
            rs1,
            imm: shamt6,
        },
        28 => Inst::Srli {
            rd,
            rs1,
            imm: shamt6,
        },
        29 => Inst::Srai {
            rd,
            rs1,
            imm: shamt6,
        },
        30 => Inst::Add { rd, rs1, rs2 },
        31 => Inst::Sub { rd, rs1, rs2 },
        32 => Inst::Sll { rd, rs1, rs2 },
        33 => Inst::Slt { rd, rs1, rs2 },
        34 => Inst::Sltu { rd, rs1, rs2 },
        35 => Inst::Xor { rd, rs1, rs2 },
        36 => Inst::Srl { rd, rs1, rs2 },
        37 => Inst::Sra { rd, rs1, rs2 },
        38 => Inst::Or { rd, rs1, rs2 },
        39 => Inst::And { rd, rs1, rs2 },
        40 => Inst::Addiw {
            rd,
            rs1,
            imm: imm12,
        },
        41 => Inst::Slliw {
            rd,
            rs1,
            imm: shamt5,
        },
        42 => Inst::Srliw {
            rd,
            rs1,
            imm: shamt5,
        },
        43 => Inst::Sraiw {
            rd,
            rs1,
            imm: shamt5,
        },
        44 => Inst::Addw { rd, rs1, rs2 },
        45 => Inst::Subw { rd, rs1, rs2 },
        46 => Inst::Sllw { rd, rs1, rs2 },
        47 => Inst::Srlw { rd, rs1, rs2 },
        48 => Inst::Sraw { rd, rs1, rs2 },
        49 => Inst::Fence,
        50 => Inst::Ecall,
        51 => Inst::Ebreak,
        52 => Inst::Mul { rd, rs1, rs2 },
        53 => Inst::Mulh { rd, rs1, rs2 },
        54 => Inst::Mulhsu { rd, rs1, rs2 },
        55 => Inst::Mulhu { rd, rs1, rs2 },
        56 => Inst::Div { rd, rs1, rs2 },
        57 => Inst::Divu { rd, rs1, rs2 },
        58 => Inst::Rem { rd, rs1, rs2 },
        59 => Inst::Remu { rd, rs1, rs2 },
        60 => Inst::Mulw { rd, rs1, rs2 },
        61 => Inst::Divw { rd, rs1, rs2 },
        62 => Inst::Divuw { rd, rs1, rs2 },
        63 => Inst::Remw { rd, rs1, rs2 },
        64 => Inst::Remuw { rd, rs1, rs2 },
        65 => Inst::FenceI,
        66 => match rng.below(6) {
            0 => Inst::Csrrw { rd, rs1, csr },
            1 => Inst::Csrrs { rd, rs1, csr },
            2 => Inst::Csrrc { rd, rs1, csr },
            3 => Inst::Csrrwi { rd, uimm, csr },
            4 => Inst::Csrrsi { rd, uimm, csr },
            _ => Inst::Csrrci { rd, uimm, csr },
        },
        _ => match rng.below(7) {
            0 => Inst::Mret,
            1 => Inst::Sret,
            2 => Inst::Wfi,
            3 => Inst::SfenceVma { rs1, rs2 },
            4 => Inst::LrW { rd, rs1, aq, rl },
            5 => Inst::ScW {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
            _ => Inst::AmoW {
                op: gen_amo_op(rng),
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
        },
    }
}

fn mutate_bytes(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    if src.is_empty() {
        return rng.bytes(256);
    }
    let mut out = src.to_vec();
    match rng.below(4) {
        // Bit flips
        0 => {
            let flips = 1 + rng.below(8);
            for _ in 0..flips {
                let idx = rng.below(out.len());
                let bit = rng.below(8);
                out[idx] ^= 1 << bit;
            }
        }
        // Truncation
        1 => {
            let new_len = rng.below(out.len() + 1);
            out.truncate(new_len);
        }
        // Splice with random chunk
        2 => {
            let cut = rng.below(out.len() + 1);
            let tail = out.split_off(cut);
            let mid = rng.bytes(64);
            out.extend_from_slice(&mid);
            out.extend_from_slice(&tail);
        }
        // Repeated chunk
        _ => {
            let start = rng.below(out.len());
            let end = start + rng.below(out.len() - start + 1);
            let chunk = out[start..end].to_vec();
            let reps = 1 + rng.below(4);
            for _ in 0..reps {
                if out.len() + chunk.len() > 8192 {
                    break;
                }
                out.extend_from_slice(&chunk);
            }
        }
    }
    out
}

fn valid_compressed_corpus(rng: &mut Rng) -> Vec<u16> {
    let mut corpus = Vec::with_capacity(128);
    let mut attempts = 0;
    while corpus.len() < 128 && attempts < 4096 {
        attempts += 1;
        let h = rng.u64() as u16;
        if decode_c(h).is_some() {
            corpus.push(h);
        }
    }
    if corpus.is_empty() {
        corpus.push(0x0001); // c.nop
    }
    corpus
}

#[test]
fn soak_decode_32() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x3232_3232);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget {
        let case_start = Instant::now();
        let w = if rng.chance(1, 2) {
            rng.u64() as u32
        } else {
            let valid = gen_valid_inst(&mut rng);
            let word = encode(valid);
            // Assert round-trip on valid input produced by the crate.
            assert_eq!(
                decode(word),
                Some(valid),
                "round-trip failed for valid inst {valid:?}"
            );
            let bytes = mutate_bytes(&mut rng, &word.to_le_bytes());
            if bytes.len() >= 4 {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                let mut buf = [0u8; 4];
                buf[..bytes.len()].copy_from_slice(&bytes);
                u32::from_le_bytes(buf)
            }
        };

        if let Some(inst) = decode(w) {
            let reencoded = encode(inst);
            assert_eq!(
                decode(reencoded),
                Some(inst),
                "decode/encode round-trip failed for word {w:#010x}"
            );
        }

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "soak_decode_32 case exceeded 1s"
        );
        cases += 1;
    }

    println!("soak_decode_32: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_decode_16() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x1616_1616);
    let corpus = valid_compressed_corpus(&mut rng);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget {
        let case_start = Instant::now();
        let h = if rng.chance(1, 2) {
            rng.u64() as u16
        } else {
            let valid_h = *rng.pick(&corpus);
            let inst = decode_c(valid_h).unwrap();
            assert_eq!(decode(encode(inst)), Some(inst));
            let bytes = mutate_bytes(&mut rng, &valid_h.to_le_bytes());
            if bytes.len() >= 2 {
                u16::from_le_bytes([bytes[0], bytes[1]])
            } else if bytes.len() == 1 {
                u16::from(bytes[0])
            } else {
                0
            }
        };

        if let Some(inst) = decode_c(h) {
            assert_eq!(
                decode(encode(inst)),
                Some(inst),
                "decode_c/encode/decode round-trip failed for halfword {h:#06x}"
            );
        }

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "soak_decode_16 case exceeded 1s"
        );
        cases += 1;
    }

    println!("soak_decode_16: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_machine_ram() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x4D41_4348);
    let compressed_corpus = valid_compressed_corpus(&mut rng);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget {
        let case_start = Instant::now();
        let stream: Vec<u8> = if rng.chance(1, 2) {
            rng.bytes(2048)
        } else {
            let count = 1 + rng.below(128);
            let mut valid = Vec::with_capacity(count * 4);
            for _ in 0..count {
                if rng.chance(1, 4) {
                    let h = *rng.pick(&compressed_corpus);
                    valid.extend_from_slice(&h.to_le_bytes());
                } else {
                    let w = encode(gen_valid_inst(&mut rng));
                    valid.extend_from_slice(&w.to_le_bytes());
                }
            }
            mutate_bytes(&mut rng, &valid)
        };

        let ram_size = 64 * 1024;
        let cfg = Config {
            ram_size,
            disk: Vec::new(),
            fb_width: 16,
            fb_height: 16,
            decode_cache: rng.chance(1, 2),
        };
        let mut machine = Machine::new(cfg);

        // Keep load within RAM bounds (<= 64 KiB).
        let to_load = &stream[..stream.len().min(ram_size)];
        machine.load(RAM_BASE, to_load).unwrap();

        if rng.chance(1, 2) {
            for reg in 1..32 {
                machine.cpu.x[reg] = rng.u64();
            }
        }

        let _exit = machine.run(256);

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "soak_machine_ram case exceeded 1s"
        );
        cases += 1;
    }

    println!("soak_machine_ram: seed={seed:#x} cases={cases}");
}
