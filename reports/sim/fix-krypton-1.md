# FIX krypton-1 (SIM p2 shell rows)
Gate: `bash tools/check.sh krypton` ends `CHECK PASS krypton`. New tests: crates/krypton/tests/sim_fixes.rs (8 pass on Windows; R21 test is unix-only).
R15 done: prompts only when stdin is a terminal (main.rs). Conflict: existing exec_external.rs asserted `$ `/`> ` on piped stdin; I inverted those two asserts to "no prompt", output and exit checks kept. Leader to confirm.
R9 done: CRLF read as LF for script files, -c text and `-` stdin (lf_only in main.rs).
R12 done: $0 is the script path as given; -c uses ARG0 if given, else `krypton` (Shell::set_arg0).
R13 done: parser says `syntax error near unexpected token ')'`; script files report `file: line N: msg` and exit 2 before running.
R8 done: `set -o pipefail` (also `set +o`), `set -o` lists options; unknown set option stops the script with exit 2. `set +o` listing is not in reusable form.
R10 done: `command -v NAME` (path, or name for builtins/functions; exit 1 if missing); `command NAME args` skips functions. `command -v exec` says not found.
R11 done: `trap ACTION EXIT` (or 0) runs once when the shell finishes (normal end, exit N, set -e); `trap - EXIT` clears. INT/TERM are stored but never run; other signals are rejected.
R3 done: bare `exec N>f`, `N<f`, `N>>f`, `N>&-` change the shell's fds for later commands; `exec cmd args` runs cmd and exits with its status. Redirs inside if/loop bodies do not persist after them.
R21 done on unix (CommandExt::arg0). Windows: std has no argv[0] override, so children still see the full path; not fixed.
Not touched: other crates, no commits.
