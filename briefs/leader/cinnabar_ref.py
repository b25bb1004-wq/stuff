"""T14 leader reference for docs/MEDUSA.md. Writes crates/sphinx/tests/fixtures/medusa.txt and
prints design statistics (diffusion per round, avalanche, keystream bias). Run: python -I cinnabar_ref.py"""
import random, sys
M = 0xffffffff
R = 24
def rotl(x, n): return ((x << n) | (x >> (32 - n))) & M
def rotr(x, n): return ((x >> n) | (x << (32 - n))) & M
def qr(a, b, c, d):
    a = (a + b) & M; d = rotl(d ^ a, 16)
    c = (c + d) & M; b = rotl(b ^ c, 12)
    a = (a + b) & M; d = rotl(d ^ a, 8)
    c = (c + d) & M; b = rotl(b ^ c, 7)
    return a, b, c, d
def iqr(a, b, c, d):
    b = rotr(b, 7) ^ c; c = (c - d) & M
    d = rotr(d, 8) ^ a; a = (a - b) & M
    b = rotr(b, 12) ^ c; c = (c - d) & M
    d = rotr(d, 16) ^ a; a = (a - b) & M
    return a, b, c, d
def words(b): return [int.from_bytes(b[4*i:4*i+4], "little") for i in range(len(b)//4)]
def wbytes(w): return b"".join(x.to_bytes(4, "little") for x in w)
def schedule(key, rounds=R):
    k = words(key); rk = []
    for r in range(rounds + 1):
        rk.append([k[i] ^ ((0x9E3779B9 * (4*r + i + 1)) & M) for i in range(4)])
        k[0:4] = qr(*k[0:4]); k[4:8] = qr(*k[4:8])
        k = k[4:] + k[:4]
        k[0] ^= k[4]
    return rk
def enc(rk, block, rounds=R):
    s = [a ^ b for a, b in zip(words(block), rk[0])]
    for r in range(1, rounds + 1):
        s = list(qr(*s)); s = s[1:] + s[:1]
        s = [a ^ b for a, b in zip(s, rk[r])]
    return wbytes(s)
def dec(rk, block, rounds=R):
    s = words(block)
    for r in range(rounds, 0, -1):
        s = [a ^ b for a, b in zip(s, rk[r])]
        s = s[3:] + s[:3]; s = list(iqr(*s))
    return wbytes([a ^ b for a, b in zip(s, rk[0])])
def ctr(key, nonce, counter, data):
    rk = schedule(key); out = bytearray()
    for i in range(0, len(data), 16):
        ks = enc(rk, nonce + ((counter + i // 16) & M).to_bytes(4, "little"))
        out += bytes(a ^ b for a, b in zip(data[i:i+16], ks))
    return bytes(out)
def popc(b): return sum(bin(x).count("1") for x in b)

if __name__ == "__main__":
    rnd = random.Random(0x14C1A)
    lines = []
    for i in range(64):
        key = bytes(rnd.randrange(256) for _ in range(32)) if i else bytes(32)
        pt = bytes(rnd.randrange(256) for _ in range(16)) if i else bytes(16)
        rk = schedule(key); ct = enc(rk, pt)
        assert dec(rk, ct) == pt
        lines.append(f"block {key.hex()} {pt.hex()} {ct.hex()}")
    for i in range(24):
        key = bytes(rnd.randrange(256) for _ in range(32)); nonce = bytes(rnd.randrange(256) for _ in range(12))
        ctrv = [0, 1, 0xfffffffe, rnd.randrange(1 << 32)][i % 4]
        data = bytes(rnd.randrange(256) for _ in range(rnd.randrange(0, 100)))
        lines.append(f"ctr {key.hex()} {nonce.hex()} {ctrv} {data.hex() or '-'} {ctr(key, nonce, ctrv, data).hex() or '-'}")
    open(sys.argv[1] if len(sys.argv) > 1 else "medusa.txt", "w", newline="\n").write("\n".join(lines) + "\n")
    # Diffusion: rounds until every output bit depends on every input bit (one-bit flips, 64 keys).
    for rounds in range(1, 7):
        full = True
        for _ in range(64):
            key = bytes(rnd.randrange(256) for _ in range(32)); rk = schedule(key, rounds)
            pt = bytes(rnd.randrange(256) for _ in range(16)); base = int.from_bytes(enc(rk, pt, rounds), "little")
            for bit in range(128):
                p2 = (int.from_bytes(pt, "little") ^ (1 << bit)).to_bytes(16, "little")
                d = base ^ int.from_bytes(enc(rk, p2, rounds), "little")
                if bin(d).count("1") < 20: full = False
        print(f"rounds {rounds}: every flip changes >= 20 bits: {full}")
    # Avalanche at full rounds.
    n = 2000; tot = 0; tk = 0
    for _ in range(n):
        key = bytes(rnd.randrange(256) for _ in range(32)); rk = schedule(key)
        pt = bytes(rnd.randrange(256) for _ in range(16)); c = enc(rk, pt)
        bit = rnd.randrange(128)
        p2 = (int.from_bytes(pt, "little") ^ (1 << bit)).to_bytes(16, "little")
        tot += popc(bytes(a ^ b for a, b in zip(c, enc(rk, p2))))
        kb = rnd.randrange(256); k2 = (int.from_bytes(key, "little") ^ (1 << kb)).to_bytes(32, "little")
        tk += popc(bytes(a ^ b for a, b in zip(c, enc(schedule(k2), pt))))
    print(f"plaintext avalanche {tot/n:.2f} bits of 128, key avalanche {tk/n:.2f}")
    ks = ctr(bytes(32), bytes(12), 0, bytes(1 << 16))
    ones = popc(ks); cnt = [0]*256
    for x in ks: cnt[x] += 1
    e = len(ks)/256; chi = sum((c - e)**2 / e for c in cnt)
    print(f"keystream 64 KiB: ones fraction {ones/(8*len(ks)):.4f}, byte chi2 {chi:.1f} (255 dof)")
