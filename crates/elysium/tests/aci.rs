// Agent-interface tests for the elysium binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use elysium::{encode, Func, Module, Op};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_elysium");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("elysium_aci_{name}_{}_{id}", std::process::id()));
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

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
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

/// The data of a successful envelope, checking the process exit code.
fn data(output: &Output, exit: i32) -> Json {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

/// The error object of a failed envelope, checking the exit code and the command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> Json {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    value.get("error").unwrap().clone()
}

fn error_code(error: &Json) -> &str {
    error.get("code").and_then(Json::as_str).unwrap()
}

fn func(name: &str, regs: u16, code: Vec<Op>) -> Func {
    Func {
        name: name.to_string(),
        params: 0,
        regs,
        code,
    }
}

fn module(funcs: Vec<Func>) -> Module {
    Module {
        consts: vec![],
        imports: vec![],
        funcs,
        entry: 0,
    }
}

/// `main` returns 2 + 3.
fn add_module() -> Vec<u8> {
    encode(&module(vec![func(
        "main",
        3,
        vec![Op::Int(0, 2), Op::Int(1, 3), Op::Add(2, 0, 1), Op::Ret(2)],
    )]))
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(&["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = manifest
        .get("commands")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|c| c.get("name").and_then(Json::as_str).unwrap())
        .collect();
    assert_eq!(names, ["verify", "dis", "run"]);
}

#[test]
fn verify_json_counts_the_module() {
    let t = Temp::new("verify");
    fs::write(t.join("add.mbc"), add_module()).unwrap();
    let out = run(&["--json", "verify", s(&t.join("add.mbc"))]);
    let d = data(&out, 0);
    assert_eq!(d.get("funcs").and_then(Json::as_i64), Some(1));
    assert_eq!(d.get("consts").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("imports").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("entry").and_then(Json::as_i64), Some(0));
}

#[test]
fn verify_json_rejects_bad_module_as_invalid() {
    let t = Temp::new("badverify");
    let bad = encode(&module(vec![func(
        "bad",
        1,
        vec![Op::Int(9, 1), Op::Ret(0)],
    )]));
    fs::write(t.join("bad.mbc"), bad).unwrap();
    let out = run(&["--json", "verify", s(&t.join("bad.mbc"))]);
    let error = fail(&out, 1, "verify");
    assert_eq!(error_code(&error), "invalid");
    assert!(error
        .get("message")
        .and_then(Json::as_str)
        .unwrap()
        .starts_with("verify error:"));
}

#[test]
fn verify_json_corrupt_input_is_invalid() {
    let t = Temp::new("corrupt");
    fs::write(t.join("junk.mbc"), b"NOTMBC").unwrap();
    let out = run(&["--json", "verify", s(&t.join("junk.mbc"))]);
    assert_eq!(error_code(&fail(&out, 1, "verify")), "invalid");
}

#[test]
fn verify_json_missing_file_is_io() {
    let t = Temp::new("missing");
    let out = run(&["--json", "verify", s(&t.join("nope.mbc"))]);
    assert_eq!(error_code(&fail(&out, 1, "verify")), "io");
}

#[test]
fn dis_json_pages_functions() {
    let t = Temp::new("dis");
    let two = encode(&module(vec![
        func("main", 1, vec![Op::Int(0, 1), Op::Ret(0)]),
        func("other", 1, vec![Op::Int(0, 2), Op::Ret(0)]),
    ]));
    fs::write(t.join("two.mbc"), two).unwrap();
    let out = run(&["--json", "dis", s(&t.join("two.mbc")), "--limit", "1"]);
    let d = data(&out, 0);
    assert_eq!(d.get("entry").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("total").and_then(Json::as_i64), Some(2));
    assert_eq!(d.get("more"), Some(&Json::Bool(true)));
    assert_eq!(d.get("next_offset").and_then(Json::as_i64), Some(1));
    let items = d.get("items").and_then(Json::as_array).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("name").and_then(Json::as_str), Some("main"));
    assert_eq!(items[0].get("regs").and_then(Json::as_i64), Some(1));
    let code = items[0].get("code").and_then(Json::as_array).unwrap();
    assert_eq!(code[1].get("text").and_then(Json::as_str), Some("ret r0"));
}

#[test]
fn run_json_returns_value_and_steps() {
    let t = Temp::new("run");
    fs::write(t.join("add.mbc"), add_module()).unwrap();
    let out = run(&["--json", "run", s(&t.join("add.mbc"))]);
    let d = data(&out, 0);
    assert_eq!(d.get("result").and_then(Json::as_str), Some("5"));
    assert_eq!(d.get("exit").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("stop").and_then(Json::as_str), Some("exit"));
    assert_eq!(d.get("steps").and_then(Json::as_i64), Some(4));
    assert!(d.get("duration_ms").and_then(Json::as_i64).is_some());
}

#[test]
fn run_json_trap_is_an_invalid_error() {
    let t = Temp::new("trap");
    let trap = encode(&module(vec![func("main", 1, vec![Op::Trap(7)])]));
    fs::write(t.join("trap.mbc"), trap).unwrap();
    let out = run(&["--json", "run", s(&t.join("trap.mbc"))]);
    let error = fail(&out, 1, "run");
    assert_eq!(error_code(&error), "invalid");
    assert!(error
        .get("message")
        .and_then(Json::as_str)
        .unwrap()
        .contains("user trap 7"));
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let out = run(&["--json", "nosuch"]);
    let error = fail(&out, 2, "");
    assert_eq!(error_code(&error), "usage");
    assert!(error
        .get("hint")
        .and_then(Json::as_str)
        .unwrap()
        .contains("--describe"));
}

#[test]
fn text_mode_unchanged() {
    let t = Temp::new("text");
    fs::write(t.join("add.mbc"), add_module()).unwrap();
    let out = run(&["verify", s(&t.join("add.mbc"))]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("ok\nconsts: 0\n"), "{stdout:?}");
    let run_out = run(&["run", s(&t.join("add.mbc"))]);
    assert_eq!(
        String::from_utf8(run_out.stdout).unwrap(),
        "result: 5\nsteps: 4\n"
    );
}
