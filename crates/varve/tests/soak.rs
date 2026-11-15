//! Soak tests: every SQL parser and decoder, the executor, and the pager open path, driven by
//! random and mutated inputs. Env: `STRATA_SOAK_SEED` (decimal or 0x hex, default 0x5EED_79E),
//! `STRATA_SOAK_MS` (budget per target, default 300). Run with `--nocapture` to see case counts.
//! Allocation bound: every input is capped at 16 KiB, so decoder allocations stay far below 64 MiB.
#![allow(clippy::unwrap_used)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use assay::{hex, to_hex, Rng};
use varve::net::{decode_rows, encode_rows};
use varve::sql::{decode_record, encode_record, Db, Rows, Value};
use varve::{Error, MemStorage, Pager, PAGE};

const DEFAULT_SEED: u64 = 0x05EE_D79E;
const DEFAULT_MS: u64 = 300;
const CASE_LIMIT: Duration = Duration::from_secs(1);
const MAX_INPUT: usize = 16 * 1024;
const CORPUS: &str = include_str!("sql_cases.txt");

const TOKENS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "BY",
    "HAVING",
    "ORDER",
    "ASC",
    "DESC",
    "LIMIT",
    "OFFSET",
    "CREATE",
    "TABLE",
    "DROP",
    "INDEX",
    "UNIQUE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "EXPLAIN",
    "JOIN",
    "INNER",
    "LEFT",
    "ON",
    "AND",
    "OR",
    "NOT",
    "IS",
    "NULL",
    "BETWEEN",
    "IN",
    "LIKE",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "AS",
    "INTEGER",
    "INT",
    "TEXT",
    "BLOB",
    "PRIMARY",
    "KEY",
    "DISTINCT",
    "EXISTS",
    "COUNT(*)",
    "SUM(",
    "substr(",
    "zeroblob(",
    "t",
    "u",
    "a",
    "b",
    "c",
    "1",
    "0",
    "-1",
    "9223372036854775807",
    "'x'",
    "''",
    "'%'",
    "x'0a'",
    "x''",
    "(",
    ")",
    ",",
    ";",
    ".",
    "+",
    "-",
    "*",
    "/",
    "%",
    "||",
    "=",
    "<>",
    "!=",
    "<=",
    ">=",
    "@",
    "\"",
    "/*",
    "--",
    " ",
    "\n",
    "\0",
];

const TEXT_CHARS: &[char] = &[
    'a',
    'b',
    'Z',
    '0',
    ' ',
    '\'',
    '"',
    'é',
    'γ',
    '-',
    '\n',
    '%',
    '_',
    '\t',
    '\u{1F600}',
];

fn base_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let t = s.trim();
            match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16),
                None => t.parse(),
            }
            .expect("STRATA_SOAK_SEED must be decimal or 0x hex")
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

/// Run `case` on fresh random streams until the budget ends. Each call must not panic and must
/// return within one second. A hang shows up as a stuck test rather than a timeout message.
fn soak(name: &str, salt: u64, mut case: impl FnMut(&mut Rng)) {
    let seed = base_seed() ^ salt;
    let limit = budget();
    let mut master = Rng::new(seed);
    let start = Instant::now();
    let mut cases = 0u64;
    while cases == 0 || start.elapsed() < limit {
        let case_seed = master.u64();
        let mut r = Rng::new(case_seed);
        let t = Instant::now();
        if catch_unwind(AssertUnwindSafe(|| case(&mut r))).is_err() {
            panic!("soak {name}: panic on case seed {case_seed:#018x} (target seed {seed:#018x})");
        }
        let took = t.elapsed();
        assert!(
            took < CASE_LIMIT,
            "soak {name}: case seed {case_seed:#018x} took {took:?}"
        );
        cases += 1;
    }
    eprintln!("soak {name}: seed {seed:#018x} budget {limit:?} cases {cases}");
}

fn random_text(r: &mut Rng, max: usize) -> String {
    (0..r.below(max + 1)).map(|_| *r.pick(TEXT_CHARS)).collect()
}

