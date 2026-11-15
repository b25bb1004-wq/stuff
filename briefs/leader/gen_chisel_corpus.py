# T21 leader corpus: crates/chisel/tests/fixtures/corpus.s (+ corpus.map "offset line").
# Every line's size is known here, so labels get exact offsets and branch ranges are respected.
# Run from the repo root: python -I -X utf8 briefs/leader/gen_chisel_corpus.py, then build_chisel_corpus.sh.
import random

random.seed(0x21_2026)
ABI = ['zero', 'ra', 'sp', 'gp', 'tp', 't0', 't1', 't2', 's0', 's1', 'a0', 'a1', 'a2', 'a3', 'a4', 'a5',
       'a6', 'a7', 's2', 's3', 's4', 's5', 's6', 's7', 's8', 's9', 's10', 's11', 't3', 't4', 't5', 't6']
P = ABI[8:16]
CSR_RW = ['mstatus', 'misa', 'medeleg', 'mideleg', 'mie', 'mtvec', 'mcounteren', 'mscratch', 'mepc',
          'mcause', 'mtval', 'mip', 'sstatus', 'sie', 'stvec', 'scounteren', 'sscratch', 'sepc',
          'scause', 'stval', 'sip', 'satp']
CSR_RO = ['mhartid', 'mvendorid', 'marchid', 'mimpid', 'cycle', 'time', 'instret']


def reg(nz=False):
    i = random.randrange(1 if nz else 0, 32)
    if random.random() < 0.25:
        return f"x{i}"
    if i == 8 and random.random() < 0.3:
        return 'fp'
    return ABI[i]


def imm(lo, hi):
    v = random.choice([lo, hi, 0, random.randint(lo, hi), random.randint(lo, hi)])
    return str(v) if random.random() < 0.7 or v < 0 else hex(v)


def nz(lo, hi, step=1):
    while True:
        v = random.randrange(lo, hi + 1, step)
        if v != 0:
            return v


def li_size(v):
    if -2048 <= v < 2048:
        return 4
    return 4 if v & 0xfff == 0 else 8


def li_val():
    edge = [2047, 2048, -2048, -2049, 4095, 4096, -4096, 0x7fffffff, -0x80000000, 0x7ffff7ff, 0x7ffff800,
            0x12345000, -1, 0, 1, 0x800, -0x801, 0x7fff_f000]
    if random.random() < 0.3:
        return random.choice(edge)
    return random.choice([random.randint(-2048, 2047), random.randint(-2**31, 2**31 - 1),
                          random.randint(-2**19, 2**19 - 1) * 4096])


def c_inst():
    k = random.randrange(24)
    if k == 0:
        return f"c.addi4spn {random.choice(P)}, sp, {nz(4, 1020, 4)}"
    if k < 5:
        op, sc = [('lw', 4), ('ld', 8), ('sw', 4), ('sd', 8)][k - 1]
        return f"c.{op} {random.choice(P)}, {random.randrange(0, 32 * sc, sc)}({random.choice(P)})"
    if k == 5:
        return "c.nop"
    if k == 6:
        return f"c.addi {reg(True)}, {nz(-32, 31)}"
    if k == 7:
        return f"c.addiw {reg(True)}, {random.randint(-32, 31)}"
    if k == 8:
        return f"c.li {reg(True)}, {random.randint(-32, 31)}"
    if k == 9:
        return f"c.addi16sp sp, {nz(-512, 496, 16)}"
    if k == 10:
        rd = random.choice([r for r in ABI[1:] if r != 'sp'])
        return f"c.lui {rd}, {random.choice([random.randint(1, 31), random.randint(0xFFFE0, 0xFFFFF)])}"
    if k in (11, 12):
        return f"c.{'srli' if k == 11 else 'srai'} {random.choice(P)}, {random.randint(1, 63)}"
    if k == 13:
        return f"c.andi {random.choice(P)}, {random.randint(-32, 31)}"
    if k == 14:
        op = random.choice(['sub', 'xor', 'or', 'and', 'subw', 'addw'])
        return f"c.{op} {random.choice(P)}, {random.choice(P)}"
    if k == 15:
        return f"c.slli {reg(True)}, {random.randint(1, 63)}"
    if k == 16:
        return f"c.lwsp {reg(True)}, {random.randrange(0, 256, 4)}(sp)"
    if k == 17:
        return f"c.ldsp {reg(True)}, {random.randrange(0, 512, 8)}(sp)"
    if k == 18:
        return f"c.jr {reg(True)}"
    if k == 19:
        return f"c.mv {reg(True)}, {reg(True)}"
    if k == 20:
        return "c.ebreak"
    if k == 21:
        return f"c.jalr {reg(True)}"
    if k == 22:
        return f"c.add {reg(True)}, {reg(True)}"
    return random.choice([f"c.swsp {reg()}, {random.randrange(0, 256, 4)}(sp)",
                          f"c.sdsp {reg()}, {random.randrange(0, 512, 8)}(sp)"])


