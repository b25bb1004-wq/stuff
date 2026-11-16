# Agent Interface (ACI) Implementation Plan

> **For agentic workers:** this plan runs under the quality loop in LOOP.md: one Haiku worker per task, the
> leader reruns the gate and commits. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** every Monolith tool describes itself with `--describe` and answers `--json` with one JSON envelope.

**Architecture:** a new std-only crate `crates/aci` holds the JSON value, the manifest types, the envelope
helpers and two validators. Each tool's `src/bin/<id>.rs` gains a manifest constant and a JSON branch beside
its existing text output; the text path is untouched.

**Tech Stack:** Rust 1.98, standard library only, `assay` for property tests.

**Spec:** `docs/ai-computer/ACI.md` (the contract) and `docs/ai-computer/PLAN.md` (phase 1).

## Global Constraints
- Zero external crates. `tools/check.sh <crate>` must end `CHECK PASS <crate>`.
- No `unsafe`. Untrusted input never panics: the JSON reader returns `Result`.
- Without `--json` and `--describe` every tool behaves exactly as before; no existing test is edited, deleted or skipped.
- `--json` must be the first argument. Stdout under `--json` is exactly one JSON object and a newline.
- Exit codes stay 0 ok, 1 operation failed, 2 usage error.
- Field names, error codes, paging and the 16384-byte text cap come from ACI.md verbatim.
- Workers do not commit, do not touch other crates, and write a report of at most 10 lines to `reports/aci/<task>.md`.

## Review Focus
1. Windows paths with backslashes and spaces: JSON stays valid and paths come out with forward slashes.
2. Program output that is not UTF-8 (krypton running a binary): replaced with U+FFFD, `"lossy":true`, still valid JSON.
3. A 10 MB output: capped at 16384 bytes with `"truncated":true` and the full `bytes` count.
4. A usage error under `--json` (unknown subcommand): an envelope with code `usage` and exit 2, not help text.
5. JSON nested 100000 levels deep fed to the reader: an error, not a stack overflow.

---

### Task 1: `crates/aci`

**Files:**
- Create: `crates/aci/Cargo.toml`, `crates/aci/src/lib.rs`, `crates/aci/src/json.rs`, `crates/aci/src/manifest.rs`,
  `crates/aci/src/envelope.rs`, `crates/aci/src/bin/aci.rs`, `crates/aci/tests/cli.rs`
- Modify: `Cargo.toml` (add `"crates/aci"` to `members`)

**Interfaces (Produces):**
```rust
// json.rs
pub enum Json { Null, Bool(bool), Int(i64), Num(f64), Str(String), Arr(Vec<Json>), Obj(Vec<(String, Json)>) }
pub struct JsonError { pub offset: usize, pub message: String }
impl Json {
    pub fn parse(text: &str) -> Result<Json, JsonError>;   // depth limit 64, input limit 8 MiB
    pub fn write(&self) -> String;                          // compact, keys in insertion order
    pub fn get(&self, key: &str) -> Option<&Json>;
    pub fn as_str(&self) -> Option<&str>;
    pub fn as_i64(&self) -> Option<i64>;
    pub fn as_bool(&self) -> Option<bool>;
    pub fn as_array(&self) -> Option<&[Json]>;
    pub fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json;
    pub fn text(bytes: &[u8]) -> (Json, bool);              // lossy UTF-8; true when bytes were replaced
    pub fn path(p: &std::path::Path) -> Json;               // forward slashes
}
// manifest.rs
pub enum Kind { Path, Text, Int, Flag, Choice }
pub enum Effect { Read, Write, Net, Exec }
pub struct Arg { pub name: &'static str, pub kind: Kind, pub required: bool,
                 pub default: Option<&'static str>, pub choices: &'static [&'static str] }
pub struct Command { pub name: &'static str, pub summary: &'static str, pub effect: Effect,
                     pub args: &'static [Arg], pub returns: &'static str, pub example: &'static str }
pub struct Manifest { pub tool: &'static str, pub version: &'static str, pub summary: &'static str,
                      pub commands: &'static [Command] }
impl Manifest { pub fn to_json(&self) -> Json; }
pub fn validate_manifest(m: &Json) -> Result<(), String>;
// envelope.rs
pub enum Code { Usage, NotFound, Exists, Invalid, Locked, Denied, Io, Unsupported, Conflict, Challenge, Internal }
pub const TEXT_CAP: usize = 16384;
pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 1000;
pub fn ok(tool: &str, cmd: &str, data: Json) -> String;                              // ends with '\n'
pub fn err(tool: &str, cmd: &str, code: Code, message: &str, hint: Option<&str>) -> String;
pub fn page(items: Vec<Json>, total: usize, offset: usize, limit: usize) -> Json;   // items already sliced
pub fn capped(bytes: &[u8]) -> Vec<(String, Json)>;  // text (+ truncated, bytes, lossy when they apply)
pub fn take_json_flag(args: &mut Vec<std::ffi::OsString>) -> bool;                   // removes a leading --json
pub fn take_describe_flag(args: &[std::ffi::OsString]) -> bool;                      // true when args == ["--describe"]
pub fn validate_envelope(e: &Json) -> Result<(), String>;
```
`aci` binary: `aci check-manifest FILE|-`, `aci check-envelope FILE|-`, `aci --describe`, `aci --help`, `aci --version`
(`aci 0.1.0`). Exit 0 valid; exit 1 and `aci: <first problem>` on stderr otherwise; exit 2 usage.

