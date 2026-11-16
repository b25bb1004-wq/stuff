# SIM: simulated users test the daily tools (2026-11-16)

Operator, 2026-11-16: "using miro fish test the product made dont depend on me". MiroFish's five steps
(seed, personas, simulate, report, interview) run here with you as one persona: you do a real day's work
with the Monolith tools, the way a person replacing git/bash/vim/VS Code would, and you log every place a
tool breaks. You only report; you never change code.

## Where things are
- SIM = C:/Users/ADMIN/AppData/Local/Temp/claude/838757565bfd39b2/98d4c426-dbca-4748-8fa8-3f3069ae87c5/scratchpad/sim
- Installed tools: $SIM/home/.monolith/bin/*.exe (MANIFEST beside it lists each with a one-line summary).
  In Bash: `export PATH="$SIM/home/.monolith/bin:$PATH"` at the start of each command.
- Your sandbox: $SIM/work/<persona>/ (create it). Do everything there.
- Source (read only, to confirm a cause): C:/Users/ADMIN/strata/crates/<crate>/

## Hard rules
- Never edit anything under C:/Users/ADMIN/strata except your one report file. Never touch the real home
  folder (~/.monolith, ~/.kryptonrc, ~/.profile): set HOME=$SIM/work/<persona>/home when a tool reads HOME.
- Network: 127.0.0.1 only. No downloads, no installs, no pushes except to bare repos inside your sandbox.
- Stop only processes you started, by PID (`kill PID` or `taskkill //PID N //F`), never by image name.
- No commits in C:/Users/ADMIN/strata. No secrets anywhere.
- A tool that waits for input forever: run it with `timeout 20` and report the hang.

## How to judge
For each task, find what the real tool does (git, Git Bash's bash, python's http.server, VS Code
behaviour you know) and compare. "Expected" is the real tool's behaviour or the tool's own --help promise.

## Report: C:/Users/ADMIN/strata/reports/sim/<persona>.md, at most 45 lines
Line 1: `<persona>: N tasks, P worked, B broke`.
Then a table, worst first:
`| id | tool | exact command | expected | actual (exit code, first line of output) | severity |`
severity is one of DATA-LOSS (lost or corrupted user data), BLOCKS (cannot do the task at all), WRONG
(wrong output or exit code), ANNOY (works but badly: slow, unclear error). Give a one-line repro that a
reader can paste. Then one line listing what worked. No prose essays, no fix proposals longer than one line.

Budget: about 150k tokens. Aim for 30 to 45 tasks, then write the report and stop.