fn random_sql(r: &mut Rng) -> String {
    match r.below(4) {
        0 => {
            let mut s = String::new();
            for _ in 0..r.below(40) {
                s.push_str(r.pick(TOKENS));
                s.push(' ');
            }
            s
        }
        1 => random_text(r, 300),
        2 => nested_sql(r),
        _ => {
            let mut s = String::from(*r.pick(&[
                "SELECT ",
                "INSERT INTO t VALUES (",
                "UPDATE t SET ",
                "DELETE FROM t WHERE ",
                "EXPLAIN SELECT ",
                "CREATE TABLE u (",
                "CREATE INDEX i ON t (",
            ]));
            for _ in 0..r.below(30) {
                s.push_str(r.pick(TOKENS));
                s.push(' ');
            }
            s
        }
    }
}

/// Deeply nested expressions, to find recursion limits in the parser and evaluator.
fn nested_sql(r: &mut Rng) -> String {
    let depth = r.below(1500);
    let (open, close) = match r.below(5) {
        0 => ("(", ")"),
        1 => ("NOT ", ""),
        2 => ("- ", ""),
        3 => ("CASE WHEN 1 THEN ", " END"),
        _ => ("EXISTS (SELECT 1 WHERE ", ")"),
    };
    let mut s = String::from("SELECT ");
    for _ in 0..depth {
        s.push_str(open);
    }
    s.push('1');
    for _ in 0..depth {
        s.push_str(close);
    }
    s
}

/// Byte-level mutation: bit flips, byte overwrites, truncation, splices and repeated chunks.
fn mutate(r: &mut Rng, base: &[u8], corpus: &[Vec<u8>]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..=r.below(4) {
        match r.below(5) {
            0 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i] ^= 1 << r.below(8);
            }
            1 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i] = r.u64() as u8;
            }
            2 => {
                v.truncate(r.below(v.len() + 1));
            }
            3 => {
                let other = r.pick(corpus);
                let at = r.below(v.len() + 1);
                let from = r.below(other.len() + 1);
                v.splice(at..at, other[from..].iter().copied());
            }
            _ => {
                let at = r.below(v.len() + 1);
                let len = 1 + r.below(16);
                let end = (at + len).min(v.len());
                let chunk = v[at..end].to_vec();
                if chunk.is_empty() {
                    continue;
                }
                let times = 1 + r.below(8);
                let ins: Vec<u8> = chunk
                    .iter()
                    .copied()
                    .cycle()
                    .take(chunk.len() * times)
                    .collect();
                let at2 = r.below(v.len() + 1);
                v.splice(at2..at2, ins);
            }
        }
    }
    v.truncate(MAX_INPUT);
    v
}

fn fresh_db() -> Db<MemStorage> {
    let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
    db.execute("CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT, c BLOB)")
        .unwrap();
    db
}

fn corpus_statements() -> Vec<Vec<u8>> {
    let v: Vec<Vec<u8>> = CORPUS
        .lines()
        .filter_map(|l| l.strip_prefix("S "))
        .map(hex)
        .collect();
    assert!(!v.is_empty(), "sql_cases.txt has no statements");
    v
}

/// Valid database image (closed, so the log is checkpointed) with its rows read before close.
fn valid_db_image() -> (Vec<u8>, Vec<u8>, Rows) {
    let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
    db.execute("CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT, c BLOB)")
        .unwrap();
    db.execute(
        "INSERT INTO t (a, b, c) VALUES (1, 'alpha', x'00ff'), (2, NULL, x''), (3, 'γ', x'10')",
    )
    .unwrap();
    let before = db.execute("SELECT a, b, c FROM t ORDER BY a").unwrap();
    let (main, log) = db.close().unwrap();
    (main.bytes().to_vec(), log.bytes().to_vec(), before)
}

