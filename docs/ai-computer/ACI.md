# ACI: the agent interface every Monolith tool speaks (v1, 2026-11-16)

Operator, 2026-11-16: "REMEMBER ALL TOOLS WE MADE PERSONALISED FOR AI" and "They can take actions without
needing an mcp". So the interface is plain commands plus JSON. All 27 tools follow this file exactly.
The leader changes it; workers do not. It replaces the line "`--json` is not required" in docs/APPS.md.

## Two additions to every tool
1. `<tool> --describe` prints one JSON object, the manifest, and exits 0.
2. `<tool> --json CMD ARGS...` runs the command and prints exactly one JSON object (the envelope) plus a
   newline on stdout. `--json` must be the first argument. Nothing else is written to stdout.
   stderr and exit codes stay as they are today (0 ok, 1 operation failed, 2 usage error).
Without `--json` the tool behaves exactly as before. No existing test may change.

## Manifest
```json
{"aci":1,"tool":"hades","version":"0.1.0","summary":"Monolith filesystem image tool",
 "commands":[{"name":"ls","summary":"List directory entries at PATH","effect":"read",
   "args":[{"name":"IMG","kind":"path","required":true},
           {"name":"PATH","kind":"text","required":true},
           {"name":"--limit","kind":"int","required":false,"default":"100"}],
   "returns":"items: [{name, kind, size}], total, more, next_offset",
   "example":"hades --json ls disk.img /notes"}]}
```
- `kind`: `path`, `text`, `int`, `flag`, `choice` (a `choice` also has `"choices":["a","b"]`).
- `effect`: `read` (changes nothing), `write` (changes files or data), `net` (uses the network),
  `exec` (starts other programs). Pick the strongest that applies: exec > net > write > read.
- Every subcommand in `--help` appears in the manifest. `example` is a command that really runs.

## Envelope
Success: `{"ok":true,"tool":"hades","cmd":"ls","data":{...}}`
Failure: `{"ok":false,"tool":"hades","cmd":"ls","error":{"code":"not_found","message":"no such path: /x","hint":"run: hades --json ls disk.img /"}}`
- `code` is one of: `usage`, `not_found`, `exists`, `invalid`, `locked`, `denied`, `io`, `unsupported`,
  `conflict`, `challenge`, `internal`.
- `hint` says what to run or change next. Leave it out only when there is nothing useful to say.
- A usage error under `--json` (unknown subcommand, missing argument) is still an envelope, code `usage`, exit 2.
  When the command name is unknown, `cmd` is the empty string.

## Data rules
- Field names are snake_case nouns. Counts and sizes are JSON numbers, never strings. Booleans are booleans.
- Paths use forward slashes, also on Windows. Times are whole seconds since 1970 (UTC) in a field ending `_time`
  or named `time`. Durations are `duration_ms`.
- Lists are paged: `{"items":[...],"total":N,"more":true,"next_offset":100}`. Flags `--limit N` (default 100,
  maximum 1000) and `--offset N` (default 0). `next_offset` is present only when `more` is true.
- Text values are capped at 16384 bytes. A capped value sits beside `"truncated":true` and `"bytes":N`
  (the full size); the hint names the flag that reads the rest.
- Bytes that are not valid UTF-8 are replaced with U+FFFD and the object carries `"lossy":true`.
- Commands that produce binary output need `--out FILE` under `--json` and return `{"path":..., "bytes":N}`.
  Without `--out` they fail with code `usage`.
- Output is always valid JSON: control characters are escaped, no NaN or Infinity, at most 8 MiB.

## Shared field names (use these, do not invent synonyms)
`path`, `paths`, `id`, `name`, `kind`, `size`, `bytes`, `line`, `col`, `text`, `message`, `severity`, `status`,
`exit`, `stdout`, `stderr`, `branch`, `change` (`added`, `modified`, `deleted`, `renamed`), `count`, `items`.

## Code
`crates/aci` holds the JSON value, reader and writer, the manifest types, the envelope helpers and the two
validators. Tools depend on it and on nothing new. `aci check-manifest FILE|-` and `aci check-envelope FILE|-`
exit 0 or print the first problem and exit 1; `tools/aci-check.sh` runs the first over every installed tool.
