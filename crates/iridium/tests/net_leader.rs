//! Leader check for T42: its own raw-socket client and codec; concurrent clients checked against
//! per-client models; transactions blocking, killed mid-way and timing out; restart; hostile frames.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]
use assay::Rng;
use iridium::net::{decode_rows, encode_rows, Server};
use iridium::sql::{Rows, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn files(tag: &str) -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("iridium-net-leader-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    (d.join("main.db"), d.join("main.log"))
}

fn start(f: &(PathBuf, PathBuf)) -> Server {
    Server::start("127.0.0.1:0".parse().unwrap(), f.0.clone(), f.1.clone()).unwrap()
}

// ---- independent codec ----
fn enc(r: &Rows) -> Vec<u8> {
    let mut b = r.changes.to_be_bytes().to_vec();
    b.extend((r.columns.len() as u32).to_be_bytes());
    for c in &r.columns {
        b.extend((c.len() as u32).to_be_bytes());
        b.extend(c.as_bytes());
    }
    b.extend((r.rows.len() as u32).to_be_bytes());
    for row in &r.rows {
        for v in row {
            match v {
                Value::Null => b.push(0),
                Value::Int(i) => {
                    b.push(1);
                    b.extend(i.to_be_bytes());
                }
                Value::Text(t) => {
                    b.push(2);
                    b.extend((t.len() as u32).to_be_bytes());
                    b.extend(t.as_bytes());
                }
                Value::Blob(x) => {
                    b.push(3);
                    b.extend((x.len() as u32).to_be_bytes());
                    b.extend(x);
                }
            }
        }
    }
    b
}

struct Cur<'a>(&'a [u8]);
impl Cur<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u32(&mut self) -> Option<usize> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
}

fn dec(b: &[u8]) -> Option<Rows> {
    let mut c = Cur(b);
    let changes = u64::from_be_bytes(c.take(8)?.try_into().ok()?);
    let nc = c.u32()?;
    let mut columns = Vec::new();
    for _ in 0..nc {
        let n = c.u32()?;
        columns.push(String::from_utf8(c.take(n)?.to_vec()).ok()?);
    }
    let nr = c.u32()?;
    if nc == 0 && nr > 0 {
        return None; // worker reading on a silent point: rows need at least one column
    }
    let mut rows = Vec::new();
    for _ in 0..nr {
        let mut row = Vec::new();
        for _ in 0..nc {
            row.push(match c.take(1)?[0] {
                0 => Value::Null,
                1 => Value::Int(i64::from_be_bytes(c.take(8)?.try_into().ok()?)),
                2 => {
                    let n = c.u32()?;
                    Value::Text(String::from_utf8(c.take(n)?.to_vec()).ok()?)
                }
                3 => {
                    let n = c.u32()?;
                    Value::Blob(c.take(n)?.to_vec())
                }
                _ => return None,
            });
        }
        rows.push(row);
    }
    c.0.is_empty().then_some(Rows {
        columns,
        rows,
        changes,
    })
}

// ---- raw client ----
struct Raw(TcpStream);
impl Raw {
    fn new(a: SocketAddr) -> Raw {
        let s = TcpStream::connect(a).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        Raw(s)
    }
    fn send(&mut self, payload: &[u8]) {
        let mut f = (payload.len() as u32).to_be_bytes().to_vec();
        f.extend(payload);
        self.0.write_all(&f).unwrap();
    }
    fn recv(&mut self) -> Option<Result<Rows, String>> {
        let mut len = [0u8; 4];
        self.0.read_exact(&mut len).ok()?;
        let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
        self.0.read_exact(&mut body).ok()?;
        match body.split_first()? {
            (0, rest) => Some(Ok(dec(rest).expect("server reply does not decode"))),
            (1, rest) => Some(Err(String::from_utf8_lossy(rest).into_owned())),
            _ => panic!("bad reply tag"),
        }
    }
    fn q(&mut self, sql: &str) -> Result<Rows, String> {
        self.send(sql.as_bytes());
        self.recv().expect("connection closed")
    }
    fn ok(&mut self, sql: &str) -> Rows {
        self.q(sql).unwrap_or_else(|e| panic!("{sql}: {e}"))
    }
}

