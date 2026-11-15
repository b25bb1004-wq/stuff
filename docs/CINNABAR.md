# Cinnabar: Strata's own block cipher (EXPERIMENTAL)

Cinnabar exists to learn from, not to protect anything. It has had no outside cryptanalysis. Every
real use in Strata goes through ChaCha20-Poly1305 (`onyx::aead`); nothing selects Cinnabar by
default, and its module documentation says EXPERIMENTAL in its first line.

## Shape
- Block: 128 bits, as 4 little-endian u32 words `s0..s3`. Key: 256 bits, 8 little-endian u32
  words `k0..k7`. Rounds: R = 24.
- The mixing step is ChaCha's quarter round `QR(a, b, c, d)` (add, xor, rotate left by 16, 12, 8, 7),
  which is a bijection on 4 words. Its inverse undoes the steps in reverse order with
  subtract and rotate right.

```
QR(a,b,c,d):  a+=b; d^=a; d<<<=16;  c+=d; b^=c; b<<<=12;
              a+=b; d^=a; d<<<=8;   c+=d; b^=c; b<<<=7
```

## Key schedule (R + 1 round keys of 4 words)
```
K = k0..k7
for r in 0..=R:
    rk[r][i] = K[i] ^ (0x9E3779B9 * (4r + i + 1) mod 2^32)      for i in 0..4
    QR(K0, K1, K2, K3); QR(K4, K5, K6, K7)
    K = K4 K5 K6 K7 K0 K1 K2 K3          (swap halves)
    K0 ^= K4
```
The golden-ratio constants make every round key different even for the all-zero key, and the
quarter rounds make the schedule non-linear.

## Encryption and decryption
```
encrypt:  s ^= rk[0]
          for r in 1..=R:  QR(s0, s1, s2, s3);  s = s1 s2 s3 s0;  s ^= rk[r]
decrypt:  for r in R..=1:  s ^= rk[r];  s = s3 s0 s1 s2;  QR^-1(s0, s1, s2, s3)
          s ^= rk[0]
```
The word rotation after each quarter round stops the four lanes from settling into fixed roles.

## Counter mode
Keystream block `j` is `encrypt(nonce (12 bytes) || le32(counter + j mod 2^32))`, xored with the
data; the last block is cut to length. The counter wraps without warning: callers must not
encrypt more than 2^32 blocks under one key and nonce.

## Measured (reference `briefs/leader/cinnabar_ref.py`, 2026-11-15)
- Diffusion: from 2 rounds on, every one-bit plaintext flip changes at least 20 of 128 output
  bits (64 random keys, all 128 flip positions). One round does not.
- Avalanche at 24 rounds: one plaintext bit flip changes 63.8 bits on average, one key bit flip
  64.1 (2000 samples each; ideal 64).
- Keystream of 64 KiB (zero key and nonce): ones fraction 0.4988, byte chi-squared 279 on 255
  degrees of freedom (p about 0.14).

## Test plan
1. Known answers: `crates/onyx/tests/fixtures/cinnabar.txt` (64 blocks, 24 counter-mode cases)
   from the Python reference.
2. `decrypt(encrypt(x)) == x` for random keys and blocks.
3. Avalanche: plaintext and key one-bit flips average 64 +- 1.5 bits over 4000 samples; every
   output bit flips with probability 0.5 +- 0.05.
4. Bias: 1 MiB keystream ones fraction 0.5 +- 0.002; byte chi-squared under 330 (255 dof).
5. Counter mode equals a hand-built block-by-block keystream, splits at any point give the
   same output, and the counter wraps at 2^32.

## Known weaknesses (honest list)
- No security proof, no outside review, round count chosen by eye from the numbers above.
- Pure ARX on 32-bit words: fast in software, but rotational and differential attacks on
  reduced rounds have not been studied here.
- 128-bit blocks: counter mode leaks after about 2^64 blocks under one key.
