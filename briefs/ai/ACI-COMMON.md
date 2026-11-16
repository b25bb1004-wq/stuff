# ACI conversion: common brief (every phase 1 conversion worker reads this)

Read in full, in this order: `docs/ai-computer/ACI.md` (the contract), then in
`docs/ai-computer/plans/2026-11-16-phase1-agent-interface.md` the sections "Global Constraints", "Review Focus",
the "Recipe" under "Tasks 3 to 11", and your own row of the table. Then `crates/aci/src/lib.rs`,
`crates/aci/src/envelope.rs` and `crates/aci/src/manifest.rs` (the helpers you must use; do not write your own
JSON code), and `crates/aci/src/bin/aci.rs` as a worked example of `--describe` and `--json`.

What you add to each tool in your group
1. `aci = { path = "../aci" }` under `[dependencies]` in the crate's `Cargo.toml`.
2. In the tool's entry file: a `MANIFEST` constant listing every subcommand shown by `--help`; `--describe`
   prints `MANIFEST.to_json().write()` and a newline; a leading `--json` switches that run to one envelope on stdout.
3. `crates/<crate>/tests/aci.rs` with the tests the Recipe names. Write them first and see them fail.

Rules
- Touch only the crates named in your task. Never build other crates: always pass the crate name to the gate.
- Gate per crate: `bash tools/check.sh <crate>` must end with `CHECK PASS <crate>`. Do not edit `tools/check.sh`.
- Without `--json` and `--describe` the tool's output, stderr and exit codes must be byte for byte what they are now.
  Do not edit, delete, skip or weaken any existing test.
- Under `--json`, stdout is exactly one JSON object and a newline, also for usage errors (code `usage`, exit 2).
  The human `<tool>: message` line on stderr stays.
- Build `data` from the same values the text path prints: share the computation, do not parse your own text output.
- Usage-error hints say `run: <tool> --describe` (agents read the manifest, not --help). A failure keeps the exit code the text path gives for the same failure.
- Field names come from ACI.md's shared list. Paths through `Json::path`. Program or file text through `capped`.
- No `unsafe`, no external crates, no panics on user input.
- Do not commit, push, stash or `git checkout` files. Do not invoke skills. Do not start other agents.
- If a subcommand cannot give a sensible JSON answer (an interactive screen, a server that never returns), return
  code `unsupported` with a hint naming the command to use instead, and say so in your report.

Report (required, at most 10 lines) to `reports/aci/<task>.md`: tools converted, commands per tool, the last gate
line per crate, every command answered with `unsupported` and why, anything not done.
