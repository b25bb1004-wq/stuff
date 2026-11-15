# Varve design (D3)
v1, written by the leader on 2026-11-15. Decides the page format, the log format, the SQL subset and
the isolation level. Tasks: T39 pager + log + recovery, T40 B+tree, T41 SQL, T42 server.

## 1. Storage and crash model
- A database is two byte stores behind one trait: the main file and the log (`<name>` and
  `<name>-wal`). `Storage`: `len`, `read_at(off, buf)`, `write_at(off, data)` (grows the store),
  `set_len(n)`, `sync`. Implementations: `MemStorage`, `FileStorage` (std::fs::File), `CrashStorage`.
- Crash model (what `CrashStorage` simulates and what the leader tests against): `sync` makes every
  earlier write durable. At a crash, every write since the last `sync` is split into 512-byte
  sectors and each sector independently survives or is lost (survivors applied in write order).
  `set_len` is durable at once. Nothing else is assumed: no ordering between unsynced writes.
- Rule: a transaction is committed exactly when the `sync` of its commit frame returns. After any
  crash and reopen, every committed transaction is present and nothing else is.

## 2. Page format
- Page size 4096. Page `n` is at offset `n * 4096` in the main file. All integers little-endian.
- Every page ends with a 4-byte CRC-32 (IEEE, reflected, poly 0xEDB88320) of its first 4092 bytes.
  A main-file page with a bad CRC and no newer copy in the log is `Error::Corrupt`, never a panic.
- Page 0, header: `b"VARVE\0v1"`, page size u32 (4096), page count u64 (pages in use, including
  page 0), free-list head u64 (0 = empty), catalog root u64, change counter u64, rest zero.
- Free page (kind 5): next free page u64 at byte 16. Allocation pops the free list, else grows
  the page count. Freed pages go to the head of the list.
- B+tree pages, 16-byte header: kind u8 (2 interior, 3 leaf), flags u8 (0), cell count u16,
  cell-content start u16, fragmented bytes u16, then `link` u64: the rightmost child (interior)
  or the next leaf to the right (leaf, 0 = none). After the header, a u16 offset per cell in key
  order; cell bodies grow down from byte 4092 (slotted page).
- Leaf cell: varint key length, varint value length, key, then the value inline if
  `key + value <= 1200` bytes, else the first `1200 - key` value bytes followed by a u64 overflow
  page. Overflow page (kind 4): next overflow page u64 at byte 16, data from byte 24 to 4092.
- Interior cell: child u64, varint key length, key. Child `i` holds keys `< key_i` (and
  `>= key_{i-1}`); `link` holds keys `>= ` the last key. Keys are byte strings compared with
  memcmp, at most 1024 bytes (`Error::TooLarge` otherwise).
- Varint: unsigned LEB128, at most 10 bytes; a longer or non-minimal one is `Error::Corrupt`.

## 3. Log format (redo only, whole pages)
- Log header, 32 bytes: `b"VARVEWAL"`, version u32 (1), page size u32, salt u64, 4 zero bytes,
  CRC-32 of bytes 0..28.
