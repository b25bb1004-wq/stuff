# fix-aci-2 report
- G1 chronos: `restore PATH...` writes index content (else HEAD) back to the work tree, creating folders; all paths resolved before any write; `--source REV` (branch, tag, id, HEAD~n); unknown path = not_found, exit 1. `--staged` unchanged. Manifest and help updated. Tests: crates/chronos/tests/restore_worktree.rs (9).
- G2 calliope: new `newline` command (file's line ending, cursor to start of new line). Manifest summary and `--help` list the full grammar. Tests: crates/calliope/tests/edit_newline.rs (8).
- G2 finding: `insert` already expands `\n \t \r \` (existing test relies on it), so the docs say that instead of "literal". Backslash handling unchanged.
- G3 krypton printf: silent 4096 cap raised to 1 MiB; larger width/precision is an error naming the limit, exit 1. Tests: crates/krypton/tests/printf_field.rs (2).
- G4 krypton: command `file` renamed `script` (also the envelope cmd), summary as specified; `-c`/`script` returns name stdout_truncated, stdout_bytes, stdout_lossy (and stderr_*). aci.rs: 2 assertions changed (names, cmd).
- Gates: CHECK PASS chronos; CHECK PASS calliope; CHECK PASS krypton; CHECK PASS titan.
- Touched only crates/chronos, crates/calliope, crates/krypton (plus this report). cargo fmt applied per crate. No commits.
