# Phase 1 gate: simulated AI agents use the tools from their manifests alone (2026-11-16)

You are an AI agent given a new computer. The only documentation you have is each tool's manifest: the
JSON that `<tool> --describe` prints. Your job is to finish 30 tasks with those tools and report honestly
what worked, what broke, and what the manifest failed to tell you.

## Where things are
- SB = C:/Users/ADMIN/AppData/Local/Temp/claude/838757565bfd39b2/98d4c426-dbca-4748-8fa8-3f3069ae87c5/scratchpad/sim/aci-home
- Tools: $SB/.monolith/bin. In every Bash command start with `export PATH="$SB/.monolith/bin:$PATH"`.
- Manifests: $SB/manifests/<tool>.json (the same as `<tool> --describe`). Read them first.
- Sample files: $SB/samples (copy what you need into your work folder; do not edit the originals).
- Your work folder: $SB/work1 (agent A1) or $SB/work2 (agent A2). Do everything there. Set
  `HOME=$SB/work1/home` (or work2) when a tool reads HOME.

## Hard rules
- Use Monolith tools only through `<tool> --json ...` (and `--describe`). Never run `--help`, never read
  source under C:/Users/ADMIN/strata, never read other reports. `--json` must be the first argument.
- You may use python, cat, ls, od, cmp only to check results (for example to hash a file and compare).
- Network: 127.0.0.1 only. Start servers in the background, read their first stdout line, and stop
  them by PID only (`netstat -ano` to find it, then `taskkill //PID N //F`). Never kill by image name.
- Wrap any command that might wait with `timeout 60`.
- Never edit anything under C:/Users/ADMIN/strata except your one report file. No secrets anywhere.

## How to judge a task
PASS when the envelope is what the task needs and you checked the value (not just `ok:true`).
FAIL when the tool errs, returns wrong data, or the manifest did not let you find the right command
within three tries. Count every task; do not skip.

## Report: C:/Users/ADMIN/strata/reports/aci/<agent>.md, at most 50 lines
Line 1: `<agent>: 30 tasks, P passed, F failed`.
Then a table, failures first:
`| id | tool | command you ran | expected | actual (exit, first 120 chars of stdout) | what the manifest failed to tell me |`
Passing tasks get one row each with the command and the checked value; the last column may be empty.
End with one line: the three manifest fixes that would help an agent most. Your final message repeats
line 1 and the failure rows.

Budget: about 150k tokens. Do the tasks in order; if you run short, write the report with what you have
and mark the rest NOT RUN.
