//! SQL core integration and property tests for Varve.
#![allow(clippy::unwrap_used)]

use std::cmp::Ordering;

use assay::{forall, Rng};
use varve::sql::{decode_record, encode_key, encode_record, Db, Rows, Value};
use varve::{Error, MemStorage};

fn mem_db() -> Db<MemStorage> {
    Db::open(MemStorage::new(), MemStorage::new()).unwrap()
}

fn sql_cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Int(_), _) => Ordering::Less,
        (_, Value::Int(_)) => Ordering::Greater,
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Text(_), Value::Blob(_)) => Ordering::Less,
        (Value::Blob(_), Value::Text(_)) => Ordering::Greater,
        (Value::Blob(x), Value::Blob(y)) => x.cmp(y),
    }
}

fn rand_value(r: &mut Rng) -> Value {
    match r.below(4) {
        0 => Value::Null,
        1 => match r.below(5) {
            0 => Value::Int(i64::MIN),
            1 => Value::Int(i64::MAX),
            2 => Value::Int(0),
            _ => Value::Int(r.u64() as i64),
        },
        2 => {
            let len = r.below(24);
            let mut s = String::new();
            for _ in 0..len {
                let ch = match r.below(6) {
                    0 => '\0',
                    1 => 'a',
                    2 => 'b',
                    3 => 'Z',
                    4 => '0',
                    _ => 'é',
                };
                s.push(ch);
            }
            Value::Text(s)
        }
        _ => Value::Blob(r.bytes(24)),
    }
}

#[test]
fn encoding_order_and_round_trips_property() {
    forall(250, 0x51a7_0001, |r| {
        let len = 1 + r.below(4);
        let row_a: Vec<Value> = (0..len).map(|_| rand_value(r)).collect();
        let row_b: Vec<Value> = (0..len).map(|_| rand_value(r)).collect();

        let rec_a = encode_record(&row_a);
        let rec_b = encode_record(&row_b);
        assert_eq!(decode_record(&rec_a).unwrap(), row_a);
        assert_eq!(decode_record(&rec_b).unwrap(), row_b);

        let ka = encode_key(&row_a);
        let kb = encode_key(&row_b);
        let expected_ord = row_a
            .iter()
            .zip(row_b.iter())
            .map(|(a, b)| sql_cmp(a, b))
            .find(|&o| o != Ordering::Equal)
            .unwrap_or(Ordering::Equal);
        assert_eq!(ka.cmp(&kb), expected_ord);
    });
}

#[test]
fn hostile_record_bytes_never_panic_property() {
    forall(500, 0x51a7_0002, |r| {
        let bytes = r.bytes(128);
        let _ = decode_record(&bytes);
    });
}

#[test]
fn ddl_dml_and_column_names() {
    let mut db = mem_db();
    assert_eq!(
        db.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE)"
        )
        .unwrap(),
        Rows::default()
    );
    assert_eq!(
        db.execute("CREATE TABLE users (id INT)"),
        Err(Error::Exists)
    );
    assert_eq!(
        db.execute("CREATE INDEX idx_users_name ON users (name)")
            .unwrap(),
        Rows::default()
    );
    assert_eq!(
        db.execute("CREATE INDEX idx_users_name ON users (name)"),
        Err(Error::Exists)
    );

    let ins = db
        .execute("INSERT INTO users (name, email) VALUES ('Alice', 'a@x'), ('Bob', NULL)")
        .unwrap();
    assert_eq!(ins.changes, 2);

    let ins2 = db
        .execute("INSERT INTO users VALUES (10, 'Cara', NULL), (NULL, 'Dan', 'd@x')")
        .unwrap();
    assert_eq!(ins2.changes, 2);

    let r = db
        .execute("SELECT id, users.name AS uname, (name), 1 + 2 FROM users ORDER BY id ASC")
        .unwrap();
    assert_eq!(r.columns, vec!["id", "uname", "(name)", "1 + 2"]);
    assert_eq!(
        r.rows,
        vec![
            vec![
                Value::Int(1),
                Value::Text("Alice".into()),
                Value::Text("Alice".into()),
                Value::Int(3),
            ],
            vec![
                Value::Int(2),
                Value::Text("Bob".into()),
                Value::Text("Bob".into()),
                Value::Int(3),
            ],
            vec![
                Value::Int(10),
                Value::Text("Cara".into()),
                Value::Text("Cara".into()),
                Value::Int(3),
            ],
            vec![
                Value::Int(11),
                Value::Text("Dan".into()),
                Value::Text("Dan".into()),
                Value::Int(3),
            ],
        ]
    );

    let star = db.execute("SELECT * FROM users WHERE id = 2").unwrap();
    assert_eq!(star.columns, vec!["id", "name", "email"]);
    assert_eq!(
        star.rows,
        vec![vec![Value::Int(2), Value::Text("Bob".into()), Value::Null]]
    );

    let upd = db
        .execute("UPDATE users SET name = upper(name) WHERE id >= 10")
        .unwrap();
    assert_eq!(upd.changes, 2);

    let del = db.execute("DELETE FROM users WHERE id = 10").unwrap();
    assert_eq!(del.changes, 1);

    assert_eq!(
        db.execute("DROP INDEX idx_users_name").unwrap(),
        Rows::default()
    );
    assert_eq!(
        db.execute("DROP INDEX idx_users_name"),
        Err(Error::NoSuchTable)
    );
    assert_eq!(db.execute("DROP TABLE users").unwrap(), Rows::default());
    assert_eq!(db.execute("SELECT * FROM users"), Err(Error::NoSuchTable));
}

