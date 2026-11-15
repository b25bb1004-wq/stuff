//! Integration tests for Iridium TCP server, client, wire protocol, and CLI.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use iridium::net::{decode_rows, encode_rows, Client, Server};
use iridium::sql::Value;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct TempDb {
    dir: PathBuf,
    main: PathBuf,
    log: PathBuf,
}

impl TempDb {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "varve_net_test_{name}_{}_{}",
            std::process::id(),
            id
        ));
        fs::create_dir_all(&dir).unwrap();
        let main = dir.join("db.main");
        let log = dir.join("db.wal");
        Self { dir, main, log }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn localhost_zero() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn round_trip_every_value_type() {
    let tmp = TempDb::new("types");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let mut cli = Client::connect(srv.local_addr()).unwrap();

    cli.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER, s TEXT, b BLOB)")
        .unwrap()
        .unwrap();
    let ins = cli
        .execute("INSERT INTO t VALUES (1, NULL, 'hello world', x'deadbeef'), (2, -999, '', x'')")
        .unwrap()
        .unwrap();
    assert_eq!(ins.changes, 2);

    let sel = cli
        .execute("SELECT id, n, s, b FROM t ORDER BY id")
        .unwrap()
        .unwrap();
    assert_eq!(sel.columns, vec!["id", "n", "s", "b"]);
    assert_eq!(
        sel.rows,
        vec![
            vec![
                Value::Int(1),
                Value::Null,
                Value::Text("hello world".into()),
                Value::Blob(vec![0xde, 0xad, 0xbe, 0xef]),
            ],
            vec![
                Value::Int(2),
                Value::Int(-999),
                Value::Text(String::new()),
                Value::Blob(Vec::new()),
            ],
        ]
    );
    assert_eq!(decode_rows(&encode_rows(&sel)), Some(sel));

    // Invalid SQL returns server error, connection stays usable.
    let err = cli.execute("SELECT FROM").unwrap().unwrap_err();
    assert!(!err.is_empty());
    let ok = cli.execute("SELECT count(*) FROM t").unwrap().unwrap();
    assert_eq!(ok.rows, vec![vec![Value::Int(2)]]);

    srv.stop();
}

