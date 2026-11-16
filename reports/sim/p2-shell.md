p2-shell: 60 tasks, 42 worked, 18 broke
| id | tool | exact command | expected | actual (exit code, first line of output) | severity |
|---|---|---|---|---|---|
| R1 | krypton | `krypton -c 'python -c "print(1)"'` with stdin inherited (run from a bash tool; add `</dev/null` for contrast) | prints `1`, exit 0 (bash does the same) | no output, killed by timeout at 6 s (rc 137). 10 of 10 hung with inherited stdin, 0 of 10 with `</dev/null`. Compound scripts T27, T31, T36, T38, T40 to T42 hung the same way | BLOCKS |
| R2 | krypton | `printf 'MYV=fromrc\n' > $HOME/.kryptonrc; printf 'echo "MYV=[$MYV]"\n' \| HOME=<windows path> krypton` | `MYV=[fromrc]` | `MYV=[]`, rc 0. `.kryptonrc` runs only on the TTY path (interactive/mod.rs:41), so `-c`, script and piped modes ignore it. TTY path not verified here | BLOCKS |
| R3 | krypton | `krypton -c 'exec 3>fx.txt; echo hi >&3; echo rc=$?'` | `rc=0`, fx.txt holds `hi` | `krypton: exec: command not found`, then `3: Bad file descriptor`, `rc=1` | BLOCKS |
| R4 | krypton | `krypton -c 'cmd //c "exit 9"; echo rc=$?'` | cmd exits 9 (bash gives `rc=9`) | opens an interactive cmd banner and prompt, then `rc=0` | WRONG |
| R5 | krypton | `krypton -c 'printf "%5.2f\|%s\n" 3.14159 ok'` | ` 3.14\|ok` | `%f\|ok`, rc 0, no error | WRONG |
| R6 | krypton | `krypton -c "echo \$'a\tb'"` | `a` TAB `b` | `$a\tb`, rc 0, no error | WRONG |
| R7 | krypton | `krypton -c 'kill -0 $$; echo rc=$?'` | `rc=0` (shell is alive) | `kill: <pid>: No such process`, rc 1. `$$` is the Windows PID (python os.getppid() agrees) and this kill does not find it | WRONG |
| R8 | krypton | `krypton -c 'set -o pipefail; false \| true; echo rc=$?'` | pipefail accepted, `rc=1` | `krypton: set: pipefail: invalid option`, rest of script skipped, krypton exits 0 | WRONG |
| R9 | krypton | `printf 'x=5\r\necho "x=$x"\r\n' > c.sh; krypton c.sh \| od -c` | same bytes as bash: `x=5\n` | `x=5\r\r\n`: CR stays inside the last word of each line | WRONG |
| R10 | krypton | `krypton -c 'command -v python; echo rc=$?'` | a path, `rc=0` | `command: command not found`, rc 127 | WRONG |
| R11 | krypton | `krypton -c 'trap "echo bye" EXIT; echo body'` | `body` then `bye` | `trap: command not found`, `body` only; the EXIT trap never runs | WRONG |
| R12 | krypton | `printf 'echo "$0"\n' > s.sh; krypton s.sh` | `s.sh` | `krypton` | WRONG |
| R13 | krypton | `printf 'echo one\necho two )\n' > bad2.sh; krypton bad2.sh` | `bad2.sh: line 2: syntax error near unexpected token` | `krypton: 2:10: unexpected token`, no file name, no line text | ANNOY |
| R14 | krypton | `krypton -c 'type repo/f1.txt'` | file contents, or a not-found message | `The syntax of the command is incorrect.`, rc 1 (cmd reads `/f1.txt` as a switch; cmd builtins get raw args) | ANNOY |
| R15 | krypton | `printf 'echo a\nfalse\n' \| krypton` | `a` only, rc 1 | `$ a` then `$ $ ` prompts written to stdout when stdin is a pipe | ANNOY |
| R16 | krypton | `krypton -c 'git status --short; echo ${PIPESTATUS[0]}'` | bash runs git and prints the status | `1:48: bad substitution`, whole line aborts before any command, rc 2 | ANNOY |
| R17 | krypton | `krypton -c '[[ abc == a* ]] && echo match'` | `match` | `[[: command not found`, rc 127 | ANNOY |
| R18 | krypton | `krypton -c 'a=(1 2 3); echo ${#a[@]}'` | `3` | `1:3: unexpected token`, rc 2 (no arrays) | ANNOY |
| R19 | krypton | `krypton -c 'alias ll="ls -l"; umask; hash -r'` | builtins run | `alias`, `umask`, `hash` (and `getopts`) each `command not found`; no alias support, so a daily alias set is impossible | ANNOY |
| R20 | krypton | `krypton -c 'pwd; cd /tmp; pwd; echo ~'` | `/c/Users/ADMIN`-style paths, as in bash | `C:\Users\...` with backslashes; `~` gives `C:\Users\ADMIN`; pasting `$(pwd)/x` into bash breaks | ANNOY |
| R21 | krypton | `krypton -c 'cat /no/such_zz'` | `cat: /no/such_zz: No such file or directory` | `/usr/bin/cat: ...` (argv[0] full path leaks into messages) | ANNOY |
| R22 | krypton | `krypton -c 'hello.cmd one two'` (hello.cmd in cwd) | runs the .cmd as cmd.exe would | `krypton: hello.cmd: command not found`, rc 127. `./hello.cmd` works (bash fails on both forms) | ANNOY |

Worked (42 of 60): git status and git log in a repo; findstr and wc in pipes; python env export and argv; 200,001-line pipe; $? and && / ||; unset, `${X:-}`, `${X:?}`, `${#v}`, `%`; positional args and $#; $((..)); nested $(..); case; functions and return codes; read; local; eval; source; while-read; globs and no-match passthrough; cd -; cd to a missing dir (clear message, rc 1); redirects > >> 2> 2>&1 to paths with spaces, to C:\ and /c/ paths; stdin <; heredocs quoted and unquoted; .cmd and .bat via ./ with args; cmd /c exit codes; set -e; background & and wait.
Not a krypton fault: Unicode args through python print mangled bytes in both shells (console codepage). About 45 further follow-up probes are summarised in the rows above.
