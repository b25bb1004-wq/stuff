# T80 iridium (database): security review
Sampled: net/codec.rs decode_rows (50-80, column and row counts checked against remaining bytes before allocation), frame reader (codec.rs:9, 210-225, 1 MiB cap), pager.rs read and write (111-150, page number checked against page_count), storage.rs file reads. unsafe: none.
- No finding in the sampled code. Not traced: the two unreachable!() sites (sql/db.rs:137, sql/expr.rs:306); whether SQL input can reach them is unverified. Not read: wal.rs, crash.rs, tree.rs, sql/parser*.rs.
Count: high 0, medium 0, low 0.
