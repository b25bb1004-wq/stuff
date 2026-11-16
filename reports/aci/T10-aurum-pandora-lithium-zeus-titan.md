# T10: ACI for aurum, pandora, lithium, zeusctl, titan
- Tools: aurum (keygen, tx, merkle, verify, demo); pandora (ps, kill, rm; run and pull are text-only); lithium (run, image); zeusctl (run); titan (boot; the interactive console is unsupported under --json).
- Every binary has `--describe` (manifest) and `--json` (envelope). Tests: crates/{aurum,pandora,lithium,zeus,neon}/tests/aci.rs.
- Gates: CHECK PASS aurum; CHECK PASS pandora; CHECK PASS lithium; CHECK PASS zeus; CHECK PASS neon; CHECK PASS titan.
- Unsupported under --json: pandora run and pull (text-only for now); titan with no --script (interactive console); on Windows, every pandora command except --describe returns `unsupported` ("pandora needs Linux") with exit 0 for ps and 125 for the rest.
- Pandora Linux-only tests (ps, rm, kill) are cfg-guarded and returned early on this Windows host, so they were not run. Only the Windows path and text mode were exercised.
- Not done: no red run was recorded before the green runs; crates/titan/src/bin/userland/sh.rs was changed (`parse(line, Escapes::Posix)`) outside the T10 list because krypton's parse signature changed and the titan gate failed at HEAD.
- Not done: aurum merkle proof has no --limit/--offset (fixed page); aurum has no serve command, so no listen envelope; zeusctl run and titan boot have no path field, so the path-with-spaces test does not apply; lithium image with a missing output directory returns `io` (it is an output path, not an input).
- Chisel, cobalt and elysium needed no `default-features = false` change: they are host-only in these crates, and the titan riscv build passes.
- Pandora `--json` is a new `aci` dependency (plain, not no-std). No crates outside the T10 list were changed except the sh.rs fix above.
