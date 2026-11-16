fix-iridium-2: REAL type, decimal literals, mixed INTEGER/REAL rules, AVG. Gate: CHECK PASS iridium.
Done: REAL/FLOAT/DOUBLE/NUMERIC columns (new tag 4 in records, key tag 0x18); float literals 1.5, -0.25, 1e3; INTEGER->REAL on insert/update; numeric compare, ORDER BY, index probes; + - * / with REAL; INTEGER/INTEGER stays integer; AVG (GROUP BY ok); SUM over REAL gives REAL; NaN becomes NULL.
Tests: new tests/real_values.rs (10 tests incl. reopen and legacy INTEGER/TEXT file tests using tests/data/legacy_int_text.*).
Not done / not verified: CLI print format (code present, no test); Value no longer derives Eq (public API change); % stays integer-only.
join_leader.rs change: only added arm `Value::Real(f) => format!("R{}", f.to_bits())` to enc(); no assertion touched.
Also needed to compile (no assertions removed): added Real arms in tests/net_leader.rs, tests/sql.rs (sql_cmp numeric), tests/sql_leader.rs (enc + numeric sql_cmp).
Format: cargo fmt -p iridium was run (reformatted earlier worker's code).
CHECK PASS iridium
