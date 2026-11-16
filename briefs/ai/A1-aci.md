# A1: crates/aci (the shared agent-interface crate)

Read first, in full: `docs/ai-computer/ACI.md`, then Task 1 of
`docs/ai-computer/plans/2026-11-16-phase1-agent-interface.md`. Task 1 is your whole job: the files, the exact
public interface, the named tests with their values. Do not read other crates except `crates/assay/src/lib.rs`
(for `Rng` and `forall`) and one existing `tests/cli.rs` (for the test style, for example `crates/hades/tests/cli.rs`).

Rules
- You may touch only `crates/aci/**` and the `members` list in the root `Cargo.toml` (add `"crates/aci"`).
- Standard library only. `assay = { path = "../assay" }` as a dev-dependency is allowed. No `unsafe`. No panics on any input.
- `Json` derives `Debug, Clone, PartialEq`. `Num` always writes with a `.` or an exponent (`1.0`, not `1`) so a
  round trip keeps `Num` and `Int` apart; the property test's generator must not produce NaN or infinities.
- Objects keep insertion order; a duplicate key while reading is an error.
- Tests first (they must fail to build), then the code, then the gate.
- Gate: `bash tools/check.sh aci` must end with `CHECK PASS aci`. Do not edit `tools/check.sh`.
- Do not commit. Do not invoke skills. Do not start other agents.
- Budget: 150k tokens. If the gate still fails near the budget, stop and report the exact failing output.

Report (required, at most 10 lines) to `reports/aci/A1.md`: files created, the last line of the gate, anything in
Task 1's interface you had to change and why, anything left undone.
