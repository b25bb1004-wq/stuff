# T7: mercury, xenon, argo (ACI)

- Tools converted: mercury, xenon, argo. All three have `--describe`, `--json`, and `tests/aci.rs` (13, 7 and 11 tests, all pass).
- mercury commands: `get` (net: status, headers [{name,text}], capped text; `--out FILE` gives path, bytes), `parse` (read: kind, method, path, status, version, headers, capped body), `serve` (net: envelope with `listen` once bound, then runs).
- xenon commands: `keygen` (read: secret_key, public_key), `echo-server` (net: `listen` once bound), `echo` (net: capped text), `recv` and `send` (net: path, bytes, hash, duration_ms).
- argo commands: `solve` (read: items page, `--limit`/`--offset`), `lock` (write: path, bytes, count), `install` (net: items page plus path, bytes, duration_ms), `serve` (net: listen, path, count).
- Last gate lines: `CHECK PASS mercury`, `CHECK PASS xenon`, `CHECK PASS argo` (run in that order).
- Commands answered with `unsupported`: none. Every subcommand gives a JSON answer.
- Text mode: mercury's text output was diffed against the pre-change binary (identical). xenon and argo had no pre-change binary kept, so their check is the existing text tests plus new `text_mode_is_unchanged` tests.
- Deviations: mercury's `aci` dependency is optional behind `std` because its lib is `#![no_std]`. argo `serve` prints its envelope after the packages are loaded and uploaded, at the same point as the text "listening on" line. `lock --json -o -` is a usage error (a file is required). Non-UTF-8 arguments in mercury, xenon and argo are now a usage error or lossy instead of a panic.
- Not done: nothing in these three crates. Nothing committed.
