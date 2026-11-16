fix-chronos-1 (Haiku, report saved by leader from its hand-back): 8 of 8 done, CHECK PASS chronos.
1 deletions stage and commit; 2 .gitignore root+nested, globs, **, !, [set], -f (not read: .git/info/exclude, global excludes);
3 status "Untracked files:" (? PATH, dir/ once); 4 add -A/--all; 5 restore --staged PATH..., reset (no paths);
6 rm [-f] [--cached], mv SRC DST (files only); 7 switch keeps non-conflicting local edits, names conflicting paths
(untracked overwrite not checked); 8 commit without -m and no terminal fails at once, COMMIT_EDITMSG removed.
Tests: crates/chronos/tests/fix_chronos_1.rs. Leader hand check: deletion absent from committed tree, target/ and *.log skipped.
