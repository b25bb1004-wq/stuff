//! Tests for T41b: joins, index planning, EXPLAIN, and indexed vs unindexed equivalence.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use iridium::sql::{Db, Rows, Value};
use iridium::{Error, MemStorage};

fn open_mem() -> Db<MemStorage> {
    Db::open(MemStorage::new(), MemStorage::new()).unwrap()
}

fn exec_ok(db: &mut Db<MemStorage>, sql: &str) -> Rows {
    db.execute(sql)
        .unwrap_or_else(|e| panic!("SQL failed: {sql}\n  error: {e:?}"))
}

fn sorted_rows(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    rows.sort_by_key(|r| iridium::sql::encode_key(r));
    rows
}

#[test]
fn inner_and_left_joins_2_and_3_tables_nulls_dups() {
    let mut db = open_mem();
    exec_ok(
        &mut db,
        "CREATE TABLE a (id INTEGER PRIMARY KEY, k INT, v TEXT)",
    );
    exec_ok(
        &mut db,
        "CREATE TABLE b (bid INTEGER PRIMARY KEY, k INT, tag TEXT)",
    );
    exec_ok(
        &mut db,
        "CREATE TABLE c (cid INTEGER PRIMARY KEY, k INT, score INT)",
    );

    exec_ok(
        &mut db,
        "INSERT INTO a (id, k, v) VALUES (1, 10, 'a1'), (2, 10, 'a2'), (3, 20, 'a3'), (4, NULL, 'anull')",
    );
    exec_ok(
        &mut db,
        "INSERT INTO b (bid, k, tag) VALUES (1, 10, 'b1'), (2, 10, 'b2'), (3, 30, 'b3'), (4, NULL, 'bnull')",
    );
    exec_ok(
        &mut db,
        "INSERT INTO c (cid, k, score) VALUES (1, 10, 100), (2, 20, 200), (3, NULL, 999)",
    );

    // 2-table INNER JOIN with duplicates and NULLs (NULL = NULL must not match).
    let r = exec_ok(
        &mut db,
        "SELECT a.id, b.bid FROM a INNER JOIN b ON a.k = b.k ORDER BY a.id, b.bid",
    );
    assert_eq!(r.columns, vec!["id", "bid"]);
    assert_eq!(
        r.rows,
        vec![
            vec![Value::Int(1), Value::Int(1)],
            vec![Value::Int(1), Value::Int(2)],
            vec![Value::Int(2), Value::Int(1)],
            vec![Value::Int(2), Value::Int(2)],
        ]
    );

    // 2-table LEFT JOIN including NULL rowid on unmatched right side.
    let r = exec_ok(
        &mut db,
        "SELECT a.id, b.bid, b.rowid, b.tag FROM a LEFT OUTER JOIN b ON a.k = b.k ORDER BY a.id, b.bid",
    );
    assert_eq!(
        r.rows,
        vec![
            vec![
                Value::Int(1),
                Value::Int(1),
                Value::Int(1),
                Value::Text("b1".into())
            ],
            vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(2),
                Value::Text("b2".into())
            ],
            vec![
                Value::Int(2),
                Value::Int(1),
                Value::Int(1),
                Value::Text("b1".into())
            ],
            vec![
                Value::Int(2),
                Value::Int(2),
                Value::Int(2),
                Value::Text("b2".into())
            ],
            vec![Value::Int(3), Value::Null, Value::Null, Value::Null],
            vec![Value::Int(4), Value::Null, Value::Null, Value::Null],
        ]
    );

    // WHERE filtering on LEFT JOIN right column must run AFTER LEFT JOIN.
    let r = exec_ok(
        &mut db,
        "SELECT a.id FROM a LEFT JOIN b ON a.k = b.k WHERE b.bid IS NULL ORDER BY a.id",
    );
    assert_eq!(r.rows, vec![vec![Value::Int(3)], vec![Value::Int(4)]]);

    // 3-table mixed INNER + LEFT JOIN.
    let r = exec_ok(
        &mut db,
        "SELECT a.id, b.bid, c.score FROM a JOIN b ON a.k = b.k LEFT JOIN c ON b.k = c.k ORDER BY a.id, b.bid",
    );
    assert_eq!(
        r.rows,
        vec![
            vec![Value::Int(1), Value::Int(1), Value::Int(100)],
            vec![Value::Int(1), Value::Int(2), Value::Int(100)],
            vec![Value::Int(2), Value::Int(1), Value::Int(100)],
            vec![Value::Int(2), Value::Int(2), Value::Int(100)],
        ]
    );

    // 3-table LEFT + LEFT JOIN.
    let r = exec_ok(
        &mut db,
        "SELECT a.id, b.bid, c.cid FROM a LEFT JOIN b ON a.k = b.k LEFT JOIN c ON a.k = c.k ORDER BY a.id, b.bid, c.cid",
    );
    assert_eq!(
        r.rows,
        vec![
            vec![Value::Int(1), Value::Int(1), Value::Int(1)],
            vec![Value::Int(1), Value::Int(2), Value::Int(1)],
            vec![Value::Int(2), Value::Int(1), Value::Int(1)],
            vec![Value::Int(2), Value::Int(2), Value::Int(1)],
            vec![Value::Int(3), Value::Null, Value::Int(2)],
            vec![Value::Int(4), Value::Null, Value::Null],
        ]
    );
}

