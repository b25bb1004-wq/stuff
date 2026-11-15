//! Fuzz soak tests for cairn parsers and package decoders.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use assay::Rng;
use cairn::lock;
use cairn::manifest::Manifest;
use cairn::pkg::{self, Package};
use cairn::solve::Solution;
use cairn::version::{Req, Version};
use onyx::ed25519;

const DEFAULT_SEED: u64 = 0x05EE_D796;
const DEFAULT_MS: u64 = 300;
const MAX_INPUT_BYTES: usize = 64 * 1024;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(val) => {
            let trimmed = val.trim().replace('_', "");
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
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

fn gen_u64_interesting(rng: &mut Rng) -> u64 {
    match rng.below(10) {
        0 => 0,
        1 => 1,
        2 => u64::MAX,
        3 => u64::MAX - 1,
        4..=7 => rng.range(0, 20),
        _ => rng.range(0, 10_000),
    }
}

fn gen_version(rng: &mut Rng) -> Version {
    Version {
        major: gen_u64_interesting(rng),
        minor: gen_u64_interesting(rng),
        patch: gen_u64_interesting(rng),
    }
}

fn gen_req_str(rng: &mut Rng) -> String {
    let count = rng.range(1, 6) as usize;
    let ops = ["", "^", "~", "=", ">", ">=", "<", "<="];
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        if rng.chance(1, 5) {
            match rng.below(3) {
                0 => parts.push("*".to_string()),
                1 => parts.push(format!("{}.*", gen_u64_interesting(rng))),
                _ => parts.push(format!(
                    "{}.{}.*",
                    gen_u64_interesting(rng),
                    gen_u64_interesting(rng)
                )),
            }
        } else {
            let op = *rng.pick(&ops);
            let m = gen_u64_interesting(rng);
            let n = gen_u64_interesting(rng);
            let p = gen_u64_interesting(rng);
            match rng.below(3) {
                0 => parts.push(format!("{op}{m}")),
                1 => parts.push(format!("{op}{m}.{n}")),
                _ => parts.push(format!("{op}{m}.{n}.{p}")),
            }
        }
    }
    parts.join(", ")
}

fn gen_req(rng: &mut Rng) -> Req {
    let s = gen_req_str(rng);
    let req = Req::parse(&s).unwrap();
    let canonical = req.to_string();
    let reparsed = Req::parse(&canonical).unwrap();
    assert_eq!(req, reparsed, "Req round-trip failed for {s:?}");
    req
}

fn gen_name(rng: &mut Rng) -> String {
    let len = rng.range(1, 16) as usize;
    let first = (b'a' + (rng.below(26) as u8)) as char;
    let mut s = String::with_capacity(len);
    s.push(first);
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
    for _ in 1..len {
        let b = *rng.pick(alphabet);
        s.push(b as char);
    }
    s
}

fn gen_manifest(rng: &mut Rng) -> Manifest {
    let name = gen_name(rng);
    let version = gen_version(rng);
    let dep_count = rng.below(9);
    let mut dependencies = Vec::with_capacity(dep_count);
    for _ in 0..dep_count {
        let dep_name = gen_name(rng);
        if dependencies.iter().any(|(d, _)| d == &dep_name) {
            continue;
        }
        dependencies.push((dep_name, gen_req(rng)));
    }
    Manifest {
        name,
        version,
        dependencies,
    }
}

fn gen_solution(rng: &mut Rng) -> Solution {
    let count = rng.below(13);
    let mut sol = BTreeMap::new();
    for _ in 0..count {
        sol.insert(gen_name(rng), gen_version(rng));
    }
    sol
}

fn gen_rel_path(rng: &mut Rng) -> String {
    let segs = rng.range(1, 5) as usize;
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut path = String::new();
    for i in 0..segs {
        if i > 0 {
            path.push('/');
        }
        let seg_len = rng.range(1, 12) as usize;
        for _ in 0..seg_len {
            path.push(*rng.pick(alphabet) as char);
        }
    }
    if path.len() > 255 {
        path.truncate(255);
        while path.ends_with('/') {
            path.pop();
        }
    }
    if path.is_empty() {
        path.push('a');
    }
    path
}

fn gen_package(rng: &mut Rng) -> Package {
    let manifest = gen_manifest(rng);
    let file_count = rng.below(9);
    let mut files = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        let path = gen_rel_path(rng);
        if files.iter().any(|(p, _)| p == &path) {
            continue;
        }
        let data = rng.bytes(512);
        files.push((path, data));
    }
    Package { manifest, files }
}

