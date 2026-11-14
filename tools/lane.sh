#!/usr/bin/env bash
# Run ONE worker on ONE brief through the Antigravity CLI (agy). No Claude tokens are used.
# Usage: bash tools/lane.sh <lane> <task-id>      e.g.  bash tools/lane.sh hydrogen T05
# A lock file makes sure only one worker runs at a time (operator rule, 2026-11-14).
# The worker may edit files and run ONLY the exact commands listed below for its own crate
# (agy headless mode denies every other shell command; a rule must match the command exactly).
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
  hydrogen-xhigh) M=gemini-4-hydrogen-xhigh ;;   # second retry of a hydrogen task
  sonnet) M=claude-sonnet-5-5-high ;;            # Claude through agy quota: careful Rust, reviews
  opus) M=claude-opus-5-5-medium ;;              # Claude through agy quota: hardest design work
  oss) M=gpt-oss-120b-medium ;;                  # cheap text and docs
  *) echo "unknown lane: ${1:-} (flash, hydrogen, hydrogen-high, hydrogen-xhigh, argon, argon-high, orchestrator, sonnet, opus, oss)"; exit 2 ;;
esac
ID="${2:-}"
[ -f "briefs/$ID.md" ] || { echo "no brief: briefs/$ID.md"; exit 2; }
CRATE="$(grep -o 'crates/[a-z_]*' "briefs/$ID.md" | head -1 | cut -d/ -f2)"
[ -n "$CRATE" ] || { echo "brief names no crates/<name> path"; exit 2; }
mkdir -p .loop-local
if [ -e .loop-local/BUSY ]; then echo "a worker is already running: $(cat .loop-local/BUSY)"; exit 3; fi
echo "$ID on $1 since $(date '+%Y-%m-%d %H:%M')" > .loop-local/BUSY
trap 'rm -f .loop-local/BUSY' EXIT
CMDS=("bash tools/check.sh $CRATE" "cargo fmt -p $CRATE" "cargo check -p $CRATE --all-targets" \
      "cargo test -p $CRATE" "cargo test -p $CRATE -q" \
      "cargo clippy -p $CRATE --all-targets -- -D warnings")
RULES=""; LIST=""
for c in "${CMDS[@]}"; do RULES="$RULES\"command($c)\","; LIST="$LIST  $c ;"; done
echo "{\"permissions\":{\"allow\":[${RULES%,}]}}" > ".loop-local/settings.$ID.json"
PROMPT="You are a worker on the Strata project in this folder. Read briefs/_RULES.md, then briefs/$ID.md, and do exactly that task. Touch only the files the brief lists. You can edit files freely. The ONLY shell commands that will run are these, typed exactly, one per call, with no cd, pipes or extra flags: $LIST every other command is denied. Use them to fix until the last line of 'bash tools/check.sh $CRATE' is CHECK PASS $CRATE. Finish by writing reports/$ID.md (at most 10 lines) and print that report as your final answer."
"$AGY" -p "$PROMPT" --model "$M" --mode accept-edits --settings ".loop-local/settings.$ID.json" \
  --print-timeout "${TIMEOUT:-60m}" 2>&1 | tee ".loop-local/$ID.$1.log" | tail -15