#[test]
fn constraints_types_and_unique_nulls() {
    let mut db = mem_db();
    db.execute("CREATE TABLE items (k TEXT PRIMARY KEY, n INTEGER NOT NULL, u INT UNIQUE, b BLOB)")
        .unwrap();

    // Multiple NULLs never clash in UNIQUE or non-integer PRIMARY KEY.
    db.execute("INSERT INTO items (k, n, u, b) VALUES (NULL, 1, NULL, x'01ff')")
        .unwrap();
    db.execute("INSERT INTO items (k, n, u, b) VALUES (NULL, 2, NULL, x'')")
        .unwrap();
    db.execute("INSERT INTO items (k, n, u, b) VALUES ('k1', 3, 100, NULL)")
        .unwrap();

    // Duplicate non-null UNIQUE and PRIMARY KEY fail.
    assert!(matches!(
        db.execute("INSERT INTO items (k, n, u) VALUES ('k1', 4, 101)"),
        Err(Error::Constraint(s)) if s.contains("UNIQUE")
    ));
    assert!(matches!(
        db.execute("INSERT INTO items (k, n, u) VALUES ('k2', 4, 100)"),
        Err(Error::Constraint(s)) if s.contains("UNIQUE")
    ));

    // NOT NULL fails with Constraint("NOT NULL ...").
    assert!(matches!(
        db.execute("INSERT INTO items (k, n, u) VALUES ('k2', NULL, 101)"),
        Err(Error::Constraint(s)) if s.contains("NOT NULL")
    ));

    // Wrong types fail with Error::Type.
    assert_eq!(
        db.execute("INSERT INTO items (k, n) VALUES ('k2', 'not_int')"),
        Err(Error::Type)
    );
    assert_eq!(
        db.execute("INSERT INTO items (k, n) VALUES (123, 5)"),
        Err(Error::Type)
    );
    assert_eq!(
        db.execute("INSERT INTO items (k, n, b) VALUES ('k2', 5, 'text')"),
        Err(Error::Type)
    );

    // Unique swap on UPDATE succeeds when old/new collide across updated rows.
    db.execute("INSERT INTO items (k, n, u) VALUES ('k2', 4, 200)")
        .unwrap();
    let sw = db
        .execute("UPDATE items SET u = 300 - u WHERE u IS NOT NULL")
        .unwrap();
    assert_eq!(sw.changes, 2);
}

#[test]
fn three_valued_logic_tables_and_predicates() {
    let mut db = mem_db();
    let res = db
        .execute(
            "SELECT
                1 AND 1, 1 AND 0, 1 AND NULL, 0 AND NULL, NULL AND NULL,
                1 OR 0, 0 OR 0, 1 OR NULL, 0 OR NULL, NULL OR NULL,
                NOT 1, NOT 0, NOT NULL,
                NULL = NULL, NULL <> NULL, NULL IS NULL, NULL IS NOT NULL,
                5 BETWEEN 1 AND 5, 5 BETWEEN 6 AND NULL, 5 BETWEEN 1 AND NULL,
                5 NOT BETWEEN 6 AND NULL,
                2 IN (1, 2, NULL), 3 IN (1, 2, NULL), 3 IN (1, 2),
                NULL IN (1, 2), NULL IN (), NULL NOT IN ()",
        )
        .unwrap();
    assert_eq!(
        res.rows,
        vec![vec![
            Value::Int(1),
            Value::Int(0),
            Value::Null,
            Value::Int(0),
            Value::Null,
            Value::Int(1),
            Value::Int(0),
            Value::Int(1),
            Value::Null,
            Value::Null,
            Value::Int(0),
            Value::Int(1),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(1),
            Value::Int(0),
            Value::Int(1),
            Value::Int(0),
            Value::Null,
            Value::Int(1),
            Value::Int(1),
            Value::Null,
            Value::Int(0),
            Value::Null,
            Value::Int(0),
            Value::Int(1),
        ]]
    );
}

