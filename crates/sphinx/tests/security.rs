//! Security tests for sphinx trust-boundary inputs, malleability, and error paths.

#![allow(clippy::unwrap_used)]

use sphinx::{
    aead, chacha20, ct, ed25519, hkdf, hmac, medusa, poly1305, rng, sealed, sha256, sha512, x25519,
    Error,
};

fn identity_edwards() -> [u8; 32] {
    let mut b = [0u8; 32];
    b[0] = 1;
    b
}

fn order2_edwards() -> [u8; 32] {
    let mut b = [0xffu8; 32];
    b[0] = 0xec;
    b[31] = 0x7f;
    b
}

#[test]
fn ed25519_rejects_small_order_public_key() {
    // Identity point (0, 1): S = 0, R = (0, 1) satisfies [0]B = R + [k]A for every message.
    let id = identity_edwards();
    let mut forged_id = [0u8; 64];
    forged_id[..32].copy_from_slice(&id);
    assert_eq!(
        ed25519::verify(&id, b"arbitrary message", &forged_id),
        Err(Error::BadSignature)
    );

    // Order-2 point (0, -1): S = 0, R = (0, -1) satisfies [0]B = R + [k]A whenever k is odd.
    let ord2 = order2_edwards();
    let mut forged_ord2 = [0u8; 64];
    forged_ord2[..32].copy_from_slice(&ord2);
    for m in 0u8..16 {
        assert_eq!(
            ed25519::verify(&ord2, &[m], &forged_ord2),
            Err(Error::BadSignature)
        );
    }

    // Order-4 point (sqrt(-1), 0) encoded as y = 0: S = 0, R = A verifies whenever k == 3 (mod 4).
    let ord4 = [0u8; 32];
    let forged_ord4 = [0u8; 64];
    for m in 0u8..32 {
        assert_eq!(
            ed25519::verify(&ord4, &[m], &forged_ord4),
            Err(Error::BadSignature)
        );
    }
}

#[test]
fn ed25519_rejects_malleable_s_and_noncanonical_points() {
    let seed = [0x5au8; 32];
    let pk = ed25519::public_key(&seed);
    let msg = b"security-check";
    let sig = ed25519::sign(&seed, msg);
    assert_eq!(ed25519::verify(&pk, msg, &sig), Ok(()));

    // S = L (exact group order)
    let l: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x10,
    ];
    let mut sig_l = sig;
    sig_l[32..].copy_from_slice(&l);
    assert_eq!(ed25519::verify(&pk, msg, &sig_l), Err(Error::BadSignature));

    // S = 0xff..ff
    let mut sig_max = sig;
    sig_max[32..].fill(0xff);
    assert_eq!(
        ed25519::verify(&pk, msg, &sig_max),
        Err(Error::BadSignature)
    );

    // Non-canonical y = p for public key and for R
    let mut y_eq_p = [0xffu8; 32];
    y_eq_p[0] = 0xed;
    y_eq_p[31] = 0x7f;
    assert_eq!(
        ed25519::verify(&y_eq_p, msg, &sig),
        Err(Error::BadSignature)
    );
    let mut sig_bad_r = sig;
    sig_bad_r[..32].copy_from_slice(&y_eq_p);
    assert_eq!(
        ed25519::verify(&pk, msg, &sig_bad_r),
        Err(Error::BadSignature)
    );

    // x = 0 with sign bit = 1: y = 1 | (1 << 255)
    let mut neg_zero = identity_edwards();
    neg_zero[31] |= 0x80;
    assert_eq!(
        ed25519::verify(&neg_zero, msg, &sig),
        Err(Error::BadSignature)
    );
    let mut sig_neg_zero_r = sig;
    sig_neg_zero_r[..32].copy_from_slice(&neg_zero);
    assert_eq!(
        ed25519::verify(&pk, msg, &sig_neg_zero_r),
        Err(Error::BadSignature)
    );

    // Off-curve y (y = 2 has no x on Edwards25519)
    let mut off_curve = [0u8; 32];
    off_curve[0] = 2;
    assert_eq!(
        ed25519::verify(&off_curve, msg, &sig),
        Err(Error::BadSignature)
    );
}

#[test]
fn aead_rejects_truncated_and_empty_ciphertexts() {
    let key = [0x11u8; 32];
    let nonce = [0x22u8; 12];
    for len in 0..16 {
        let buf = vec![0u8; len];
        assert_eq!(
            aead::open(&key, &nonce, b"aad", &buf),
            Err(Error::BadLength)
        );
    }
    let sealed = aead::seal(&key, &nonce, b"aad", b"secret plaintext");
    for cut in 0..sealed.len() {
        let err = aead::open(&key, &nonce, b"aad", &sealed[..cut]).unwrap_err();
        assert!(matches!(err, Error::BadLength | Error::BadTag));
    }
}