- [ ] **Step 1: failing tests** in `json.rs`, `manifest.rs`, `envelope.rs` and `tests/cli.rs`:
  - `parse_write_round_trip`: an `assay::forall` property over generated values, `Json::parse(&v.write()) == v`.
  - `write_escapes_control_and_quotes`: `Json::Str("a\"b\\c\n\u{1}".into()).write() == r#""a\"b\\c\n\u0001""#`.
  - `parse_rejects_deep_nesting`: 100000 `[` characters give `Err`, no overflow (Review Focus 5).
  - `parse_rejects_trailing_garbage`, `parse_reads_unicode_escapes_and_surrogate_pairs` (`"\ud83d\ude00"` is one char).
  - `write_never_emits_nan`: `Json::Num(f64::NAN).write() == "null"`.
  - `path_uses_forward_slashes`: `Json::path(Path::new(r"C:\a b\c.txt")).write() == r#""C:/a b/c.txt""#` (Review Focus 1).
  - `text_marks_lossy`: `Json::text(&[0x61, 0xff])` is `("a\u{fffd}", true)` (Review Focus 2).
  - `capped_truncates_at_text_cap`: 20000 bytes of `x` give a 16384-byte `text`, `truncated: true`, `bytes: 20000` (Review Focus 3).
  - `page_sets_more_and_next_offset`: total 250, offset 100, limit 100 gives `more: true`, `next_offset: 200`;
    offset 200 gives `more: false` and no `next_offset`.
  - `validate_manifest_rejects`: missing `commands`, an unknown `effect`, a `choice` arg without `choices`, `aci` not 1.
  - `validate_envelope_rejects`: `ok: true` without `data`, `ok: false` with a code outside the list, a non-object.
  - `take_json_flag_only_leading`: `["--json","ls"]` gives true and `["ls"]`; `["ls","--json"]` gives false, unchanged.
  - CLI: the ACI.md manifest example passes `check-manifest`; both envelope examples pass `check-envelope`;
    `aci --json bogus` prints an envelope with code `usage` and exits 2 (Review Focus 4);
    `aci --describe | aci check-manifest -` exits 0.
- [ ] **Step 2:** `bash tools/check.sh aci` fails to build.
- [ ] **Step 3:** implement the interfaces above. Recursive-descent reader with an explicit depth counter;
  integers that fit `i64` become `Int`, everything else numeric becomes `Num`.
- [ ] **Step 4:** `bash tools/check.sh aci` ends `CHECK PASS aci`.
- [ ] **Step 5 (leader):** rerun the gate, feed the reader 1 MiB of random bytes by hand, commit.

### Task 2: `tools/aci-check.sh` and docs/APPS.md (leader, inline)

- [ ] Write `tools/aci-check.sh [BIN_DIR]` (default `target/release`): for every tool named in the installer's
  list, run `<tool> --describe | aci check-manifest -`; print `ACI PASS n/n` or the failing tools and exit 1.
- [ ] In docs/APPS.md replace "`--json` is not required." with "`--describe` and `--json` follow docs/ai-computer/ACI.md."

### Tasks 3 to 11: one worker per group, same recipe

**Recipe (every tool in the group):**
- Modify: `crates/<crate>/Cargo.toml` (add `aci = { path = "../aci" }`), `crates/<crate>/src/bin/<id>.rs`
- Create: `crates/<crate>/tests/aci.rs`
- Consumes: Task 1's `Manifest`, `Command`, `Arg`, `Kind`, `Effect`, `Json`, `ok`, `err`, `page`, `capped`,
  `take_json_flag`, `take_describe_flag`, `Code`.
- [ ] **Step 1: failing tests** in `tests/aci.rs`, run through `env!("CARGO_BIN_EXE_<id>")`:
  - `describe_is_a_valid_manifest`: stdout parses and `aci::validate_manifest` accepts it; the set of command names
    equals the subcommands in `--help`.
  - one test per subcommand: `--json` on a good input in a temp folder; `aci::validate_envelope` accepts it,
    `ok` is true, and each field named in that command's `returns` is present with the right JSON type.
  - `json_bad_input_is_an_error_envelope`: one bad input per subcommand; `ok` false, the expected `code`, exit 1.
  - `json_unknown_subcommand_is_usage`: `<id> --json nosuch` gives code `usage`, `cmd` `""`, exit 2.
  - `json_path_with_spaces_and_backslashes` for any command that returns a path (Review Focus 1).
  - `text_mode_unchanged`: one existing text command still prints its old first line.
