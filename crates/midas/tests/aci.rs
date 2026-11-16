// Agent-interface tests for the midas binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_midas");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("midas_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn trace(&self, text: &str) -> String {
        let path = self.0.join("trace.txt");
        fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
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

fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "not one line: {stdout:?}");
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

fn data(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

/// The error code of a failed envelope; checks the exit code and the command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> String {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    value
        .get("error")
        .unwrap()
        .get("code")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("no {key} in {value:?}"))
}

fn item(page: &Json, index: usize) -> &Json {
    &field(page, "items").as_array().unwrap()[index]
}

#[test]
fn describe_is_a_valid_manifest() {
    let output = run(&["--describe"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = manifest
        .get("commands")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(names, ["sim"]);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    assert!(help.contains("sim FILE"), "{help}");
}

#[test]
fn json_sim_returns_one_record_per_step() {
    let t = Temp::new("sim");
    let trace = t.trace("heap 65536\nalloc 100\nfree 0\n");
    let page = data(&run(&["--json", "sim", &trace]));
    assert_eq!(field(&page, "total").as_i64(), Some(3));
    assert_eq!(field(&page, "more"), &Json::Bool(false));
    assert_eq!(field(item(&page, 0), "op").as_str(), Some("heap"));
    assert_eq!(field(item(&page, 0), "size").as_i64(), Some(65536));
    let alloc = item(&page, 1);
    assert_eq!(field(alloc, "op").as_str(), Some("alloc"));
    assert_eq!(field(alloc, "id").as_i64(), Some(0));
    assert_eq!(field(alloc, "size").as_i64(), Some(100));
    assert_eq!(field(alloc, "check"), &Json::Bool(true));
    let total = field(alloc, "total").as_i64().unwrap();
    let used = field(alloc, "used").as_i64().unwrap();
    assert_eq!(field(alloc, "free").as_i64(), Some(total - used));
    let free = item(&page, 2);
    assert_eq!(field(free, "op").as_str(), Some("free"));
    assert_eq!(field(free, "id").as_i64(), Some(0));
    assert!(free.get("size").is_none());
}

#[test]
fn json_sim_pages_the_records() {
    let t = Temp::new("page");
    let trace = t.trace("heap 65536\nalloc 100\nfree 0\n");
    let first = data(&run(&["--json", "sim", &trace, "--limit", "2"]));
    assert_eq!(field(&first, "items").as_array().unwrap().len(), 2);
    assert_eq!(field(&first, "more"), &Json::Bool(true));
    assert_eq!(field(&first, "next_offset").as_i64(), Some(2));
    let rest = data(&run(&["--json", "sim", &trace, "--offset", "2"]));
    assert_eq!(field(&rest, "items").as_array().unwrap().len(), 1);
    assert!(rest.get("next_offset").is_none());
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let t = Temp::new("bad");
    let missing = t.0.join("missing.txt");
    assert_eq!(
        fail(
            &run(&["--json", "sim", missing.to_str().unwrap()]),
            1,
            "sim"
        ),
        "not_found"
    );
    let bogus = t.trace("heap 65536\nbogus 1\n");
    assert_eq!(fail(&run(&["--json", "sim", &bogus]), 1, "sim"), "invalid");
    let early = t.trace("alloc 10\n");
    assert_eq!(fail(&run(&["--json", "sim", &early]), 1, "sim"), "invalid");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    assert_eq!(fail(&output, 2, ""), "usage");
    let value = envelope(&output);
    let hint = value.get("error").unwrap().get("hint").unwrap();
    assert_eq!(hint.as_str(), Some("run: midas --describe"));
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let trace = t.trace("heap 65536\nalloc 100\nfree 0\n");
    let out = run(&["sim", &trace]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].starts_with("step 1: heap 65536: used="),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("step 2: alloc #0 (100B): used="),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("step 3: free #0: used="),
        "{}",
        lines[2]
    );
    assert!(lines[2].ends_with("check=true"), "{}", lines[2]);
    // The JSON-only flags stay usage errors in text mode.
    let limited = run(&["sim", &trace, "--limit", "2"]);
    assert_eq!(limited.status.code(), Some(2));
}
