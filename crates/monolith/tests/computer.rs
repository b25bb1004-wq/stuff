#![allow(clippy::unwrap_used)]
//! Integration tests for computers, the path fence, tool runs, the journal and the CLI.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;
use monolith::computer::{self, split_command_line, Computer, TOOLS};
use monolith::journal;

/// A unique temp folder for one test, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("monolith-test-{}-{test}-{ns}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn argv(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn create_open_list_remove_round_trip() {
    let t = TempDir::new("roundtrip");
    let c = Computer::create(t.path(), "dev1").unwrap();
    assert_eq!(c.name, "dev1");
    assert!(c.root().is_dir());
    assert!(c.dir.join("memory").is_dir());
    assert_eq!(
        Computer::create(t.path(), "dev1").unwrap_err().code,
        Code::Exists
    );
    Computer::create(t.path(), "beta").unwrap();
    assert_eq!(Computer::list(t.path()).unwrap(), ["beta", "dev1"]);
    assert_eq!(Computer::open(t.path(), "dev1").unwrap(), c);
    Computer::remove(t.path(), "dev1").unwrap();
    assert_eq!(Computer::list(t.path()).unwrap(), ["beta"]);
    assert_eq!(
        Computer::open(t.path(), "dev1").unwrap_err().code,
        Code::NotFound
    );
    assert_eq!(
        Computer::remove(t.path(), "dev1").unwrap_err().code,
        Code::NotFound
    );
}

#[test]
fn list_without_a_folder_is_empty() {
    let t = TempDir::new("nofolder");
    assert!(Computer::list(&t.path().join("missing"))
        .unwrap()
        .is_empty());
}

#[test]
fn bad_names_are_invalid_and_forty_is_the_limit() {
    let t = TempDir::new("names");
    let long = "a".repeat(41);
    for bad in ["", "A", "a b", "../x", long.as_str()] {
        let e = Computer::create(t.path(), bad).unwrap_err();
        assert_eq!(e.code, Code::Invalid, "{bad}");
        assert!(e.hint.is_some(), "{bad}");
    }
    let ok = "a".repeat(40);
    assert!(Computer::create(t.path(), &ok).is_ok());
    assert!(Computer::create(t.path(), "under_score-1").is_ok());
}

