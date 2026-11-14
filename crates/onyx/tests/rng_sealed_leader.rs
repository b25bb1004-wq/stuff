//! Leader check for T20: keystream against RFC 8439 A.1 and `chacha20::xor`, split reads, bias at
//! a modulus where naive `%` is far off, and the sealed box rebuilt byte for byte from its spec.
#![allow(clippy::unwrap_used)]
use onyx::rng::ChaChaRng;
use onyx::sealed::{open, seal, OVERHEAD};
use onyx::{aead, chacha20, hkdf, x25519};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn stream(seed: [u8; 32], n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    chacha20::xor(&seed, &[0; 12], 0, &mut v);
    v
}

#[test]
fn leader_keystream_known_answer_and_differential() {
    let mut r = ChaChaRng::from_seed([0; 32]);
    let mut b = [0u8; 64];
    r.fill(&mut b);
    // RFC 8439 A.1 test vector 1 (all-zero key, nonce, counter 0).
    let want = hex(concat!(
        "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7",
        "da41597c5157488d7724e03fb8d84a376a43b8f41518a11cc387b669b2ee6586"
    ));
    assert_eq!(b.to_vec(), want);
    let mut seed = [0u8; 32];
    for (i, x) in seed.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(37) ^ 0x5a;
    }
    let mut r = ChaChaRng::from_seed(seed);
    let mut got = vec![0u8; 1000];
    r.fill(&mut got);
    assert_eq!(got, stream(seed, 1000), "keystream past the first blocks");
}

#[test]
fn leader_split_reads_share_one_stream() {
    let seed = [7u8; 32];
    let want = stream(seed, 3000);
    let mut r = ChaChaRng::from_seed(seed);
    let mut got = Vec::new();
    let mut k = 0usize;
    while got.len() + 80 < want.len() {
        k = (k * 31 + 17) % 71;
        if k.is_multiple_of(5) {
            got.extend_from_slice(&r.next_u64().to_le_bytes());
        } else {
            let mut b = vec![0u8; k];
            r.fill(&mut b);
            got.extend_from_slice(&b);
        }
    }
    assert_eq!(got[..], want[..got.len()]);
}

#[test]
fn leader_below_is_unbiased() {
    let mut r = ChaChaRng::from_seed([3; 32]);
    assert!((0..1000).all(|_| r.below(1) == 0));
    // With n = 3 * 2^62, naive `u64 % n` puts half the draws below 2^62 instead of a third.
    let n = 3u64 << 62;
    let draws = 30_000;
    let low = (0..draws)
        .map(|_| r.below(n))
        .inspect(|&v| assert!(v < n))
        .filter(|&v| v < 1 << 62)
        .count();
    let frac = low as f64 / draws as f64;
    assert!(
        (frac - 1.0 / 3.0).abs() < 0.02,
        "fraction below 2^62: {frac}"
    );
    let mut counts = [0u32; 6];
    for _ in 0..60_000 {
        counts[r.below(6) as usize] += 1;
    }
    let chi: f64 = counts
        .iter()
        .map(|&c| (f64::from(c) - 10_000.0).powi(2) / 10_000.0)
        .sum();
    assert!(chi < 25.0, "chi-square {chi} over {counts:?}");
}

#[test]
fn leader_reseed_is_deterministic_and_changes_the_stream() {
    let mut a = ChaChaRng::from_seed([9; 32]);
    let mut b = ChaChaRng::from_seed([9; 32]);
    let mut c = ChaChaRng::from_seed([9; 32]);
    a.reseed(b"x");
    b.reseed(b"x");
    c.reseed(b"y");
    let (x, y, z) = (a.next_u64(), b.next_u64(), c.next_u64());
    assert_eq!(x, y);
    assert_ne!(x, z);
    assert_ne!(x, ChaChaRng::from_seed([9; 32]).next_u64());
}

#[test]
fn leader_sealed_matches_spec_and_rejects_tampering() {
    let rsec = [0x11u8; 32];
    let rpub = x25519::public_key(&rsec);
    let esec = [0x22u8; 32];
    let msg = b"meet at the quarry";
    let boxed = seal(&rpub, &esec, msg);
    assert_eq!(boxed.len(), msg.len() + OVERHEAD);

    // Rebuild from the brief's formula.
    let epub = x25519::public_key(&esec);
    let shared = x25519::shared_secret(&esec, &rpub).unwrap();
    let mut ctx = epub.to_vec();
    ctx.extend_from_slice(&rpub);
    let prk = hkdf::extract(&ctx, &shared);
    let mut key = [0u8; 32];
    hkdf::expand(&prk, b"strata sealed v1", &mut key).unwrap();
    let mut want = epub.to_vec();
    want.extend(aead::seal(&key, &[0; 12], &ctx, msg));
    assert_eq!(boxed, want, "box differs from the spec formula");
    assert_eq!(open(&rsec, &boxed).unwrap(), msg);

    for i in 0..boxed.len() {
        for bit in [1u8, 0x80] {
            let mut t = boxed.clone();
            t[i] ^= bit;
            assert!(open(&rsec, &t).is_err(), "flip at {i}");
        }
    }
    for n in 0..boxed.len() {
        assert!(open(&rsec, &boxed[..n]).is_err(), "truncated to {n}");
    }
    assert!(open(&[0x12; 32], &boxed).is_err(), "wrong recipient");
    let empty = seal(&rpub, &esec, b"");
    assert_eq!(open(&rsec, &empty).unwrap(), b"");

    let order8 = hex("e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800");
    let mut one = [0u8; 32];
    one[0] = 1;
    for weak in [[0u8; 32], one, order8.try_into().unwrap()] {
        let b = seal(&weak, &esec, msg);
        // No secret can open a box keyed by an all-zero shared secret.
        assert!(open(&rsec, &b).is_err());
        assert!(open(&[0; 32], &b).is_err());
        let mut forged = weak.to_vec();
        forged.extend(aead::seal(&[0; 32], &[0; 12], b"", msg));
        assert!(open(&rsec, &forged).is_err(), "weak ephemeral key accepted");
    }
}
