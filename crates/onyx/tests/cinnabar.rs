//! Integration and statistical tests for `onyx::cinnabar`.
#![allow(clippy::unwrap_used)]

use assay::{forall, hex, Rng};
use onyx::cinnabar::{ctr_xor, Cinnabar, ROUNDS};

#[test]
fn known_answers() {
    assert_eq!(ROUNDS, 24);
    let fixture = include_str!("fixtures/cinnabar.txt");
    let mut block_count = 0usize;
    let mut ctr_count = 0usize;

    for (line_no, raw_line) in fixture.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["block", key_hex, pt_hex, ct_hex] => {
                let key: [u8; 32] = hex(key_hex).try_into().unwrap();
                let pt: [u8; 16] = hex(pt_hex).try_into().unwrap();
                let ct: [u8; 16] = hex(ct_hex).try_into().unwrap();

                let cipher = Cinnabar::new(&key);
                let mut buf = pt;
                cipher.encrypt_block(&mut buf);
                assert_eq!(buf, ct, "block encrypt mismatch at line {}", line_no + 1);

                cipher.decrypt_block(&mut buf);
                assert_eq!(buf, pt, "block decrypt mismatch at line {}", line_no + 1);
                block_count += 1;
            }
            ["ctr", key_hex, nonce_hex, ctr_str, pt_hex, ct_hex] => {
                let key: [u8; 32] = hex(key_hex).try_into().unwrap();
                let nonce: [u8; 12] = hex(nonce_hex).try_into().unwrap();
                let counter: u32 = ctr_str.parse().unwrap();
                let pt = hex(pt_hex);
                let ct = hex(ct_hex);
                assert_eq!(pt.len(), ct.len());

                let mut buf = pt.clone();
                ctr_xor(&key, &nonce, counter, &mut buf);
                assert_eq!(buf, ct, "ctr encrypt mismatch at line {}", line_no + 1);

                ctr_xor(&key, &nonce, counter, &mut buf);
                assert_eq!(buf, pt, "ctr decrypt mismatch at line {}", line_no + 1);
                ctr_count += 1;
            }
            _ => panic!("unexpected fixture line {}: {line}", line_no + 1),
        }
    }

    assert_eq!(block_count, 64);
    assert_eq!(ctr_count, 24);
}

#[test]
fn round_trip_random() {
    forall(500, 0xC1AA_BA20, |r| {
        let mut key = [0u8; 32];
        r.fill(&mut key);
        let mut pt = [0u8; 16];
        r.fill(&mut pt);
        let cipher = Cinnabar::new(&key);
        let mut buf = pt;
        cipher.encrypt_block(&mut buf);
        cipher.decrypt_block(&mut buf);
        assert_eq!(buf, pt);
    });
}

#[test]
fn avalanche_plaintext_and_key() {
    const SAMPLES: usize = 4000;
    let mut rng = Rng::new(0xA7A1_A4C3_0001);

    // Plaintext bit flips
    let mut total_flips = 0u64;
    let mut bit_flips = [0u32; 128];
    for _ in 0..SAMPLES {
        let mut key = [0u8; 32];
        rng.fill(&mut key);
        let mut pt = [0u8; 16];
        rng.fill(&mut pt);

        let cipher = Cinnabar::new(&key);
        let mut c0 = pt;
        cipher.encrypt_block(&mut c0);

        let bit = rng.below(128);
        pt[bit / 8] ^= 1 << (bit % 8);
        let mut c1 = pt;
        cipher.encrypt_block(&mut c1);

        for byte_idx in 0..16 {
            let diff = c0[byte_idx] ^ c1[byte_idx];
            total_flips += u64::from(diff.count_ones());
            for b in 0..8 {
                if (diff >> b) & 1 == 1 {
                    bit_flips[byte_idx * 8 + b] += 1;
                }
            }
        }
    }
    let avg = total_flips as f64 / SAMPLES as f64;
    assert!(
        (avg - 64.0).abs() <= 1.5,
        "plaintext avalanche avg {avg} out of 64 +- 1.5"
    );
    for (i, &count) in bit_flips.iter().enumerate() {
        let p = f64::from(count) / SAMPLES as f64;
        assert!(
            (p - 0.5).abs() <= 0.05,
            "plaintext output bit {i} flip prob {p} out of 0.5 +- 0.05"
        );
    }

    // Key bit flips
    let mut total_flips = 0u64;
    let mut bit_flips = [0u32; 128];
    for _ in 0..SAMPLES {
        let mut key = [0u8; 32];
        rng.fill(&mut key);
        let mut pt = [0u8; 16];
        rng.fill(&mut pt);

        let mut c0 = pt;
        Cinnabar::new(&key).encrypt_block(&mut c0);

        let bit = rng.below(256);
        key[bit / 8] ^= 1 << (bit % 8);
        let mut c1 = pt;
        Cinnabar::new(&key).encrypt_block(&mut c1);

        for byte_idx in 0..16 {
            let diff = c0[byte_idx] ^ c1[byte_idx];
            total_flips += u64::from(diff.count_ones());
            for b in 0..8 {
                if (diff >> b) & 1 == 1 {
                    bit_flips[byte_idx * 8 + b] += 1;
                }
            }
        }
    }
    let avg = total_flips as f64 / SAMPLES as f64;
    assert!(
        (avg - 64.0).abs() <= 1.5,
        "key avalanche avg {avg} out of 64 +- 1.5"
    );
    for (i, &count) in bit_flips.iter().enumerate() {
        let p = f64::from(count) / SAMPLES as f64;
        assert!(
            (p - 0.5).abs() <= 0.05,
            "key output bit {i} flip prob {p} out of 0.5 +- 0.05"
        );
    }
}

