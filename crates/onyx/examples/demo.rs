//! Demo of onyx: hashes, MACs, key derivation, the AEAD, signatures, key agreement and a sealed box.
//! Every key and seed is a fixed constant so the output is the same on every run. Demo only.

use onyx::{
    aead, chacha20, cinnabar, ct, ed25519, hkdf, hmac, rng, sealed, sha256, sha512, x25519, Error,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn main() -> Result<(), Error> {
    // Hashes: one-shot and incremental give the same digest.
    println!("sha256(\"abc\") = {}", hex(&sha256::digest(b"abc")));
    let mut hasher = sha512::Sha512::new();
    hasher.update(b"a");
    hasher.update(b"bc");
    let incremental = hasher.finalize();
    println!(
        "sha512 incremental matches one-shot: {}",
        incremental == sha512::digest(b"abc")
    );

    // MAC and key derivation.
    let tag = hmac::hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog");
    println!("hmac-sha256 = {}", hex(&tag));
    let prk = hkdf::extract(b"salt", b"input key material");
    let mut okm = [0u8; 16];
    hkdf::expand(&prk, b"context", &mut okm)?;
    println!("hkdf output (16 bytes) = {}", hex(&okm));

    // Stream cipher: applying the same XOR twice gives the input back.
    let key = [1u8; 32];
    let nonce = [2u8; 12];
    let mut data = *b"attack at dawn";
    chacha20::xor(&key, &nonce, 1, &mut data);
    println!("chacha20 ciphertext = {}", hex(&data));
    chacha20::xor(&key, &nonce, 1, &mut data);
    println!(
        "chacha20 after second xor = {:?}",
        String::from_utf8_lossy(&data)
    );

    // AEAD: a tampered byte is rejected with BadTag.
    let message = b"hello, onyx";
    let sealed_msg = aead::seal(&key, &nonce, b"header", message);
    println!(
        "aead output: {} bytes (11 plaintext + 16 tag)",
        sealed_msg.len()
    );
    let opened = aead::open(&key, &nonce, b"header", &sealed_msg)?;
    println!("aead open ok: {}", opened == message);
    let mut bad = sealed_msg.clone();
    bad[0] ^= 1;
    println!(
        "aead tampered: {:?}",
        aead::open(&key, &nonce, b"header", &bad)
    );

    // Signatures: a changed message fails to verify.
    let seed = [3u8; 32];
    let public = ed25519::public_key(&seed);
    let signature = ed25519::sign(&seed, b"release 1.0");
    println!("ed25519 public key = {}", hex(&public));
    println!(
        "ed25519 verify = {:?}",
        ed25519::verify(&public, b"release 1.0", &signature)
    );
    println!(
        "ed25519 verify changed message = {:?}",
        ed25519::verify(&public, b"release 1.1", &signature)
    );

    // Key agreement: both sides derive the same shared secret.
    let alice = [5u8; 32];
    let bob = [6u8; 32];
    let alice_pub = x25519::public_key(&alice);
    let bob_pub = x25519::public_key(&bob);
    let shared_a = x25519::shared_secret(&alice, &bob_pub)?;
    let shared_b = x25519::shared_secret(&bob, &alice_pub)?;
    println!("x25519 shared secrets equal: {}", shared_a == shared_b);

    // Sealed box: only the holder of Bob's secret key can open it.
    let ephemeral = [8u8; 32];
    let boxed = sealed::seal(&bob_pub, &ephemeral, b"for bob only");
    println!(
        "sealed box: {} bytes ({} bytes overhead)",
        boxed.len(),
        sealed::OVERHEAD
    );
    let plain = sealed::open(&bob, &boxed)?;
    println!("bob opens: {:?}", String::from_utf8_lossy(&plain));
    println!("alice cannot open: {:?}", sealed::open(&alice, &boxed));

    // Constant-time compare and a seeded random generator.
    println!(
        "ct::eq same = {}, different = {}",
        ct::eq(b"same", b"same"),
        ct::eq(b"same", b"sane")
    );
    let mut rng = rng::ChaChaRng::from_seed([4u8; 32]);
    let mut dice = Vec::new();
    for _ in 0..5 {
        dice.push(rng.below(6) + 1);
    }
    println!("five dice from a fixed seed: {:?}", dice);
    rng.reseed(b"extra entropy");
    println!("first u64 after reseed: {}", rng.next_u64());

    // Experimental block cipher: encrypt then decrypt returns the block.
    let cipher = cinnabar::Cinnabar::new(&key);
    let mut block = *b"0123456789abcdef";
    cipher.encrypt_block(&mut block);
    println!("cinnabar block changed: {}", &block != b"0123456789abcdef");
    cipher.decrypt_block(&mut block);
    println!("cinnabar round trip ok: {}", &block == b"0123456789abcdef");
    Ok(())
}
