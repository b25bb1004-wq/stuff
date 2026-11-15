# T80 hades (filesystem): security review
Sampled: layout.rs fixed-width reads (40-55, bounds-checked via sub()), fs.rs device block count (15, 52), non-test panic scan (only const tables and the layout reads). unsafe: none.
- No finding in the sampled code. Not read: fsck.rs, journal.rs replay, directory and inode traversal, and path handling in fs.rs.
Count: high 0, medium 0, low 0.
