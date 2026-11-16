# T8 hades, midas, kairos, sphinx: ACI conversion
- Converted: hades, midas, kairos, sphinx (entry files, Cargo.toml, tests/aci.rs). Sphinx constant-time code untouched.
- hades: mkfs, ls (paged, --limit/--offset), cat (capped text, --out FILE), put, rm, mkdir, fsck. midas: sim (paged step records). kairos: sim (paged CPU timelines plus task rows). sphinx: hash, hmac (field `mac`), keygen, sign, verify, seal, open.
- Gates: `CHECK PASS hades`, `CHECK PASS midas`, `CHECK PASS kairos`, `CHECK PASS sphinx` (run in that order; hades rerun after the optional-aci fix).
- Not answered with JSON: `keygen -o -` and binary output to stdout (seal, open) under --json return `unsupported` or usage with a hint to use -o FILE; `seal` without -o is usage under --json.
- Choices: fsck with problems and verify with a wrong signature are ok:true with exit 1 (text exit unchanged). `--help`/`--version` are not manifest commands. No Medusa CLI exists, so no `experimental` flag.
- Cargo: hades, midas, sphinx got a default `std` feature with optional `aci` and `[[bin]] required-features = ["std"]`; kairos got the same (its `std` feature now enables `dep:aci`).
- Dependents: titan now uses `default-features = false` for hades and midas (kairos already did). `cargo tree -p titan` shows no aci. Titan `cargo check` passes. Athena untouched.
- Not done: nothing in scope. Large sim outputs can exceed the 8 MiB envelope limit (not guarded).