#[test]
fn eight_concurrent_clients_insert_and_count() {
    let tmp = TempDb::new("concurrent8");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    {
        let mut init = Client::connect(addr).unwrap();
        init.execute("CREATE TABLE items (worker INTEGER, seq INTEGER)")
            .unwrap()
            .unwrap();
    }

    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for w in 0..8i64 {
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let mut cli = Client::connect(addr).unwrap();
            b.wait();
            for s in 0..15i64 {
                let sql = format!("INSERT INTO items VALUES ({w}, {s})");
                let res = cli.execute(&sql).unwrap().unwrap();
                assert_eq!(res.changes, 1);
            }
            let cnt = cli.execute("SELECT count(*) FROM items").unwrap().unwrap();
            assert!(!cnt.rows.is_empty());
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let mut check = Client::connect(addr).unwrap();
    let total = check
        .execute("SELECT count(*) FROM items")
        .unwrap()
        .unwrap();
    assert_eq!(total.rows, vec![vec![Value::Int(120)]]);

    srv.stop();
}

#[test]
fn transaction_isolation_waits_and_sees_commit() {
    let tmp = TempDb::new("tx_iso");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    let mut c1 = Client::connect(addr).unwrap();
    c1.execute("CREATE TABLE acct (id INTEGER PRIMARY KEY, bal INTEGER)")
        .unwrap()
        .unwrap();
    c1.execute("INSERT INTO acct VALUES (1, 100)")
        .unwrap()
        .unwrap();

    c1.execute("BEGIN").unwrap().unwrap();
    c1.execute("UPDATE acct SET bal = 250 WHERE id = 1")
        .unwrap()
        .unwrap();

    let c2_handle = thread::spawn(move || {
        let mut c2 = Client::connect(addr).unwrap();
        c2.execute("SELECT bal FROM acct WHERE id = 1")
            .unwrap()
            .unwrap()
    });

    thread::sleep(Duration::from_millis(250));
    c1.execute("COMMIT").unwrap().unwrap();

    let rows = c2_handle.join().unwrap();
    assert_eq!(rows.rows, vec![vec![Value::Int(250)]]);

    srv.stop();
}

#[test]
fn rollback_on_disconnect() {
    let tmp = TempDb::new("rollback_disc");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    {
        let mut c1 = Client::connect(addr).unwrap();
        c1.execute("CREATE TABLE t (x INTEGER)").unwrap().unwrap();
        c1.execute("INSERT INTO t VALUES (1)").unwrap().unwrap();
        c1.execute("BEGIN").unwrap().unwrap();
        c1.execute("INSERT INTO t VALUES (2)").unwrap().unwrap();
        // Drop c1 without COMMIT or ROLLBACK
    }

    let mut c2 = Client::connect(addr).unwrap();
    let res = c2.execute("SELECT x FROM t ORDER BY x").unwrap().unwrap();
    assert_eq!(res.rows, vec![vec![Value::Int(1)]]);

    srv.stop();
}

#[test]
fn busy_after_two_seconds() {
    let tmp = TempDb::new("busy");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    let mut c1 = Client::connect(addr).unwrap();
    c1.execute("CREATE TABLE t (x INTEGER)").unwrap().unwrap();
    c1.execute("BEGIN").unwrap().unwrap();
    c1.execute("INSERT INTO t VALUES (10)").unwrap().unwrap();

    let mut c2 = Client::connect(addr).unwrap();
    let start = Instant::now();
    let err = c2.execute("SELECT * FROM t").unwrap().unwrap_err();
    let elapsed = start.elapsed();

    assert_eq!(err, "busy");
    assert!(elapsed >= Duration::from_millis(1900));

    c1.execute("COMMIT").unwrap().unwrap();
    let rows = c2.execute("SELECT x FROM t").unwrap().unwrap();
    assert_eq!(rows.rows, vec![vec![Value::Int(10)]]);

    srv.stop();
}

#[test]
fn restart_keeps_data_and_stop_rolls_back_open_tx() {
    let tmp = TempDb::new("restart");

    {
        let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
        let mut c1 = Client::connect(srv.local_addr()).unwrap();
        c1.execute("CREATE TABLE t (x INTEGER)").unwrap().unwrap();
        c1.execute("INSERT INTO t VALUES (42)").unwrap().unwrap();
        c1.execute("BEGIN").unwrap().unwrap();
        c1.execute("INSERT INTO t VALUES (99)").unwrap().unwrap();
        // Stop while c1 has an uncommitted transaction.
        srv.stop();
    }

    let srv2 = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let mut c2 = Client::connect(srv2.local_addr()).unwrap();
    let rows = c2.execute("SELECT x FROM t ORDER BY x").unwrap().unwrap();
    assert_eq!(rows.rows, vec![vec![Value::Int(42)]]);
    srv2.stop();
}

#[test]
fn oversize_and_truncated_frames_close_without_reply() {
    let tmp = TempDb::new("hostile_frames");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    // 1. Oversize frame (> 1 MiB)
    {
        let mut raw = TcpStream::connect(addr).unwrap();
        let too_big: u32 = (1024 * 1024) + 1;
        raw.write_all(&too_big.to_be_bytes()).unwrap();
        raw.flush().unwrap();
        let mut buf = [0u8; 16];
        let n = raw.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    // 2. Truncated frame header
    {
        let mut raw = TcpStream::connect(addr).unwrap();
        raw.write_all(&[0x00, 0x00]).unwrap();
        raw.flush().unwrap();
        raw.shutdown(std::net::Shutdown::Write).unwrap();
        let mut buf = [0u8; 16];
        let n = raw.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    // 3. Truncated frame body
    {
        let mut raw = TcpStream::connect(addr).unwrap();
        raw.write_all(&10u32.to_be_bytes()).unwrap();
        raw.write_all(b"SELECT").unwrap();
        raw.flush().unwrap();
        raw.shutdown(std::net::Shutdown::Write).unwrap();
        let mut buf = [0u8; 16];
        let n = raw.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    // 4. Invalid UTF-8 frame gets an error reply (0x01)
    {
        let mut raw = TcpStream::connect(addr).unwrap();
        let bad_utf8 = [0xffu8, 0xfe];
        raw.write_all(&(bad_utf8.len() as u32).to_be_bytes())
            .unwrap();
        raw.write_all(&bad_utf8).unwrap();
        raw.flush().unwrap();
        let mut len_buf = [0u8; 4];
        raw.read_exact(&mut len_buf).unwrap();
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut reply = vec![0u8; len];
        raw.read_exact(&mut reply).unwrap();
        assert_eq!(reply.first().copied(), Some(0x01));
    }

    // Server still healthy after hostile clients.
    let mut cli = Client::connect(addr).unwrap();
    let res = cli.execute("SELECT 1 + 1").unwrap().unwrap();
    assert_eq!(res.rows, vec![vec![Value::Int(2)]]);

    srv.stop();
}

#[test]
fn cli_executes_script() {
    let tmp = TempDb::new("cli");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr_str = srv.local_addr().to_string();

    let bin = env!("CARGO_BIN_EXE_iridium-cli");
    let mut child = Command::new(bin)
        .arg(&addr_str)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let script = concat!(
        "CREATE TABLE t (a INTEGER, b TEXT, c BLOB)\n",
        "\n",
        "INSERT INTO t VALUES (1, 'hi', x'0a0b'), (NULL, 'lo', x'')\n",
        "SELECT a, b, c FROM t\n",
        "SELECT * FROM missing_table\n",
        "SELECT 7 * 6\n",
    );

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(script.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = concat!(
        "ok 0\n",
        "ok 2\n",
        "1\thi\tx'0a0b'\n",
        "NULL\tlo\tx''\n",
        "ok 0\n",
        "error: no such table or index\n",
        "42\n",
        "ok 0\n",
    );
    assert_eq!(stdout, expected);

    srv.stop();
}
