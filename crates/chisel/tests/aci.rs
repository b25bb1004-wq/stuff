// Agent-interface tests for the silicon binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use chisel::elf::{self, Image, Segment, PF_R, PF_X};
use silicon::machine::RAM_BASE;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_silicon");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("silicon_aci_{name}_{}_{id}", std::process::id()));
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

/// The error envelope, checking the process exit code and the command name.
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

fn build_elf(asm: &str) -> Vec<u8> {
    let vaddr = RAM_BASE;
    let out = chisel::assemble(asm, vaddr).unwrap();
    let img = Image {
        entry: vaddr,
        segments: vec![Segment {
            vaddr,
            flags: PF_R | PF_X,
            data: out.bytes.clone(),
            mem_size: out.bytes.len() as u64,
        }],
        symbols: out.symbols,
    };
    elf::write(&img).unwrap()
}

/// Prints "hi" on the UART and passes.
fn hi_elf() -> Vec<u8> {
    build_elf(
        "
        li t0, 0x10000000
        li t1, 0x68
        sb t1, 0(t0)
        li t1, 0x69
        sb t1, 0(t0)
        li t0, 0x00100000
        li t1, 0x5555
        sw t1, 0(t0)
        ",
    )
}

/// Writes guest exit code 2 and fails.
fn fail_elf() -> Vec<u8> {
    build_elf(
        "
        li t0, 0x00100000
        li t1, 0x00023333
        sw t1, 0(t0)
        ",
    )
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
    assert_eq!(names, ["run"]);
}

#[test]
fn run_json_pass_returns_uart_and_steps() {
    let t = Temp::new("pass");
    fs::write(t.join("hi.elf"), hi_elf()).unwrap();
    let out = run(&["--json", "run", s(&t.join("hi.elf"))]);
    let d = data(&out, 0);
    assert_eq!(d.get("exit").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("stop").and_then(Json::as_str), Some("exit"));
    assert_eq!(d.get("stdout").and_then(Json::as_str), Some("hi"));
    assert!(d.get("steps").and_then(Json::as_i64).unwrap() > 0);
    assert!(d.get("duration_ms").and_then(Json::as_i64).is_some());
    assert!(d.get("stdout_truncated").is_none());
}

#[test]
fn run_json_guest_failure_is_ok_with_exit_code() {
    let t = Temp::new("fail");
    fs::write(t.join("fail.elf"), fail_elf()).unwrap();
    let out = run(&["--json", "run", s(&t.join("fail.elf"))]);
    let d = data(&out, 1);
    assert_eq!(d.get("exit").and_then(Json::as_i64), Some(2));
    assert_eq!(d.get("stop").and_then(Json::as_str), Some("exit"));
}

#[test]
fn run_json_step_limit_is_ok_with_stop() {
    let t = Temp::new("steps");
    fs::write(t.join("hi.elf"), hi_elf()).unwrap();
    let out = run(&["--json", "run", s(&t.join("hi.elf")), "--steps", "1"]);
    let d = data(&out, 0);
    assert_eq!(d.get("stop").and_then(Json::as_str), Some("step_limit"));
    assert_eq!(d.get("exit"), Some(&Json::Null));
    assert_eq!(d.get("steps").and_then(Json::as_i64), Some(1));
}

#[test]
fn json_bad_elf_is_invalid() {
    let t = Temp::new("bad");
    fs::write(t.join("junk.elf"), b"not an elf").unwrap();
    let out = run(&["--json", "run", s(&t.join("junk.elf"))]);
    let error = fail(&out, 1, "run");
    assert_eq!(error_code(&error), "invalid");
    assert!(error
        .get("message")
        .and_then(Json::as_str)
        .unwrap()
        .starts_with("invalid ELF"));
}

#[test]
fn json_missing_file_is_io() {
    let t = Temp::new("missing");
    let out = run(&["--json", "run", s(&t.join("nope.elf"))]);
    assert_eq!(error_code(&fail(&out, 1, "run")), "io");
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
fn json_trace_is_refused_as_usage() {
    let out = run(&["--json", "run", "x.elf", "--trace"]);
    assert_eq!(error_code(&fail(&out, 2, "run")), "usage");
}

#[test]
fn text_mode_unchanged() {
    let t = Temp::new("text");
    fs::write(t.join("hi.elf"), hi_elf()).unwrap();
    let out = run(&["run", s(&t.join("hi.elf"))]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("hi\nexit: Pass\n"), "{stdout:?}");
    let version = run(&["--version"]);
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        "silicon 0.1.0\n"
    );
}
