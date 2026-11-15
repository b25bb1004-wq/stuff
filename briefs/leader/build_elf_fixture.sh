#!/bin/bash
# T22 leader fixture: an LLD-linked RV64 ELF with a text segment (RX) and a data segment (RW,
# with .bss so memsz > filesz). Writes crates/chisel/tests/fixtures/elf/prog.elf and prog.txt
# (entry, LOAD segments and named symbols, from llvm-readelf). Run from the repo root.
set -e
D=crates/chisel/tests/fixtures/elf
T=$(mktemp -d)
cat > "$T/link.ld" <<'LD'
ENTRY(_start)
PHDRS { text PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS {
  . = 0x80000000;
  .text : { *(.text.start) *(.text*) } :text
  .rodata : { *(.rodata*) } :text
  . = ALIGN(0x1000) + 0x10;
  .data : { *(.data.prog) } :data
  .bss : { *(.bss.prog) } :data
}
LD
cat > "$T/prog.s" <<'S'
.option norelax
.option arch, +m, +a, +c
.section .text.start, "ax"
.globl _start
_start:
    la a0, counter          # .data word starts at 41, must reach 42
    lw a1, 0(a0)
    addi a1, a1, 1
    sw a1, 0(a0)
    li a2, 42
    bne a1, a2, fail
    la a0, zeros            # .bss must read as zero
    li a3, 64
1:  ld a4, 0(a0)
    bnez a4, fail
    addi a0, a0, 8
    addi a3, a3, -8
    bnez a3, 1b
    la a0, msg
    call puts
    li t0, 0x100000
    li t1, 0x5555
    sw t1, 0(t0)
2:  j 2b
fail:
    li t0, 0x100000
    li t1, (7 << 16) | 0x3333
    sw t1, 0(t0)
3:  j 3b
.type puts, @function
puts:
    li t0, 0x10000000
1:  lbu t1, 0(a0)
    beqz t1, 3f
2:  lbu t2, 5(t0)
    andi t2, t2, 0x20
    beqz t2, 2b
    sb t1, 0(t0)
    addi a0, a0, 1
    j 1b
3:  ret
.section .rodata.prog, "a"
.type msg, @object
msg: .asciz "elf ok\n"
.section .data.prog, "aw"
.globl counter
.type counter, @object
counter: .word 41
.section .bss.prog, "aw", @nobits
.p2align 3
.type zeros, @object
zeros: .zero 64
S
cat > "$T/c.rs" <<RS
#![no_std]
#![no_main]
core::arch::global_asm!(include_str!("$(cygpath -m "$T/prog.s")"));
#[panic_handler]
fn p(_: &core::panic::PanicInfo) -> ! { loop {} }
RS
rustc --edition 2021 --target riscv64gc-unknown-none-elf -C opt-level=2 -C panic=abort \
  -C link-arg=-T$(cygpath -m "$T/link.ld") -C link-arg=--no-relax "$T/c.rs" -o "$D/prog.elf"
llvm-readelf -h -l -s "$D/prog.elf" > "$T/re.txt"
python -I -X utf8 - "$T/re.txt" "$D/prog.txt" <<'PY'
import re, sys
t = open(sys.argv[1], encoding="utf8").read()
out = ["entry " + re.search(r"Entry point address:\s+(0x[0-9a-f]+)", t).group(1)]
for m in re.finditer(r"^\s+LOAD\s+(0x\w+) (0x\w+) 0x\w+ (0x\w+) (0x\w+) (\S+(?: \S)?)\s+0x\w+$", t, re.M):
    off, va, fs, ms, fl = m.groups()
    f = (4 if "R" in fl else 0) | (2 if "W" in fl else 0) | (1 if "E" in fl else 0)
    out.append(f"load {va} {off} {fs} {ms} {f}")
for m in re.finditer(r"^\s+\d+: ([0-9a-f]{16})\s+\d+ (NOTYPE|FUNC|OBJECT)\s+\w+\s+\w+\s+\S+ (\S+)$", t, re.M):
    out.append(f"sym {m.group(3)} 0x{m.group(1)}")
open(sys.argv[2], "w", encoding="utf8", newline="\n").write("\n".join(out) + "\n")
print("\n".join(out))
PY
rm -rf "$T"
