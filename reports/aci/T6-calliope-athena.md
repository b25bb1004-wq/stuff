# T6 calliope, athena: report
- Converted: calliope (`grep`, `edit`, `FILE`), athena (`check`, `fmt`, `test`, `serve`). `aci` dependency added to both Cargo.toml files (std only, no feature gate).
- calliope gate: `CHECK PASS calliope` (tests in crates/calliope/tests/aci.rs). `calliope --json` with no subcommand, or with a FILE, gives `unsupported` (cmd `FILE`) at once.
- athena gate: `CHECK PASS athena` on the first run; after the hades worker added an unconditional `aci` dependency (crates/hades/Cargo.toml line 9), the athena gate now FAILS in `pack_produces_valid_wasm_and_conforming_web_bundle` because the riscv kernel build (titan -> hades -> aci) cannot compile std. Every athena test except that one passes, including crates/athena/tests/aci.rs (9 tests). Fix belongs in hades: make `aci` optional behind a `std` feature like krypton.
- Not done: `athena --json serve` was not run for real (no safe start/stop in tests); its code path only has the missing-folder test. `fmt`/`check`/`test` take several FILE args under --json only; text path still takes one.
- `serve.rs`: `Served` gained a `listen` field (only construction site changed).