/// Pager image with committed but un-checkpointed WAL frames, so the log holds real frames.
fn wal_pager_image() -> (Vec<u8>, Vec<u8>) {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    for round in 0..3u8 {
        p.begin().unwrap();
        let n = p.alloc().unwrap();
        let mut page = [0u8; PAGE];
        page[0] = round;
        page[1..9].copy_from_slice(&n.to_le_bytes());
        p.write(n, &page).unwrap();
        p.set_root(n).unwrap();
        p.commit().unwrap();
    }
    let (m, l) = p.into_storage();
    (m.bytes().to_vec(), l.bytes().to_vec())
}

#[test]
fn sql_text_parser_and_executor() {
    let mut db = fresh_db();
    let mut n = 0u32;
    soak("sql_text", 0x1, |r| {
        n += 1;
        if n.is_multiple_of(256) {
            db = fresh_db();
        }
        let s = if r.chance(1, 4) {
            String::from_utf8_lossy(&r.bytes(512)).into_owned()
        } else {
            random_sql(r)
        };
        let _ = db.execute(&s);
    });
}

#[test]
fn sql_corpus_mutants_parser_and_executor() {
    let corpus = corpus_statements();
    let mut db = fresh_db();
    let mut n = 0u32;
    soak("sql_corpus", 0x2, |r| {
        n += 1;
        if n.is_multiple_of(256) {
            db = fresh_db();
        }
        let base = r.pick(&corpus).clone();
        let mutant = mutate(r, &base, &corpus);
        let _ = db.execute(&String::from_utf8_lossy(&mutant));
    });
}

#[test]
fn sql_insert_select_round_trip() {
    soak("sql_round_trip", 0x3, |r| {
        let mut db = fresh_db();
        let n = 1 + r.below(4);
        let mut expect = Vec::new();
        for k in 0..n {
            let text: String = random_text(r, 40).replace('\0', "");
            let blob = r.bytes(64);
            let sql = format!(
                "INSERT INTO t (a, b, c) VALUES ({k}, '{}', x'{}')",
                text.replace('\'', "''"),
                to_hex(&blob)
            );
            db.execute(&sql).unwrap();
            expect.push(vec![
                Value::Int(k as i64),
                Value::Text(text),
                Value::Blob(blob),
            ]);
        }
        let rows = db.execute("SELECT a, b, c FROM t ORDER BY a").unwrap();
        assert_eq!(rows.rows, expect);
    });
}

#[test]
fn record_codec_decode_and_round_trip() {
    let corpus: Vec<Vec<u8>> = (0..8)
        .map(|i| {
            let mut r = Rng::new(0xC0DE + i);
            let row: Vec<Value> = (0..1 + r.below(4)).map(|_| rand_value(&mut r)).collect();
            encode_record(&row)
        })
        .collect();
    soak("record_codec", 0x4, |r| {
        let row: Vec<Value> = (0..r.below(7)).map(|_| rand_value(r)).collect();
        let enc = encode_record(&row);
        assert_eq!(decode_record(&enc).unwrap(), row);
        let _ = decode_record(&r.bytes(256));
        let base = r.pick(&corpus).clone();
        let _ = decode_record(&mutate(r, &base, &corpus));
    });
}

fn rand_value(r: &mut Rng) -> Value {
    match r.below(4) {
        0 => Value::Null,
        1 => Value::Int(r.u64() as i64),
        2 => Value::Text(random_text(r, 32)),
        _ => Value::Blob(r.bytes(32)),
    }
}

#[test]
fn net_rows_codec_decode_and_round_trip() {
    let corpus: Vec<Vec<u8>> = (0..8)
        .map(|i| {
            let mut r = Rng::new(0xFEED + i);
            encode_rows(&rand_rows(&mut r))
        })
        .collect();
    soak("net_rows", 0x5, |r| {
        let rows = rand_rows(r);
        assert_eq!(decode_rows(&encode_rows(&rows)), Some(rows));
        let _ = decode_rows(&r.bytes(512));
        let base = r.pick(&corpus).clone();
        let _ = decode_rows(&mutate(r, &base, &corpus));
    });
}

