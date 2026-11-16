p4-server-data: 49 tasks, 33 worked, 16 broke

| id | tool | exact command | expected | actual (exit code, first line) | severity |
|---|---|---|---|---|---|
| 36 | iridium | `iridium sql db/c.idb "CREATE TABLE c (id INTEGER PRIMARY KEY, who INTEGER)"; (iridium sql db/c.idb < a.sql) & (iridium sql db/c.idb < b.sql) & wait` (a.sql: 2000 disjoint INSERTs) | both succeed, or one refused with a lock error; DB stays readable | rc=1 "corrupt page or log" (A), "storage I/O error" (B); every later open, even `SELECT COUNT(*)`, fails with "corrupt page or log" | DATA-LOSS |
| 21 | iridium | `iridium sql t.idb "CREATE TABLE t (x REAL)"` | table created (REAL is standard SQL) | rc=1 "parse error at byte 73: unsupported column type REAL"; float literals also refused | BLOCKS |
| 25 | iridium | `iridium sql t.idb "SELECT AVG(score) FROM users"` | average value | rc=1 "parse error at byte 17: unknown function AVG" (COUNT/SUM/MIN/MAX work) | BLOCKS |
| 10 | mercury | `curl -sI -H 'If-Modified-Since: Sun, 01 Jan 2040 00:00:00 GMT' http://127.0.0.1:18401/index.html` | 304 (python http.server gives 304) | 200 with full headers; no Last-Modified header at all | WRONG |
| 11 | mercury | `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18401/index.html/` | 404 (python gives 404) | 200, serves the file | WRONG |
| 48 | hades | `hades mkfs img.hd --blocks 64` | empty 64-block image | rc=1 "hades: mkfs: no space left on device" (--blocks 256 and default 1024 work) | WRONG |
| 3 | mercury | `curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:18401/café ünï 日本.txt"` (raw UTF-8 in URL) | 200 (file exists) | 404; the percent-encoded form returns 200 | ANNOY |
| 5 | mercury | `curl -si http://127.0.0.1:18401/missing.html` | 404 with a readable body | 404, Content-Length: 0, empty body | ANNOY |
| 6 | mercury | `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18401/nodir/` | listing or documented 404 (python lists) | 404 with no body | ANNOY |
| 29 | iridium | `iridium sql t.idb "SELEC * FORM users"` | error naming the bad keyword and its position | rc=1 "parse error at byte 0: expected SQL statement" (DB unharmed) | ANNOY |
| 31 | iridium | `iridium sql t.idb "INSERT INTO t VALUES (1,'a'); SELECT 1"` | both statements run (as sqlite3 does) | rc=1 "unexpected trailing tokens"; the same text on stdin works | ANNOY |
| 37 | iridium | `iridium sql t.idb "PRAGMA integrity_check"` | a result or a clear "not supported" | rc=1 "parse error at byte 0: expected SQL statement"; no integrity check command exists | ANNOY |
| 43 | install.sh | `HOME=$W/home bash C:/Users/ADMIN/strata/tools/install.sh; cat $HOME/.monolith/MANIFEST` | one useful summary per tool | midas, kairos, argo, chronos, calliope show only "Usage:" | ANNOY |
| 46 | midas, kairos | `midas sim trace.txt` / `kairos sim workload.txt` | the help example runs | rc=1 "expected '<op> <arg>'" / "unknown directive 'work'"; the file format is not in --help | ANNOY |
| 47 | titan | `titan boot --script commands.txt` | boots the script | rc=1 "cannot read kernel ... target/titan-kernel/..."; needs a cargo build first (the message says how) | ANNOY |
| 50 | hades | `hades ls img.hd /` (Git Bash) | lists the root | rc=1 "C:/Program Files/Git/: invalid path": Git Bash rewrites "/"; `MSYS_NO_PATHCONV=1` fixes it | ANNOY |

Worked: mercury static types (html/css/js/svg/png/wasm/json), 50 MB file byte-exact with Content-Length, HEAD, Range (206, suffix, 416), path escapes (8 variants, all 404), 100 parallel requests, slow half-request client (other clients served), garbage request (400), keepalive, port-in-use and bad-address errors (rc=1, first server unharmed), mercury get/parse; iridium CRUD, WHERE/ORDER/LIMIT, NULL, joins, GROUP BY, quoting and unicode, duplicate-key and missing-table errors (DB intact), stdin, persistence across reopen, 10 000 inserts in 3.7 s, kill mid-insert (9070 rows, consistent, writable after); installer first run, update in place, --add-to-path twice (no duplicate line), --uninstall twice, bad flag (rc=2), strata git status unchanged; hades mkdir/put/ls/cat/rm/fsck, full-image put (clean error, fsck ok); all 27 tools --help rc=0, examples run for sphinx, odyssey, aurum, lithium, pandora, zeusctl, xenon, bench.

Setup notes: installer run offline (CARGO_NET_OFFLINE=true) with CARGO_TARGET_DIR in the sandbox, so no build output landed in strata; it does not pass --offline itself. Mercury server was PID 50020 (verified via netstat), stopped by PID. Python compare server PID 54064 stopped. Iridium PID 17960 was killed on purpose in test 35.
