# FIX iridium-1: two writers must not lose data (from SIM p4, reports/sim/p4-server-data.md row 36)
Files you may touch: crates/iridium/** only. Gate: `bash tools/check.sh iridium` must end `CHECK PASS iridium`.
Zero external crates (std only; rustc is 1.98, so std::fs::File::lock / try_lock / lock_shared are available).
Do not weaken, skip or delete tests. Report: reports/sim/fix-iridium-1.md, 10 lines max. Budget about 150k. No skills.

1. DATA-LOSS, leader-reproduced: two `iridium sql c.idb < x.sql` processes inserting disjoint rows at once:
   one fails "storage I/O error", the one that exits 0 has silently lost rows (300 INSERTs, COUNT(*) = 196),
   and with 2000 rows the DB is left "corrupt page or log" for every later open.
   Required: a process that opens a DB for writing holds an exclusive OS lock for as long as it has the DB
   open (lock the main .idb file handle, or a sibling `<db>-lock` file, with File::lock/try_lock). A second
   opener waits up to 5 s (retry try_lock every 50 ms), then fails cleanly with
   "iridium: database is locked by another process" exit 1 before touching any file. Readers may take a shared
   lock if the code makes that simple; otherwise everything is exclusive. A process that exits 0 must have
   committed every statement it reported OK. Also check the WAL/recovery path can never treat another live
   process's half-written log as a crash to recover (the lock makes that impossible; say so in a comment).
   Test: spawn two child processes of the built binary (env!("CARGO_BIN_EXE_iridium")) inserting 500 disjoint
   rows each; afterwards the DB opens, and COUNT(*) equals the sum of rows from the processes that exited 0.
2. Row 31: `iridium sql DB "INSERT ...; SELECT 1"` fails "unexpected trailing tokens" though stdin accepts it.
   Required: the QUERY argument runs every ;-separated statement, like stdin.
3. Row 29: "parse error at byte 0: expected SQL statement" for `SELEC * FORM users`. Required: name the token
   found: "parse error at byte 0: expected a statement (SELECT, INSERT, ...), found 'SELEC'".
