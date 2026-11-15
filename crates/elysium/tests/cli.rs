//! CLI integration tests for `elysium`.

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]

use elysium::{encode, Const, Func, Module, Op};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn write(name: &str, bytes: &[u8]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("elysium_cli_{pid}_{id}_{name}.mbc"));
        fs::write(&path, bytes).unwrap();
        Self { path }
    }

    fn path_str(&self) -> &str {
        self.path.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_elysium")
}

fn run_cmd(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn good_add_module() -> Module {
    Module {
        consts: vec![Const::Str(String::from("hello"))],
        imports: vec![],
        funcs: vec![Func {
            name: String::from("main"),
            params: 0,
            regs: 3,
            code: vec![Op::Int(0, 19), Op::Int(1, 23), Op::Add(2, 0, 1), Op::Ret(2)],
        }],
        entry: 0,
    }
}

#[test]
fn help_and_version() {
    let help = run_cmd(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8(help.stdout).unwrap();
    assert!(stdout.contains("verify"));
    assert!(stdout.contains("dis"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("Examples:"));

    let ver = run_cmd(&["--version"]);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(ver.stdout).unwrap().trim(),
        "elysium 0.1.0"
    );
}

#[test]
fn verify_good_and_bad() {
    let good = TempFile::write("good", &encode(&good_add_module()));
    let out = run_cmd(&["verify", good.path_str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ok"));
    assert!(stdout.contains("funcs: 1"));
    assert!(stdout.contains("entry: 0"));

    let bad_verify = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: String::from("bad"),
            params: 0,
            regs: 1,
            code: vec![Op::Int(9, 1), Op::Ret(0)],
        }],
        entry: 0,
    };
    let bad_file = TempFile::write("bad_verify", &encode(&bad_verify));
    let out = run_cmd(&["verify", bad_file.path_str()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("elysium: "));

    let corrupt = TempFile::write("corrupt", b"NOTMBC");
    let out = run_cmd(&["verify", corrupt.path_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("elysium: "));
}

#[test]
fn dis_good_and_bad() {
    let good = TempFile::write("dis_good", &encode(&good_add_module()));
    let out = run_cmd(&["dis", good.path_str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("func 0 main"));
    assert!(stdout.contains("0000: int r0, 19"));
    assert!(stdout.contains("0001: int r1, 23"));
    assert!(stdout.contains("0002: add r2, r0, r1"));
    assert!(stdout.contains("0003: ret r2"));

    let corrupt = TempFile::write("dis_bad", b"MBC1\xff");
    let out = run_cmd(&["dis", corrupt.path_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("elysium: "));
}

#[test]
fn run_good_fuel_and_bad() {
    let good = TempFile::write("run_good", &encode(&good_add_module()));
    let out = run_cmd(&["run", good.path_str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("result: 42"));
    assert!(stdout.contains("steps: 4"));

    let out_fuel = run_cmd(&["run", good.path_str(), "--fuel", "10"]);
    assert_eq!(out_fuel.status.code(), Some(0));
    assert!(String::from_utf8(out_fuel.stdout)
        .unwrap()
        .contains("result: 42"));

    let out_oob_fuel = run_cmd(&["run", good.path_str(), "--fuel", "2"]);
    assert_eq!(out_oob_fuel.status.code(), Some(1));
    let stderr = String::from_utf8(out_oob_fuel.stderr).unwrap();
    assert!(stderr.starts_with("elysium: "));
    assert!(stderr.contains("out of fuel"));

    let str_mod = Module {
        consts: vec![Const::Str(String::from("hi"))],
        imports: vec![],
        funcs: vec![Func {
            name: String::from("main"),
            params: 0,
            regs: 1,
            code: vec![Op::Const(0, 0), Op::Ret(0)],
        }],
        entry: 0,
    };
    let str_file = TempFile::write("run_str", &encode(&str_mod));
    let out_str = run_cmd(&["run", str_file.path_str()]);
    assert_eq!(out_str.status.code(), Some(0));
    assert!(String::from_utf8(out_str.stdout)
        .unwrap()
        .contains("result: \"hi\""));

    let trap_mod = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: String::from("main"),
            params: 0,
            regs: 2,
            code: vec![Op::Int(0, 1), Op::Int(1, 0), Op::Div(0, 0, 1), Op::Ret(0)],
        }],
        entry: 0,
    };
    let trap_file = TempFile::write("run_trap", &encode(&trap_mod));
    let out_trap = run_cmd(&["run", trap_file.path_str()]);
    assert_eq!(out_trap.status.code(), Some(1));
    assert!(String::from_utf8(out_trap.stderr)
        .unwrap()
        .starts_with("elysium: "));
}

#[test]
fn stdin_and_usage_errors() {
    let bytes = encode(&good_add_module());
    let out = run_with_stdin(&["run", "-"], &bytes);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .contains("result: 42"));

    let unknown = run_cmd(&["nope"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8(unknown.stderr)
        .unwrap()
        .starts_with("elysium: "));

    let no_args = run_cmd(&[]);
    assert_eq!(no_args.status.code(), Some(2));
    assert!(String::from_utf8(no_args.stderr)
        .unwrap()
        .starts_with("elysium: "));

    let bad_fuel = run_cmd(&["run", "-", "--fuel", "not_a_number"]);
    assert_eq!(bad_fuel.status.code(), Some(2));
    assert!(String::from_utf8(bad_fuel.stderr)
        .unwrap()
        .starts_with("elysium: "));
}
