// Agent-interface tests for the kairos binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_kairos");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("kairos_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn workload(&self, text: &str) -> String {
        let path = self.0.join("workload.txt");
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

const WORKLOAD: &str = "cpus 2\ntask a 1024 10\ntask b 512 10\n";

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
fn json_sim_reports_timelines_and_shares() {
    let t = Temp::new("sim");
    let page = data(&run(&["--json", "sim", &t.workload(WORKLOAD)]));
    assert_eq!(field(&page, "cpus").as_i64(), Some(2));
    assert_eq!(field(&page, "total").as_i64(), Some(2));
    assert_eq!(field(&page, "more"), &Json::Bool(false));
    let ticks = field(&page, "ticks").as_i64().unwrap();
    assert!(ticks >= 10, "{ticks}");
    let cpu0 = &field(&page, "items").as_array().unwrap()[0];
    assert_eq!(field(cpu0, "cpu").as_i64(), Some(0));
    assert_eq!(field(cpu0, "slots").as_array().unwrap().len() as i64, ticks);
    let tasks = field(&page, "tasks").as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(field(&tasks[0], "name").as_str(), Some("a"));
    assert_eq!(field(&tasks[0], "weight").as_i64(), Some(1024));
    assert_eq!(field(&tasks[0], "ticks").as_i64(), Some(10));
    let Json::Num(expected) = *field(&tasks[0], "expected") else {
        panic!("expected is not a number");
    };
    assert!((expected - 2.0 / 3.0).abs() < 1e-9, "{expected}");
}

#[test]
fn json_sim_pages_the_cpus() {
    let t = Temp::new("page");
    let file = t.workload(WORKLOAD);
    let page = data(&run(&["--json", "sim", &file, "--limit", "1"]));
    assert_eq!(field(&page, "items").as_array().unwrap().len(), 1);
    assert_eq!(field(&page, "more"), &Json::Bool(true));
    assert_eq!(field(&page, "next_offset").as_i64(), Some(1));
    let rest = data(&run(&["--json", "sim", &file, "--offset", "1"]));
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
    let no_cpus = t.workload("task a 1 1\n");
    assert_eq!(
        fail(&run(&["--json", "sim", &no_cpus]), 1, "sim"),
        "invalid"
    );
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    assert_eq!(fail(&output, 2, ""), "usage");
    let value = envelope(&output);
    let hint = value.get("error").unwrap().get("hint").unwrap();
    assert_eq!(hint.as_str(), Some("run: kairos --describe"));
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let out = run(&["sim", &t.workload(WORKLOAD)]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("cpu 0: "), "{}", lines[0]);
    assert!(
        lines[2].starts_with("task a: ticks=10 share="),
        "{}",
        lines[2]
    );
    assert!(lines[3].ends_with("expected=0.3333"), "{}", lines[3]);
    let limited = run(&["sim", &t.workload(WORKLOAD), "--limit", "1"]);
    assert_eq!(limited.status.code(), Some(2));
}