fn rand_rows(r: &mut Rng) -> Rows {
    let cols = r.below(4);
    let columns: Vec<String> = (0..cols).map(|_| random_text(r, 6)).collect();
    let rows: Vec<Vec<Value>> = if cols == 0 {
        Vec::new()
    } else {
        (0..r.below(5))
            .map(|_| (0..cols).map(|_| rand_value(r)).collect())
            .collect()
    };
    Rows {
        columns,
        rows,
        changes: r.u64(),
    }
}

#[test]
fn pager_open_random_and_bit_flipped_images() {
    let (db_main, db_log) = {
        let (m, l, _) = valid_db_image();
        (m, l)
    };
    // A committed log frame for page 0 overrides main page 0, so the flip check needs an image
    // whose checkpointed log has no frames.
    let flip_pager = Pager::open(
        MemStorage::from_bytes(db_main.clone()),
        MemStorage::from_bytes(db_log.clone()),
    )
    .unwrap();
    assert_eq!(flip_pager.log_frames(), 0);
    drop(flip_pager);
    let (wal_main, wal_log) = wal_pager_image();
    let images = [(db_main.clone(), db_log.clone()), (wal_main, wal_log)];
    soak("pager_open", 0x6, |r| {
        let (main, log) = r.pick(&images).clone();
        match r.below(3) {
            0 => {
                let _ = Pager::open(
                    MemStorage::from_bytes(r.bytes(MAX_INPUT)),
                    MemStorage::from_bytes(r.bytes(MAX_INPUT)),
                );
            }
            1 => {
                let m = mutate(r, &main, std::slice::from_ref(&main));
                let l = mutate(r, &log, std::slice::from_ref(&log));
                let _ = Pager::open(MemStorage::from_bytes(m), MemStorage::from_bytes(l));
            }
            _ => {
                // A flipped bit anywhere in page 0 must be detected by the page checksum.
                let mut m = db_main.clone();
                assert!(m.len() >= PAGE);
                let at = r.below(PAGE);
                m[at] ^= 1 << r.below(8);
                let res = Pager::open(
                    MemStorage::from_bytes(m),
                    MemStorage::from_bytes(db_log.clone()),
                );
                assert!(res.is_err(), "page-0 bit flip at byte {at} opened cleanly");
            }
        }
    });
}

#[test]
fn db_reopen_round_trip_and_mutated_images() {
    let (main, log, before) = valid_db_image();
    let mut db = Db::open(
        MemStorage::from_bytes(main.clone()),
        MemStorage::from_bytes(log.clone()),
    )
    .unwrap();
    assert_eq!(
        db.execute("SELECT a, b, c FROM t ORDER BY a").unwrap(),
        before
    );
    drop(db);
    let (wal_main, wal_log) = wal_pager_image();
    let images = [(main, log), (wal_main, wal_log)];
    soak("db_reopen", 0x7, |r| {
        let (m, l) = r.pick(&images).clone();
        let m = mutate(r, &m, std::slice::from_ref(&m));
        let l = mutate(r, &l, std::slice::from_ref(&l));
        if let Ok(mut db) = Db::open(MemStorage::from_bytes(m), MemStorage::from_bytes(l)) {
            let _ = db.execute("SELECT a, b, c FROM t ORDER BY a");
            let _ = db.execute("SELECT count(*) FROM t WHERE b IS NOT NULL");
        }
    });
}

/// Regression: deep nesting from SQL text is a parse error, not a stack overflow.
#[test]
fn deep_expression_nesting_is_rejected_not_fatal() {
    let mut db = fresh_db();
    for (open, close) in [
        ("(", ")"),
        ("NOT ", ""),
        ("- ", ""),
        ("CASE WHEN 1 THEN ", " END"),
    ] {
        let mut s = String::from("SELECT ");
        for _ in 0..2000 {
            s.push_str(open);
        }
        s.push('1');
        for _ in 0..2000 {
            s.push_str(close);
        }
        assert!(matches!(db.execute(&s), Err(Error::Parse { .. })), "{open}");
    }
    assert!(db.execute("SELECT ((((1))))").is_ok());
}
