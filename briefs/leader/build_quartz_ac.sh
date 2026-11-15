#!/bin/bash
# T13a leader fixtures with rustc's LLVM: c_pairs.txt and amo.txt (encodings), rvc.bin and
# amo_sem.bin (self-checking programs), smoke.bin (rustc-compiled Rust, compressed + atomics).
# Run from the repo root after gen_quartz_ac.py. Needs riscv64gc-unknown-none-elf, llvm-objcopy.
set -e
D=crates/silicon/tests/fixtures/ac
M=crates/silicon/tests/fixtures/machine
T=$(mktemp -d)
RC="rustc --edition 2021 --target riscv64gc-unknown-none-elf -C opt-level=2 -C panic=abort"
cat > "$T/link.ld" <<'LD'
ENTRY(_start)
SECTIONS {
  . = 0x80000000;
  .text : { *(.text.start) *(.text .text.*) }
  .rodata : { *(.rodata .rodata.* .srodata .srodata.*) }
  .data : { *(.data .data.* .sdata .sdata.*) }
  .bss : { *(.bss .bss.* .sbss .sbss.*) }
}
LD
asm_bin() { # $1 name, $2 file list for include_str (common first if wanted)
  cat > "$T/$1.rs" <<RS
#![no_std]
#![no_main]
core::arch::global_asm!(".section .text.start\n.globl _start\n", $2);
#[panic_handler]
fn p(_: &core::panic::PanicInfo) -> ! { loop {} }
RS
  $RC -C link-arg=-T$(cygpath -m "$T/link.ld") -C link-arg=-e_start "$T/$1.rs" -o "$T/$1.elf"
  llvm-objcopy -O binary "$T/$1.elf" "$T/$1.bin"
}
inc() { echo "include_str!(\"$(cygpath -m "$PWD/$1")\")"; }
asm_bin c_pairs "\"_start:\n\", $(inc $D/c_pairs.s)"
asm_bin amo "\"_start:\n\", $(inc $D/amo.s)"
asm_bin rvc "$(inc $M/common.inc), \".option rvc\n.option arch, +a\n\", $(inc $D/rvc.S)"
asm_bin amo_sem "$(inc $M/common.inc), $(inc $D/amo_sem.S)"
cp "$T/rvc.bin" "$T/amo_sem.bin" "$D/"
$RC -C link-arg=-T$(cygpath -m "$T/link.ld") "$D/smoke.rs" -o "$T/smoke.elf"
llvm-objcopy -O binary "$T/smoke.elf" "$D/smoke.bin"
python -I - "$T" "$D" <<'PY'
import sys
t, d = sys.argv[1:3]
b = open(f"{t}/c_pairs.bin", "rb").read()
src = open(f"{d}/c_pairs_src.txt").read().splitlines()
assert len(b) == 6 * len(src), (len(b), len(src))
with open(f"{d}/c_pairs.txt", "w", newline="\n") as f:
    for i, line in enumerate(src):
        h = int.from_bytes(b[6 * i:6 * i + 2], "little")
        w = int.from_bytes(b[6 * i + 2:6 * i + 6], "little")
        assert h & 3 != 3 and w & 3 == 3, line
        f.write(f"{h:04x} {w:08x} {line}\n")
b = open(f"{t}/amo.bin", "rb").read()
fields = open(f"{d}/amo_fields.txt").read().splitlines()
assert len(b) == 4 * len(fields)
with open(f"{d}/amo.txt", "w", newline="\n") as f:
    for i, line in enumerate(fields):
        f.write(f"{int.from_bytes(b[4 * i:4 * i + 4], 'little'):08x} {line}\n")
PY
rm -f "$D/c_pairs.s" "$D/amo.s" "$D/c_pairs_src.txt" "$D/amo_fields.txt"
for n in rvc amo_sem smoke; do echo "$n $(wc -c < "$D/$n.bin") bytes"; done
wc -l "$D/c_pairs.txt" "$D/amo.txt"
rm -rf "$T"
