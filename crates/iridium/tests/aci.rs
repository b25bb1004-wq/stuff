//! Agent-interface tests for `iridium` and `iridium-cli`: `--describe` manifests and `--json` envelopes (docs/ai-computer/ACI.md).
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use aci::envelope::validate_envelope;
use aci::json::Json;
use aci::manifest::validate_manifest;
use iridium::net::Server;

static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let id = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("iridium_aci_{tag}_{pid}_{id}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn db(&self) -> String {
        self.file("app.idb").to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn iridium() -> &'static str {
    env!("CARGO_BIN_EXE_iridium")
}

fn iridium_cli() -> &'static str {
    env!("CARGO_BIN_EXE_iridium-cli")
}

fn run(bin: &str, args: &[&str], stdin_data: Option<&str>) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(data) = stdin_data {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(data.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

/// Stdout must be exactly one JSON object and a newline; it must validate as an envelope.
fn envelope(out: &Output) -> Json {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(text.ends_with('\n'), "no trailing newline: {text:?}");
    assert_eq!(
        text.matches('\n').count(),
        1,
        "more than one line: {text:?}"
    );
    let value = Json::parse(text.trim_end()).unwrap();
    assert_eq!(validate_envelope(&value), Ok(()), "bad envelope: {text}");
    value
}

fn error_code(env: &Json) -> &str {
    assert_eq!(env.get("ok"), Some(&Json::Bool(false)));
    env.get("error")
        .and_then(|e| e.get("code"))
        .and_then(Json::as_str)
        .unwrap()
}

fn error_message(env: &Json) -> &str {
    env.get("error")
        .and_then(|e| e.get("message"))
        .and_then(Json::as_str)
        .unwrap()
}

fn statements(env: &Json) -> &[Json] {
    assert_eq!(env.get("ok"), Some(&Json::Bool(true)));
    env.get("data")
        .and_then(|d| d.get("statements"))
        .and_then(Json::as_array)
        .unwrap()
}

fn field<'a>(o: &'a Json, key: &str) -> &'a Json {
    o.get(key)
        .unwrap_or_else(|| panic!("missing {key} in {o:?}"))
}

fn command_names(manifest: &Json) -> Vec<String> {
    field(manifest, "commands")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| field(c, "name").as_str().unwrap().to_owned())
        .collect()
}

/// An address nothing listens on: bind, note the port, drop the listener.
fn dead_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    addr
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(iridium(), &["--describe"], None);
    assert_eq!(out.status.code(), Some(0));
    let manifest = Json::parse(String::from_utf8(out.stdout).unwrap().trim_end()).unwrap();
    assert_eq!(validate_manifest(&manifest), Ok(()));
    assert_eq!(command_names(&manifest), ["sql", "connect"]);

    let out = run(iridium_cli(), &["--describe"], None);
    assert_eq!(out.status.code(), Some(0));
    let manifest = Json::parse(String::from_utf8(out.stdout).unwrap().trim_end()).unwrap();
    assert_eq!(validate_manifest(&manifest), Ok(()));
    assert_eq!(command_names(&manifest), ["connect"]);
}

#[test]
fn describe_lists_the_sql_flags() {
    let out = run(iridium(), &["--describe"], None);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"--limit\""));
    assert!(text.contains("\"--offset\""));
}

