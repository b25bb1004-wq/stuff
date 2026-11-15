# T80 sphinx (crypto): security review
Sampled: ct.rs, aead.rs, sealed.rs, ed25519.rs verify, x25519.rs, hkdf.rs, rng.rs, medusa.rs ctr_xor, src/bin/sphinx.rs keygen and fill_random, non-test panic scan. AEAD nonces are unique per key; ct::eq is constant-time; ed25519 verify rejects S >= L and small-order A.
- HIGH crates/sphinx/src/bin/sphinx.rs:124-135 fill_random reads only /dev/urandom. On win32 (no such file) it silently falls back to std RandomState/SipHash, which is not a CSPRNG and gives about 128 bits. Input: sphinx keygen -o key on Windows (line 246). Fix: use the OS CSPRNG and return an error instead of falling back.
- LOW crates/sphinx/src/rng.rs:27-31 block counter wraps silently after 2^32 blocks (256 GiB), repeating the keystream for the same seed. Fix: refuse or reseed at counter wrap.
- LOW crates/sphinx/src/medusa.rs:68-80 ctr_xor counter wraps after 2^32 blocks (64 GiB). Medusa is labelled experimental and is used only in tests and demo. Fix: reject data longer than 2^36 bytes.
- LOW crates/sphinx/src/sealed.rs:21-27 seal returns an empty Vec silently for a low-order recipient. The zero nonce is safe only if the caller passes a fresh eph_secret (API footgun). Fix: return Result and document eph_secret freshness.
Count: high 1, medium 0, low 3.
