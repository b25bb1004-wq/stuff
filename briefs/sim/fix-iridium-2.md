# FIX iridium-2: decimal numbers (from SIM p4 rows 21, 25)
Files: crates/iridium/** only. Gate: `bash tools/check.sh iridium` ends CHECK PASS iridium. Zero external crates.
Do not weaken, skip or delete tests; old databases written before this change must still open and read
(add a test that opens a DB created with only INTEGER/TEXT columns by the current code path). Budget about 150k. No skills.
Report: reports/sim/fix-iridium-2.md, 10 lines max.
1. `CREATE TABLE t (x REAL)` (also accept FLOAT, DOUBLE, NUMERIC as REAL) and float literals (1.5, -0.25, 1e3).
   Store as f64 in the row encoding with a new type tag (do not change existing tags). Comparison and ORDER BY
   between INTEGER and REAL compare numerically. Arithmetic + - * / mixing INTEGER and REAL gives REAL;
   INTEGER / INTEGER stays integer division (SQLite rule). Printing: shortest round-trip form (Rust `{}` of f64,
   with ".0" kept for whole numbers, e.g. 2.0).
2. `AVG(col)`: REAL average of non-NULL values, NULL for no rows; works with GROUP BY like SUM. SUM over REAL
   gives REAL.
