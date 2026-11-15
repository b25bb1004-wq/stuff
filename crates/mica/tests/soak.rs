//! Soak fuzz tests for the Mica lexer, parser, printer round-trip, and type checker.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use mica::check::check;
use mica::lexer::lex;
use mica::parser::parse;
use mica::print::print;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_SEED: u64 = 0x5EED79A;
const DEFAULT_MS: u64 = 300;
const MAX_INPUT_BYTES: usize = 64 * 1024;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let trimmed = s.trim();
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
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

fn no_span<T: Debug>(value: &T) -> String {
    let text = format!("{value:?}");
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Span { start:") {
        out.push_str(&rest[..i]);
        out.push_str("Span");
        match rest[i..].find('}') {
            Some(j) => rest = &rest[i + j + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

fn load_corpus() -> Vec<String> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests");
    dir.push("corpus");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "mica"))
        .collect();
    entries.sort();
    let mut inputs = Vec::with_capacity(entries.len() * 2);
    for path in entries {
        let src = fs::read_to_string(&path).unwrap();
        if let Ok(m) = parse(&src) {
            inputs.push(print(&m));
        }
        inputs.push(src);
    }
    assert!(!inputs.is_empty(), "tests/corpus must not be empty");
    inputs
}

fn random_input(rng: &mut Rng) -> String {
    if rng.chance(1, 5) {
        // Deeply nested delimiters or unary operators to stress recursion limits.
        let depth = rng.range(100, 450) as usize;
        let (open, close) = *rng.pick(&[("(", ")"), ("[", "]"), ("{", "}"), ("-", "")]);
        let mut s = String::with_capacity(depth * (open.len() + close.len()) + 16);
        if rng.chance(1, 2) {
            s.push_str("fn f() { ");
        }
        for _ in 0..depth {
            s.push_str(open);
        }
        s.push('0');
        for _ in 0..depth {
            s.push_str(close);
        }
        if s.starts_with("fn f()") {
            s.push_str(" }");
        }
        return s;
    }
    let len = rng.range(0, 2048) as usize;
    if rng.chance(1, 2) {
        let mut bytes = vec![0u8; len];
        rng.fill(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        const CHARS: &[u8] =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_ \n\t;()[]{},.:+-*/%<>=!&|^?\"'\\#";
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            s.push(*rng.pick(CHARS) as char);
        }
        s
    }
}

fn mutate_input(corpus: &[String], rng: &mut Rng) -> String {
    let a = rng.pick(corpus);
    let b = rng.pick(corpus);
    let mut bytes = a.as_bytes().to_vec();

    match rng.below(4) {
        0 => {
            // Bit flips.
            if bytes.is_empty() {
                bytes.push(rng.u64() as u8);
            } else {
                let flips = rng.range(1, 9);
                for _ in 0..flips {
                    let idx = rng.below(bytes.len());
                    let bit = 1u8 << rng.below(8);
                    bytes[idx] ^= bit;
                }
            }
        }
        1 => {
            // Truncation.
            if !bytes.is_empty() {
                let new_len = rng.below(bytes.len() + 1);
                bytes.truncate(new_len);
            }
        }
        2 => {
            // Splice from another corpus file.
            let b_bytes = b.as_bytes();
            let cut_a = rng.below(bytes.len() + 1);
            let cut_b = rng.below(b_bytes.len() + 1);
            bytes.truncate(cut_a);
            bytes.extend_from_slice(&b_bytes[cut_b..]);
        }
        _ => {
            // Repeated chunk.
            if !bytes.is_empty() {
                let start = rng.below(bytes.len());
                let end = start + rng.range(1, (bytes.len() - start).min(128) as u64 + 1) as usize;
                let chunk = bytes[start..end].to_vec();
                let reps = rng.range(2, 24) as usize;
                let insert_at = rng.below(bytes.len() + 1);
                let mut out = Vec::with_capacity(bytes.len() + chunk.len() * reps);
                out.extend_from_slice(&bytes[..insert_at]);
                for _ in 0..reps {
                    if out.len() + chunk.len() > MAX_INPUT_BYTES {
                        break;
                    }
                    out.extend_from_slice(&chunk);
                }
                out.extend_from_slice(&bytes[insert_at..]);
                bytes = out;
            }
        }
    }

    if bytes.len() > MAX_INPUT_BYTES {
        bytes.truncate(MAX_INPUT_BYTES);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn next_case(corpus: &[String], rng: &mut Rng) -> String {
    if rng.chance(1, 2) {
        random_input(rng)
    } else {
        mutate_input(corpus, rng)
    }
}

#[test]
fn soak_lexer() {
    let seed = soak_seed();
    let budget = soak_budget();
    let corpus = load_corpus();
    let mut master = Rng::new(seed ^ 0x4C45_5845);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while cases == 0 || start.elapsed() < budget {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let src = next_case(&corpus, &mut rng);
        let t0 = Instant::now();
        let _ = lex(&src);
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "soak_lexer case {cases} (seed {case_seed:#018x}) took {elapsed:?}"
        );
        cases += 1;
    }
    println!(
        "soak_lexer: seed={seed:#018x} cases={cases} elapsed={:?}",
        start.elapsed()
    );
}

#[test]
fn soak_parser_and_roundtrip() {
    let seed = soak_seed();
    let budget = soak_budget();
    let corpus = load_corpus();
    let mut master = Rng::new(seed ^ 0x5041_5253);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while cases == 0 || start.elapsed() < budget {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let src = next_case(&corpus, &mut rng);
        let t0 = Instant::now();
        if let Ok(m1) = parse(&src) {
            let p1 = print(&m1);
            let m2 = parse(&p1).unwrap_or_else(|d| {
                panic!(
                    "soak_parser case {cases} (seed {case_seed:#018x}) reprint failed: {d:?}\nsrc={src:?}\np1={p1:?}"
                )
            });
            assert_eq!(
                print(&m2),
                p1,
                "soak_parser case {cases} (seed {case_seed:#018x}) print not idempotent"
            );
            assert_eq!(
                no_span(&m2),
                no_span(&m1),
                "soak_parser case {cases} (seed {case_seed:#018x}) AST changed on roundtrip"
            );
        }
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "soak_parser case {cases} (seed {case_seed:#018x}) took {elapsed:?}"
        );
        cases += 1;
    }
    println!(
        "soak_parser_and_roundtrip: seed={seed:#018x} cases={cases} elapsed={:?}",
        start.elapsed()
    );
}

#[test]
fn soak_type_checker() {
    let seed = soak_seed();
    let budget = soak_budget();
    let corpus = load_corpus();
    let mut master = Rng::new(seed ^ 0x4348_4B52);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while cases == 0 || start.elapsed() < budget {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let src = next_case(&corpus, &mut rng);
        let t0 = Instant::now();
        if let Ok(m) = parse(&src) {
            let _ = check(&m);
        }
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "soak_type_checker case {cases} (seed {case_seed:#018x}) took {elapsed:?}"
        );
        cases += 1;
    }
    println!(
        "soak_type_checker: seed={seed:#018x} cases={cases} elapsed={:?}",
        start.elapsed()
    );
}
