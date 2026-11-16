# Agent A1: a developer's day (read ACI-AGENTS.md first; work folder $SB/work1)

1. Make a new version-control repository in $SB/work1/proj.
2. Using the shell tool, create proj/notes.txt with the three lines `one`, `two`, `three`.
3. Stage everything and commit with message "first". Record the commit id.
4. Check the repository status: it must be clean.
5. With the text editor's scripted edit, change line 2 of notes.txt to `two changed` and save.
6. Status must list notes.txt as an unstaged modification.
7. Show the diff: report the `-` and `+` lines of the hunk.
8. Commit with message "second". The log must show 2 commits; fetch only the older one using paging.
9. Create a branch `feature` and switch to it.
10. Add a new file feature.txt (any content), commit it.
11. Switch back to the main branch and merge `feature`: report the merge status (expect fast forward).
12. Make a conflict: on a new branch `b1` change line 1 of notes.txt to `ONE-b1` and commit; on main
    change line 1 to `ONE-main` and commit; merge b1. Report the error code and the conflicted path.
13. Resolve it: rewrite line 1 with the editor, stage, commit. Status must show nothing unmerged.
14. Tag the current commit `v1` and list tags.
15. Make an uncommitted change, stash it, list the stash, then pop it back. The change must be back.
16. Delete notes.txt from disk, then restore it from the repository. Status must be clean.
17. Show the commit tagged v1: report its message and the files it changed.
18. Search all .txt files in proj for the regex `two|feature` with the editor's search: report each
    path, line and column.
19. Search for a regex that matches nothing: report total and exit code.
20. Copy samples/good.lg and samples/bad.lg into work1. Check good.lg with the compiler: it must be clean.
21. Check bad.lg with the compiler: report the diagnostic's line and message.
22. Run good.lg with the language runner: report its result.
23. Build good.lg to a bytecode file with the compiler: report the output path and bytes.
24. Run that bytecode file with the bytecode VM: the result must match task 22.
25. Run the tests in samples/tests.lg: report passed and failed counts.
26. Check samples/bad.lg with the IDE's checker: report the diagnostic's line.
27. Format a copy of samples/messy.lg with the IDE: report whether it changed, and show the new text.
28. With the shell tool, run a command that prints 20000 characters: report whether the output was
    truncated and its full size in bytes.
29. With the shell tool, run a command that exits with status 3: report the exit value and `ok`.
30. Give the shell tool a script with a syntax error: report the error code.