R_OPS = ('add sub sll slt sltu xor srl sra or and addw subw sllw srlw sraw mul mulh mulhsu mulhu div divu '
         'rem remu mulw divw divuw remw remuw').split()
I_OPS = 'addi slti sltiu xori ori andi addiw'.split()
LOADS = 'lb lh lw ld lbu lhu lwu'.split()
STORES = 'sb sh sw sd'.split()
AMO = 'swap add xor and or min max minu maxu'.split()
SUF = ['', '', '.aq', '.rl', '.aqrl']


def fence_set():
    s = ''.join(c for c in 'iorw' if random.random() < 0.5)
    return s or 'rw'


def plain():
    """(size, text) for a line with no label operand."""
    k = random.randrange(20)
    if k < 3:
        return 4, f"{random.choice(R_OPS)} {reg()}, {reg()}, {reg()}"
    if k < 5:
        return 4, f"{random.choice(I_OPS)} {reg()}, {reg()}, {imm(-2048, 2047)}"
    if k == 5:
        op = random.choice(['slli', 'srli', 'srai', 'slliw', 'srliw', 'sraiw'])
        return 4, f"{op} {reg()}, {reg()}, {random.randint(0, 31 if op.endswith('w') else 63)}"
    if k == 6:
        if random.random() < 0.2:
            return 4, f"{random.choice(LOADS)} {reg()}, ({reg()})"
        return 4, f"{random.choice(LOADS)} {reg()}, {imm(-2048, 2047)}({reg()})"
    if k == 7:
        return 4, f"{random.choice(STORES)} {reg()}, {imm(-2048, 2047)}({reg()})"
    if k == 8:
        return 4, f"{random.choice(['lui', 'auipc'])} {reg()}, {imm(0, 0xfffff)}"
    if k == 9:
        v = li_val()
        return li_size(v), f"li {reg()}, {v if random.random() < 0.6 or v < 0 else hex(v)}"
    if k == 10:
        p = random.choice(['nop', 'mv', 'not', 'neg', 'negw', 'sext.w', 'seqz', 'snez', 'sltz', 'sgtz'])
        return 4, p if p == 'nop' else f"{p} {reg()}, {reg()}"
    if k == 11:
        c = random.randrange(8)
        rw, ro = random.choice(CSR_RW), random.choice(CSR_RW + CSR_RO)
        if random.random() < 0.2:
            rw = str(random.choice([0x340, 0x7c0, 0x5c0, 0x9c0]))
        return 4, [f"csrrw {reg()}, {rw}, {reg()}", f"csrrs {reg()}, {ro}, zero", f"csrrc {reg()}, {rw}, {reg()}",
                   f"csrrwi {reg()}, {rw}, {random.randint(0, 31)}", f"csrsi {rw}, {random.randint(0, 31)}",
                   f"csrr {reg()}, {ro}", f"csrw {rw}, {reg()}",
                   random.choice([f"csrs {rw}, {reg()}", f"csrc {rw}, {reg()}", f"csrci {rw}, 7",
                                  f"csrwi {rw}, 1", 'rdcycle a0', 'rdtime t1', 'rdinstret s2'])][c]
    if k == 12:
        return 4, random.choice(['ecall', 'ebreak', 'mret', 'sret', 'wfi', 'fence.i', 'fence', 'sfence.vma',
                                 f"sfence.vma {reg()}", f"sfence.vma {reg()}, {reg()}",
                                 f"fence {fence_set()}, {fence_set()}"])
    if k in (13, 14):
        w = random.choice(['w', 'd'])
        a = random.randrange(3)
        if a == 0:
            return 4, f"lr.{w}{random.choice(SUF)} {reg()}, ({reg()})"
        if a == 1:
            return 4, f"sc.{w}{random.choice(SUF)} {reg()}, {reg()}, ({reg()})"
        return 4, f"amo{random.choice(AMO)}.{w}{random.choice(SUF)} {reg()}, {reg()}, ({reg()})"
    if k == 15:
        return 4, random.choice([f"jalr {reg(True)}", f"jalr {reg()}, {imm(-2048, 2047)}({reg()})",
                                 f"jr {reg(True)}", 'ret', f"jalr ra, 0({reg()})"])
    if k == 16:
        return 4, random.choice([f"addi {reg()}, {reg()}, K{random.randrange(3)}",
                                 f"li {reg()}, (1 << 9) | 5", f"andi {reg()}, {reg()}, ~0xf",
                                 f"lui {reg()}, %hi(0x12345678)", f"addi {reg()}, {reg()}, %lo(0x12345678)",
                                 f"lui {reg()}, %hi(K{random.randrange(4)})",
                                 f"addi {reg()}, {reg()}, (K1 - 3) * 2"])
    return 4, f"addi {reg()}, {reg()}, {imm(-2048, 2047)}  # trailing comment"