- [ ] **Step 2:** `bash tools/check.sh <crate>` fails on the new tests only.
- [ ] **Step 3:** add `const MANIFEST: Manifest`, the `--describe` branch, and a JSON branch per subcommand that
  builds `data` from the same values the text path prints. Data fields: the nouns the text output shows, named
  from ACI.md's shared list; lists paged with `--limit` and `--offset`; binary output behind `--out FILE`.
- [ ] **Step 4:** `bash tools/check.sh <crate>` ends `CHECK PASS <crate>` for every crate in the group.

| Task | Tools | Shapes the plan pins (others follow the recipe) |
|---|---|---|
| 3 | chronos | `status`: `branch`, `head` (id or null), `clean`, `staged` and `unstaged` as `[{path, change}]`, `untracked` `[path]`, `unmerged` `[path]`. `log`: page of `{id, parents, author, time, message}`. `diff`: `files: [{path, change, hunks: [{old_start, old_lines, new_start, new_lines, lines}]}]`, `--limit` counts files. `commit`: `id`, `branch`, `count`. `add`, `rm`, `mv`, `restore`, `reset`: `paths`. `branch` list: page of `{name, id, current}`. `switch`: `branch`, `id`. `merge`: `status` (`fast_forward`, `merged`, `conflict`, `up_to_date`), `id`, `unmerged`. Conflict is `ok: false`, code `conflict`. |
| 4 | krypton | `krypton --json -c SCRIPT` and `krypton --json FILE`: `exit`, `stdout`, `stderr` (each capped, with `truncated`, `bytes`, `lossy` when they apply), `duration_ms`. A script that exits non-zero is still `ok: true`; `ok: false` is for a script krypton could not run (syntax error: code `invalid`, with `line`). Tests for Review Focus 2 and 3. |
| 5 | iridium, iridium-cli | query: `statements: [{kind, columns: [{name, type}], rows: [[...]], total, more}]` or `{kind, count}` for writes. NULL is JSON null, INTEGER a number, TEXT a string, REAL a number. Locked database: code `locked`. Parse error: code `invalid` with `line`, `col`. |
| 6 | calliope, athena | calliope `grep`: page of `{path, line, col, text}`; `edit`: `path`, `changed`, `lines_before`, `lines_after`. athena `check`: `diagnostics: [{path, line, col, severity, message}]`, `count`; `test`: `passed`, `failed`, `results: [{name, status, message}]`; `format`: `path`, `changed`. `calliope` with no subcommand under `--json`: code `unsupported`, hint names `edit` and `grep`. |
| 7 | mercury, xenon, argo | mercury `get`: `status`, `headers: [{name, text}]`, body via `capped` or `--out`; `parse`: `kind`, `method`, `path`, `status`, `headers`. `serve` under `--json`: one envelope with `listen` once bound, then runs. |
| 8 | hades, midas, kairos, sphinx | hades `ls`: page of `{name, kind, size}`; `fsck`: `clean`, `problems`. sphinx digests and keys as lowercase hex in `text`-named fields (`hash`, `public_key`, `signature`). |
| 9 | silicon (in chisel), chisel, elysium, cobalt, logos | run-style commands: `exit`, `stdout` capped, `steps` where known; compile-style: `path`, `bytes`, `diagnostics`. |
| 10 | aurum, pandora, lithium, zeusctl, titan | boot-style commands: `exit`, `stdout` capped, `duration_ms`. pandora on Windows: code `unsupported`, hint "pandora needs Linux". |
| 11 | indium, iris, odyssey, helios, bench | image-producing commands need `--out FILE` and return `path`, `bytes`, `width`, `height`. odyssey `tree` and `select`: page of `{path, name, text}`. |

Order: Task 1 first and alone. Tasks 3 and 5 wait for the phase 0 chronos and iridium workers to land
(same crates). Then three per wave: 4, 6, 8; then 3, 5, 7; then 9, 10, 11.

### Task 12: simulated agents (phase gate)

- [ ] Install into a fresh sandbox HOME. Two Haiku workers each get only the 27 `--describe` outputs and 30 tasks
  (one a developer flow across chronos, calliope, krypton, athena; one a data flow across iridium, mercury,
  hades, sphinx, argo). They may use `--json` commands only and may not read `--help` or source.
- [ ] Report format as in `briefs/sim/COMMON.md`, plus a column "what the manifest failed to tell me".
- [ ] Pass: 54 of 60 tasks or better, and `bash tools/aci-check.sh` prints `ACI PASS 27/27`.
  Each break is rerun by the leader, then fixed in one batched worker.