#[test]
fn aead_rejects_forged_ciphertext_tag_and_aad() {
    let key = [0x33u8; 32];
    let nonce = [0x44u8; 12];
    let aad = b"header";
    let pt = b"confidential payload";
    let sealed = aead::seal(&key, &nonce, aad, pt);

    // Bit flip in every byte of ciphertext and tag
    for i in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[i] ^= 1;
        assert_eq!(aead::open(&key, &nonce, aad, &bad), Err(Error::BadTag));
    }

    // Wrong AAD, wrong nonce, wrong key
    assert_eq!(
        aead::open(&key, &nonce, b"headeR", &sealed),
        Err(Error::BadTag)
    );
    let mut bad_nonce = nonce;
    bad_nonce[11] ^= 1;
    assert_eq!(
        aead::open(&key, &bad_nonce, aad, &sealed),
        Err(Error::BadTag)
    );
    let mut bad_key = key;
    bad_key[0] ^= 1;
    assert_eq!(
        aead::open(&bad_key, &nonce, aad, &sealed),
        Err(Error::BadTag)
    );

    // Boundary shift between AAD and plaintext (empty vs padding)
    let sealed_empty_aad = aead::seal(&key, &nonce, b"", b"0123456789abcdef");
    assert_eq!(
        aead::open(&key, &nonce, b"0123456789abcdef", &sealed_empty_aad[16..]),
        Err(Error::BadTag)
    );
}

#[test]
fn aead_handles_zero_and_boundary_lengths() {
    let key = [0x55u8; 32];
    let nonce = [0x66u8; 12];
    for len in [0usize, 1, 15, 16, 17, 63, 64, 65, 4096] {
        let pt = vec![0xa5u8; len];
        let aad = vec![0x5au8; len % 33];
        let sealed = aead::seal(&key, &nonce, &aad, &pt);
        assert_eq!(sealed.len(), len + 16);
        assert_eq!(aead::open(&key, &nonce, &aad, &sealed).unwrap(), pt);
    }
}

#[test]
fn sealed_rejects_truncated_and_forged_boxes() {
    let secret = [0x77u8; 32];
    let pk = x25519::public_key(&secret);
    let eph = [0x88u8; 32];

    for len in 0..sealed::OVERHEAD {
        let buf = vec![0u8; len];
        assert_eq!(sealed::open(&secret, &buf), Err(Error::BadLength));
    }

    let boxed = sealed::seal(&pk, &eph, b"top secret");
    assert_eq!(sealed::open(&secret, &boxed).unwrap(), b"top secret");

    for i in 0..boxed.len() {
        let mut bad = boxed.clone();
        bad[i] ^= 0x01;
        assert!(sealed::open(&secret, &bad).is_err());
    }

    let mut wrong_secret = secret;
    wrong_secret[0] ^= 8;
    assert_eq!(sealed::open(&wrong_secret, &boxed), Err(Error::BadTag));
}

