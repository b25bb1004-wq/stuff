// Agent-interface tests for the pandora binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use pandora::state::{self, Status};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pandora");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pandora_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn envelope(out: &Output) -> Json {
    let text = stdout(out);
    Json::parse(text.trim()).unwrap_or_else(|e| panic!("bad envelope {text:?}: {e}"))
}

fn field<'a>(j: &'a Json, key: &str) -> &'a Json {
    j.get(key)
        .unwrap_or_else(|| panic!("missing {key} in {j:?}"))
}

#[test]
fn describe_prints_a_complete_manifest() {
    let out = run(&["--describe"]);
    assert!(out.status.success());
    let m = Json::parse(stdout(&out).trim()).unwrap();
    aci::manifest::validate_manifest(&m).unwrap();
    assert_eq!(field(&m, "tool").as_str(), Some("pandora"));
    let names: Vec<&str> = field(&m, "commands")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| field(c, "name").as_str().unwrap())
        .collect();
    assert_eq!(names, ["ps", "kill", "rm", "run", "pull"]);
}

#[test]
fn json_ps_lists_containers_as_a_page() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let t = Temp::new("ps");
    let store = t.0.join("store");
    state::create(&store, "web-1", "img").unwrap();
    let out = run(&["--store", store.to_str().unwrap(), "--json", "ps"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let e = envelope(&out);
    aci::envelope::validate_envelope(&e).unwrap();
    assert_eq!(field(&e, "ok"), &Json::Bool(true));
    let data = field(&e, "data");
    let items = field(data, "items").as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(field(&items[0], "id").as_str(), Some("web-1"));
    assert_eq!(field(&items[0], "status").as_str(), Some("created"));
    assert_eq!(field(&items[0], "pid"), &Json::Null);
    assert_eq!(field(data, "total"), &Json::Int(1));
    assert_eq!(field(data, "more"), &Json::Bool(false));
}

#[test]
fn json_rm_removes_a_stopped_container() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let t = Temp::new("rm");
    let store = t.0.join("store");
    state::create(&store, "gone", "img").unwrap();
    let out = run(&["--store", store.to_str().unwrap(), "--json", "rm", "gone"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        field(field(&envelope(&out), "data"), "id").as_str(),
        Some("gone")
    );
    assert!(state::read(&store, "gone").is_err());
}

#[test]
fn json_kill_refuses_a_container_that_is_not_running() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let t = Temp::new("kill");
    let store = t.0.join("store");
    state::create(&store, "idle", "img").unwrap();
    assert_eq!(state::read(&store, "idle").unwrap().status, Status::Created);
    let out = run(&["--store", store.to_str().unwrap(), "--json", "kill", "idle"]);
    assert!(!out.status.success());
    let e = envelope(&out);
    assert_eq!(field(&e, "ok"), &Json::Bool(false));
    assert_eq!(field(field(&e, "error"), "code").as_str(), Some("invalid"));
}

#[test]
fn json_missing_container_is_not_found() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let t = Temp::new("missing");
    let store = t.0.join("store");
    let out = run(&["--store", store.to_str().unwrap(), "--json", "rm", "nope"]);
    assert!(!out.status.success());
    assert_eq!(
        field(field(&envelope(&out), "error"), "code").as_str(),
        Some("not_found")
    );
}

#[test]
fn json_run_and_pull_are_unsupported() {
    for cmd in ["run", "pull"] {
        let out = run(&["--json", cmd]);
        let e = envelope(&out);
        assert_eq!(field(&e, "ok"), &Json::Bool(false), "{cmd}");
        assert_eq!(
            field(field(&e, "error"), "code").as_str(),
            Some("unsupported"),
            "{cmd}"
        );
    }
}

#[test]
fn json_on_windows_is_unsupported_with_the_text_exit_code() {
    if cfg!(target_os = "linux") {
        return;
    }
    let out = run(&["--json", "ps"]);
    assert_eq!(out.status.code(), Some(0));
    let e = envelope(&out);
    assert_eq!(
        field(field(&e, "error"), "code").as_str(),
        Some("unsupported")
    );
    assert_eq!(
        field(field(&e, "error"), "message").as_str(),
        Some("pandora needs Linux")
    );
}

#[test]
fn text_ps_is_unchanged() {
    let t = Temp::new("text");
    let store = t.0.join("store");
    let out = run(&["--store", store.to_str().unwrap(), "ps"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "ID  IMAGE  STATUS\n");
}