fn random_text(rng: &mut Rng, max_len: usize) -> String {
    if rng.chance(1, 2) {
        let len = rng.below(max_len + 1);
        let mut s = String::with_capacity(len);
        let interesting = b"abcdefghijklmnopqrstuvwxyz0123456789._-=[]\"#*<>=^~, \n\r\t:/\\";
        for _ in 0..len {
            if rng.chance(3, 4) {
                s.push(*rng.pick(interesting) as char);
            } else {
                let b = (rng.u64() & 0x7f) as u8;
                s.push(b as char);
            }
        }
        s
    } else {
        let bytes = rng.bytes(max_len);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn mutate_bytes(rng: &mut Rng, base: &[u8], other: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    let ops = rng.range(1, 5);
    for _ in 0..ops {
        match rng.below(5) {
            // Bit flips
            0 => {
                if !out.is_empty() {
                    let idx = rng.below(out.len());
                    let bit = 1u8 << rng.below(8);
                    out[idx] ^= bit;
                }
            }
            // Truncation
            1 => {
                if !out.is_empty() {
                    let new_len = rng.below(out.len() + 1);
                    out.truncate(new_len);
                }
            }
            // Splice from `other`
            2 => {
                if !other.is_empty() {
                    let start = rng.below(other.len());
                    let end = rng.range(start as u64, other.len() as u64 + 1) as usize;
                    let slice = &other[start..end];
                    let pos = rng.below(out.len() + 1);
                    if out.len().saturating_add(slice.len()) <= MAX_INPUT_BYTES {
                        out.splice(pos..pos, slice.iter().copied());
                    }
                }
            }
            // Repeated chunk
            3 => {
                if !out.is_empty() {
                    let start = rng.below(out.len());
                    let max_end = (start + 64).min(out.len());
                    let end = rng.range(start as u64, max_end as u64 + 1) as usize;
                    let chunk = out[start..end].to_vec();
                    let reps = rng.range(2, 6) as usize;
                    for _ in 0..reps {
                        if out.len().saturating_add(chunk.len()) <= MAX_INPUT_BYTES {
                            out.extend_from_slice(&chunk);
                        }
                    }
                }
            }
            // Insert random bytes
            _ => {
                let extra = rng.bytes(32);
                let pos = rng.below(out.len() + 1);
                if out.len().saturating_add(extra.len()) <= MAX_INPUT_BYTES {
                    out.splice(pos..pos, extra);
                }
            }
        }
    }
    out
}

fn mutate_text(rng: &mut Rng, base: &str, other: &str) -> String {
    let mutated = mutate_bytes(rng, base.as_bytes(), other.as_bytes());
    String::from_utf8_lossy(&mutated).into_owned()
}

fn check_elapsed(target: &str, seed: u64, case: u64, start: Instant) {
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "target {target} exceeded 1s on case {case} (seed {seed:#x}): {elapsed:?}"
    );
}

#[test]
fn soak_version_parse() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x1);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget || cases < 100 {
        let valid = gen_version(&mut rng);
        let valid_s = valid.to_string();
        assert_eq!(Version::parse(&valid_s), Ok(valid));

        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 128)
        } else {
            let other = gen_version(&mut rng).to_string();
            mutate_text(&mut rng, &valid_s, &other)
        };

        let case_start = Instant::now();
        let _ = Version::parse(&input);
        check_elapsed("Version::parse", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=Version::parse seed={seed:#x} cases={cases}");
}

#[test]
fn soak_req_parse() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x2);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget || cases < 100 {
        let valid = gen_req(&mut rng);
        let valid_s = valid.to_string();
        let probe_ver = gen_version(&mut rng);
        let _ = valid.matches(&probe_ver);

        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 256)
        } else {
            let other = gen_req_str(&mut rng);
            mutate_text(&mut rng, &valid_s, &other)
        };

        let case_start = Instant::now();
        if let Ok(req) = Req::parse(&input) {
            let _ = req.matches(&probe_ver);
        }
        check_elapsed("Req::parse", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=Req::parse seed={seed:#x} cases={cases}");
}

#[test]
fn soak_manifest_parse() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x3);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget || cases < 100 {
        let valid = gen_manifest(&mut rng);
        let valid_s = valid.to_string();
        assert_eq!(Manifest::parse(&valid_s), Ok(valid));

        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 1024)
        } else {
            let other = gen_manifest(&mut rng).to_string();
            mutate_text(&mut rng, &valid_s, &other)
        };

        let case_start = Instant::now();
        let _ = Manifest::parse(&input);
        check_elapsed("Manifest::parse", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=Manifest::parse seed={seed:#x} cases={cases}");
}

#[test]
fn soak_lock_read() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x4);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget || cases < 100 {
        let valid = gen_solution(&mut rng);
        let valid_s = lock::write(&valid);
        assert_eq!(lock::read(&valid_s), Ok(valid));

        let input = if cases.is_multiple_of(2) {
            random_text(&mut rng, 1024)
        } else {
            let other = lock::write(&gen_solution(&mut rng));
            mutate_text(&mut rng, &valid_s, &other)
        };

        let case_start = Instant::now();
        let _ = lock::read(&input);
        check_elapsed("lock::read", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=lock::read seed={seed:#x} cases={cases}");
}

#[test]
fn soak_pkg_decode() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x5);
    let start = Instant::now();
    let mut cases: u64 = 0;

    while start.elapsed() < budget || cases < 100 {
        let mut valid = gen_package(&mut rng);
        let encoded = pkg::encode(&valid);
        valid
            .files
            .sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        assert_eq!(pkg::decode(&encoded), Ok(valid));

        let input = if cases.is_multiple_of(2) {
            rng.bytes(4096)
        } else {
            let other = pkg::encode(&gen_package(&mut rng));
            mutate_bytes(&mut rng, &encoded, &other)
        };

        let case_start = Instant::now();
        let _ = pkg::decode(&input);
        check_elapsed("pkg::decode", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=pkg::decode seed={seed:#x} cases={cases}");
}

#[test]
fn soak_pkg_open() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x6);
    let start = Instant::now();
    let mut cases: u64 = 0;

    let signing_seed = [0x42u8; 32];
    let pk = ed25519::public_key(&signing_seed);
    let trusted = [pk];

    while start.elapsed() < budget || cases < 100 {
        let mut valid = gen_package(&mut rng);
        let signed = pkg::sign(&valid, &signing_seed);
        valid
            .files
            .sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        assert_eq!(pkg::open(&signed, &trusted), Ok(valid));

        let input = if cases.is_multiple_of(2) {
            rng.bytes(4096)
        } else {
            let other = pkg::sign(&gen_package(&mut rng), &signing_seed);
            mutate_bytes(&mut rng, &signed, &other)
        };

        let case_start = Instant::now();
        let _ = pkg::open(&input, &trusted);
        check_elapsed("pkg::open", seed, cases, case_start);
        cases += 1;
    }

    eprintln!("[soak] target=pkg::open seed={seed:#x} cases={cases}");
}