#[test]
fn keystream_bias_1mib() {
    const LEN: usize = 1024 * 1024;
    let key = [0u8; 32];
    let nonce = [0u8; 12];
    let mut buf = vec![0u8; LEN];
    ctr_xor(&key, &nonce, 0, &mut buf);

    let mut ones = 0u64;
    let mut counts = [0u64; 256];
    for &b in &buf {
        ones += u64::from(b.count_ones());
        counts[usize::from(b)] += 1;
    }

    let ones_frac = ones as f64 / (LEN * 8) as f64;
    assert!(
        (ones_frac - 0.5).abs() <= 0.002,
        "ones fraction {ones_frac} out of 0.5 +- 0.002"
    );

    let expected = LEN as f64 / 256.0;
    let chi2: f64 = counts
        .iter()
        .map(|&c| {
            let d = c as f64 - expected;
            d * d / expected
        })
        .sum();
    assert!(chi2 < 330.0, "byte chi-squared {chi2} >= 330");
}

#[test]
fn ctr_matches_manual_blocks_splits_and_wraps() {
    let mut rng = Rng::new(0xC72_0001);
    let mut key = [0u8; 32];
    rng.fill(&mut key);
    let mut nonce = [0u8; 12];
    rng.fill(&mut nonce);

    let start_ctr = u32::MAX - 2;
    let data = rng.bytes(100);
    let len = data.len().max(70);
    let mut plain = vec![0u8; len];
    rng.fill(&mut plain);

    // Hand-built block-by-block keystream with wrapping counter
    let cipher = Cinnabar::new(&key);
    let mut expected = plain.clone();
    let mut ctr = start_ctr;
    for chunk in expected.chunks_mut(16) {
        let mut ks = [0u8; 16];
        ks[..12].copy_from_slice(&nonce);
        ks[12..16].copy_from_slice(&ctr.to_le_bytes());
        cipher.encrypt_block(&mut ks);
        for (d, k) in chunk.iter_mut().zip(ks.iter()) {
            *d ^= *k;
        }
        ctr = ctr.wrapping_add(1);
    }

    let mut actual = plain.clone();
    ctr_xor(&key, &nonce, start_ctr, &mut actual);
    assert_eq!(actual, expected);

    // Split at every block boundary (including across the u32::MAX -> 0 wrap)
    for split_blocks in 0..=(len / 16) {
        let split = split_blocks * 16;
        let mut split_buf = plain.clone();
        ctr_xor(&key, &nonce, start_ctr, &mut split_buf[..split]);
        ctr_xor(
            &key,
            &nonce,
            start_ctr.wrapping_add(split_blocks as u32),
            &mut split_buf[split..],
        );
        assert_eq!(split_buf, expected, "split at block {split_blocks} failed");
    }
}