def main():
    lines = ['# chisel corpus: every word must equal LLVM 19 (rustc), base 0x80000000', '',
             '.equ K0, 12', '.equ K1, K0 * 3 + 1', '.set K2, -7', '.equ K3, 0x7fffe000']
    items = []  # [size, text-or-spec, label]
    off = 0
    n = 0
    while n < 2200:
        label = None
        r = random.random()
        if r < 0.15:
            label = f"L{n}"
        elif r < 0.2:
            label = str(random.randint(1, 9))
        k = random.random()
        if k < 0.10:
            items.append([4, ('br4', random.choice(['beq', 'bne', 'blt', 'bge', 'bltu', 'bgeu', 'bgt', 'ble', 'bgtu', 'bleu'])), label])
        elif k < 0.14:
            items.append([4, ('br1', random.choice(['beqz', 'bnez', 'blez', 'bgez', 'bltz', 'bgtz'])), label])
        elif k < 0.17:
            items.append([4, ('jal', random.randrange(3)), label])
        elif k < 0.19:
            items.append([8, ('far', random.choice(['call', 'tail', 'lla'])), label])
        elif k < 0.20:
            items.append([8, ('pcrel', random.choice(['addi', 'ld', 'lw'])), label])
        elif k < 0.25:
            cnt = random.choice([1, 2, 3])
            push = random.random() < 0.3
            items.append([0, '.option push' if push else '.option rvc', label])
            if push:
                items.append([0, '.option rvc', None])
            for _ in range(cnt):
                if random.random() < 0.15:
                    items.append([2, ('cbr', random.choice(['c.beqz', 'c.bnez', 'c.j'])), None])
                else:
                    items.append([2, c_inst(), None])
            items.append([0, '.option pop' if push else '.option norvc', None])
        elif k < 0.28 and off % 4 == 0:
            d = random.randrange(7)
            if d == 0:
                items.append([12, ('word', None), label])
            elif d == 1:
                items.append([16, ('dword', None), label])
            elif d == 2:
                items.append([4, f".half {random.randint(-32768, 32767)}, {hex(random.randint(0, 65535))}", label])
            elif d == 3:
                items.append([4, f".byte 1, 255, -1, {hex(random.randint(0, 127))}", label])
            elif d == 4:
                items.append([4, '.asciz "hey"', label])
                items.append([4, '.ascii "a\\tb\\n"', None])
            elif d == 5:
                items.append([8, f".zero 8", label])
            else:
                items.append([4, ('diff', None), label])
            if random.random() < 0.3:
                pad = (-(off + sum(i[0] for i in items[-2:] if True)) % 8)
                items.append([None, '.balign 8', None])
        else:
            s, t = plain()
            items.append([s, t, label])
        off = sum(i[0] for i in items if i[0] is not None)  # balign sizes fixed below
        # recompute exact offset including balign
        o = 0
        for it in items:
            if it[1] == '.balign 8' and it[0] is None or it[1] == '.balign 8':
                it[0] = (-o) % 8
            o += it[0]
        off = o
        n += 1
    # offsets and label positions
    offs, o = [], 0
    named, numeric = {}, []
    for i, it in enumerate(items):
        offs.append(o)
        if it[2] is not None:
            if it[2].startswith('L'):
                named[it[2]] = o
            else:
                numeric.append((o, it[2], i))
        o += it[0]
    total = o
    pc_n = 0

    def pick_named(here, rng, align=2):
        c = [nm for nm, t in named.items() if abs(t - here) <= rng and t % align == 0]
        return random.choice(c) if c else None

    def pick_numeric(i, here, rng):
        cands = []
        prev = {}
        for (t, d, j) in numeric:
            if j <= i:
                prev[d] = t
        for d, t in prev.items():
            if abs(t - here) <= rng:
                cands.append(f"{d}b")
        nxt = {}
        for (t, d, j) in numeric:
            if j > i and d not in nxt:
                nxt[d] = t
        for d, t in nxt.items():
            if abs(t - here) <= rng:
                cands.append(f"{d}f")
        return random.choice(cands) if cands else None

    def target(i, here, rng):
        if random.random() < 0.25:
            t = pick_numeric(i, here, rng)
            if t:
                return t
        return pick_named(here, rng)

    out, mp = [], []
    for i, (size, spec, label) in enumerate(items):
        here = offs[i]
        text = spec
        if isinstance(spec, tuple):
            kind, op = spec
            if kind == 'br4':
                t = target(i, here, 4000)
                text = f"{op} {reg()}, {reg()}, {t}" if t else 'nop'
            elif kind == 'br1':
                t = target(i, here, 4000)
                text = f"{op} {reg()}, {t}" if t else 'nop'
            elif kind == 'jal':
                t = target(i, here, 100000)
                text = ([f"jal {t}", f"jal {reg()}, {t}", f"j {t}"][op]) if t else 'nop'
            elif kind == 'far':
                t = pick_named(here, 10 ** 9)
                text = (f"{op} {reg(True)}, {t}" if op == 'lla' else f"{op} {t}") if t else 'nop\nnop'
            elif kind == 'pcrel':
                t = pick_named(here, 10 ** 9)
                rd = reg(True)
                pc_n += 1
                if t is None:
                    text = 'nop\nnop'
                else:
                    lo = {'addi': f"addi {rd}, {rd}, %pcrel_lo(.Lpc{pc_n})",
                          'ld': f"ld {rd}, %pcrel_lo(.Lpc{pc_n})({rd})",
                          'lw': f"lw {rd}, %pcrel_lo(.Lpc{pc_n})({rd})"}[op]
                    text = f".Lpc{pc_n}: auipc {rd}, %pcrel_hi({t})\n{lo}"
            elif kind == 'cbr':
                rng = 2000 if op == 'c.j' else 250
                t = target(i, here, rng)
                if t is None:
                    text = 'c.nop'
                else:
                    text = f"c.j {t}" if op == 'c.j' else f"{op} {random.choice(P)}, {t}"
            elif kind == 'word':
                a, b = random.choice(list(named)), random.choice(list(named))
                text = f".word {hex(random.randint(0, 2**32 - 1))}, -1, {a}" if random.random() < 0.5 else f".word {a}, {b}, 7"
            elif kind == 'dword':
                text = f".dword {random.choice(list(named))}, {hex(random.randint(0, 2**64 - 1))}"
            elif kind == 'diff':
                a, b = random.choice(list(named)), random.choice(list(named))
                text = f".word {a} - {b}"
        lab = f"{label}: " if label else ''
        first = len(lines) + len(out) + 1
        if size:
            mp.append(f"{here} {first}")
        if label and (text.startswith('.option') or random.random() < 0.3):
            out.append(f"{label}:")
            lab = ''
            first += 1
            if size:
                mp[-1] = f"{here} {first}"
        out.extend((lab + text).split(chr(10)))
    src = '\n'.join(lines + out) + '\n'
    open('crates/chisel/tests/fixtures/corpus.s', 'w', newline='\n').write(src)
    open('crates/chisel/tests/fixtures/corpus.map', 'w', newline='\n').write('\n'.join(mp) + '\n')
    print(f"{len(src.splitlines())} lines, {total} bytes expected")
    open('crates/chisel/tests/fixtures/corpus.size', 'w').write(str(total))


main()