#[test]
fn functions_operators_case_like_and_overflow() {
    let mut db = mem_db();
    let r = db
        .execute(
            "SELECT
                abs(-42), abs(NULL),
                length('héllo'), length(x'000102'), length(NULL),
                lower('AbC!'), upper('AbC!'),
                coalesce(NULL, NULL, 'ok', 'later'), coalesce(NULL, NULL),
                substr('abcdef', 2, 3), substr('abcdef', 0, 3), substr('abcdef', -2),
                substr('abcdef', 4, -2), substr(x'0a0b0c0d', 2, 2), substr('abc', NULL, 2),
                typeof(NULL), typeof(1), typeof('a'), typeof(x'aa'),
                'a' || 1 || 'b', 'a' || NULL,
                -7 / 2, -7 % 2, 5 / 0, 5 % 0,
                'AbCd' LIKE 'a_c%', 'AbCd' NOT LIKE 'z%', NULL LIKE '%',
                CASE 2 WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END,
                CASE NULL WHEN NULL THEN 'bad' ELSE 'ok' END,
                CASE WHEN 0 THEN 'a' WHEN 1 THEN 'b' END",
        )
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::Int(42),
            Value::Null,
            Value::Int(5),
            Value::Int(3),
            Value::Null,
            Value::Text("abc!".into()),
            Value::Text("ABC!".into()),
            Value::Text("ok".into()),
            Value::Null,
            Value::Text("bcd".into()),
            Value::Text("ab".into()),
            Value::Text("ef".into()),
            Value::Text("bc".into()),
            Value::Blob(vec![0x0b, 0x0c]),
            Value::Null,
            Value::Text("null".into()),
            Value::Text("integer".into()),
            Value::Text("text".into()),
            Value::Text("blob".into()),
            Value::Text("a1b".into()),
            Value::Null,
            Value::Int(-3),
            Value::Int(-1),
            Value::Null,
            Value::Null,
            Value::Int(1),
            Value::Int(1),
            Value::Null,
            Value::Text("two".into()),
            Value::Text("ok".into()),
            Value::Text("b".into()),
        ]]
    );

    // Minimum i64 literal and overflow checks.
    let min_res = db.execute("SELECT -9223372036854775808").unwrap();
    assert_eq!(min_res.rows, vec![vec![Value::Int(i64::MIN)]]);

    assert_eq!(
        db.execute("SELECT 9223372036854775807 + 1"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT -9223372036854775808 - 1"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT 9223372036854775807 * 2"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT -(-9223372036854775808)"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT abs(-9223372036854775808)"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT -9223372036854775808 / -1"),
        Err(Error::Overflow)
    );
}

