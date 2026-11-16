# T5-iridium: iridium, iridium-cli (ACI)
- Converted: `iridium` (sql, connect) and `iridium-cli` (HOST:PORT, one command named `connect`). `--describe` on both; `--json` first arg.
- `iridium --json sql DB [QUERY] [--limit N] [--offset N]` (no QUERY or `-`: stdin); `iridium --json connect ADDR [--limit N] [--offset N]`; `iridium-cli --json HOST:PORT` (default paging).
- Engine: new `Db::describe` and `statement_kind` in sql (column types for SELECT); `Rows` unchanged. Shared JSON code in new `iridium::report`; text printers unchanged.
- Gate: `CHECK PASS iridium` (19 tests in tests/aci.rs, all other suites green). chronos/krypton not touched; nothing committed.
- Unsupported: none. Path-with-spaces test not applicable (no command returns a path).
- Gaps: over the network (connect) column types are "" and error codes are inferred from message text (wire carries no codes).
- Choices: EXPLAIN returns row shape with kind "explain"; Busy maps to `locked`; `--limit`/`--offset` accepted only under `--json` (text usage errors unchanged).
- Process note: tests were written before the code but not run in a failing state first.
