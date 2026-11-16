# Persona p2-shell: a power user who makes krypton their only shell
Read briefs/sim/COMMON.md first. Persona name for paths: p2-shell.
Use `krypton -c '...'`, script files (`krypton file.sh`), and piped stdin (`printf 'cmd\n' | krypton`).
Compare every result with Git Bash's `bash -c` on the same input.
A day's work to try:
- real programs from PATH: git status, cargo --version, python -c, where, findstr, cmd builtins (dir, ver, type), a .cmd and a .bat file you write.
- pipes between real programs (git log --oneline | findstr D6 | wc -l), big output (python -c "print('x\n'*200000)" | wc -l), exit codes ($?, pipelines, && ||, set -e if supported).
- redirection: > >> 2> 2>&1 < and here-docs; to paths with spaces; to /c/... style paths and C:\ paths.
- quoting: Windows paths with backslashes, single vs double quotes, $VAR inside quotes, unicode args (echo naïve), empty args.
- globbing (*.rs, **), no-match behaviour; tilde, cd -, cd to a missing dir.
- variables: export to a child (python -c "import os;print(os.environ['X'])"), unset, ${X:-default}, positional args, $#, $@.
- control flow: if/elif, for, while, case, functions, return codes, arithmetic $((..)), command substitution $(..) and nested.
- startup file: HOME=$SIM/work/p2-shell/home with a .kryptonrc that sets an alias or variable; does it apply?
- a CRLF-line-ending script file; a script with a syntax error (clear message and line number?).
- what a daily bash user reaches for that krypton lacks (name it).
