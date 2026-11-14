# T13a leader fixtures. Writes crates/quartz/tests/fixtures/ac/{c_pairs,amo}.s for LLVM:
# c_pairs: each case is one explicit c.* instruction (2 bytes) then its expansion (4 bytes).
# amo: one A-extension instruction per line; amo_fields.txt records what each line was given.
# Run from the repo root: python -I briefs/leader/gen_quartz_ac.py (then build_quartz_ac.sh).
import random

random.seed(0x13A_2026)
R = ['zero', 'ra', 'sp', 'gp', 'tp', 't0', 't1', 't2', 's0', 's1', 'a0', 'a1', 'a2', 'a3', 'a4', 'a5',
     'a6', 'a7', 's2', 's3', 's4', 's5', 's6', 's7', 's8', 's9', 's10', 's11', 't3', 't4', 't5', 't6']
P = R[8:16]  # the x8..x15 registers compressed forms can name


def nz(lo, hi, step=1):
    while True:
        v = random.randrange(lo, hi + 1, step) if step > 1 else random.randint(lo, hi)
        if v != 0:
            return v


def rnz():
    return random.choice(R[1:])


def case():
    k = random.randrange(31)
    if k == 0:
        rd, u = random.choice(P), nz(4, 1020, 4)
        return f"c.addi4spn {rd}, sp, {u}", f"addi {rd}, sp, {u}"
    if k in (1, 2, 3, 4):
        op, sc = [('lw', 4), ('ld', 8), ('sw', 4), ('sd', 8)][k - 1]
        a, b, u = random.choice(P), random.choice(P), random.randrange(0, 32 * sc, sc)
        return f"c.{op} {a}, {u}({b})", f"{op} {a}, {u}({b})"
    if k == 5:
        return "c.nop", "addi zero, zero, 0"
    if k == 6:
        rd, i = rnz(), nz(-32, 31)
        return f"c.addi {rd}, {i}", f"addi {rd}, {rd}, {i}"
    if k == 7:
        rd, i = rnz(), random.randint(-32, 31)
        return f"c.addiw {rd}, {i}", f"addiw {rd}, {rd}, {i}"
    if k == 8:
        rd, i = rnz(), random.randint(-32, 31)
        return f"c.li {rd}, {i}", f"addi {rd}, zero, {i}"
    if k == 9:
        i = nz(-512, 496, 16)
        return f"c.addi16sp sp, {i}", f"addi sp, sp, {i}"
    if k == 10:
        rd = random.choice([r for r in R[1:] if r != 'sp'])
        i = random.choice([random.randint(1, 31), random.randint(0xFFFE0, 0xFFFFF)])
        return f"c.lui {rd}, {i}", f"lui {rd}, {i}"
    if k in (11, 12):
        op = 'srli' if k == 11 else 'srai'
        rd, s = random.choice(P), random.randint(1, 63)
        return f"c.{op} {rd}, {s}", f"{op} {rd}, {rd}, {s}"
    if k == 13:
        rd, i = random.choice(P), random.randint(-32, 31)
        return f"c.andi {rd}, {i}", f"andi {rd}, {rd}, {i}"
    if k in (14, 15, 16, 17, 18, 19):
        op = ['sub', 'xor', 'or', 'and', 'subw', 'addw'][k - 14]
        a, b = random.choice(P), random.choice(P)
        return f"c.{op} {a}, {b}", f"{op} {a}, {a}, {b}"
    if k == 20:
        o = random.randrange(-2048, 2047, 2)
        return f"c.j {o}", f"jal zero, {o}"
    if k in (21, 22):
        op = ['beqz', 'bnez'][k - 21]
        a, o = random.choice(P), random.randrange(-256, 255, 2)
        return f"c.{op} {a}, {o}", f"{op[:3]} {a}, zero, {o}"
    if k == 23:
        rd, s = rnz(), random.randint(1, 63)
        return f"c.slli {rd}, {s}", f"slli {rd}, {rd}, {s}"
    if k in (24, 25):
        op, sc = [('lw', 4), ('ld', 8)][k - 24]
        rd, u = rnz(), random.randrange(0, 64 * sc, sc)
        return f"c.{op}sp {rd}, {u}(sp)", f"{op} {rd}, {u}(sp)"
    if k in (26, 27):
        op, sc = [('sw', 4), ('sd', 8)][k - 26]
        rs, u = random.choice(R), random.randrange(0, 64 * sc, sc)
        return f"c.{op}sp {rs}, {u}(sp)", f"{op} {rs}, {u}(sp)"
    if k == 28:
        r = rnz()
        return random.choice([(f"c.jr {r}", f"jalr zero, 0({r})"), (f"c.jalr {r}", f"jalr ra, 0({r})")])
    if k == 29:
        a, b = rnz(), rnz()
        return random.choice([(f"c.mv {a}, {b}", f"add {a}, zero, {b}"), (f"c.add {a}, {b}", f"add {a}, {a}, {b}")])
    return "c.ebreak", "ebreak"


cases = [case() for _ in range(2500)]
with open("crates/quartz/tests/fixtures/ac/c_pairs.s", "w", newline="\n") as f:
    for c, e in cases:
        f.write(f".option rvc\n{c}\n.option norvc\n{e}\n")
with open("crates/quartz/tests/fixtures/ac/c_pairs_src.txt", "w", newline="\n") as f:
    for c, e in cases:
        f.write(f"{c} | {e}\n")

OPS = ['swap', 'add', 'xor', 'and', 'or', 'min', 'max', 'minu', 'maxu']
with open("crates/quartz/tests/fixtures/ac/amo.s", "w", newline="\n") as f, \
        open("crates/quartz/tests/fixtures/ac/amo_fields.txt", "w", newline="\n") as g:
    f.write(".option norvc\n.option arch, +a\n")
    for _ in range(1200):
        w = random.choice('wd')
        aq, rl = random.randint(0, 1), random.randint(0, 1)
        suf = {(0, 0): '', (1, 0): '.aq', (0, 1): '.rl', (1, 1): '.aqrl'}[(aq, rl)]
        rd, rs1, rs2 = (random.randrange(32) for _ in range(3))
        k = random.randrange(11)
        if k == 0:
            f.write(f"lr.{w}{suf} {R[rd]}, ({R[rs1]})\n")
            g.write(f"lr {w} {rd} {rs1} 0 {aq} {rl}\n")
        elif k == 1:
            f.write(f"sc.{w}{suf} {R[rd]}, {R[rs2]}, ({R[rs1]})\n")
            g.write(f"sc {w} {rd} {rs1} {rs2} {aq} {rl}\n")
        else:
            op = OPS[k - 2]
            f.write(f"amo{op}.{w}{suf} {R[rd]}, {R[rs2]}, ({R[rs1]})\n")
            g.write(f"{op} {w} {rd} {rs1} {rs2} {aq} {rl}\n")
print(len(cases), "compressed cases")