- Frame = 32-byte header + 4096-byte page image. Header: page number u64, commit u64 (0, or the
  page count of the database after this transaction: marks the last frame of a transaction),
  salt u64 (must equal the log header's), 4 zero bytes, CRC-32 of the header's bytes 0..28 and
  the page image.
- Commit: append one frame per dirty page (the commit frame last), then `sync` the log. Readers of
  the main file are never touched by a commit.
- Open / recovery: an invalid log header means an empty log: `set_len(0)`, write a fresh header
  (salt 1), `sync`. Otherwise scan frames from the start and stop at the first frame that is
  short, has a bad CRC or a different salt. Keep frames up to the last commit frame; the page
  map is "page -> latest kept frame". Frames after the last commit frame are ignored and get
  overwritten by the next commit.
- Checkpoint (when the log passes 1000 frames, and on clean close): write the latest kept version
  of each page to the main file, `sync` the main file, then write a new log header with
  `salt + 1` and `sync` the log. A crash at any point leaves either the old log (replayed again,
  idempotent) or an invalid header (main file already complete).
- Page images are written and read whole; a torn page shows up as a bad CRC.

## 4. Isolation and concurrency
- One writer at a time (a lock), any number of readers. **Serializable**: write transactions run
  one after another; a write transaction sees the last committed state plus its own changes.
- A read transaction takes a snapshot = the number of kept log frames when it starts, and reads
  each page from the newest frame at or below its snapshot, else from the main file. It never
  sees a later commit and never blocks the writer.
- A checkpoint never runs while a reader holds a snapshot older than the log end (it is skipped,
  the log grows). Rollback drops the transaction's dirty pages; nothing was written.

## 5. Tables and indexes (T41 on the T40 B+tree)
- The catalog is a B+tree (root in page 0): key = object name (UTF-8), value = record
  `(kind "table"|"index", table name, root page, SQL text)`.
- A table is a B+tree keyed by rowid (i64, encoded as below), value = the row record. An
  `INTEGER PRIMARY KEY` column is the rowid. New rowids are max + 1 (1 for an empty table).
- An index is a B+tree keyed by `encode(indexed values..., rowid)` with an empty value. Any other
  `PRIMARY KEY` or `UNIQUE` is a unique index.
- Row record: varint column count, then per column a tag (0 NULL, 1 INTEGER as zigzag varint,
  2 TEXT, 3 BLOB as varint length + bytes).
- Key encoding (memcmp order = SQL order, NULL < INTEGER < TEXT < BLOB): NULL `0x05`; INTEGER
  `0x10` + 8 bytes big-endian of `v XOR 1<<63`; TEXT `0x20` and BLOB `0x30` + the bytes with
  `0x00` written as `0x00 0xFF`, then `0x00 0x00`.

## 6. SQL subset (semantics of SQLite STRICT tables)
The leader compares 300+ generated queries against Python's sqlite3 (3.53) using the same
statements with `STRICT` added to every `CREATE TABLE`. Where this list is silent, SQLite wins.
- Types: INTEGER (i64), TEXT (UTF-8), BLOB, and NULL. No REAL: `AVG`, real literals and `/` on
  reals are out of v1. Storing a value of the wrong type is `Error::Type`. Integer overflow in
  arithmetic is `Error::Overflow` (SQLite would switch to REAL; the comparison avoids it).
- Statements: `CREATE TABLE t (c TYPE [PRIMARY KEY] [NOT NULL] [UNIQUE], ...)`, `DROP TABLE`,
  `CREATE [UNIQUE] INDEX i ON t (c, ...)`, `DROP INDEX`, `INSERT INTO t [(c, ...)] VALUES (...), ...`,
  `UPDATE t SET c = e, ... [WHERE e]`, `DELETE FROM t [WHERE e]`, `BEGIN`, `COMMIT`, `ROLLBACK`,
  `SELECT [DISTINCT] e [AS n], ... | * FROM t [AS a] [[INNER | LEFT] JOIN u [AS b] ON e]...
  [WHERE e] [GROUP BY e, ...] [HAVING e] [ORDER BY e [ASC | DESC], ...] [LIMIT n [OFFSET m]]`,
  `EXPLAIN SELECT ...` (the plan, below). Keywords are case-insensitive; names are ASCII.
- Expressions: integer, `'text'` (`''` escapes a quote), `x'hex'` and NULL literals; `t.c` or `c`;
  `+ - * / %` (integer; `/` and `%` truncate toward zero; by zero gives NULL); `||`;
  `= == <> != < <= > >=`; `AND OR NOT`; `IS [NOT] NULL`; `[NOT] BETWEEN`; `[NOT] IN (list)`;
  `[NOT] LIKE` (`%`, `_`, ASCII case-insensitive, no ESCAPE); `CASE [e] WHEN .. THEN .. [ELSE ..] END`;
  unary `- +`. Functions: `count(*)`, `count(e)`, `sum`, `min`, `max`, `abs`, `length`, `lower`,
  `upper`, `coalesce`, `substr(s, start [, len])`, `typeof`. No subqueries in v1.
- Three-valued logic, comparisons across types and NULL ordering (first in ASC) as SQLite;
  TEXT compares bytewise. A SELECT without ORDER BY may return rows in any order.
- Constraints: NOT NULL, UNIQUE, PRIMARY KEY. A failing statement changes nothing (a
  multi-row INSERT that breaks a constraint on row 5 leaves rows 1 to 4 out too).
- Autocommit: each statement outside `BEGIN ... COMMIT` is its own transaction.
- Plan (`EXPLAIN`), one line per table in join order: `SCAN t`, `SEARCH t USING ROWID`,
  `SEARCH t USING INDEX i (a=? AND b>?)`. The planner uses the rowid for `rowid = e` /
  `ipk = e`, an index whose leading columns have equality or one range term, else a scan;
  joins are nested loops, using an index on the inner table when one matches the ON term.

## 7. Errors and limits
`Error`: `Io`, `Corrupt`, `TooLarge`, `Parse { pos, msg }`, `NoSuchTable`, `NoSuchColumn`,
`Exists`, `Type`, `Overflow`, `Constraint(String)`, `Busy` (second writer). Rows up to 1 MiB; keys up
to 1024 bytes encoded. Every decoder (pages, frames, records, keys, SQL text) returns an error on
hostile input and never panics.
