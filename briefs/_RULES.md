# Rules for every worker (read once, then your brief)

1. Touch only the files your brief lists. Never edit the root `Cargo.toml`, `tools/`, `docs/`,
   `LOOP.md`, other crates or other briefs. No git commands.
2. Zero external crates. Standard library only. `assay` (already a dev-dependency) gives
   you `Rng`, `forall`, `hex`, `to_hex` for tests. Read `crates/assay/src/lib.rs` if needed.
3. Keep the public API exactly as the brief states it. Other crates are being written against it
   right now. Extra private helpers are fine; extra public items need a reason in your report.
4. Library code must not panic on any input: no `unwrap`, `expect`, indexing that can go out of
   range, or arithmetic that can overflow on caller data. Return the crate's `Error`. Tests may
   unwrap (put `#![allow(clippy::unwrap_used)]` at the top of test modules and test files).
5. Keep `#![forbid(unsafe_code)]` unless your brief says the crate may use `unsafe`. Where it
   may, every `unsafe` block gets a `// SAFETY:` comment saying why it is sound.
6. Every public item has a one-line doc comment. Split the crate into modules of at most about
   400 lines each.
7. Tests are the deliverable as much as the code: unit tests beside the code, published test
   vectors where they exist, and property tests with `forall`. Do not invent test vectors from
   memory if you are unsure of them; say so in the report and the leader will supply them.
8. Your gate is `bash tools/check.sh <crate>` run from the repository root. It must end with
   `CHECK PASS <crate>`. Run it, fix, repeat. Use `cargo fmt -p <crate>` to format.
9. Do not read other crates, do not load skills, do not browse the web, do not start
   sub-agents. Stay inside your token budget; if you are running out, stop adding features,
   make what exists pass the gate, and list what is missing.
10. Finish by writing `reports/<id>.md`, at most 10 lines: what was built, the last line of the
    gate output, the number of tests, anything missing or uncertain. Your final message is that
    same report and nothing else.

## Team runs
If `comms/ROSTER.md` exists and lists your ID, follow its protocol: read the other outboxes at each milestone and post to `comms/<your ID>.md`.
