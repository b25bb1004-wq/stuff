//! REAL columns, decimal literals, mixed INTEGER/REAL rules, AVG, and files written without REAL.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use iridium::sql::{Db, Value};
use iridium::{Error, FileStorage, MemStorage};

fn mem_db() -> Db<MemStorage> {
    Db::open(MemStorage::new(), MemStorage::new()).unwrap()
}

/// First column of every row, in result order.
fn column(db: &mut Db<MemStorage>, sql: &str) -> Vec<Value> {
    db.execute(sql)
        .unwrap()
        .rows
        .into_iter()
        .map(|mut r| r.remove(0))
        .collect()
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "iridium-real-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_file_db(main: &Path, log: &Path) -> Db<FileStorage> {
    Db::open(
        FileStorage::open(main).unwrap(),
        FileStorage::open(log).unwrap(),
    )
    .unwrap()
}

#[test]
fn real_column_stores_decimals_and_orders_numerically() {
    let mut db = mem_db();
    db.execute("CREATE TABLE t (x REAL, k INTEGER)").unwrap();
    db.execute("INSERT INTO t VALUES (1.5, 1), (-0.25, 2), (1e3, 3), (2, 4), (NULL, 5)")
        .unwrap();
    assert_eq!(
        column(&mut db, "SELECT x FROM t ORDER BY x"),
        vec![
            Value::Null,
            Value::Real(-0.25),
            Value::Real(1.5),
            Value::Real(2.0),
            Value::Real(1000.0),
        ]
    );
    assert_eq!(
        column(&mut db, "SELECT x FROM t WHERE x > 1 ORDER BY x"),
        vec![Value::Real(1.5), Value::Real(2.0), Value::Real(1000.0)]
    );
}

#[test]
fn float_type_aliases_are_real() {
    let mut db = mem_db();
    db.execute("CREATE TABLE a (x FLOAT, y DOUBLE, z NUMERIC)")
        .unwrap();
    db.execute("INSERT INTO a VALUES (1, 2.5, 3)").unwrap();
    assert_eq!(column(&mut db, "SELECT x FROM a"), vec![Value::Real(1.0)]);
    assert_eq!(column(&mut db, "SELECT z FROM a"), vec![Value::Real(3.0)]);
    assert!(db.execute("INSERT INTO a VALUES ('abc', 1, 1)").is_err());
}

#[test]
fn integer_and_real_compare_numerically() {
    let mut db = mem_db();
    db.execute("CREATE TABLE m (i INTEGER)").unwrap();
    db.execute("INSERT INTO m VALUES (1), (2), (3)").unwrap();
    assert_eq!(
        column(&mut db, "SELECT i FROM m WHERE i > 1.5 ORDER BY i"),
        vec![Value::Int(2), Value::Int(3)]
    );
    assert_eq!(
        column(&mut db, "SELECT i FROM m WHERE i = 2.0"),
        vec![Value::Int(2)]
    );
}

#[test]
fn mixed_arithmetic_follows_sqlite_rules() {
    let mut db = mem_db();
    assert_eq!(column(&mut db, "SELECT 1 + 0.5"), vec![Value::Real(1.5)]);
    assert_eq!(column(&mut db, "SELECT 1.5 * 2"), vec![Value::Real(3.0)]);
    assert_eq!(column(&mut db, "SELECT 7 / 2"), vec![Value::Int(3)]);
    assert_eq!(column(&mut db, "SELECT 7 / 2.0"), vec![Value::Real(3.5)]);
    assert_eq!(column(&mut db, "SELECT 1.0 / 0"), vec![Value::Null]);
    assert_eq!(column(&mut db, "SELECT -0.25"), vec![Value::Real(-0.25)]);
    assert!(db.execute("SELECT 1e").is_err());
}

#[test]
fn avg_and_sum_over_groups() {
    let mut db = mem_db();
    db.execute("CREATE TABLE g (k TEXT, v INTEGER, r REAL)")
        .unwrap();
    db.execute(
        "INSERT INTO g VALUES ('a', 1, 1.5), ('a', 2, NULL), ('b', NULL, NULL), ('c', 4, 2.0)",
    )
    .unwrap();
    assert_eq!(
        column(&mut db, "SELECT AVG(v) FROM g"),
        vec![Value::Real(7.0 / 3.0)]
    );
    assert_eq!(
        column(&mut db, "SELECT AVG(v) FROM g WHERE v > 100"),
        vec![Value::Null]
    );
    assert_eq!(column(&mut db, "SELECT SUM(v) FROM g"), vec![Value::Int(7)]);
    assert_eq!(
        column(&mut db, "SELECT SUM(r) FROM g"),
        vec![Value::Real(3.5)]
    );
    let grouped = db
        .execute("SELECT k, AVG(r) FROM g GROUP BY k ORDER BY k")
        .unwrap()
        .rows;
    assert_eq!(
        grouped,
        vec![
            vec![Value::Text("a".into()), Value::Real(1.5)],
            vec![Value::Text("b".into()), Value::Null],
            vec![Value::Text("c".into()), Value::Real(2.0)],
        ]
    );
}