#[test]
fn group_by_having_distinct_order_by_and_limit_offset() {
    let mut db = mem_db();
    db.execute("CREATE TABLE sales (grp TEXT, val INT)")
        .unwrap();
    db.execute(
        "INSERT INTO sales VALUES
        ('a', 10), ('a', 20), ('a', NULL),
        ('b', 5),
        ('c', NULL),
        (NULL, 7), (NULL, 8)",
    )
    .unwrap();

    let agg = db
        .execute(
            "SELECT grp, count(*), count(val), sum(val), min(val), max(val)
             FROM sales
             GROUP BY grp
             HAVING count(*) >= 2 OR grp = 'c'
             ORDER BY grp ASC",
        )
        .unwrap();
    assert_eq!(
        agg.rows,
        vec![
            vec![
                Value::Null,
                Value::Int(2),
                Value::Int(2),
                Value::Int(15),
                Value::Int(7),
                Value::Int(8),
            ],
            vec![
                Value::Text("a".into()),
                Value::Int(3),
                Value::Int(2),
                Value::Int(30),
                Value::Int(10),
                Value::Int(20),
            ],
            vec![
                Value::Text("c".into()),
                Value::Int(1),
                Value::Int(0),
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ]
    );

    // Aggregate on empty result set without GROUP BY yields 1 row.
    let empty_agg = db
        .execute("SELECT count(*), sum(val), min(val), max(val) FROM sales WHERE val = 999")
        .unwrap();
    assert_eq!(
        empty_agg.rows,
        vec![vec![Value::Int(0), Value::Null, Value::Null, Value::Null]]
    );

    // DISTINCT, mixed-type ORDER BY, LIMIT/OFFSET.
    db.execute("CREATE TABLE mix (id INT, v BLOB)").unwrap();
    db.execute("CREATE TABLE anyv (k INT)").unwrap();
    let _ = db.execute("DROP TABLE mix");
    let _ = db.execute("DROP TABLE anyv");

    let ord = db
        .execute("SELECT DISTINCT val FROM sales ORDER BY val DESC LIMIT 3 OFFSET 1")
        .unwrap();
    assert_eq!(
        ord.rows,
        vec![
            vec![Value::Int(10)],
            vec![Value::Int(8)],
            vec![Value::Int(7)],
        ]
    );

    // Cross-type ORDER BY using literals/expressions.
    db.execute("CREATE TABLE t2 (k INT, a INT, b TEXT)")
        .unwrap();
    db.execute("INSERT INTO t2 VALUES (1, 10, 'z'), (2, NULL, 'a'), (3, 5, 'b')")
        .unwrap();
    let mixed_ord = db
        .execute(
            "SELECT k, CASE k WHEN 1 THEN x'00' WHEN 2 THEN 'hi' ELSE a END AS m
             FROM t2 ORDER BY m ASC, k DESC",
        )
        .unwrap();
    assert_eq!(
        mixed_ord.rows,
        vec![
            vec![Value::Int(3), Value::Int(5)],
            vec![Value::Int(2), Value::Text("hi".into())],
            vec![Value::Int(1), Value::Blob(vec![0x00])],
        ]
    );
}

#[test]
fn multi_row_insert_atomicity_and_transactions() {
    let mut db = mem_db();
    db.execute("CREATE TABLE t (id INT PRIMARY KEY, name TEXT NOT NULL)")
        .unwrap();
    db.execute("INSERT INTO t VALUES (1, 'one')").unwrap();

    // Multi-row INSERT failing on row 5 leaves rows 1..4 out too (autocommit).
    let err = db.execute(
        "INSERT INTO t VALUES (2, 'two'), (3, 'three'), (4, 'four'), (5, 'five'), (6, NULL)",
    );
    assert!(matches!(err, Err(Error::Constraint(_))));
    assert_eq!(
        db.execute("SELECT id FROM t ORDER BY id").unwrap().rows,
        vec![vec![Value::Int(1)]]
    );

    // Same inside explicit BEGIN: failing statement changes nothing, earlier tx writes stay.
    assert!(!db.in_transaction());
    assert_eq!(db.execute("COMMIT"), Err(Error::Misuse));
    assert_eq!(db.execute("ROLLBACK"), Err(Error::Misuse));

    db.execute("BEGIN").unwrap();
    assert!(db.in_transaction());
    assert_eq!(db.execute("BEGIN"), Err(Error::Misuse));

    db.execute("INSERT INTO t VALUES (2, 'two')").unwrap();
    let err2 = db.execute(
        "INSERT INTO t VALUES (3, 'three'), (4, 'four'), (5, 'five'), (6, 'six'), (2, 'dup')",
    );
    assert!(matches!(err2, Err(Error::Constraint(_))));
    assert_eq!(
        db.execute("SELECT id FROM t ORDER BY id").unwrap().rows,
        vec![vec![Value::Int(1)], vec![Value::Int(2)]]
    );

    db.execute("ROLLBACK").unwrap();
    assert!(!db.in_transaction());
    assert_eq!(
        db.execute("SELECT id FROM t ORDER BY id").unwrap().rows,
        vec![vec![Value::Int(1)]]
    );

    // Commit persists across close and reopen.
    db.execute("BEGIN").unwrap();
    db.execute("INSERT INTO t VALUES (10, 'ten')").unwrap();
    db.execute("COMMIT").unwrap();
    let (main, log) = db.close().unwrap();

    let mut db2 = Db::open(main, log).unwrap();
    assert_eq!(
        db2.execute("SELECT id, name FROM t ORDER BY id")
            .unwrap()
            .rows,
        vec![
            vec![Value::Int(1), Value::Text("one".into())],
            vec![Value::Int(10), Value::Text("ten".into())],
        ]
    );
}

#[test]
fn join_and_explain_parse_and_return_misuse() {
    let mut db = mem_db();
    db.execute("CREATE TABLE a (id INT)").unwrap();
    db.execute("CREATE TABLE b (id INT)").unwrap();

    assert_eq!(
        db.execute("SELECT a.id FROM a INNER JOIN b ON a.id = b.id"),
        Err(Error::Misuse)
    );
    assert_eq!(
        db.execute("SELECT a.id FROM a LEFT JOIN b ON a.id = b.id"),
        Err(Error::Misuse)
    );
    assert_eq!(
        db.execute("EXPLAIN SELECT * FROM a WHERE id = 1"),
        Err(Error::Misuse)
    );
}

#[test]
fn parse_error_positions_and_single_statement_rule() {
    let mut db = mem_db();
    let err = db.execute("SELECT 1 FROM @").unwrap_err();
    assert!(matches!(err, Error::Parse { pos: 14, .. }));

    let err2 = db.execute("SELECT 1; SELECT 2").unwrap_err();
    assert!(matches!(err2, Error::Parse { pos: 10, .. }));

    let err3 = db.execute("   ").unwrap_err();
    assert!(matches!(err3, Error::Parse { .. }));

    assert_eq!(
        db.execute("SELECT missing FROM nope"),
        Err(Error::NoSuchTable)
    );
    db.execute("CREATE TABLE t (a INT)").unwrap();
    assert_eq!(db.execute("SELECT b FROM t"), Err(Error::NoSuchColumn));
}

#[test]
fn random_sql_text_never_panics_property() {
    let tokens = [
        "SELECT", "FROM", "WHERE", "GROUP", "BY", "HAVING", "ORDER", "ASC", "DESC", "LIMIT",
        "OFFSET", "CREATE", "TABLE", "DROP", "INDEX", "UNIQUE", "INSERT", "INTO", "VALUES",
        "UPDATE", "SET", "DELETE", "BEGIN", "COMMIT", "ROLLBACK", "EXPLAIN", "JOIN", "INNER",
        "LEFT", "ON", "AND", "OR", "NOT", "IS", "NULL", "BETWEEN", "IN", "LIKE", "CASE", "WHEN",
        "THEN", "ELSE", "END", "AS", "INTEGER", "INT", "TEXT", "BLOB", "PRIMARY", "KEY", "t", "a",
        "b", "1", "0", "-1", "'x'", "''", "x'0a'", "(", ")", ",", ";", ".", "+", "-", "*", "/",
        "%", "||", "=", "<>", "!=", "<=", ">=", "@", "\"", "/*", "--", " ", "\n",
    ];
    let mut db = mem_db();
    db.execute("CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT)")
        .unwrap();
    forall(400, 0x51a7_0003, |r| {
        let count = r.below(18);
        let mut s = String::new();
        for _ in 0..count {
            s.push_str(r.pick(&tokens));
            if r.chance(2, 3) {
                s.push(' ');
            }
        }
        let _ = db.execute(&s);
    });
}

#[test]
fn unique_index_creation_and_ipk_updates() {
    let mut db = mem_db();
    db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, code TEXT, val INT)")
        .unwrap();
    db.execute("INSERT INTO t VALUES (1, 'a', 10), (2, 'a', 20)")
        .unwrap();

    // Creating a UNIQUE index on duplicate values fails cleanly and leaves no index.
    assert!(matches!(
        db.execute("CREATE UNIQUE INDEX u_code ON t (code)"),
        Err(Error::Constraint(_))
    ));
    db.execute("UPDATE t SET code = 'b' WHERE rowid = 2")
        .unwrap();
    db.execute("CREATE UNIQUE INDEX u_code ON t (code)")
        .unwrap();

    // Index rejects duplicate inserts and updates.
    assert!(matches!(
        db.execute("INSERT INTO t (code, val) VALUES ('b', 30)"),
        Err(Error::Constraint(_))
    ));
    assert!(matches!(
        db.execute("UPDATE t SET code = 'a' WHERE id = 2"),
        Err(Error::Constraint(_))
    ));

    // INTEGER PRIMARY KEY updates: reject NULL, type error, collision, allow valid move.
    assert!(matches!(
        db.execute("UPDATE t SET id = NULL WHERE id = 1"),
        Err(Error::Constraint(_))
    ));
    assert_eq!(
        db.execute("UPDATE t SET id = 'bad' WHERE id = 1"),
        Err(Error::Type)
    );
    assert!(matches!(
        db.execute("UPDATE t SET id = 2 WHERE id = 1"),
        Err(Error::Constraint(_))
    ));
    db.execute("UPDATE t SET id = 5 WHERE id = 1").unwrap();
    let res = db
        .execute("SELECT rowid, code FROM t ORDER BY 1 DESC")
        .unwrap();
    assert_eq!(
        res.rows,
        vec![
            vec![Value::Int(5), Value::Text("a".into())],
            vec![Value::Int(2), Value::Text("b".into())],
        ]
    );
}
