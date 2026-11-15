#!/bin/bash
# T13 leader data: build the official riscv-tests (p environment: machine mode, no paging) for
# RV64 I M A C + the S and M privileged suites into flat binaries at 0x80000000.
# Source: a clone of github.com/riscv-software-src/riscv-tests (+ env submodule) in $SRC.
# Compiles with WSL clang (rustc's and mingw's toolchains have no RISC-V C front end), links
# with rustc's rust-lld, flattens with llvm-objcopy. Writes crates/silicon/tests/fixtures/riscv-tests/:
# <test>.bin, index.txt ("<test> <tohost hex> <size>"), SKIPPED (tests that do not build), LICENSE, SOURCE.
set -e
SRC="${SRC:-/c/Users/ADMIN/strata-ext/riscv-tests}"
OUT=crates/silicon/tests/fixtures/riscv-tests
T=$(mktemp -d)
LLD="$(ls "$(rustc --print sysroot)"/lib/rustlib/*/bin/rust-lld.exe | head -1)"
mkdir -p "$OUT"
rm -f "$OUT"/*.bin "$OUT/index.txt" "$OUT/SKIPPED"
# clang's assembler refuses `.weak X` followed by `.globl X` for a test's own trap handler (GNU as
# allows it). A test that defines a handler builds against a copy of env/p without that handler's
# .weak line (env-m, env-s, env-ms); the other handler stays weak.
cp "$SRC/env/encoding.h" "$T/"
for v in m s ms; do
  mkdir -p "$T/env-$v"; cp "$SRC"/env/p/* "$T/env-$v/"
  for h in $(echo $v | grep -o .); do sed -i "/\.weak ${h}tvec_handler;/d" "$T/env-$v/riscv_test.h"; done
done
WSRC=$(wsl -e wslpath -u "$(cygpath -m "$SRC")")
WT=$(wsl -e wslpath -u "$(cygpath -m "$T")")
suite() { sed -n "/^$1_sc_tests = /,/^\$/p" "$SRC/isa/$1/Makefrag" | sed 's/.*=//' | tr -d '\\' | tr -s ' \t\n' ' '; }
cc() {
  MSYS_NO_PATHCONV=1 wsl -e clang --target=riscv64-unknown-elf -march=rv64imac_zicsr_zifencei \
    -mabi=lp64 -mcmodel=medany -mno-relax -nostdlib -I"$1" -I"$WSRC/isa/macros/scalar" \
    -c "$WSRC/isa/$2/$3.S" -o "$WT/$4.o" 2> "$T/err.txt"
}
for s in rv64ui rv64um rv64ua rv64uc rv64si rv64mi; do
  for t in $(suite $s); do
    name="$s-p-$t"
    if ! cc "$WSRC/env/p" $s $t $name; then
      v=$(grep -oE "(m|s)tvec_handler changed binding" "$T/err.txt" | cut -c1 | sort -u | tr -d '
')
      if ! { [ -n "$v" ] && grep -q "changed binding" "$T/err.txt" && cc "$WT/env-$v" $s $t $name; }; then
        echo "$name: $(grep -m1 error "$T/err.txt" | sed 's/.*error: //')" | tee -a "$OUT/SKIPPED"
        continue
      fi
    fi
    "$LLD" -flavor gnu -static --script="$(cygpath -m "$SRC/env/p/link.ld")" "$T/$name.o" -o "$T/$name.elf"
    llvm-objcopy -O binary "$T/$name.elf" "$OUT/$name.bin"
    th=$(llvm-nm "$T/$name.elf" | awk '$3=="tohost"{print $1}')
    echo "$name $th $(wc -c < "$OUT/$name.bin")" >> "$OUT/index.txt"
  done
done
cp "$SRC/LICENSE" "$OUT/LICENSE"
echo "riscv-tests $(git -C "$SRC" rev-parse HEAD), env $(git -C "$SRC/env" rev-parse HEAD); built by briefs/leader/build_riscv_tests.sh with WSL clang $(wsl -e clang -dumpversion)" > "$OUT/SOURCE"
rm -rf "$T"
echo "built $(wc -l < "$OUT/index.txt")"
