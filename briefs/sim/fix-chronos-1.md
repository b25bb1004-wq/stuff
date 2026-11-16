# FIX chronos-1: staging is correct (from SIM p1, reports/sim/p1-git-dev.md)
Files you may touch: crates/chronos/** only. Gate: `bash tools/check.sh chronos` must end `CHECK PASS chronos`.
Zero external crates. Do not weaken, skip or delete existing tests. Add one test per item in crates/chronos/tests/.
Report: reports/sim/fix-chronos-1.md, 10 lines max (item, done/not, test name).
Budget about 150k tokens. Do not read the briefs other than this one; do not invoke skills.

Each item: symptom (repro) -> required behaviour (git's).
1. T10/T8: deleted file cannot be committed. `rm f; chronos add .` shows `D f` in status, but `chronos commit -m x`
   says "nothing to commit"; `chronos add f` for a deleted tracked f fails "cannot stat". Required: add of a
   deleted tracked path (or of a folder containing one, or `.`) stages the removal; commit records it.
2. T34: `chronos add .` stages files ignored by .gitignore (target/leak.txt). Required: honour the repo-root
   .gitignore and nested .gitignore files for add of folders and for status untracked listing: blank lines,
   `#` comments, `name`, `dir/`, `/anchored`, `*` and `?` globs, `**/`, `!negation`. Always skip .chronos/ and .git/.
   Explicitly named ignored file: refuse like git ("paths are ignored ... use -f") unless `-f`.
3. T4: status does not list untracked files. Required: after the staged/unstaged sections, "Untracked files:"
   with each untracked, non-ignored path (a wholly untracked folder shown once as `dir/`, like git).
4. T9: `chronos add -A` / `--all` stages every change in the worktree (new, modified, deleted) honouring .gitignore.
5. T35: no way to unstage. Add `chronos restore --staged PATH...` (index entry back to HEAD's, or removed if not
   in HEAD) and `chronos reset` (no args: whole index back to HEAD). Worktree untouched by both.
6. T36: add `chronos rm PATH...` (remove from worktree and index; `--cached` keeps the file) and
   `chronos mv SRC DST` (rename in worktree and index).
7. T15: `chronos switch B` refuses whenever the tree is dirty. Required (git): allowed when no dirty path differs
   between the current HEAD and B; refused, naming the paths, only when a dirty path would be overwritten.
8. T11: with no EDITOR/VISUAL and no terminal, `chronos commit` (no -m) must fail at once with
   "no commit message: use -m MSG or set EDITOR" and exit 1; do not spawn calliope unless stdin is a terminal.
   Remove .chronos/COMMIT_EDITMSG on any abort.
Update `chronos --help` for every new verb and flag. Keep output going through the locked stdout helper.
