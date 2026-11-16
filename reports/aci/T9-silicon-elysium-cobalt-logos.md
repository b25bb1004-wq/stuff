# T9: silicon, elysium, cobalt, logos (ACI)

- Converted: silicon (crates/chisel, `run`), elysium (`verify`, `dis`, `run`), cobalt (`check`, `fmt`, `ir`, `opt`, `build`), logos (`run`, `test`). The logos binary lives in crates/cobalt/src/main.rs; crates/logos is library-only, so it has no gate of its own.
- crates/chisel has no assembler binary besides silicon (silicon is the emulator; assembly is a library call only), so no second conversion.
- Gates: `CHECK PASS chisel`, `CHECK PASS elysium`, `CHECK PASS cobalt`, `CHECK PASS titan`.
- `aci` is optional behind a default-on `std` feature in chisel, elysium and cobalt; bins have `required-features = ["std"]`. helios, neon, titan, zeus and lithium depend on these, so a bare-metal build of them needs `default-features = false`.
- Deviations: elysium has no exit code or stdout, so `run` returns `result` (capped), `exit: 0`, `stop: exit`, `steps`. Under silicon `--json`, `exit` is null at the step limit. `stop: trap` is unreachable in silicon (Exit has no trap variant).
- Traps and compile errors in elysium, logos and cobalt are `ok: false`, code `invalid`, exit as the text path (1 or 2). The compile-error message carries line and column; `cobalt --json check` is named in its hint.
- Refused under `--json`: cobalt `build -o -` (code `usage`), silicon `--trace` (`usage`). logos `test` `results` is a plain list, not paged.
- Tests: crates/chisel/tests/aci.rs (9), crates/elysium/tests/aci.rs (10), crates/cobalt/tests/aci.rs (17). Each covers describe, run or compile-style success, error envelopes, unknown subcommand, and text mode unchanged.
- Not done: nothing in scope; no command returned `unsupported`.