#[test]
fn aliases_star_expansion_and_ambiguous_names() {
    let mut db = open_mem();
    exec_ok(&mut db, "CREATE TABLE t1 (id INT, val TEXT)");
    exec_ok(&mut db, "CREATE TABLE t2 (id INT, extra INT)");
    exec_ok(&mut db, "INSERT INTO t1 VALUES (1, 'x'), (2, 'y')");
    exec_ok(&mut db, "INSERT INTO t2 VALUES (1, 10)");

    // * expansion produces bare column names in table order.
    let r = exec_ok(
        &mut db,
        "SELECT * FROM t1 AS a LEFT JOIN t2 b ON a.id = b.id ORDER BY a.id",
    );
    assert_eq!(r.columns, vec!["id", "val", "id", "extra"]);
    assert_eq!(
        r.rows,
        vec![
            vec![
                Value::Int(1),
                Value::Text("x".into()),
                Value::Int(1),
                Value::Int(10)
            ],
            vec![
                Value::Int(2),
                Value::Text("y".into()),
                Value::Null,
                Value::Null
            ],
        ]
    );

    // Table-qualified star expansion.
    let r = exec_ok(
        &mut db,
        "SELECT a.*, b.extra FROM t1 AS a JOIN t2 AS b ON a.id = b.id",
    );
    assert_eq!(r.columns, vec!["id", "val", "extra"]);
    assert_eq!(
        r.rows,
        vec![vec![Value::Int(1), Value::Text("x".into()), Value::Int(10)]]
    );

    // Self-join with aliases.
    let r = exec_ok(
        &mut db,
        "SELECT x.id, y.val FROM t1 AS x JOIN t1 AS y ON x.id + 1 = y.id",
    );
    assert_eq!(r.rows, vec![vec![Value::Int(1), Value::Text("y".into())]]);

    // Old table name is out of scope when aliased.
    assert_eq!(
        db.execute("SELECT t1.id FROM t1 AS a"),
        Err(Error::NoSuchColumn)
    );

    // Ambiguous unqualified column names in SELECT, WHERE, ON, ORDER BY, rowid.
    for bad_sql in [
        "SELECT id FROM t1 JOIN t2 ON t1.id = t2.id",
        "SELECT t1.id FROM t1 JOIN t2 ON id = 1",
        "SELECT t1.id FROM t1 JOIN t2 ON t1.id = t2.id WHERE id = 1",
        "SELECT t1.id FROM t1 JOIN t2 ON t1.id = t2.id ORDER BY id",
        "SELECT rowid FROM t1 JOIN t2 ON t1.id = t2.id",
        "SELECT x.id FROM t1 AS x JOIN t2 AS x ON x.id = x.id",
    ] {
        let err = db.execute(bad_sql).unwrap_err();
        assert!(
            matches!(err, Error::Parse { ref msg, .. } if msg.contains("ambiguous")),
            "expected ambiguous Parse error for `{bad_sql}`, got {err:?}"
        );
    }

    // ON clause cannot see tables joined later.
    exec_ok(&mut db, "CREATE TABLE t3 (z INT)");
    assert_eq!(
        db.execute("SELECT * FROM t1 JOIN t2 ON t1.id = t3.z JOIN t3 ON t2.id = t3.z"),
        Err(Error::NoSuchColumn)
    );
}

