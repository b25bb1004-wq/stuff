//! CLI integration tests for the `iridium` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use iridium::net::Server;

static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let id = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("iridium_cli_{tag}_{pid}_{id}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_iridium")
}

fn run_cli(args: &[&str], stdin_data: Option<&str>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(input) = stdin_data {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn help_and_version() {
    let help = run_cli(&["--help"], None);
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("sql "));
    assert!(stdout.contains("connect "));
    assert!(stdout.contains("iridium sql "));
    assert!(stdout.contains("iridium connect "));

    let ver = run_cli(&["--version"], None);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "iridium 0.1.0");
}

#[test]
fn unknown_subcommand_and_usage_errors() {
    let unknown = run_cli(&["bogus"], None);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("iridium:"));

    let empty = run_cli(&[], None);
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("iridium:"));

    let sql_missing = run_cli(&["sql"], None);
    assert_eq!(sql_missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&sql_missing.stderr).contains("iridium:"));

    let connect_missing = run_cli(&["connect"], None);
    assert_eq!(connect_missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&connect_missing.stderr).contains("iridium:"));
}

#[test]
fn sql_good_query_and_stdin() {
    let dir = TempDir::new("sql_good");
    let db = dir.file("test.idb");
    let db_str = db.to_str().unwrap();

    let out = run_cli(
        &[
            "sql",
            db_str,
            "CREATE TABLE items (id INT PRIMARY KEY, name TEXT, data BLOB)",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());

    let out = run_cli(
        &[
            "sql",
            db_str,
            "INSERT INTO items VALUES (1, 'alpha', x'0a0b'), (2, NULL, x'')",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));

    let out = run_cli(
        &[
            "sql",
            db_str,
            "SELECT id, name, data FROM items ORDER BY id",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "id\tname\tdata\n1\talpha\tx'0a0b'\n2\tNULL\tx''\n"
    );

    let out = run_cli(
        &["sql", db_str],
        Some("INSERT INTO items VALUES (3, 'beta', NULL);\nSELECT id, name FROM items WHERE id = 3;\n"),
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "id\tname\n3\tbeta\n");

    let out = run_cli(
        &["sql", db_str, "-"],
        Some("SELECT count(*) AS n FROM items\n"),
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "n\n3\n");
}

#[test]
fn sql_bad_input() {
    let dir = TempDir::new("sql_bad");
    let db = dir.file("test.idb");
    let db_str = db.to_str().unwrap();

    let out = run_cli(&["sql", db_str, "SELECT * FROM missing_table"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("iridium:"));

    let out = run_cli(&["sql", db_str, "-"], Some("NOT VALID SQL\n"));
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("iridium:"));

    let bad_path = dir.file("no_such_dir").join("bad.idb");
    let out = run_cli(&["sql", bad_path.to_str().unwrap(), "SELECT 1"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("iridium:"));
}

#[test]
fn connect_good_input() {
    let dir = TempDir::new("connect_good");
    let server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        dir.file("srv.idb"),
        dir.file("srv.wal"),
    )
    .unwrap();
    let addr = server.local_addr().to_string();

    let out = run_cli(
        &["connect", &addr],
        Some(
            "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)\n\
             INSERT INTO t VALUES (7, 'seven')\n\
             SELECT id, val FROM t\n\
             SELECT * FROM missing\n",
        ),
    );
    server.stop();

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ok 0\nok 1\n7\tseven\nok 0\nerror: no such table or index\n"
    );
}

#[test]
fn connect_bad_input() {
    let bad_addr = run_cli(&["connect", "not-a-valid-host-or-port"], None);
    assert_eq!(bad_addr.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad_addr.stderr).contains("iridium:"));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_addr = listener.local_addr().unwrap().to_string();
    drop(listener);

    let closed = run_cli(&["connect", &closed_addr], None);
    assert_eq!(closed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&closed.stderr).contains("iridium:"));
}
