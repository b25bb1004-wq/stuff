# Persona p1-git-dev: a developer who is replacing git with chronos
Read briefs/sim/COMMON.md first. Persona name for paths: p1-git-dev.
Seed: `git clone --no-hardlinks C:/Users/ADMIN/strata $SIM/work/p1-git-dev/repo` (local clone, no network),
then `chronos import <that repo's .git>` as `chronos --help` describes, inside a fresh folder.
A day's work to try (compare each with git on the same repo):
- status right after import (clean?), and its time vs `git status` (use `time`).
- edit a tracked file, create an untracked file, delete a file, rename a file: status, diff, diff --staged.
- add, commit -m, commit with no -m and EDITOR unset in a non-terminal (must fail fast, not hang), log, log | head.
- branch, switch, a fast-forward merge, a real conflicting merge (what does the worktree look like? can you finish it?), stash and stash pop if offered.
- CRLF text file, a binary file (any .png from the repo), a unicode file name (e.g. résumé.md), a path with spaces, an empty file, a 5 MB file.
- -C DIR from another folder; running chronos in a subfolder of the repo.
- export back to a NEW git repo, then `git -C that fsck`, `git log --oneline | wc -l` and the HEAD tree id against the original (`git rev-parse HEAD^{tree}`). Then push that git repo to a local bare repo (`git init --bare`).
- anything a git user does daily that chronos lacks (say which verb is missing).