#[test]
fn aggregates_group_having_distinct_order_limit_over_joins() {
    let mut db = open_mem();
    exec_ok(&mut db, "CREATE TABLE dept (id INT, name TEXT)");
    exec_ok(&mut db, "CREATE TABLE emp (id INT, did INT, sal INT)");
    exec_ok(
        &mut db,
        "INSERT INTO dept VALUES (1, 'eng'), (2, 'ops'), (3, 'empty')",
    );
    exec_ok(
        &mut db,
        "INSERT INTO emp VALUES (1, 1, 100), (2, 1, 200), (3, 2, 150), (4, 2, NULL)",
    );

    let r = exec_ok(
        &mut db,
        "SELECT d.name, count(*), count(e.sal), coalesce(sum(e.sal), 0) AS total \
         FROM dept d LEFT JOIN emp e ON d.id = e.did \
         GROUP BY d.id, d.name \
         HAVING count(*) >= 1 \
         ORDER BY total DESC, d.name ASC",
    );
    assert_eq!(r.columns, vec!["name", "count(*)", "count(e.sal)", "total"]);
    assert_eq!(
        r.rows,
        vec![
            vec![
                Value::Text("eng".into()),
                Value::Int(2),
                Value::Int(2),
                Value::Int(300)
            ],
            vec![
                Value::Text("ops".into()),
                Value::Int(2),
                Value::Int(1),
                Value::Int(150)
            ],
            vec![
                Value::Text("empty".into()),
                Value::Int(1),
                Value::Int(0),
                Value::Int(0)
            ],
        ]
    );

    let r = exec_ok(
        &mut db,
        "SELECT DISTINCT d.name FROM dept d JOIN emp e ON d.id = e.did ORDER BY d.name LIMIT 1 OFFSET 1",
    );
    assert_eq!(r.rows, vec![vec![Value::Text("ops".into())]]);
}

#[test]
fn explain_plan_forms_and_created_order() {
    let mut db = open_mem();
    exec_ok(
        &mut db,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, a INT, b INT, c TEXT)",
    );
    exec_ok(&mut db, "CREATE INDEX idx_b ON t (b)");
    exec_ok(&mut db, "CREATE INDEX idx_ab ON t (a, b)");
    exec_ok(&mut db, "CREATE TABLE u (uid INT, k INT)");
    exec_ok(&mut db, "CREATE INDEX idx_uk ON u (k)");

    let explain = |db: &mut Db<MemStorage>, q: &str| -> Vec<String> {
        let r = exec_ok(db, q);
        assert_eq!(r.columns, vec!["plan"]);
        r.rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::Text(s) => s.clone(),
                other => panic!("expected text plan, got {other:?}"),
            })
            .collect()
    };

    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t AS alias_t"),
        vec!["SCAN t"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE id = 5"),
        vec!["SEARCH t USING ROWID"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE 5 = rowid AND b = 1"),
        vec!["SEARCH t USING ROWID"]
    );
    // Created order: idx_b was created before idx_ab, so first match wins.
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a = 1 AND b = 2"),
        vec!["SEARCH t USING INDEX idx_b (b=?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a = 1 AND b > 5"),
        vec!["SEARCH t USING INDEX idx_b (b>?)"]
    );

    // Drop and recreate idx_b so idx_ab is now first in created order.
    exec_ok(&mut db, "DROP INDEX idx_b");
    exec_ok(&mut db, "CREATE INDEX idx_b ON t (b)");
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a = 1 AND b = 2"),
        vec!["SEARCH t USING INDEX idx_ab (a=? AND b=?)"]
    );
    // Single-column and multi-column range and BETWEEN forms, including flipped operators.
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a = 1 AND b > 5"),
        vec!["SEARCH t USING INDEX idx_ab (a=? AND b>?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE 5 < a"),
        vec!["SEARCH t USING INDEX idx_ab (a>?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a <= 5"),
        vec!["SEARCH t USING INDEX idx_ab (a<=?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE 5 <= a"),
        vec!["SEARCH t USING INDEX idx_ab (a>=?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a < 5"),
        vec!["SEARCH t USING INDEX idx_ab (a<?)"]
    );
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a BETWEEN 1 AND 5"),
        vec!["SEARCH t USING INDEX idx_ab (a>? AND a<?)"]
    );
    // Join EXPLAIN: one row per table in join order.
    assert_eq!(
        explain(
            &mut db,
            "EXPLAIN SELECT * FROM t AS x LEFT JOIN u AS y ON x.a = y.k WHERE x.id = 1"
        ),
        vec!["SEARCH t USING ROWID", "SEARCH u USING INDEX idx_uk (k=?)"]
    );
    // No-FROM EXPLAIN has 0 rows and column `plan`.
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT 1 + 2"),
        Vec::<String>::new()
    );

    // Reopen preserves index order.
    let (main, log) = db.close().unwrap();
    let mut db = Db::open(main, log).unwrap();
    assert_eq!(
        explain(&mut db, "EXPLAIN SELECT * FROM t WHERE a = 1 AND b = 2"),
        vec!["SEARCH t USING INDEX idx_ab (a=? AND b=?)"]
    );
}

