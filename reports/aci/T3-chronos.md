T3 chronos: done. Files: crates/chronos/src/bin/chronos.rs, src/diff.rs (hunks API), src/lib.rs, Cargo.toml (aci dep), tests/aci.rs (new, 17 tests, all pass).
--describe lists all 21 subcommands; --json prints one envelope per command (serve answers unsupported). Text output unchanged: scenario diff vs baseline shows only commit ids and timestamps.
Gate: `bash tools/check.sh chronos` ends `CHECK PASS chronos`.
Unsupported under --json: serve (code unsupported, hint "run: chronos serve (without --json)", exit 1).
Deviations: commit without -m under --json exits 2 (usage), text path exits 1. --help/-h under --json returns a usage envelope. Unclassified Op errors become code invalid. status builds its values before printing, so partial stdout on I/O error could differ.
Untested by dedicated tests: import, export, cat-file (smoke-tested by hand only).
Log order within one second follows Repo::log (time, then id); the test asserts only the message prefix.