#[test]
fn x25519_and_sealed_reject_low_order_points() {
    let secret = [0x99u8; 32];
    // Low-order Curve25519 u-coordinates: 0, 1, p-1, p, p+1, and RFC 7748 order-8 points.
    let mut one = [0u8; 32];
    one[0] = 1;
    let mut p_minus_1 = [0xffu8; 32];
    p_minus_1[0] = 0xec;
    p_minus_1[31] = 0x7f;
    let mut p = [0xffu8; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    let mut p_plus_1 = [0xffu8; 32];
    p_plus_1[0] = 0xee;
    p_plus_1[31] = 0x7f;
    let ord8_a: [u8; 32] = [
        0xe0, 0xeb, 0x7a, 0x7c, 0x3b, 0x41, 0xb8, 0xae, 0x16, 0x56, 0xe3, 0xfa, 0xf1, 0x9f, 0xc4,
        0x6a, 0xda, 0x09, 0x8d, 0xeb, 0x9c, 0x32, 0xb1, 0xfd, 0x86, 0x62, 0x05, 0x16, 0x5f, 0x49,
        0xb8, 0x00,
    ];
    let ord8_b: [u8; 32] = [
        0x5f, 0x9c, 0x95, 0xbc, 0xa3, 0x50, 0x8c, 0x24, 0xb1, 0xd0, 0xb1, 0x55, 0x9c, 0x83, 0xef,
        0x5b, 0x04, 0x44, 0x5c, 0xc4, 0x58, 0x1c, 0x8e, 0x86, 0xd8, 0x22, 0x4e, 0xdd, 0xd0, 0x9f,
        0x11, 0x57,
    ];

    for pt in [[0u8; 32], one, p_minus_1, p, p_plus_1, ord8_a, ord8_b] {
        assert_eq!(x25519::shared_secret(&secret, &pt), Err(Error::WeakKey));
        let mut with_top_bit = pt;
        with_top_bit[31] |= 0x80;
        assert_eq!(
            x25519::shared_secret(&secret, &with_top_bit),
            Err(Error::WeakKey)
        );
        assert!(sealed::seal(&pt, &secret, b"msg").is_empty());
        let mut forged = pt.to_vec();
        forged.extend_from_slice(&[0u8; 32]);
        assert_eq!(sealed::open(&secret, &forged), Err(Error::WeakKey));
    }
}

#[test]
fn medusa_handles_hostile_inputs_and_lengths() {
    for key in [[0u8; 32], [0xffu8; 32]] {
        let cipher = medusa::Medusa::new(&key);
        assert_eq!(format!("{cipher:?}"), "Medusa { .. }");
        let mut block = [0u8; 16];
        cipher.encrypt_block(&mut block);
        cipher.decrypt_block(&mut block);
        assert_eq!(block, [0u8; 16]);

        let nonce = [0xaau8; 12];
        for len in [0usize, 1, 15, 16, 17, 64, 257] {
            let orig = vec![0x3cu8; len];
            let mut data = orig.clone();
            medusa::ctr_xor(&key, &nonce, u32::MAX - 1, &mut data);
            medusa::ctr_xor(&key, &nonce, u32::MAX - 1, &mut data);
            assert_eq!(data, orig);
        }
    }
}

#[test]
fn hkdf_and_hmac_enforce_bounds_and_hostile_keys() {
    let prk = hkdf::extract(&[], &[]);
    let mut okm_empty = [];
    assert_eq!(hkdf::expand(&prk, b"info", &mut okm_empty), Ok(()));

    let mut okm_too_long = vec![0u8; 255 * 32 + 1];
    assert_eq!(
        hkdf::expand(&prk, b"info", &mut okm_too_long),
        Err(Error::BadLength)
    );

    // HMAC handles 0-byte, block-sized (64), 65-byte, and oversized keys without panic
    let _ = hmac::hmac_sha256(b"", b"");
    let _ = hmac::hmac_sha256(&[0x12u8; 64], b"msg");
    let _ = hmac::hmac_sha256(&[0x34u8; 65], b"msg");
    let _ = hmac::hmac_sha256(&[0x56u8; 2048], &[0x78u8; 2048]);
}

#[test]
fn hashes_handle_block_boundaries_without_panic() {
    for len in [
        0usize, 1, 55, 56, 63, 64, 65, 111, 112, 119, 120, 127, 128, 129,
    ] {
        let data = vec![0xabu8; len];
        let mut h256 = sha256::Sha256::new();
        let mut h512 = sha512::Sha512::new();
        for b in &data {
            h256.update(core::slice::from_ref(b));
            h512.update(core::slice::from_ref(b));
        }
        assert_eq!(h256.finalize(), sha256::digest(&data));
        assert_eq!(h512.finalize(), sha512::digest(&data));
    }
}

#[test]
fn poly1305_and_chacha20_handle_boundary_lengths_and_wrap() {
    let key = [0xffu8; 32];
    for len in [0usize, 1, 15, 16, 17, 31, 32, 33, 128] {
        let msg = vec![0xffu8; len];
        let tag = poly1305::mac(&key, &msg);
        assert_eq!(tag.len(), 16);
    }

    let nonce = [0u8; 12];
    let orig = vec![0x55u8; 192];
    let mut data = orig.clone();
    chacha20::xor(&key, &nonce, u32::MAX, &mut data);
    chacha20::xor(&key, &nonce, u32::MAX, &mut data);
    assert_eq!(data, orig);
}

#[test]
fn ct_eq_and_rng_handle_hostile_inputs() {
    assert!(ct::eq(b"", b""));
    assert!(!ct::eq(b"", b"\x00"));
    assert!(!ct::eq(b"abc", b"abcd"));
    let a = [0xaau8; 32];
    for bit in 0..256 {
        let mut b = a;
        b[bit / 8] ^= 1 << (bit % 8);
        assert!(!ct::eq(&a, &b));
    }

    let mut r = rng::ChaChaRng::from_seed([0u8; 32]);
    r.fill(&mut []);
    assert_eq!(r.below(0), 0);
    assert_eq!(r.below(1), 0);
    assert!(r.below(u64::MAX) < u64::MAX);
    r.reseed(&[]);
    r.reseed(&[0xffu8; 1024]);
}
