# fix-iridium-1: report
1. Lock: new `crates/iridium/src/lock.rs` (`lock_database`, `DbLock`, `Error::Locked`) takes an exclusive File::try_lock on `<db>-lock`, retried every 50 ms for 5 s, then "database is locked by another process" (exit 1). CLI `sql` and `Server::start` hold it for their lifetime. Also `sql` now exits 1 if a BEGIN is left open (was silently rolled back). Tests: CLI concurrent writers (2x500 rows), lock-held refusal, open-transaction. Comment in wal.rs recover_or_init explains why a live log is never recovered.
2. QUERY argument runs every `;`-separated statement through the same splitter as stdin; `--` comments now end at the newline.
3. Parse error now reads "expected a statement (SELECT, INSERT, ...), found 'SELEC'" (token text from source span).
Public additions: `Error::Locked`, `lock_database`, `DbLock` (needed by the CLI binary and tests).
Tests: 119 passed in crate (new: 5 CLI tests + 1 lock unit test). Existing tests unchanged.
Gate: CHECK PASS iridium (exit 0).
Not done: lock file `<db>-lock` is left on disk (empty); readers are exclusive, not shared.