fn count(c: &mut Raw, t: &str) -> i64 {
    match c.ok(&format!("SELECT count(*) FROM {t}")).rows[0][0] {
        Value::Int(n) => n,
        ref v => panic!("{v:?}"),
    }
}

#[test]
fn codec_matches_independent_one() {
    let mut r = Rng::new(0x42_0001);
    for _ in 0..500 {
        let nc = r.below(4);
        let rows = Rows {
            columns: (0..nc).map(|i| format!("c{i}\u{e9}")).collect(),
            rows: (0..if nc == 0 { 0 } else { r.below(5) })
                .map(|_| {
                    (0..nc)
                        .map(|_| match r.below(4) {
                            0 => Value::Null,
                            1 => Value::Int(r.u64() as i64),
                            2 => Value::Text("t\u{4e2d}".repeat(r.below(3))),
                            _ => Value::Blob(r.bytes(6)),
                        })
                        .collect()
                })
                .collect(),
            changes: r.u64(),
        };
        let b = enc(&rows);
        assert_eq!(encode_rows(&rows), b);
        assert_eq!(decode_rows(&b), Some(rows.clone()));
        let mut extra = b.clone();
        extra.push(0);
        assert_eq!(decode_rows(&extra), None, "trailing byte accepted");
        if !b.is_empty() {
            assert_eq!(
                decode_rows(&b[..r.below(b.len())]),
                None,
                "short input accepted"
            );
        }
        let junk = r.bytes(40);
        assert_eq!(decode_rows(&junk), dec(&junk));
    }
}

#[test]
fn concurrent_clients_match_models() {
    let f = files("conc");
    let srv = start(&f);
    let addr = srv.local_addr();
    Raw::new(addr).ok("CREATE TABLE shared (id INTEGER PRIMARY KEY, who INTEGER)");
    let threads: Vec<_> = (0..8u64)
        .map(|k| {
            std::thread::spawn(move || {
                let mut r = Rng::new(0x42_1000 + k);
                let mut c = Raw::new(addr);
                let t = format!("t{k}");
                c.ok(&format!(
                    "CREATE TABLE {t} (id INTEGER PRIMARY KEY, v INTEGER, s TEXT)"
                ));
                let mut model: BTreeMap<i64, (i64, String)> = BTreeMap::new();
                let mut mine = 0;
                for step in 0..150 {
                    match r.below(6) {
                        0 | 1 => {
                            let (v, s) = (r.below(1000) as i64, format!("s{}", r.below(50)));
                            let got = c.ok(&format!("INSERT INTO {t} (v, s) VALUES ({v}, '{s}')"));
                            assert_eq!(got.changes, 1);
                            let id = model.keys().next_back().map_or(1, |m| m + 1);
                            model.insert(id, (v, s));
                        }
                        2 => {
                            let id = r.below(40) as i64;
                            let got = c.ok(&format!("UPDATE {t} SET v = v + 1 WHERE id = {id}"));
                            let hit = model.get_mut(&id).map(|e| e.0 += 1).is_some();
                            assert_eq!(got.changes, u64::from(hit), "client {k} step {step}");
                        }
                        3 => {
                            c.ok(&format!("INSERT INTO shared (who) VALUES ({k})"));
                            mine += 1;
                        }
                        _ => {
                            let got = c.ok(&format!("SELECT id, v, s FROM {t} ORDER BY id"));
                            let want: Vec<Vec<Value>> = model
                                .iter()
                                .map(|(&id, (v, s))| {
                                    vec![Value::Int(id), Value::Int(*v), Value::Text(s.clone())]
                                })
                                .collect();
                            assert_eq!(got.rows, want, "client {k} step {step}");
                            assert_eq!(got.columns, ["id", "v", "s"]);
                        }
                    }
                }
                mine
            })
        })
        .collect();
    let total: i64 = threads.into_iter().map(|h| h.join().unwrap()).sum();
    let mut c = Raw::new(addr);
    assert_eq!(count(&mut c, "shared"), total);
    drop(c);
    srv.stop();
    let srv = start(&f);
    assert_eq!(
        count(&mut Raw::new(srv.local_addr()), "shared"),
        total,
        "restart lost data"
    );
    srv.stop();
}

