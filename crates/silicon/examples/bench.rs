//! Benchmark comparing `Machine::step` with `decode_cache` on and off.
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use silicon::inst::{encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};

const SMOKE: &[u8] = include_bytes!("../tests/fixtures/ac/smoke.bin");
const SMOKE_TARGET_INSTS: u64 = 20_000_000;
const LOOP_ITERS: u32 = 5_000_000;

fn build_loop_prog() -> Vec<u8> {
    let lo = (LOOP_ITERS & 0xFFF) as i32;
    let lo_sext = (lo << 20) >> 20;
    let hi = LOOP_ITERS.wrapping_sub(lo_sext as u32) as i32 as i64;
    let insts = [
        Inst::Lui { rd: 1, imm: hi },
        Inst::Addi {
            rd: 1,
            rs1: 1,
            imm: i64::from(lo_sext),
        },
        Inst::Addi {
            rd: 1,
            rs1: 1,
            imm: -1,
        },
        Inst::Bne {
            rs1: 1,
            rs2: 0,
            imm: -4,
        },
        Inst::Lui {
            rd: 2,
            imm: 0x0010_0000,
        },
        Inst::Lui { rd: 3, imm: 0x5000 },
        Inst::Addi {
            rd: 3,
            rs1: 3,
            imm: 0x555,
        },
        Inst::Sw {
            rs1: 2,
            rs2: 3,
            imm: 0,
        },
    ];
    let mut out = Vec::with_capacity(insts.len() * 4);
    for inst in insts {
        out.extend_from_slice(&encode(inst).to_le_bytes());
    }
    out
}

fn bench_smoke(decode_cache: bool) -> (u64, f64) {
    let mut insts = 0u64;
    let t0 = Instant::now();
    while insts < SMOKE_TARGET_INSTS {
        let mut m = Machine::new(Config {
            decode_cache,
            ..Config::default()
        });
        m.load(RAM_BASE, SMOKE).unwrap();
        let exit = m.run(10_000_000);
        assert_eq!(exit, Exit::Pass);
        insts += m.cpu.instret;
    }
    (insts, t0.elapsed().as_secs_f64())
}

fn bench_loop(prog: &[u8], decode_cache: bool) -> (u64, f64) {
    let mut m = Machine::new(Config {
        ram_size: 64 * 1024,
        decode_cache,
        ..Config::default()
    });
    m.load(RAM_BASE, prog).unwrap();
    let t0 = Instant::now();
    let exit = m.run(20_000_000);
    let dt = t0.elapsed().as_secs_f64();
    assert_eq!(exit, Exit::Pass);
    (m.cpu.instret, dt)
}

fn mips(insts: u64, secs: f64) -> f64 {
    (insts as f64) / (secs * 1_000_000.0)
}

fn main() {
    let loop_prog = build_loop_prog();

    let (sa_on, ta_on) = bench_smoke(true);
    let (sa_off, ta_off) = bench_smoke(false);
    let ma_on = mips(sa_on, ta_on);
    let ma_off = mips(sa_off, ta_off);
    println!(
        "smoke: cache-on {ma_on:.2} MIPS, cache-off {ma_off:.2} MIPS, speed-up {:.2}x",
        ma_on / ma_off
    );

    let (sb_on, tb_on) = bench_loop(&loop_prog, true);
    let (sb_off, tb_off) = bench_loop(&loop_prog, false);
    let mb_on = mips(sb_on, tb_on);
    let mb_off = mips(sb_off, tb_off);
    println!(
        "loop: cache-on {mb_on:.2} MIPS, cache-off {mb_off:.2} MIPS, speed-up {:.2}x",
        mb_on / mb_off
    );

    let tot_on = mips(sa_on + sb_on, ta_on + tb_on);
    let tot_off = mips(sa_off + sb_off, ta_off + tb_off);
    println!(
        "bench: cache-on {tot_on:.2} MIPS, cache-off {tot_off:.2} MIPS, speed-up {:.2}x",
        tot_on / tot_off
    );
}
