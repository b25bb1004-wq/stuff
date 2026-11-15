# onyx

onyx is the cryptography crate of Strata. It provides hash functions, a MAC, key derivation,
the ChaCha20-Poly1305 AEAD, X25519 key agreement, Ed25519 signatures, a sealed box built from
them, a seeded random generator and one experimental block cipher. It uses only the standard
library, so every primitive the rest of Strata relies on can be read in this one crate.

## Features

- SHA-256 and SHA-512, one-shot (`digest`) and incremental (`Sha256`, `Sha512`)
- HMAC-SHA-256 (`hmac::hmac_sha256`) and HKDF-SHA-256 extract and expand (`hkdf`)
- ChaCha20 stream cipher (`chacha20::xor`), Poly1305 MAC (`poly1305::mac`)
- ChaCha20-Poly1305 AEAD with additional data (`aead::seal`, `aead::open`)
- X25519 key agreement; a low-order peer key is rejected with `WeakKey`
- Ed25519 signing and verification; verify rejects S >= L, points that do not decode and
  non-canonical y encodings
- Sealed box: encrypt to an X25519 public key (`sealed`)
- Seeded ChaCha20 random generator (`rng::ChaChaRng`) with rejection-sampled `below`
- Constant-time byte comparison (`ct::eq`)
- Experimental: Cinnabar, a 128-bit block cipher. It has had no outside cryptanalysis, so
  do not use it for real data.

## Public API

| Item | What it does |
| --- | --- |
| `aead::seal`, `aead::open` | ChaCha20-Poly1305 encrypt and authenticate; `open` checks the tag |
| `chacha20::xor` | XOR the keystream into data; a second call restores the input |
| `poly1305::mac` | 16-byte one-time MAC |
| `sha256::digest`, `sha256::Sha256` | SHA-256, one-shot or incremental |
| `sha512::digest`, `sha512::Sha512` | SHA-512, one-shot or incremental |
| `hmac::hmac_sha256` | HMAC-SHA-256 tag |
| `hkdf::extract`, `hkdf::expand` | HKDF key derivation |
| `x25519::public_key`, `x25519::shared_secret` | Diffie-Hellman key agreement |
| `ed25519::public_key`, `ed25519::sign`, `ed25519::verify` | Ed25519 signatures |
| `sealed::seal`, `sealed::open`, `sealed::OVERHEAD` | encrypt to a public key |
| `rng::ChaChaRng` | `from_seed`, `fill`, `next_u64`, `below`, `reseed` |
| `ct::eq` | constant-time equality |
| `cinnabar::Cinnabar`, `cinnabar::ctr_xor` | experimental block cipher and counter mode |
| `Error` | `BadTag`, `BadLength`, `WeakKey`, `BadSignature` |

## Demo

    cargo run -p onyx --example demo

Real output:

```text
sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
sha512 incremental matches one-shot: true
hmac-sha256 = f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
hkdf output (16 bytes) = 790773b8093544d7052c18034ec05ddb
chacha20 ciphertext = 8768864feec8c8bb050d8d98a4e5
chacha20 after second xor = "attack at dawn"
aead output: 27 bytes (11 plaintext + 16 tag)
aead open ok: true
aead tampered: Err(BadTag)
ed25519 public key = ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1
ed25519 verify = Ok(())
ed25519 verify changed message = Err(BadSignature)
x25519 shared secrets equal: true
sealed box: 60 bytes (48 bytes overhead)
bob opens: "for bob only"
alice cannot open: Err(BadTag)
ct::eq same = true, different = false
five dice from a fixed seed: [1, 4, 6, 1, 6]
first u64 after reseed: 11970843059752319968
cinnabar block changed: true
cinnabar round trip ok: true
```

The demo uses fixed keys and seeds, so its output is the same on every run. Those constants
are for the demo only and must never be used for real keys.

## Tests

    cargo test -p onyx

The gate for this crate is `bash tools/check.sh onyx`, which runs format, clippy, the tests
and the zero-dependency check.
