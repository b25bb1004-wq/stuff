//! Fuzz soak for `geode`: decode, verify, and VM execution with small fuel.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use geode::{decode, encode, verify, Const, Func, Heap, Host, Limits, Module, Op, Trap, Value, Vm};
use std::time::{Duration, Instant};

const DEFAULT_SEED: u64 = 0x05EE_D794;
const DEFAULT_MS: u64 = 300;
const MAX_CASE_TIME: Duration = Duration::from_secs(1);

fn soak_seed() -> u64 {
    let Ok(raw) = std::env::var("STRATA_SOAK_SEED") else {
        return DEFAULT_SEED;
    };
    let clean: String = raw.chars().filter(|&c| c != '_').collect();
    if let Some(hex) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).unwrap_or(DEFAULT_SEED)
    } else {
        clean.parse::<u64>().unwrap_or(DEFAULT_SEED)
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

struct DummyHost;

impl Host for DummyHost {
    fn call(&mut self, name: &str, args: &[Value], _heap: &mut Heap) -> Result<Value, Trap> {
        if name == "io.f0" {
            Ok(args.first().copied().unwrap_or(Value::Unit))
        } else {
            Err(Trap::Host(String::from("stub")))
        }
    }
}

fn span(r: &mut Rng, regs: u16) -> (u16, u16) {
    let n = r.below(usize::from(regs.min(4)) + 1);
    let first = r.below(usize::from(regs) - n + 1);
    (first as u16, n as u16)
}

fn gen_op(
    r: &mut Rng,
    regs: u16,
    len: usize,
    nconsts: usize,
    strs: &[u32],
    nimports: usize,
    params: &[u16],
) -> Op {
    let reg = |r: &mut Rng| r.below(usize::from(regs)) as u16;
    let d = reg(r);
    let a = reg(r);
    let b = reg(r);
    let c = reg(r);
    let (first, n) = span(r, regs);
    let f = r.below(params.len());
    let callee_n = params[f];
    let call_first = r.below(usize::from(regs) - usize::from(callee_n) + 1) as u16;
    let t = r.below(len) as u32;
    let k = r.below(nconsts) as u32;
    let s = *r.pick(strs);
    let h = r.below(nimports) as u32;
    let fi = r.below(4) as u16;
    let imm = r.u64() as i64;
    let byte = r.u64() as u8;
    let flag = r.chance(1, 2);
    let word = r.u64() as u32;
    match r.below(57) {
        0 => Op::Int(d, imm),
        1 => Op::Byte(d, byte),
        2 => Op::Bool(d, flag),
        3 => Op::Unit(d),
        4 => Op::FnRef(d, f as u32),
        5 => Op::Const(d, k),
        6 => Op::Move(d, a),
        7 => Op::Add(d, a, b),
        8 => Op::Sub(d, a, b),
        9 => Op::Mul(d, a, b),
        10 => Op::AddW(d, a, b),
        11 => Op::SubW(d, a, b),
        12 => Op::MulW(d, a, b),
        13 => Op::Div(d, a, b),
        14 => Op::Rem(d, a, b),
        15 => Op::Neg(d, a),
        16 => Op::And(d, a, b),
        17 => Op::Or(d, a, b),
        18 => Op::Xor(d, a, b),
        19 => Op::Shl(d, a, b),
        20 => Op::Shr(d, a, b),
        21 => Op::Not(d, a),
        22 => Op::Eq(d, a, b),
        23 => Op::Ne(d, a, b),
        24 => Op::Lt(d, a, b),
        25 => Op::Le(d, a, b),
        26 => Op::Gt(d, a, b),
        27 => Op::Ge(d, a, b),
        28 => Op::Jump(t),
        29 => Op::JumpIf(a, t),
        30 => Op::JumpIfNot(a, t),
        31 => Op::Call(d, f as u32, call_first, callee_n),
        32 => Op::CallValue(d, a, first, n),
        33 => Op::CallHost(d, h, first, n),
        34 => Op::Ret(a),
        35 => Op::NewArray(d, first, n),
        36 => Op::NewBytes(d, first, n),
        37 => Op::NewFill(d, a, b),
        38 => Op::NewRecord(d, first, n),
        39 => Op::NewVariant(d, word, first, n),
        40 => Op::GetField(d, a, fi),
        41 => Op::SetField(a, fi, b),
        42 => Op::Tag(d, a),
        43 => Op::GetIndex(d, a, b),
        44 => Op::SetIndex(a, b, c),
        45 => Op::Len(d, a),
        46 => Op::Push(a, b),
        47 => Op::Pop(d, a),
        48 => Op::Slice(d, a, b, c),
        49 => Op::Concat(d, a, b),
        50 => Op::IntToStr(d, a),
        51 => Op::ParseInt(d, a),
        52 => Op::StrToBytes(d, a),
        53 => Op::BytesToStr(d, a),
        54 => Op::ToInt(d, a),
        55 => Op::ToByte(d, a),
        _ => Op::Assert(a, s),
    }
}

fn random_module(r: &mut Rng) -> Module {
    let mut consts = vec![Const::Str(String::from("assert failed"))];
    let mut strs = vec![0u32];
    for i in 1..=r.below(4) {
        if r.chance(1, 2) {
            strs.push(i as u32);
            consts.push(Const::Str(format!("msg{i}")));
        } else {
            consts.push(Const::Bytes(r.bytes(8)));
        }
    }
    let imports: Vec<String> = (0..=r.below(2)).map(|i| format!("io.f{i}")).collect();
    let nfuncs = 1 + r.below(4);
    let params: Vec<u16> = (0..nfuncs).map(|_| r.below(4) as u16).collect();
    let maxp = params.iter().copied().max().unwrap_or(0);
    let mut funcs = Vec::with_capacity(nfuncs);
    for (f, &p) in params.iter().enumerate() {
        let regs = maxp + 1 + r.below(6) as u16;
        let body = 1 + r.below(24);
        let mut code = Vec::with_capacity(body + 1);
        for _ in 0..body {
            code.push(gen_op(
                r,
                regs,
                body + 1,
                consts.len(),
                &strs,
                imports.len(),
                &params,
            ));
        }
        code.push(Op::Ret(r.below(usize::from(regs)) as u16));
        funcs.push(Func {
            name: format!("f{f}"),
            params: p,
            regs,
            code,
        });
    }
    let entry = r.below(nfuncs) as u32;
    Module {
        consts,
        imports,
        funcs,
        entry,
    }
}

fn mutate_bytes(r: &mut Rng, base: &[u8], other: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    match r.below(4) {
        0 => {
            // Bit flips
            if !out.is_empty() {
                let flips = 1 + r.below(8);
                for _ in 0..flips {
                    let idx = r.below(out.len());
                    let bit = 1u8 << r.below(8);
                    out[idx] ^= bit;
                }
            }
        }
        1 => {
            // Truncation
            let new_len = r.below(out.len() + 1);
            out.truncate(new_len);
        }
        2 => {
            // Splice from another encoded module
            if !out.is_empty() && !other.is_empty() {
                let cut_a = r.below(out.len() + 1);
                let start_b = r.below(other.len());
                let end_b = start_b + r.below(other.len() - start_b + 1);
                out.truncate(cut_a);
                out.extend_from_slice(&other[start_b..end_b]);
            }
        }
        _ => {
            // Repeated chunk
            if !out.is_empty() {
                let start = r.below(out.len());
                let len = 1 + r.below((out.len() - start).min(64));
                let chunk = out[start..start + len].to_vec();
                let reps = 2 + r.below(7);
                for _ in 0..reps {
                    if out.len() + chunk.len() > 64 * 1024 {
                        break;
                    }
                    out.extend_from_slice(&chunk);
                }
            }
        }
    }
    out
}

fn mutate_module(r: &mut Rng, mut m: Module) -> Module {
    match r.below(6) {
        0 => m.entry = r.u64() as u32,
        1 => {
            if let Some(f) = m.funcs.first_mut() {
                f.regs = (r.u64() as u16).min(8192);
                f.params = r.u64() as u16;
            }
        }
        2 => {
            if let Some(f) = m.funcs.first_mut() {
                f.code.clear();
            }
        }
        3 => {
            if let Some(f) = m.funcs.first_mut() {
                f.code.push(Op::Int(0, 1));
            }
        }
        4 => {
            if let Some(f) = m.funcs.first_mut() {
                if !f.code.is_empty() {
                    let idx = r.below(f.code.len());
                    f.code[idx] = Op::Jump(r.u64() as u32);
                }
            }
        }
        _ => {
            if let Some(f) = m.funcs.first_mut() {
                f.name = "x".repeat(r.below(300));
            }
        }
    }
    m
}

#[test]
fn soak_decode() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut r = Rng::new(seed ^ 0xD3C0_DE01);
    let start = Instant::now();
    let mut cases: u64 = 0;
    while cases == 0 || start.elapsed() < budget {
        let input = if r.chance(1, 2) {
            r.bytes(2048)
        } else {
            let m1 = random_module(&mut r);
            assert_eq!(verify(&m1), Ok(()));
            let enc1 = encode(&m1);
            assert_eq!(decode(&enc1), Ok(m1));
            let enc2 = encode(&random_module(&mut r));
            mutate_bytes(&mut r, &enc1, &enc2)
        };
        let t0 = Instant::now();
        let res = decode(&input);
        assert!(t0.elapsed() < MAX_CASE_TIME, "decode exceeded 1s");
        if let Ok(m) = res {
            let t1 = Instant::now();
            let _ = verify(&m);
            assert!(t1.elapsed() < MAX_CASE_TIME, "verify exceeded 1s");
        }
        cases += 1;
    }
    println!("soak decode: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_verify() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut r = Rng::new(seed ^ 0x7E21_F702);
    let start = Instant::now();
    let mut cases: u64 = 0;
    while cases == 0 || start.elapsed() < budget {
        let t0 = Instant::now();
        if r.chance(1, 2) {
            let bytes = r.bytes(2048);
            if let Ok(m) = decode(&bytes) {
                let _ = verify(&m);
            }
        } else {
            let base = random_module(&mut r);
            let m = if r.chance(1, 2) {
                mutate_module(&mut r, base)
            } else {
                base
            };
            let _ = verify(&m);
        }
        assert!(t0.elapsed() < MAX_CASE_TIME, "verify exceeded 1s");
        cases += 1;
    }
    println!("soak verify: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_vm_exec() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut r = Rng::new(seed ^ 0xE8EC_0003);
    let start = Instant::now();
    let mut cases: u64 = 0;
    let limits = Limits {
        fuel: 500,
        max_heap_bytes: 1024 * 1024,
        max_depth: 32,
    };
    while cases == 0 || start.elapsed() < budget {
        let m = if r.chance(1, 2) {
            random_module(&mut r)
        } else {
            let enc1 = encode(&random_module(&mut r));
            let enc2 = encode(&random_module(&mut r));
            let mut_bytes = mutate_bytes(&mut r, &enc1, &enc2);
            match decode(&mut_bytes) {
                Ok(decoded) if verify(&decoded).is_ok() => decoded,
                _ => random_module(&mut r),
            }
        };
        let t0 = Instant::now();
        let host_names: Vec<&str> = m.imports.iter().map(String::as_str).collect();
        if let Ok(mut vm) = Vm::new(m.clone(), &host_names) {
            let mut host = DummyHost;
            let _ = vm.run(&mut host, limits);
            if let Some(f) = m.funcs.get(r.below(m.funcs.len())) {
                let args: Vec<Value> = (0..f.params).map(|i| Value::Int(i64::from(i))).collect();
                let _ = vm.call(&f.name, &args, &mut host, limits);
            }
        }
        assert!(t0.elapsed() < MAX_CASE_TIME, "vm_exec exceeded 1s");
        cases += 1;
    }
    println!("soak vm_exec: seed={seed:#x} cases={cases}");
}
