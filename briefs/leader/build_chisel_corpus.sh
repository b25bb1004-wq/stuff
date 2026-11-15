#!/bin/bash
# T21 leader fixture: assemble crates/chisel/tests/fixtures/corpus.s with rustc's LLVM into
# corpus.bin (base 0x80000000). Run from the repo root after gen_chisel_corpus.py.
set -e
D=crates/chisel/tests/fixtures
T=$(mktemp -d)
cat > "$T/link.ld" <<'LD'
ENTRY(_start)
SECTIONS { . = 0x80000000; .text : { *(.text.start) } }
LD
cat > "$T/c.rs" <<RS
#![no_std]
#![no_main]
core::arch::global_asm!(".section .text.start, \"ax\"\n.globl _start\n.option norelax\n.option norvc\n.option arch, +m, +a, +c\n_start:\n", include_str!("$(cygpath -m "$PWD/$D/corpus.s")"));
#[panic_handler]
fn p(_: &core::panic::PanicInfo) -> ! { loop {} }
RS
rustc --edition 2021 --target riscv64gc-unknown-none-elf -C opt-level=2 -C panic=abort \
  -C link-arg=-T$(cygpath -m "$T/link.ld") "$T/c.rs" -o "$T/c.elf"
llvm-objcopy -O binary -j .text "$T/c.elf" "$D/corpus.bin"
llvm-nm "$T/c.elf" | grep -v ' [tT] \.L' > "$T/syms.txt" || true
grep -E ' L[0-9]+$' "$T/syms.txt" | awk '{print $3, $1}' | sort -V > "$D/corpus.syms"
echo "corpus.bin $(wc -c < "$D/corpus.bin") bytes, expected $(cat "$D/corpus.size")"
rm -rf "$T"
python -I -X utf8 - "$D" <<'PY'
import re, sys
d = sys.argv[1]
src = open(f"{d}/corpus.s", encoding="utf8").read().splitlines()
b = open(f"{d}/corpus.bin", "rb").read()
mp = {}
for l in open(f"{d}/corpus.map", encoding="utf8"):
    o, n = l.split()[:2]
    mp[int(n)] = int(o)
syms = {}
for l in open(f"{d}/corpus.syms"):
    nm, a = l.split()
    syms[nm] = int(a, 16) - 0x80000000
# each label's address: next sized line at or after it
lines = sorted(mp)
checked = 0
for i, l in enumerate(src, 1):
    m = re.match(r"\s*(L\d+):", l)
    if m:
        nxt = next(n for n in lines if n >= i)
        assert syms[m.group(1)] == mp[nxt], (i, l, syms[m.group(1)], mp[nxt])
        checked += 1
assert checked == len(syms), (checked, len(syms))
w = lambda o: int.from_bytes(b[o:o + 4], "little")
opc = lambda x: x & 0x7f
rd = lambda x: (x >> 7) & 31
f3 = lambda x: (x >> 12) & 7
rs1 = lambda x: (x >> 15) & 31
ex = 0
for n in lines:
    t = re.sub(r"^\s*(\w+:\s*)?", "", src[n - 1]).split("#")[0].strip()
    o = mp[n]
    if t.startswith("call "):
        assert opc(w(o)) == 0x17 and rd(w(o)) == 1 and opc(w(o + 4)) == 0x67 and rd(w(o + 4)) == 1 and rs1(w(o + 4)) == 1, n
        ex += 1
    elif t.startswith("tail "):
        assert opc(w(o)) == 0x17 and rd(w(o)) == 6 and opc(w(o + 4)) == 0x67 and rd(w(o + 4)) == 0 and rs1(w(o + 4)) == 6, n
        ex += 1
    elif t.startswith("lla "):
        assert opc(w(o)) == 0x17 and opc(w(o + 4)) == 0x13 and f3(w(o + 4)) == 0, n
        ex += 1
    elif t.startswith("li ") and re.fullmatch(r"li \w+, -?\d+", t):
        v = int(t.split(",")[1])
        n_ins = 1 if -2048 <= v < 2048 or (v & 0xfff == 0 and -2**31 <= v < 2**31) else 2
        x = 0
        for k in range(n_ins):
            ins = w(o + 4 * k)
            im = (ins >> 20) - (4096 if ins >> 31 else 0)
            sv = x if rs1(ins) else 0
            if opc(ins) == 0x37: x = ((ins & 0xfffff000) ^ 0x80000000) - 0x80000000
            elif opc(ins) == 0x13 and f3(ins) == 0: x = sv + im
            elif opc(ins) == 0x13 and f3(ins) == 1: x = (sv << (im & 63)) & (2**64 - 1)
            elif opc(ins) == 0x1b and f3(ins) == 0: x = (((sv + im) & 0xffffffff) ^ 0x80000000) - 0x80000000
            else: raise AssertionError((n, t, hex(ins)))
            x = ((x & (2**64 - 1)) ^ 2**63) - 2**63
        assert x == v or rd(w(o)) == 0, (n, t, x)
        ex += 1
    elif t == ".balign 8":
        nxt = lines[lines.index(n) + 1] if n != lines[-1] else None
        end = mp[nxt] if nxt else len(b)
        pad = b[o:end]
        assert end % 8 == 0 and len(pad) % 4 == 0 and all(w(o + k) == 0x13 for k in range(0, len(pad), 4)), (n, pad.hex())
        ex += 1
print(f"validated: {checked} labels, {ex} expansions/paddings")
PY