#[test]
fn sql_json_returns_one_entry_per_statement() {
    let dir = TempDir::new("multi");
    let db = dir.db();
    let out = run(
        iridium(),
        &[
            "--json",
            "sql",
            &db,
            "CREATE TABLE m(a INTEGER); INSERT INTO m VALUES (1),(2); SELECT a FROM m; DROP TABLE m",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    let env = envelope(&out);
    let stmts = statements(&env);
    assert_eq!(stmts.len(), 4);
    assert_eq!(field(&stmts[0], "kind").as_str(), Some("create"));
    assert_eq!(field(&stmts[0], "count"), &Json::Int(0));
    assert_eq!(field(&stmts[1], "kind").as_str(), Some("insert"));
    assert_eq!(field(&stmts[1], "count"), &Json::Int(2));
    assert_eq!(field(&stmts[2], "kind").as_str(), Some("select"));
    assert_eq!(field(&stmts[2], "total"), &Json::Int(2));
    assert_eq!(field(&stmts[3], "kind").as_str(), Some("drop"));
}

#[test]
fn sql_json_reads_statements_from_stdin() {
    let dir = TempDir::new("stdin");
    let db = dir.db();
    let out = run(
        iridium(),
        &["--json", "sql", &db],
        Some("CREATE TABLE s(a INTEGER);\nINSERT INTO s VALUES (7);\nSELECT a FROM s;\n"),
    );
    let env = envelope(&out);
    let stmts = statements(&env);
    assert_eq!(stmts.len(), 3);
    assert_eq!(field(&stmts[2], "kind").as_str(), Some("select"));
    let rows = field(&stmts[2], "rows").as_array().unwrap();
    assert_eq!(rows, [Json::Arr(vec![Json::Int(7)])]);
}

#[test]
fn sql_json_values_have_exact_json_types() {
    let dir = TempDir::new("types");
    let db = dir.db();
    let query = "CREATE TABLE v(id INTEGER, name TEXT, x REAL, n INTEGER); \
                 INSERT INTO v VALUES (1, 'a', 1.5, NULL); \
                 SELECT * FROM v";
    let env = envelope(&run(iridium(), &["--json", "sql", &db, query], None));
    let select = &statements(&env)[2];
    let columns = field(select, "columns").as_array().unwrap();
    let types: Vec<(&str, &str)> = columns
        .iter()
        .map(|c| {
            (
                field(c, "name").as_str().unwrap(),
                field(c, "type").as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        types,
        [
            ("id", "INTEGER"),
            ("name", "TEXT"),
            ("x", "REAL"),
            ("n", "INTEGER")
        ]
    );
    let row = field(select, "rows").as_array().unwrap()[0].clone();
    assert_eq!(
        row,
        Json::Arr(vec![
            Json::Int(1),
            Json::Str("a".into()),
            Json::Num(1.5),
            Json::Null
        ])
    );
}

#[test]
fn sql_json_pages_selects_with_limit_and_offset() {
    let dir = TempDir::new("paging");
    let db = dir.db();
    let mut setup = String::from("CREATE TABLE p(a INTEGER)");
    for i in 0..250 {
        setup.push_str(&format!("; INSERT INTO p VALUES ({i})"));
    }
    let query = format!("{setup}; SELECT a FROM p");
    let env = envelope(&run(iridium(), &["--json", "sql", &db, &query], None));
    let page = &statements(&env)[251];
    assert_eq!(field(page, "rows").as_array().unwrap().len(), 100);
    assert_eq!(field(page, "total"), &Json::Int(250));
    assert_eq!(field(page, "more"), &Json::Bool(true));
    assert_eq!(field(page, "next_offset"), &Json::Int(100));

    let env = envelope(&run(
        iridium(),
        &["--json", "sql", &db, "SELECT a FROM p", "--offset", "200"],
        None,
    ));
    let last = &statements(&env)[0];
    assert_eq!(field(last, "rows").as_array().unwrap().len(), 50);
    assert_eq!(field(last, "total"), &Json::Int(250));
    assert_eq!(field(last, "more"), &Json::Bool(false));
    assert!(last.get("next_offset").is_none());
}

#[test]
fn sql_json_parse_error_is_invalid() {
    let dir = TempDir::new("parse");
    let out = run(iridium(), &["--json", "sql", &dir.db(), "SELEC 1"], None);
    assert_eq!(out.status.code(), Some(1));
    let env = envelope(&out);
    assert_eq!(error_code(&env), "invalid");
    assert!(error_message(&env).starts_with("statement 1 of 1:"));
}

#[test]
fn sql_json_missing_table_is_not_found() {
    let dir = TempDir::new("missing");
    let out = run(
        iridium(),
        &["--json", "sql", &dir.db(), "SELECT * FROM nosuch"],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(error_code(&envelope(&out)), "not_found");
}

#[test]
fn sql_json_failure_names_the_statement_and_keeps_earlier_work() {
    let dir = TempDir::new("partial");
    let db = dir.db();
    let out = run(
        iridium(),
        &[
            "--json",
            "sql",
            &db,
            "CREATE TABLE k(a INTEGER); INSERT INTO k VALUES (5); SELEC 1",
        ],
        None,
    );
    let env = envelope(&out);
    assert_eq!(error_code(&env), "invalid");
    assert!(error_message(&env).starts_with("statement 3 of 3:"));
    let check = run(iridium(), &["--json", "sql", &db, "SELECT a FROM k"], None);
    let rows = field(&statements(&envelope(&check))[0], "rows").clone();
    assert_eq!(rows, Json::Arr(vec![Json::Arr(vec![Json::Int(5)])]));
}

#[test]
fn sql_json_locked_database_is_locked() {
    let dir = TempDir::new("locked");
    let db = dir.db();
    let _held = iridium::lock_database(std::path::Path::new(&db)).unwrap();
    let out = run(iridium(), &["--json", "sql", &db, "SELECT 1"], None);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(error_code(&envelope(&out)), "locked");
}

#[test]
fn json_text_round_trips_through_the_reader() {
    let dir = TempDir::new("text");
    let db = dir.db();
    let text = "q\"uo\\te\nnew \u{e9}\u{1F600}";
    let insert =
        format!("CREATE TABLE t(s TEXT); INSERT INTO t VALUES ('{text}'); SELECT s FROM t");
    let env = envelope(&run(iridium(), &["--json", "sql", &db, &insert], None));
    let row = &field(&statements(&env)[2], "rows").as_array().unwrap()[0];
    assert_eq!(row, &Json::Arr(vec![Json::Str(text.into())]));
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let out = run(iridium(), &["--json", "nosuch"], None);
    assert_eq!(out.status.code(), Some(2));
    let env = envelope(&out);
    assert_eq!(error_code(&env), "usage");
    assert_eq!(field(&env, "cmd").as_str(), Some(""));
}

#[test]
fn json_missing_argument_is_usage_for_sql() {
    let out = run(iridium(), &["--json", "sql"], None);
    assert_eq!(out.status.code(), Some(2));
    let env = envelope(&out);
    assert_eq!(error_code(&env), "usage");
    assert_eq!(field(&env, "cmd").as_str(), Some("sql"));
}

#[test]
fn connect_json_sends_statements_to_a_server() {
    let dir = TempDir::new("connect");
    let server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        dir.file("srv.idb"),
        dir.file("srv.wal"),
    )
    .unwrap();
    let addr = server.local_addr().to_string();
    let out = run(
        iridium(),
        &["--json", "connect", &addr],
        Some("CREATE TABLE c(a INTEGER); INSERT INTO c VALUES (3);\nSELECT a FROM c;\n"),
    );
    assert_eq!(out.status.code(), Some(0));
    let env = envelope(&out);
    let stmts = statements(&env);
    assert_eq!(stmts.len(), 3);
    assert_eq!(field(&stmts[1], "count"), &Json::Int(1));
    assert_eq!(
        field(&stmts[2], "rows").as_array().unwrap(),
        [Json::Arr(vec![Json::Int(3)])]
    );
    server.stop();
}

#[test]
fn connect_refused_is_io_with_address_hint() {
    let addr = dead_addr();
    let out = run(iridium(), &["--json", "connect", &addr], Some("SELECT 1\n"));
    assert_eq!(out.status.code(), Some(1));
    let env = envelope(&out);
    assert_eq!(error_code(&env), "io");
    let hint = env
        .get("error")
        .and_then(|e| e.get("hint"))
        .and_then(Json::as_str);
    assert!(hint.unwrap().contains(&addr));
}

#[test]
fn iridium_cli_json_sends_statements_to_a_server() {
    let dir = TempDir::new("cli");
    let server = Server::start(
        "127.0.0.1:0".parse().unwrap(),
        dir.file("srv.idb"),
        dir.file("srv.wal"),
    )
    .unwrap();
    let addr = server.local_addr().to_string();
    let out = run(
        iridium_cli(),
        &["--json", &addr],
        Some("SELECT 1 + 2 AS sum;\n"),
    );
    assert_eq!(out.status.code(), Some(0));
    let env = envelope(&out);
    let select = &statements(&env)[0];
    assert_eq!(field(select, "kind").as_str(), Some("select"));
    assert_eq!(
        field(select, "rows").as_array().unwrap(),
        [Json::Arr(vec![Json::Int(3)])]
    );
    server.stop();
}

#[test]
fn iridium_cli_refused_connection_is_io() {
    let addr = dead_addr();
    let out = run(iridium_cli(), &["--json", &addr], Some("SELECT 1\n"));
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(error_code(&envelope(&out)), "io");
}

#[test]
fn iridium_cli_missing_address_is_usage() {
    let out = run(iridium_cli(), &["--json"], None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error_code(&envelope(&out)), "usage");
}

#[test]
fn text_mode_output_is_unchanged() {
    let dir = TempDir::new("text_mode");
    let out = run(iridium(), &["sql", &dir.db(), "SELECT 1 + 2 AS sum"], None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "sum\n3\n");
    assert!(out.stderr.is_empty());
}
