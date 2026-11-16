#!/usr/bin/env bash
# Checks that every installed Monolith tool describes itself as docs/ai-computer/ACI.md requires.
# Usage: tools/aci-check.sh [BIN_DIR]   (default: ~/.monolith/bin). Needs `aci` in BIN_DIR.
set -uo pipefail
BIN="${1:-$HOME/.monolith/bin}"
pass=0; total=0; bad=""
for exe in "$BIN"/*; do
  t="$(basename "$exe" .exe)"
  [ -x "$exe" ] || continue
  [ "$t" = "aci" ] && continue
  total=$((total+1))
  if "$exe" --describe 2>/dev/null | "$BIN/aci" check-manifest - 2>/dev/null; then pass=$((pass+1)); else bad="$bad $t"; fi
done
if [ "$pass" -eq "$total" ] && [ "$total" -gt 0 ]; then echo "ACI PASS $pass/$total"; else echo "ACI FAIL $pass/$total:$bad"; exit 1; fi
