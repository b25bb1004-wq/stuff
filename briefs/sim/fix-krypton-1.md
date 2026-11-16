# FIX krypton-1: scripts behave like a POSIX shell (from SIM p2, reports/sim/p2-shell.md)
Files you may touch: crates/krypton/** only. Gate: `bash tools/check.sh krypton` must end `CHECK PASS krypton`.
Zero external crates. Do not weaken, skip or delete existing tests (the Escapes::Posix/Windows tests stay as they are).
Add one test per item (tests/sim_fixes.rs). Report: reports/sim/fix-krypton-1.md, 12 lines max. Budget about 150k. No skills.

Expected behaviour = POSIX sh / bash. Row ids refer to the report.
1. R15: with stdin not a terminal (`printf 'echo a\n' | krypton`), no prompt text ("$ ") is written. Prompts only
   when stdin is a terminal (std::io::IsTerminal). Output for that input is exactly `a\n`.
2. R9: CRLF script files: a CR right before LF is treated as part of the line ending (strip it when reading a
   script file or -c text). `x=5\r\necho "x=$x"\r\n` prints `x=5\n`.
3. R12: in `krypton s.sh`, $0 is `s.sh` (the path as given). With -c, $0 is ARG0 if given, else `krypton`.
4. R13: syntax errors in a script name the file and line: `bad2.sh: line 2: syntax error near unexpected token ')'`.
5. R8: `set -o pipefail` (and `set +o pipefail`, `set -o` listing) is supported: the pipeline status is the last
   non-zero status. An unknown `set` option is an error that stops the script with exit 2, not exit 0.
6. R10: `command -v NAME` prints the resolved path (or the name for builtins/functions), exit 0; exit 1 if not found.
   `command NAME args` runs NAME skipping functions.
7. R11: `trap 'cmds' EXIT` runs cmds when the shell exits (normal end, `exit N`, set -e failure); `trap - EXIT` clears it.
   INT/TERM may be accepted and stored; say in the report what is not supported.
8. R3: `exec` builtin: `exec N>file`, `exec N<file`, `exec N>>file`, `exec N>&-` change the shell's own fds for later
   commands; `exec cmd args` runs cmd and exits with its status. `echo hi >&3` then writes to the file.
9. R21: a child program gets argv[0] as typed (`cat`), not the resolved full path, so its messages say `cat: ...`.