#[test]
fn transactions_block_roll_back_and_time_out() {
    let f = files("tx");
    let srv = start(&f);
    let addr = srv.local_addr();
    let mut a = Raw::new(addr);
    a.ok("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER)");
    // commit releases a waiter, which then sees the rows
    a.ok("BEGIN");
    a.ok("INSERT INTO t (v) VALUES (1)");
    let waiter = std::thread::spawn(move || {
        let t0 = Instant::now();
        let n = count(&mut Raw::new(addr), "t");
        (n, t0.elapsed())
    });
    std::thread::sleep(Duration::from_millis(500));
    a.ok("INSERT INTO t (v) VALUES (2)");
    a.ok("COMMIT");
    let (n, waited) = waiter.join().unwrap();
    assert_eq!(n, 2, "reader saw uncommitted or partial state");
    assert!(
        waited >= Duration::from_millis(400),
        "reader did not wait: {waited:?}"
    );
    // a client killed inside a transaction is rolled back
    for _ in 0..3 {
        let mut k = Raw::new(addr);
        k.ok("BEGIN");
        k.ok("INSERT INTO t (v) VALUES (9)");
        drop(k);
        assert_eq!(count(&mut a, "t"), 2, "killed transaction not rolled back");
    }
    // a holder longer than 2 s makes others get `busy`
    a.ok("BEGIN");
    let t0 = Instant::now();
    let got = Raw::new(addr).q("SELECT count(*) FROM t");
    let el = t0.elapsed();
    assert_eq!(got, Err("busy".to_string()));
    assert!(
        el >= Duration::from_millis(1500) && el < Duration::from_secs(6),
        "busy after {el:?}"
    );
    a.ok("ROLLBACK");
    // stop with an open transaction rolls it back
    a.ok("BEGIN");
    a.ok("INSERT INTO t (v) VALUES (7)");
    srv.stop();
    drop(a);
    let srv = start(&f);
    assert_eq!(count(&mut Raw::new(srv.local_addr()), "t"), 2);
    srv.stop();
}

#[test]
fn hostile_frames() {
    let f = files("hostile");
    let srv = start(&f);
    let addr = srv.local_addr();
    let mut r = Rng::new(0x42_0002);
    let mut good = Raw::new(addr);
    good.ok("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER)");
    // invalid UTF-8 gets an error reply on a live connection
    let mut c = Raw::new(addr);
    c.send(&[0xff, 0xfe, b'x']);
    assert!(matches!(c.recv(), Some(Err(_))));
    assert!(
        c.q("SELECT count(*) FROM t").is_ok(),
        "connection died after a bad statement"
    );
    // oversize frame: connection closed without reply
    let mut c = Raw::new(addr);
    c.0.write_all(&(2u32 << 20).to_be_bytes()).unwrap();
    let _ = c.0.write_all(&[b' '; 64]);
    assert!(c.recv().is_none(), "oversize frame got a reply");
    // random garbage connections, truncated frames, abrupt closes
    for _ in 0..60 {
        let mut s = TcpStream::connect(addr).unwrap();
        let mut junk = r.bytes(64);
        if r.chance(1, 2) {
            let mut f = 30u32.to_be_bytes().to_vec();
            f.extend(&junk[..junk.len().min(10)]);
            junk = f;
        }
        let _ = s.write_all(&junk);
        drop(s);
    }
    // the server still serves everyone
    for i in 0..5 {
        good.ok(&format!("INSERT INTO t (v) VALUES ({i})"));
    }
    assert_eq!(count(&mut Raw::new(addr), "t"), 5);
    let c = iridium::net::Client::connect(addr).unwrap();
    let mut c = c;
    let got = c.execute("SELECT v FROM t ORDER BY v").unwrap().unwrap();
    assert_eq!(got.rows.len(), 5);
    assert!(c.execute("SELECT nope FROM t").unwrap().is_err());
    srv.stop();
}