#[test]
fn property_queries_match_with_and_without_indexes() {
    forall(30, 0x41B0_2026_u64, |r| {
        let mut db_idx = open_mem();
        let mut db_plain = open_mem();

        let ddls = [
            "CREATE TABLE t1 (id INTEGER PRIMARY KEY, a INT, b INT, s TEXT)",
            "CREATE TABLE t2 (rid INTEGER PRIMARY KEY, a INT, c INT, s TEXT)",
            "CREATE TABLE t3 (x INT, y TEXT)",
        ];
        for sql in ddls {
            exec_ok(&mut db_idx, sql);
            exec_ok(&mut db_plain, sql);
        }

        let num_rows = r.range(4, 22);
        for i in 1..=num_rows {
            let a1 = rand_int_sql(r);
            let b1 = rand_int_sql(r);
            let s1 = rand_text_sql(r);
            let ins1 = format!("INSERT INTO t1 VALUES ({i}, {a1}, {b1}, {s1})");
            exec_ok(&mut db_idx, &ins1);
            exec_ok(&mut db_plain, &ins1);

            let a2 = rand_int_sql(r);
            let c2 = rand_int_sql(r);
            let s2 = rand_text_sql(r);
            let ins2 = format!("INSERT INTO t2 VALUES ({i}, {a2}, {c2}, {s2})");
            exec_ok(&mut db_idx, &ins2);
            exec_ok(&mut db_plain, &ins2);

            if r.chance(3, 4) {
                let x3 = rand_int_sql(r);
                let y3 = rand_text_sql(r);
                let ins3 = format!("INSERT INTO t3 VALUES ({x3}, {y3})");
                exec_ok(&mut db_idx, &ins3);
                exec_ok(&mut db_plain, &ins3);
            }
        }

        // Create a random subset of indexes on both, then drop all of them on db_plain.
        let candidate_indexes = [
            ("i_t1_a", "CREATE INDEX i_t1_a ON t1 (a)"),
            ("i_t1_ab", "CREATE INDEX i_t1_ab ON t1 (a, b)"),
            ("i_t1_ba", "CREATE INDEX i_t1_ba ON t1 (b, a)"),
            ("i_t1_s", "CREATE INDEX i_t1_s ON t1 (s)"),
            ("i_t2_a", "CREATE INDEX i_t2_a ON t2 (a)"),
            ("i_t2_ac", "CREATE INDEX i_t2_ac ON t2 (a, c)"),
            ("i_t3_x", "CREATE INDEX i_t3_x ON t3 (x)"),
            ("i_t3_yx", "CREATE INDEX i_t3_yx ON t3 (y, x)"),
        ];
        let mut created = Vec::new();
        for (name, sql) in candidate_indexes {
            if r.chance(1, 2) {
                exec_ok(&mut db_idx, sql);
                exec_ok(&mut db_plain, sql);
                created.push(name);
            }
        }
        for name in created {
            exec_ok(&mut db_plain, &format!("DROP INDEX {name}"));
        }

        // Compare diverse queries: single-table, multi-column ranges, NULL bounds, long strings, 2/3-table joins.
        let mut queries = vec![
            "SELECT * FROM t1 WHERE a = NULL".to_owned(),
            "SELECT * FROM t1 WHERE a < NULL".to_owned(),
            "SELECT * FROM t1 WHERE a BETWEEN NULL AND 5".to_owned(),
            "SELECT * FROM t1 WHERE a BETWEEN 5 AND 1".to_owned(),
            format!("SELECT * FROM t1 WHERE s = '{}'", "z".repeat(1100)),
            format!("SELECT * FROM t1 WHERE s < '{}'", "z".repeat(1100)),
            format!("SELECT * FROM t1 WHERE s > '{}'", "a".repeat(1100)),
            "SELECT * FROM t1 WHERE id = 3".to_owned(),
            "SELECT * FROM t1 WHERE rowid = 2".to_owned(),
            "SELECT t1.id, t2.rid, t3.x FROM t1 LEFT JOIN t2 ON t1.a = t2.a INNER JOIN t3 ON t1.b = t3.x".to_owned(),
            "SELECT * FROM t1 LEFT JOIN t2 ON t1.a = t2.a AND t2.c > 0 LEFT JOIN t3 ON t2.c = t3.x WHERE t1.b IS NOT NULL".to_owned(),
            "SELECT t1.a, count(*), count(t2.c), sum(t2.c) FROM t1 LEFT JOIN t2 ON t1.a = t2.a GROUP BY t1.a".to_owned(),
        ];

        for _ in 0..18 {
            let v1 = rand_int_sql(r);
            let v2 = rand_int_sql(r);
            let sv = rand_text_sql(r);
            let op = *r.pick(&["=", "<", "<=", ">", ">="]);
            let join_kw = *r.pick(&["INNER JOIN", "LEFT JOIN"]);

            queries.push(format!("SELECT * FROM t1 WHERE a {op} {v1}"));
            queries.push(format!("SELECT * FROM t1 WHERE a = {v1} AND b {op} {v2}"));
            queries.push(format!("SELECT * FROM t1 WHERE a BETWEEN {v1} AND {v2}"));
            queries.push(format!("SELECT * FROM t1 WHERE s {op} {sv}"));
            queries.push(format!(
                "SELECT t1.id, t2.rid, t2.c FROM t1 {join_kw} t2 ON t1.a = t2.a AND t2.c {op} {v1} WHERE t1.b {op} {v2}"
            ));
            queries.push(format!(
                "SELECT * FROM t1 {join_kw} t2 ON t1.a = t2.a AND t2.c BETWEEN {v1} AND {v2}"
            ));
        }

        for q in queries {
            let ri = exec_ok(&mut db_idx, &q);
            let rp = exec_ok(&mut db_plain, &q);
            assert_eq!(ri.columns, rp.columns, "column mismatch on `{q}`");
            assert_eq!(
                sorted_rows(ri.rows),
                sorted_rows(rp.rows),
                "row mismatch on `{q}`"
            );
        }
    });
}

fn rand_int_sql(r: &mut Rng) -> String {
    if r.chance(1, 6) {
        "NULL".to_owned()
    } else {
        let choices = [-10, -1, 0, 1, 2, 3, 4, 5, 10];
        choices[r.below(choices.len())].to_string()
    }
}

fn rand_text_sql(r: &mut Rng) -> String {
    if r.chance(1, 6) {
        "NULL".to_owned()
    } else {
        let choices = ["''", "'a'", "'ab'", "'b'", "'z'"];
        (*r.pick(&choices)).to_owned()
    }
}
