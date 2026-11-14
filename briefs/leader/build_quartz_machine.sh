#!/bin/bash
# Assemble the T23 leader programs with rustc's LLVM (rv64im, no compressed forms) into raw
# binaries linked at 0x8000_0000. Needs the riscv64gc-unknown-none-elf target and llvm-objcopy.
set -e
D=crates/quartz/tests/fixtures/machine
T=$(mktemp -d)
for s in "$D"/*.S; do
  n=$(basename "$s" .S)
  cat > "$T/$n.rs" <<RS
#![no_std]
#![no_main]
core::arch::global_asm!(".section .text.start\n.globl _start\n", include_str!("$(cygpath -m "$PWD/$D/common.inc")"), include_str!("$(cygpath -m "$PWD/$s")"));
#[panic_handler]
fn p(_: &core::panic::PanicInfo) -> ! { loop {} }
RS
  rustc --edition 2021 --target riscv64gc-unknown-none-elf -C opt-level=2 -C panic=abort \
    -C target-feature=-c -C link-arg=-Ttext=0x80000000 -C link-arg=--gc-sections -C link-arg=-e_start \
    "$T/$n.rs" -o "$T/$n.elf"
  llvm-objcopy -O binary "$T/$n.elf" "$D/$n.bin"
  echo "$n $(wc -c < "$D/$n.bin") bytes"
done
rm -rf "$T"
