p1-git-dev: 36 tasks, 19 worked, 17 broke
| id | tool | exact command | expected | actual (exit code, first line of output) | severity |
|---|---|---|---|---|---|
| T34 | chronos add | `echo s > target/leak.txt; chronos add .; chronos status` | target/ is in .gitignore, so skip it | exit 0; status lists "A target/leak.txt" (ignored file staged for commit) | WRONG |
| T8 | chronos add | `rm rustfmt.toml; chronos add rustfmt.toml` | stage the deletion like git add | exit 1, "cannot stat '...\rustfmt.toml': os error 2" | BLOCKS |
| T10 | chronos add and commit | `rm GEMINI.md; chronos add .; chronos commit -m x` | commit includes the deletion | exit 0; deletion not staged, GEMINI.md still in HEAD tree (chronos add . skips deletions) | BLOCKS |
| T30 | chronos export | `git init -q exp; chronos export exp` (GIT_DIR form, per help) | append chronos commits to the new repo | exit 1, "git fast-import: The pipe is being closed"; "not a valid commit: e84b2b2e..." (needs base history in target) | BLOCKS |
| T19 | chronos merge | `chronos merge feature-x` (ff or conflicting) | merge branch, conflict markers on conflict | exit 2, "unknown subcommand 'merge'" (no merge verb at all) | BLOCKS |
| T21 | chronos stash | `chronos stash list` | stash and pop | exit 2, "unknown subcommand 'stash'" | BLOCKS |
| T35 | chronos (unstage) | `chronos add target/leak.txt; chronos reset` | unstage a mistaken add | exit 2, "unknown subcommand 'reset'"; no verb can unstage, so the file stays staged | BLOCKS |
| T36 | chronos (verbs) | `for v in rm mv reset restore show tag blame pull push fetch clone remote config rebase; do chronos $v; done` | git's daily verbs | each exit 2, "unknown subcommand"; no rm, mv, show, tag, remote, or push (only serve HTTP push) | BLOCKS |
| T4 | chronos status | `echo x > new.txt; chronos status` | "Untracked files: new.txt" | exit 0; new.txt not shown at all (only M/D/A lines) | WRONG |
| T15 | chronos switch | `(edit AGENTS.md) ; chronos switch feature-x` where feature-x is at HEAD | switch carries the edit (no conflict) | exit 1, "checkout 'feature-x' failed: dirty index or active branch" | WRONG |
| T11 | chronos commit | `unset EDITOR; chronos commit < /dev/null` | fail fast, "empty commit message" | exit 1 (no hang), but "calliope: unknown subcommand '.../COMMIT_EDITMSG'" (calliope used as editor) | WRONG |
| T9 | chronos add | `chronos add -A` | stage all changes, as git add -A | exit 1, "cannot stat '...\-A': os error 2" (flag read as path) | WRONG |
| T31 | chronos export | `chronos export <clone>/.git` on an existing clone | works, as the GIT_DIR wording says | exit 1, "git status --porcelain: must be run in a work tree"; the worktree path form works | ANNOY |
| T27 | chronos add | `head -c 5242880 /dev/urandom > big.bin; time chronos add .` | under 1 s, as git add | exit 0 after 19.6 s | ANNOY |
| T2 | chronos status | `time chronos status` on a clean 281-commit tree | fast, like git status (0.04 s) | exit 0 after 7.0 to 9.9 s | ANNOY |
| T1 | chronos import | `time chronos import <repo>/.git` in an empty dir | seconds for 281 commits | exit 0 after 25.9 s | ANNOY |
| T18 | chronos branch | `chronos branch -d feature-x` | delete the branch | exit 2, "branch expects zero or one NAME" | ANNOY |
Worked (19): status after import (clean), edit then diff and diff --staged, delete, rename, add untracked, commit -m, log and log pipe head, branch create, switch on clean tree, checkout of a commit id (detached), CRLF bytes, binary corpus.bin, unicode name, path with spaces, empty file, 5 MB round trip, -C DIR, status from a subfolder, export into a clone (fsck exit 0, tree bytes match), push to a local bare repo.
Setup: PATH must use the POSIX form /c/Users/ADMIN/... (the C: form in COMMON.md breaks PATH splitting). Sandbox: C:/Users/ADMIN/AppData/Local/Temp/claude/838757565bfd39b2/98d4c426-dbca-4748-8fa8-3f3069ae87c5/scratchpad/sim/work/p1-git-dev/chr (HOME set to .../p1-git-dev/home). chronos 0.1.0.
