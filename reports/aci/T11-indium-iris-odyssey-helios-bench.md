# T11: --describe and --json for indium, iris, odyssey, helios, bench
- Tools converted: indium, iris, odyssey, helios, bench. Each has a manifest (`--describe`), a `--json` envelope, and tests in crates/<name>/tests/aci.rs.
- Commands: indium `render`; iris `replay`; odyssey `tokens`, `tree`, `select`, `render`; helios `run`, `record`, `replay`; bench `bench` (the bare invocation, named to match --help).
- Gates (last line): CHECK PASS indium; CHECK PASS iris; CHECK PASS odyssey; CHECK PASS helios; CHECK PASS bench; CHECK PASS neon. Also `cargo build -p helios --no-default-features --lib` passes (no_std). Odyssey aci tests: 12 passed; helios aci tests: 13 passed.
- Unsupported, and why: bench takes no file paths, so the path, not_found and `--out -` tests do not apply. helios `record -o -` is refused under --json (usage, hint `use -o FILE.trec`), because TREC bytes need a file.
- Deviation to decide: odyssey `tree` caps each node's text at 400 bytes (leader instruction), while ACI.md TEXT_CAP is 16384. The 400-byte cap is what the test checks.
- Limit: helios `replay` has no match or divergence field, because TREC stores inputs only with no expected hashes. It reports the final hash only.
- Test ordering: indium, iris, odyssey, helios and bench tests were written after their implementation, not before. Each test file checks the shapes and codes the brief lists.
- bench: a full run takes about 24 s, so its JSON test uses `--only mercury` (the fastest suite, about 0.6 s). Suites run with the same budgets as before; `--full` is parsed but not run in tests.
- Not done: nothing in the brief is left open beyond the deviations above. Source of truth for behaviour is the code and tests in each crate.
