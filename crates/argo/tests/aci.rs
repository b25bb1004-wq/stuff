// Agent-interface tests for the argo binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use argo::manifest::Manifest;
use argo::pkg::{self, Package};
use argo::version::{Req, Version};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_argo");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("argo_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Kills the child when the test ends, even on failure.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The path with forward slashes, as the envelope spells every path.
fn fwd(path: &Path) -> String {
    s(path).replace('\\', "/")
}

/// Parse stdout as exactly one envelope and validate it.
fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "not one line: {stdout:?}");
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

/// The data of a successful envelope; checks the exit code is 0.
fn data(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

/// The error code and hint of a failed envelope, checking the exit code and the command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> (String, Option<String>) {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    let error = value.get("error").unwrap();
    let code = error.get("code").unwrap().as_str().unwrap().to_string();
    let hint = error.get("hint").and_then(Json::as_str).map(String::from);
    (code, hint)
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {value:?}"))
}

/// The `items` of a page as (name, version) pairs.
fn items(page: &Json) -> Vec<(String, String)> {
    field(page, "items")
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                field(i, "name").as_str().unwrap().to_string(),
                field(i, "version").as_str().unwrap().to_string(),
            )
        })
        .collect()
}

const INDEX: &str = "alpha\t1.0.0\nbeta\t2.0.0\talpha\t^1.0\n";
const MANIFEST: &str =
    "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nbeta = \"^2.0\"\n";

/// Write INDEX and MANIFEST into `t`; return their paths.
fn write_solve_inputs(t: &Temp) -> (PathBuf, PathBuf) {
    let index = t.join("index.txt");
    let manifest = t.join("Cargo.argo");
    fs::write(&index, INDEX).unwrap();
    fs::write(&manifest, MANIFEST).unwrap();
    (index, manifest)
}

