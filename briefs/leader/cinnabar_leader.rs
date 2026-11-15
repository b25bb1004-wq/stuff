//! Leader check for T14 (Cinnabar). Copied to crates/onyx/tests/cinnabar_leader.rs. Known answers
//! come from briefs/leader/cinnabar_ref.py (an independent Python model of docs/CINNABAR.md).
#![allow(clippy::unwrap_used)]
use assay::{hex, Rng};
use onyx::cinnabar::{ctr_xor, Cinnabar, ROUNDS};

const VECTORS: &str = include_str!("fixtures/cinnabar.txt");

fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().unwrap()
}
fn bytes(s: &str) -> Vec<u8> {
    if s == "-" {
        Vec::new()
    } else {
        hex(s)
    }
}
fn key(r: &mut Rng) -> [u8; 32] {
    let mut k = [0u8; 32];
    r.fill(&mut k);
    k
}
fn block(r: &mut Rng) -> [u8; 16] {
    let mut b = [0u8; 16];
    r.fill(&mut b);
    b
}
fn popcount(a: &[u8; 16], b: &[u8; 16]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

#[test]
fn leader_known_answers() {
    assert_eq!(ROUNDS, 24);
    let (mut nb, mut nc) = (0, 0);
    for l in VECTORS.lines() {
        let w: Vec<&str> = l.split(' ').collect();
        match w[0] {
            "block" => {
                let c = Cinnabar::new(&arr(w[1]));
                let (pt, ct): ([u8; 16], [u8; 16]) = (arr(w[2]), arr(w[3]));
                let mut b = pt;
                c.encrypt_block(&mut b);
                assert_eq!(b, ct, "encrypt {l}");
                c.decrypt_block(&mut b);
                assert_eq!(b, pt, "decrypt {l}");
                nb += 1;
            }
            "ctr" => {
                let (k, n, ctr) = (arr(w[1]), arr(w[2]), w[3].parse().unwrap());
                let (pt, ct) = (bytes(w[4]), bytes(w[5]));
                let mut d = pt.clone();
                ctr_xor(&k, &n, ctr, &mut d);
                assert_eq!(d, ct, "ctr {l}");
                ctr_xor(&k, &n, ctr, &mut d);
                assert_eq!(d, pt, "ctr back {l}");
                nc += 1;
            }
            _ => panic!("bad vector line {l}"),
        }
    }
    assert_eq!((nb, nc), (64, 24));
}

#[test]
fn leader_round_trips() {
    let mut r = Rng::new(0x14_7777_0001);
    for _ in 0..2000 {
        let c = Cinnabar::new(&key(&mut r));
        let pt = block(&mut r);
        let mut b = pt;
        c.encrypt_block(&mut b);
        assert_ne!(b, pt);
        c.decrypt_block(&mut b);
        assert_eq!(b, pt);
    }
}

#[test]
fn leader_avalanche() {
    let mut r = Rng::new(0x14_7777_0002);
    let n = 4000u32;
    let (mut pt_total, mut key_total) = (0u32, 0u32);
    let mut per_bit = [0u32; 128];
    for _ in 0..n {
        let k = key(&mut r);
        let c = Cinnabar::new(&k);
        let pt = block(&mut r);
        let mut base = pt;
        c.encrypt_block(&mut base);
        let mut p2 = pt;
        p2[r.below(16)] ^= 1 << r.below(8);
        c.encrypt_block(&mut p2);
        pt_total += popcount(&base, &p2);
        for (i, slot) in per_bit.iter_mut().enumerate() {
            *slot += u32::from((base[i / 8] ^ p2[i / 8]) >> (i % 8) & 1);
        }
        let mut k2 = k;
        k2[r.below(32)] ^= 1 << r.below(8);
        let mut b2 = pt;
        Cinnabar::new(&k2).encrypt_block(&mut b2);
        key_total += popcount(&base, &b2);
    }
    let (pa, ka) = (
        f64::from(pt_total) / f64::from(n),
        f64::from(key_total) / f64::from(n),
    );
    assert!((pa - 64.0).abs() <= 1.5, "plaintext avalanche {pa}");
    assert!((ka - 64.0).abs() <= 1.5, "key avalanche {ka}");
    for (i, &f) in per_bit.iter().enumerate() {
        let p = f64::from(f) / f64::from(n);
        assert!((p - 0.5).abs() <= 0.05, "output bit {i} flips with p {p}");
    }
}

#[test]
fn leader_keystream_bias() {
    let mut r = Rng::new(0x14_7777_0003);
    for _ in 0..2 {
        let (k, mut n) = (key(&mut r), [0u8; 12]);
        r.fill(&mut n);
        let mut ks = vec![0u8; 1 << 20];
        ctr_xor(&k, &n, r.u64() as u32, &mut ks);
        let ones: u64 = ks.iter().map(|b| u64::from(b.count_ones())).sum();
        let frac = ones as f64 / (8.0 * ks.len() as f64);
        assert!((frac - 0.5).abs() <= 0.002, "ones fraction {frac}");
        let mut cnt = [0u64; 256];
        ks.iter().for_each(|&b| cnt[b as usize] += 1);
        let e = ks.len() as f64 / 256.0;
        let chi: f64 = cnt.iter().map(|&c| (c as f64 - e).powi(2) / e).sum();
        assert!(chi < 330.0, "byte chi-squared {chi}");
    }
}

#[test]
fn leader_counter_mode_structure() {
    let mut r = Rng::new(0x14_7777_0004);
    for case in 0..50 {
        let (k, mut n) = (key(&mut r), [0u8; 12]);
        r.fill(&mut n);
        let ctr = if case % 5 == 0 {
            u32::MAX - 1
        } else {
            r.u64() as u32
        };
        let data = r.bytes(200);
        // Hand-built keystream from the block function.
        let c = Cinnabar::new(&k);
        let mut want = data.clone();
        for (j, chunk) in want.chunks_mut(16).enumerate() {
            let mut b = [0u8; 16];
            b[..12].copy_from_slice(&n);
            b[12..].copy_from_slice(&ctr.wrapping_add(j as u32).to_le_bytes());
            c.encrypt_block(&mut b);
            chunk.iter_mut().zip(b).for_each(|(x, y)| *x ^= y);
        }
        let mut got = data.clone();
        ctr_xor(&k, &n, ctr, &mut got);
        assert_eq!(got, want, "case {case}");
        // Splitting at a block boundary gives the same bytes.
        if data.len() > 32 {
            let cut = 16 * (1 + r.below(data.len() / 16 - 1));
            let mut split = data.clone();
            let (a, b) = split.split_at_mut(cut);
            ctr_xor(&k, &n, ctr, a);
            ctr_xor(&k, &n, ctr.wrapping_add((cut / 16) as u32), b);
            assert_eq!(split, want, "case {case}: split at {cut}");
        }
    }
}

#[test]
fn leader_labelled_and_isolated() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let own = std::fs::read_to_string(format!("{dir}/cinnabar.rs")).unwrap();
    assert!(
        own.trim_start().starts_with("//! EXPERIMENTAL"),
        "module doc must start with EXPERIMENTAL"
    );
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name == "cinnabar.rs" || p.is_dir() {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap().to_lowercase();
        let uses = text.matches("cinnabar").count();
        let allowed = if name == "lib.rs" { 2 } else { 0 };
        assert!(uses <= allowed, "{name} mentions cinnabar {uses} times");
    }
    let at = own.find("pub struct Cinnabar").unwrap();
    let before = &own[at.saturating_sub(120)..at];
    let derive = before.rfind("#[derive(").map(|d| &before[d..]);
    assert!(
        !derive.is_some_and(|d| d.contains("Debug")),
        "Cinnabar must not derive Debug"
    );
}
