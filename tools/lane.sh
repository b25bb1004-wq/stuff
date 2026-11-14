#!/usr/bin/env bash
# Run ONE worker on ONE brief through the Antigravity CLI (agy). No Claude tokens are used.
# Usage: bash tools/lane.sh <lane> <task-id>      e.g.  bash tools/lane.sh hydrogen T05
# A lock file makes sure only one worker runs at a time (operator rule, 2026-11-14).
set -euo pipefail
cd "$(dirname "$0")/.."
AGY="${AGY:-$HOME/.gemini/bin/agy.exe}"
case "${1:-}" in
  flash) M=gemini-3.8-flash-medium ;;            # cheapest: text, docs, tables
  hydrogen) M=gemini-4-hydrogen-medium ;;        # default worker
  hydrogen-high) M=gemini-4-hydrogen-high ;;     # first retry of a hydrogen task
  argon) M=gemini-4-argon-medium ;;              # hard tasks, second reviews
  argon-high) M=gemini-4-argon-high ;;           # hardest tasks
  orchestrator) M=gemini-pro-orchestrator-g1-pro ;;
  *) echo "unknown lane: ${1:-} (flash, hydrogen, hydrogen-high, argon, argon-high, orchestrator)"; exit 2 ;;
esac
ID="${2:-}"
[ -f "briefs/$ID.md" ] || { echo "no brief: briefs/$ID.md"; exit 2; }
mkdir -p .loop-local
if [ -e .loop-local/BUSY ]; then echo "a worker is already running: $(cat .loop-local/BUSY)"; exit 3; fi
echo "$ID on $1 since $(date '+%Y-%m-%d %H:%M')" > .loop-local/BUSY
trap 'rm -f .loop-local/BUSY' EXIT
PROMPT="You are a worker on the Strata project in this folder. Read briefs/_RULES.md, then briefs/$ID.md, and do exactly that task. Touch only the files the brief lists. Run the gate the brief names and fix until its last line is CHECK PASS. Finish by writing reports/$ID.md (at most 10 lines) and print that report as your final answer."
"$AGY" -p "$PROMPT" --model "$M" --mode auto --print-timeout "${TIMEOUT:-60m}" 2>&1 \
  | tee ".loop-local/$ID.$1.log" | tail -15
