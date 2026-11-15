# iridium

iridium is a transactional storage engine in plain Rust. It stores data in 4 KiB pages
behind a B+tree, protects every commit with a redo write-ahead log, and runs a small
SQL engine on top. The crate uses only the Rust standard library.

## Features

- Fixed 4 KiB pages, each with a trailing CRC-32 checksum.
- Redo write-ahead log with crash recovery when the database is opened again.
- Transactions in the pager: begin, write, commit, rollback.
- Read snapshots, so a reader keeps a stable view while a writer commits.
- B+tree with get, insert, remove, scan, range and destroy.
- In-memory and file-backed storage, plus a crash-injection storage for tests.
- SQL: CREATE and DROP for tables and indexes, INSERT, UPDATE, DELETE, SELECT.
- SELECT supports JOIN, WHERE, GROUP BY, HAVING, ORDER BY, LIMIT and OFFSET.
- EXPLAIN, and BEGIN, COMMIT and ROLLBACK for explicit transactions.
- PRIMARY KEY, UNIQUE and NOT NULL constraints.
- Built-in functions: length, lower, upper, count.
- TCP server and client that exchange rows in a binary codec.

## Public API

| Item | Purpose |
| --- | --- |
| `Db::open(main, log)` | Open or create a SQL database over two stores |
| `Db::execute(sql)` | Run one SQL statement and return `Rows` |
| `Db::in_transaction()` | Report whether an explicit BEGIN is open |
| `Db::close()` | Roll back any open transaction and return the two stores |
| `Pager` | Page-level transactions, reads and writes, snapshots |
| `Tree` | B+tree over pager pages |
| `MemStorage`, `FileStorage` | Byte stores used for the page file and the log |
| `CrashStorage` | Store that simulates lost writes, for recovery tests |
| `Server::start`, `Client::connect` | TCP server and client |
| `Value`, `Rows`, `Error` | SQL values, result sets and error type |

## Demo

The demo runs a short script on an in-memory database. It uses no files, no network
and no clock, so the output is the same on every run.

```
cargo run -p iridium --example demo
```

Real output:

```
> CREATE TABLE city (id INTEGER PRIMARY KEY, name TEXT NOT NULL, pop INTEGER)
  ok, 0 row(s) changed
> INSERT INTO city (name, pop) VALUES ('Oslo', 709037), ('Lima', 9751717), ('Pune', 3124458)
  ok, 3 row(s) changed
> BEGIN
  ok, 0 row(s) changed
  in transaction: true
> UPDATE city SET pop = pop + 1 WHERE name = 'Oslo'
  ok, 1 row(s) changed
> ROLLBACK
  ok, 0 row(s) changed
  in transaction: false
> SELECT name, pop FROM city ORDER BY pop DESC
  name | pop
  Lima | 9751717
  Pune | 3124458
  Oslo | 709037
> DELETE FROM city WHERE pop < 1000000
  ok, 1 row(s) changed
> SELECT COUNT(*) AS n FROM city
  n
  2
> duplicate primary key refused: constraint failed: UNIQUE constraint failed: city.id
database closed cleanly
```

## Tests

```
cargo test -p iridium
```

The gate for this crate is `bash tools/check.sh iridium` from the repository root.
