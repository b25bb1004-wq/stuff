# fix-chronos-2 report
Done: 1 merge (ff, conflict markers, status Unmerged paths, add+commit gives 2 parents, merge --abort, dirty refusal before writes); 2 stash (push, list, pop, drop; untracked left alone); 3 show [ID|BRANCH] and tag NAME [ID] / tag -d / tag list; 4 branch -d/-D via Repo::delete_branch with merged check; 5 export into an empty git repo sets HEAD and sends full history, second target gets full history, .git folder accepted.
Not done: none of the five items is missing. Pop refuses (keeps the stash) instead of 3-way merging when a changed path is dirty or HEAD moved it.
Tests: crates/chronos/tests/fix_chronos_2.rs, 13 tests, real binary, temp dirs.
Surprising: the brief's "not a valid commit" failure did not reproduce in a fresh chronos repo; the real bug is a silent "nothing to export" for a second target (fixed by dropping git-map links and refs the target lacks).
Surprising: annotated tags need the peeled sha (%(*objectname)) when checking refs, or the existing git_bridge round-trip test fails.
Surprising: git on this PC checks files out with CRLF (autocrlf), so the export tests compare LF-normalised text.
Gate last line: CHECK PASS chronos