fn make_signed_pkg(
    name: &str,
    ver: &str,
    deps: Vec<(&str, &str)>,
    files: Vec<(&str, &[u8])>,
    seed: &[u8; 32],
) -> Vec<u8> {
    let manifest = Manifest {
        name: name.to_string(),
        version: Version::parse(ver).unwrap(),
        dependencies: deps
            .into_iter()
            .map(|(n, r)| (n.to_string(), Req::parse(r).unwrap()))
            .collect(),
    };
    let pkg = Package {
        manifest,
        files: files
            .into_iter()
            .map(|(p, b)| (p.to_string(), b.to_vec()))
            .collect(),
    };
    pkg::sign(&pkg, seed).unwrap()
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(&["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let commands = field(&manifest, "commands").as_array().unwrap();
    let mut names: Vec<&str> = commands
        .iter()
        .map(|c| field(c, "name").as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["install", "lock", "serve", "solve"]);
    for c in commands {
        let expect = match field(c, "name").as_str().unwrap() {
            "solve" => "read",
            "lock" => "write",
            _ => "net",
        };
        assert_eq!(field(c, "effect").as_str(), Some(expect), "{c:?}");
    }
}

#[test]
fn json_solve_returns_a_page_of_name_and_version() {
    let t = Temp::new("solve");
    let (index, manifest) = write_solve_inputs(&t);
    let d = data(&run(&["--json", "solve", s(&index), s(&manifest)]));
    assert_eq!(
        items(&d),
        [
            ("alpha".to_string(), "1.0.0".to_string()),
            ("beta".to_string(), "2.0.0".to_string())
        ]
    );
    assert_eq!(field(&d, "total"), &Json::Int(2));
    assert_eq!(field(&d, "more"), &Json::Bool(false));
    assert!(d.get("next_offset").is_none());
}

#[test]
fn json_solve_pages_with_limit_and_offset() {
    let t = Temp::new("page");
    let (index, manifest) = write_solve_inputs(&t);
    let first = data(&run(&[
        "--json",
        "solve",
        s(&index),
        s(&manifest),
        "--limit",
        "1",
    ]));
    assert_eq!(items(&first), [("alpha".to_string(), "1.0.0".to_string())]);
    assert_eq!(field(&first, "more"), &Json::Bool(true));
    assert_eq!(field(&first, "next_offset"), &Json::Int(1));
    let second = data(&run(&[
        "--json",
        "solve",
        s(&index),
        s(&manifest),
        "--limit",
        "1",
        "--offset",
        "1",
    ]));
    assert_eq!(items(&second), [("beta".to_string(), "2.0.0".to_string())]);
    assert_eq!(field(&second, "more"), &Json::Bool(false));
}

#[test]
fn json_solve_conflict_and_bad_input() {
    let t = Temp::new("bad");
    let (index, _) = write_solve_inputs(&t);
    let conflict = t.join("conflict.argo");
    fs::write(
        &conflict,
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nalpha = \"^9.0\"\n",
    )
    .unwrap();
    let (code, hint) = fail(
        &run(&["--json", "solve", s(&index), s(&conflict)]),
        1,
        "solve",
    );
    assert_eq!(code, "conflict");
    assert!(hint.is_some());

    let broken = t.join("broken.txt");
    fs::write(&broken, "alpha\tnot-a-version\n").unwrap();
    let (_, manifest) = write_solve_inputs(&t);
    let (code, _) = fail(
        &run(&["--json", "solve", s(&broken), s(&manifest)]),
        1,
        "solve",
    );
    assert_eq!(code, "invalid");

    let (code, _) = fail(
        &run(&["--json", "solve", s(&t.join("missing.txt")), s(&manifest)]),
        1,
        "solve",
    );
    assert_eq!(code, "not_found");
}

#[test]
fn json_lock_writes_the_file_and_reports_its_path() {
    let t = Temp::new("lock");
    let (index, manifest) = write_solve_inputs(&t);
    let dest = t.join("out dir").join("argo lock.txt");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let d = data(&run(&[
        "--json",
        "lock",
        s(&index),
        s(&manifest),
        "-o",
        s(&dest),
    ]));
    assert_eq!(field(&d, "path").as_str(), Some(fwd(&dest).as_str()));
    assert!(!fwd(&dest).contains('\\'));
    let written = fs::read(&dest).unwrap();
    assert_eq!(field(&d, "bytes"), &Json::Int(written.len() as i64));
    assert_eq!(field(&d, "count"), &Json::Int(2));
}

#[test]
fn json_lock_to_stdout_is_usage() {
    let t = Temp::new("lockstdout");
    let (index, manifest) = write_solve_inputs(&t);
    let (code, hint) = fail(
        &run(&["--json", "lock", s(&index), s(&manifest), "-o", "-"]),
        2,
        "lock",
    );
    assert_eq!(code, "usage");
    assert!(hint.unwrap().contains("run: argo --describe"));
}

#[test]
fn json_install_pages_the_installed_packages() {
    let t = Temp::new("install");
    let pkg_dir = t.join("pkgs_src");
    let store_dir = t.join("store");
    let lock_path = t.join("argo.lock");
    fs::create_dir_all(&pkg_dir).unwrap();
    let seed = [0x42u8; 32];
    let dep = make_signed_pkg(
        "core",
        "1.0.0",
        vec![],
        vec![("lib.txt", b"hello core")],
        &seed,
    );
    let app = make_signed_pkg(
        "tool",
        "0.2.0",
        vec![("core", "^1.0")],
        vec![("bin/run.txt", b"run")],
        &seed,
    );
    fs::write(pkg_dir.join("core-1.0.0.pkg"), &dep).unwrap();
    fs::write(pkg_dir.join("tool-0.2.0.pkg"), &app).unwrap();
    fs::write(
        &lock_path,
        "# argo lock v1\ncore = \"1.0.0\"\ntool = \"0.2.0\"\n",
    )
    .unwrap();

    let d = data(&run(&[
        "--json",
        "install",
        s(&lock_path),
        s(&store_dir),
        "--from",
        s(&pkg_dir),
        "--limit",
        "1",
    ]));
    assert_eq!(items(&d), [("core".to_string(), "1.0.0".to_string())]);
    assert_eq!(field(&d, "total"), &Json::Int(2));
    assert_eq!(field(&d, "more"), &Json::Bool(true));
    assert_eq!(field(&d, "path").as_str(), Some(fwd(&store_dir).as_str()));
    assert_eq!(
        field(&d, "bytes"),
        &Json::Int((dep.len() + app.len()) as i64)
    );
    assert!(field(&d, "duration_ms").as_i64().is_some());

    let (code, _) = fail(
        &run(&[
            "--json",
            "install",
            s(&lock_path),
            s(&store_dir),
            "--from",
            s(&t.join("nowhere")),
        ]),
        1,
        "install",
    );
    assert_eq!(code, "not_found");
    let bad_lock = t.join("bad.lock");
    fs::write(&bad_lock, "wrong header\n").unwrap();
    let (code, _) = fail(
        &run(&[
            "--json",
            "install",
            s(&bad_lock),
            s(&store_dir),
            "--from",
            s(&pkg_dir),
        ]),
        1,
        "install",
    );
    assert_eq!(code, "invalid");
}

#[test]
fn json_serve_prints_listen_once_bound_then_runs() {
    let t = Temp::new("serve");
    let pkgs = t.join("pkgs");
    fs::create_dir_all(&pkgs).unwrap();
    let mut child = Command::new(BIN)
        .args(["--json", "serve", s(&pkgs), "--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _guard = Guard(child);
    let value = Json::parse(line.trim_end()).unwrap();
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{line}");
    let d = value.get("data").unwrap();
    let listen = field(d, "listen").as_str().unwrap();
    assert!(
        listen.starts_with("127.0.0.1:") && !listen.ends_with(":0"),
        "{listen}"
    );
    assert_eq!(field(d, "path").as_str(), Some(fwd(&pkgs).as_str()));
}

#[test]
fn json_serve_bind_failure_is_io() {
    let t = Temp::new("busy");
    let pkgs = t.join("pkgs");
    fs::create_dir_all(&pkgs).unwrap();
    let hold = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = hold.local_addr().unwrap().to_string();
    let (code, _) = fail(
        &run(&["--json", "serve", s(&pkgs), "--listen", &addr]),
        1,
        "serve",
    );
    assert_eq!(code, "io");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let (code, hint) = fail(&run(&["--json", "nosuch"]), 2, "");
    assert_eq!(code, "usage");
    assert_eq!(hint.as_deref(), Some("run: argo --describe"));
    let t = Temp::new("usage");
    let (index, _) = write_solve_inputs(&t);
    let (code, _) = fail(&run(&["--json", "solve", s(&index)]), 2, "solve");
    assert_eq!(code, "usage");
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let (index, manifest) = write_solve_inputs(&t);
    let out = run(&["solve", s(&index), s(&manifest)]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "alpha 1.0.0\nbeta 2.0.0\n"
    );
    let limit = run(&["solve", s(&index), s(&manifest), "--limit", "1"]);
    assert_eq!(
        limit.status.code(),
        Some(2),
        "text mode must not accept --limit"
    );
}
