# FIX chronos-2: the rest of a git user's day (from SIM p1, reports/sim/p1-git-dev.md)
Files: crates/chronos/** only. Gate: `bash tools/check.sh chronos` ends CHECK PASS chronos. Zero external crates.
Do not weaken, skip or delete tests; add tests in crates/chronos/tests/fix_chronos_2.rs. Budget about 150k. No skills.
Report: put it in your final message (10 lines); the leader saves it.
1. T19 merge: `chronos merge BRANCH`. The library already has Repo::merge and merge3 (src/merge.rs): wire them.
   Fast-forward when possible ("Fast-forward" line). On conflicts: write git-style markers
   (<<<<<<< HEAD / ======= / >>>>>>> BRANCH) into the worktree files, list them under "Unmerged paths:" in status,
   exit 1 with "CONFLICT (content): Merge conflict in PATH" lines; `chronos add PATH` then `chronos commit -m`
   finishes the merge with two parents (store MERGE_HEAD under .chronos/). `chronos merge --abort` restores HEAD.
   A dirty worktree that the merge would touch is refused before anything is written.
2. T21 stash: `chronos stash` (save worktree+index changes as a commit object on .chronos/refs/stash with a
   stack, then reset the worktree to HEAD), `stash list`, `stash pop` (apply and drop; on conflict keep it),
   `stash drop`. Untracked files are left alone (git default).
3. T36 `chronos show [ID|BRANCH]`: the commit header plus its diff against the first parent (reuse the diff code).
   `chronos tag NAME [ID]` (lightweight tag in .chronos/refs/tags), `tag` lists, `tag -d NAME`.
4. T18 `chronos branch -d NAME` (refuse the current branch; refuse unmerged unless -D) using Repo::delete_branch.
5. T30 `git init -q exp; chronos export exp` fails ("not a valid commit ..."): exporting into an EMPTY git repo
   must export the full history and set the branch. T31: accept both the work-tree folder and its .git folder.
Update --help for every verb.