#[test]
fn real_index_probes_find_the_same_rows_as_a_scan() {
    let mut db = mem_db();
    db.execute("CREATE TABLE p (x REAL, n INTEGER)").unwrap();
    db.execute("CREATE INDEX px ON p (x)").unwrap();
    db.execute("INSERT INTO p VALUES (1.5, 1), (2, 2), (2.5, 3), (-1, 4)")
        .unwrap();
    assert_eq!(
        column(&mut db, "SELECT n FROM p WHERE x = 2"),
        vec![Value::Int(2)]
    );
    assert_eq!(
        column(
            &mut db,
            "SELECT n FROM p WHERE x BETWEEN 1 AND 2.5 ORDER BY n"
        ),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    assert_eq!(
        column(&mut db, "SELECT n FROM p WHERE x < 0 ORDER BY n"),
        vec![Value::Int(4)]
    );
}

#[test]
fn real_values_survive_reopen() {
    let dir = scratch_dir("reopen");
    let (main, log) = (dir.join("r.db"), dir.join("r.db-log"));
    {
        let mut db = open_file_db(&main, &log);
        db.execute("CREATE TABLE t (x REAL)").unwrap();
        db.execute("INSERT INTO t VALUES (0.1), (-2.5e-3), (7)")
            .unwrap();
        db.close().unwrap();
    }
    let mut db = open_file_db(&main, &log);
    let got = db
        .execute("SELECT x FROM t ORDER BY x")
        .unwrap()
        .rows
        .into_iter()
        .map(|mut r| r.remove(0))
        .collect::<Vec<_>>();
    assert_eq!(
        got,
        vec![Value::Real(-2.5e-3), Value::Real(0.1), Value::Real(7.0)]
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn database_with_only_integer_and_text_columns_still_opens() {
    // Written by the current code path with no REAL column; must open and read unchanged.
    let dir = scratch_dir("compat");
    let (main, log) = (dir.join("c.db"), dir.join("c.db-log"));
    {
        let mut db = open_file_db(&main, &log);
        db.execute("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)")
            .unwrap();
        db.execute("CREATE INDEX people_age ON people (age)")
            .unwrap();
        db.execute("INSERT INTO people (name, age) VALUES ('bob', 40), ('ann', 31)")
            .unwrap();
        db.close().unwrap();
    }
    let mut db = open_file_db(&main, &log);
    let names = db
        .execute("SELECT name FROM people WHERE age > 30 ORDER BY name")
        .unwrap()
        .rows;
    assert_eq!(
        names,
        vec![
            vec![Value::Text("ann".into())],
            vec![Value::Text("bob".into())]
        ]
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn checked_in_legacy_file_opens_and_reads() {
    // tests/data/legacy_int_text.* hold a database with only INTEGER and TEXT columns.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let dir = scratch_dir("legacy");
    let (main, log) = (dir.join("legacy.db"), dir.join("legacy.db-log"));
    fs::copy(src.join("legacy_int_text.main"), &main).unwrap();
    fs::copy(src.join("legacy_int_text.log"), &log).unwrap();
    let mut db = open_file_db(&main, &log);
    let names = db
        .execute("SELECT name FROM people ORDER BY name")
        .unwrap()
        .rows;
    assert_eq!(
        names,
        vec![
            vec![Value::Text("ann".into())],
            vec![Value::Text("bob".into())]
        ]
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn record_with_nan_is_rejected_as_corrupt() {
    // A NaN can never be stored; decoding one must fail rather than panic.
    let bytes = iridium::sql::encode_record(&[Value::Real(1.0)]);
    let mut bad = bytes.clone();
    let n = bad.len();
    bad[n - 8..].copy_from_slice(&f64::NAN.to_bits().to_be_bytes());
    assert!(matches!(
        iridium::sql::decode_record(&bad),
        Err(Error::Corrupt)
    ));
}