#[test]
fn split_command_line_table() {
    let cases: &[(&str, &[&str])] = &[
        ("chronos init proj", &["chronos", "init", "proj"]),
        ("  a   b  ", &["a", "b"]),
        ("a 'b c' d", &["a", "b c", "d"]),
        (r#"a "b \"c\" d""#, &["a", r#"b "c" d"#]),
        ("a \"\"", &["a", ""]),
        (r"a b\ c", &["a", "b c"]),
        (r"cd C:\dir", &["cd", r"C:\dir"]),
        (r#"x "a\\b""#, &["x", r"a\b"]),
        ("'a\\b'", &[r"a\b"]),
    ];
    for (line, want) in cases {
        assert_eq!(split_command_line(line).unwrap(), *want, "{line}");
    }
    for bad in ["a 'b", "a \"b", "", "   "] {
        assert!(split_command_line(bad).is_err(), "{bad:?}");
    }
    assert_eq!(split_command_line("a 'b").unwrap_err().code, Code::Invalid);
    assert_eq!(split_command_line("  ").unwrap_err().code, Code::Invalid);
}

#[test]
fn resolve_keeps_paths_inside_the_root() {
    let t = TempDir::new("resolve");
    let c = Computer::create(t.path(), "dev1").unwrap();
    let p = c.resolve("a/b.txt").unwrap();
    assert!(p.starts_with(c.root()));
    assert!(p.ends_with("b.txt"));
    for bad in [
        "../x",
        "a/../../x",
        "/etc/passwd",
        "\\x",
        "C:\\x",
        "~/x",
        "",
        "a\\..\\x",
    ] {
        let e = c.resolve(bad).unwrap_err();
        assert_eq!(e.code, Code::Denied, "{bad}");
        assert!(e.hint.is_some(), "{bad}");
    }
}

#[cfg(unix)]
#[test]
fn resolve_refuses_a_link_that_leaves_the_root() {
    let t = TempDir::new("symlink");
    let c = Computer::create(t.path(), "dev1").unwrap();
    let outside = t.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, c.root().join("link")).unwrap();
    assert_eq!(c.resolve("link/x.txt").unwrap_err().code, Code::Denied);
    assert!(c.resolve("inside/x.txt").is_ok());
}

#[test]
fn check_args_allows_tool_runs_inside_the_computer() {
    let t = TempDir::new("allowed");
    let c = Computer::create(t.path(), "dev1").unwrap();
    let allowed: &[&[&str]] = &[
        &["chronos", "init", "proj"],
        &[
            "hades",
            "ls",
            "disk.img",
            "/docs-that-does-not-exist-on-host",
        ],
        &["mercury", "get", "http://127.0.0.1:8080/x"],
        &["mercury", "get", "http://localhost/x"],
    ];
    for case in allowed {
        assert!(c.check_args(&argv(case)).is_ok(), "{case:?}");
    }
}

#[test]
fn check_args_denies_what_leaves_the_computer() {
    let t = TempDir::new("denied");
    let c = Computer::create(t.path(), "dev1").unwrap();
    let mut denied: Vec<Vec<&str>> = vec![
        vec!["bash", "-c", "x"],
        vec!["monolith", "list"],
        vec!["chronos", "init", "../out"],
        vec!["chronos", "--dir=../out"],
        vec!["chronos", "init", "C:\\Users"],
        vec!["chronos", "init", "~/x"],
        vec!["krypton", "run", "cat ../secret"],
        vec!["mercury", "get", "https://example.com/"],
        vec!["xenon", "dial", "8.8.8.8:53"],
    ];
    if cfg!(unix) {
        denied.push(vec!["calliope", "/etc/passwd"]);
    } else {
        denied.push(vec!["calliope", "/Windows/win.ini"]);
    }
    for case in &denied {
        let e = c.check_args(&argv(case)).unwrap_err();
        assert_eq!(e.code, Code::Denied, "{case:?}");
        assert!(e.hint.is_some(), "{case:?}");
    }
    assert_eq!(c.check_args(&[]).unwrap_err().code, Code::Usage);
    assert!(!TOOLS.contains(&"monolith"));
}

#[test]
fn exec_in_runs_a_real_program_and_refuses_a_denied_one() {
    let t = TempDir::new("exec");
    let home = t.path().join("home");
    let c = Computer::create(&home, "dev1").unwrap();
    let tools = t.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_monolith"));
    let ext = std::env::consts::EXE_SUFFIX;
    fs::copy(&bin, tools.join(format!("chronos{ext}"))).unwrap();
    fs::copy(&bin, tools.join(format!("monolith{ext}"))).unwrap();
    let limit = Duration::from_secs(60);

    let res = c
        .exec_in(&tools, &argv(&["chronos", "--describe"]), limit)
        .unwrap();
    assert_eq!(res.exit, 0);
    assert!(!res.timed_out);
    assert!(String::from_utf8_lossy(&res.stdout).contains("\"tool\":\"monolith\""));

    let res = c
        .exec_in(
            &tools,
            &argv(&["chronos", "definitely-not-a-command"]),
            limit,
        )
        .unwrap();
    assert_eq!(res.exit, 2);

    let missing = c
        .exec_in(&tools, &argv(&["hades", "ls"]), limit)
        .unwrap_err();
    assert_eq!(missing.code, Code::NotFound);
    assert!(missing.hint.unwrap().contains(&tools.display().to_string()));

    // `monolith` exists in the folder but is not a tool a computer may run.
    let denied = c
        .exec_in(&tools, &argv(&["monolith", "--describe"]), limit)
        .unwrap_err();
    assert_eq!(denied.code, Code::Denied);
}

#[test]
fn timeout_kills_a_sleeping_program() {
    let t = TempDir::new("timeout");
    let (prog, args): (&str, Vec<String>) = if cfg!(windows) {
        ("ping", argv(&["-n", "6", "127.0.0.1"]))
    } else {
        ("sleep", argv(&["5"]))
    };
    let start = Instant::now();
    let res = computer::run_program_for_test(
        Path::new(prog),
        &args,
        t.path(),
        Duration::from_millis(300),
    )
    .unwrap();
    assert!(res.timed_out);
    assert_eq!(res.exit, -1);
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[cfg(unix)]
#[test]
fn output_is_capped_at_256_kib_and_the_program_still_finishes() {
    let t = TempDir::new("cap");
    let res = computer::run_program_for_test(
        Path::new("sh"),
        &argv(&["-c", "head -c 400000 /dev/zero"]),
        t.path(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(res.exit, 0);
    assert!(!res.timed_out);
    assert_eq!(res.stdout.len(), 256 * 1024);
}

#[test]
fn journal_appends_in_order_and_skips_garbage() {
    let t = TempDir::new("journal");
    let c = Computer::create(t.path(), "dev1").unwrap();
    assert!(journal::read(&c).unwrap().is_empty());

    let d1 = Json::obj([("text", Json::Str("hi".into()))]);
    let d2 = Json::obj([("n", Json::Int(2))]);
    let d3 = Json::obj([("answer", Json::Str("x".into()))]);
    assert_eq!(journal::append(&c, "s1", "task", d1.clone()).unwrap(), 1);
    assert_eq!(journal::append(&c, "s1", "action", d2.clone()).unwrap(), 2);
    assert_eq!(journal::append(&c, "s1", "done", d3.clone()).unwrap(), 3);

    let entries = journal::read(&c).unwrap();
    let seqs: Vec<u64> = entries.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [1, 2, 3]);
    let kinds: Vec<&str> = entries.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["task", "action", "done"]);
    assert_eq!(entries[0].data, d1);
    assert_eq!(entries[1].data, d2);
    assert_eq!(entries[2].data, d3);
    assert!(entries.iter().all(|e| e.session == "s1"));

    let mut f = OpenOptions::new()
        .append(true)
        .open(c.dir.join("journal.jsonl"))
        .unwrap();
    f.write_all(b"this is { not json\n").unwrap();
    drop(f);
    assert_eq!(journal::read(&c).unwrap().len(), 3);
    assert_eq!(journal::append(&c, "s1", "task", d1).unwrap(), 4);
    assert_eq!(journal::read(&c).unwrap().len(), 4);
}

#[test]
fn cli_round_trip_through_the_binary() {
    let t = TempDir::new("cli");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_monolith"))
            .args(args)
            .env("MONOLITH_HOME", t.path())
            .output()
            .unwrap()
    };
    let text = |o: &std::process::Output| String::from_utf8_lossy(&o.stdout).into_owned();

    let out = run(&["--json", "new", "dev1"]);
    assert!(out.status.success());
    assert!(text(&out).contains("\"ok\":true"), "{}", text(&out));

    let out = run(&["--json", "new", "dev1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("\"code\":\"exists\""), "{}", text(&out));

    let out = run(&["--json", "list"]);
    assert!(text(&out).contains("\"total\":1"), "{}", text(&out));

    let out = run(&["--json", "log", "dev1"]);
    assert!(out.status.success());
    assert!(text(&out).contains("\"total\":0"), "{}", text(&out));

    let out = run(&["--json", "rm", "dev1"]);
    assert!(out.status.success());
    let out = run(&["--json", "list"]);
    assert!(text(&out).contains("\"total\":0"), "{}", text(&out));
}

#[test]
fn drive_style_unix_paths_are_refused() {
    let home = std::env::temp_dir().join(format!(
        "monolith-test-{}-drivepath-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let c = Computer::create(&home, "c1").unwrap();
    for argv in [
        vec!["krypton", "-c", "ls /c/Users"],
        vec!["calliope", "/d/data.txt"],
        vec!["hades", "ls", "disk.img", "/c"],
    ] {
        let argv: Vec<String> = argv.into_iter().map(String::from).collect();
        let e = c.check_args(&argv).unwrap_err();
        assert_eq!(e.code, Code::Denied, "{argv:?}");
    }
    let ok: Vec<String> = ["hades", "ls", "disk.img", "/docs-not-on-host/a"]
        .into_iter()
        .map(String::from)
        .collect();
    assert!(c.check_args(&ok).is_ok());
    for argv in [
        vec!["hades", "ls", "disk.img", "/"],
        vec!["iridium", "sql", "a.idb", "SELECT 10 / 2"],
    ] {
        let argv: Vec<String> = argv.into_iter().map(String::from).collect();
        assert!(c.check_args(&argv).is_ok(), "{argv:?}");
    }
    let unc: Vec<String> = ["calliope", "//server/share/x"]
        .into_iter()
        .map(String::from)
        .collect();
    assert!(c.check_args(&unc).is_err());
    fs::remove_dir_all(&home).unwrap();
}
