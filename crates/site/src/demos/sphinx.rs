//! Sphinx cryptographic primitives demo.

use std::fmt::Write as _;

use sphinx::{chacha20, ed25519, hmac, sha256, sha512};

use super::{check_input, hex_lower, Demo, Output};

const DEMO_SEED: [u8; 32] = [
    0x4d, 0x6f, 0x6e, 0x6f, 0x6c, 0x69, 0x74, 0x68, 0x2d, 0x53, 0x70, 0x68, 0x69, 0x6e, 0x78, 0x2d,
    0x44, 0x65, 0x6d, 0x6f, 0x2d, 0x53, 0x65, 0x65, 0x64, 0x2d, 0x30, 0x30, 0x30, 0x30, 0x30, 0x31,
];
const DEMO_NONCE: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 1];
const HMAC_KEY: &[u8] = b"monolith";
const PREVIEW_BYTES: usize = 48;

/// Registered Sphinx cryptography demo.
pub const DEMO: Demo = Demo {
    id: "sphinx",
    title: "Sphinx Crypto",
    default_input: "abc",
    run,
};

/// Run SHA-256, SHA-512, HMAC-SHA256, ChaCha20 round trip, and Ed25519 sign/verify on `input`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let msg = input.as_bytes();

    let h256 = sha256::digest(msg);
    let h512 = sha512::digest(msg);
    let mac = hmac::hmac_sha256(HMAC_KEY, msg);

    let key = sha256::digest(b"monolith-chacha20-demo-key");
    let mut buf = msg.to_vec();
    chacha20::xor(&key, &DEMO_NONCE, 1, &mut buf);
    let ct_preview = if buf.len() > PREVIEW_BYTES {
        format!("{}...", hex_lower(&buf[..PREVIEW_BYTES]))
    } else {
        hex_lower(&buf)
    };
    chacha20::xor(&key, &DEMO_NONCE, 1, &mut buf);
    let roundtrip_ok = buf.as_slice() == msg;

    let pubkey = ed25519::public_key(&DEMO_SEED);
    let sig = ed25519::sign(&DEMO_SEED, msg);
    let verify_ok = ed25519::verify(&pubkey, msg, &sig).is_ok();

    let mut out = String::new();
    let _ = writeln!(out, "sha256: {}", hex_lower(&h256));
    let _ = writeln!(out, "sha512: {}", hex_lower(&h512));
    let _ = writeln!(out, "hmac-sha256(monolith): {}", hex_lower(&mac));
    let _ = writeln!(
        out,
        "chacha20: ct={ct_preview} roundtrip={}",
        if roundtrip_ok { "ok" } else { "fail" }
    );
    let _ = writeln!(
        out,
        "ed25519: pub={} sig={} verify={}",
        hex_lower(&pubkey),
        hex_lower(&sig),
        if verify_ok { "ok" } else { "fail" }
    );
    Ok(Output::Text(out))
}
