// Agent-interface tests for the odyssey binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_odyssey");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const PAGE: &str = "<!DOCTYPE html><html><body><p class=\"x\">hi</p><ul><li>a</li><li>b</li><li>c</li></ul></body></html>";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("odyssey_aci_{name}_{}_{id}", std::process::id()));
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

/// The error code and error object of a failed envelope, checking the exit code and command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> (String, Json) {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    let error = value.get("error").unwrap();
    let code = error.get("code").unwrap().as_str().unwrap().to_string();
    (code, error.clone())
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("no {key} in {value:?}"))
}

fn page_file(t: &Temp, name: &str, text: &str) -> PathBuf {
    let path = t.join(name);
    fs::write(&path, text).unwrap();
    path
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
    assert_eq!(names, ["tokens", "tree", "select", "render"]);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    for name in names {
        assert!(help.contains(name), "{name} not in --help");
    }
}

#[test]
fn json_tokens_lists_tags_with_attrs() {
    let t = Temp::new("tokens");
    let src = page_file(&t, "page.html", PAGE);
    let value = data(&run(&["--json", "tokens", s(&src)]));
    let items = field(&value, "items").as_array().unwrap();
    assert_eq!(field(&value, "more"), &Json::Bool(false));
    let p = items
        .iter()
        .find(|i| i.get("name").and_then(Json::as_str) == Some("p"))
        .expect("a p start tag");
    assert_eq!(field(p, "kind").as_str(), Some("start_tag"));
    let attrs = field(p, "attrs").as_array().unwrap();
    assert_eq!(field(&attrs[0], "name").as_str(), Some("class"));
    assert_eq!(field(&attrs[0], "text").as_str(), Some("x"));
    assert!(items
        .iter()
        .any(|i| field(i, "kind").as_str() == Some("doctype")));
}

#[test]
fn json_tree_nodes_in_document_order() {
    let t = Temp::new("tree");
    let src = page_file(&t, "page.html", PAGE);
    let value = data(&run(&["--json", "tree", s(&src)]));
    let nodes = field(&value, "nodes").as_array().unwrap();
    assert!(field(&value, "total").as_i64().unwrap() > 3);
    let first = &nodes[0];
    assert_eq!(field(first, "depth"), &Json::Int(0));
    assert_eq!(field(first, "kind").as_str(), Some("doctype"));
    assert!(nodes
        .iter()
        .any(|n| field(n, "name").as_str() == Some("li")));
}

#[test]
fn json_tree_cuts_long_node_text_to_400_bytes() {
    let t = Temp::new("long");
    let long = "x".repeat(1000);
    let src = page_file(&t, "long.html", &format!("<p>{long}</p>"));
    let value = data(&run(&["--json", "tree", s(&src)]));
    let node = field(&value, "nodes")
        .as_array()
        .unwrap()
        .iter()
        .find(|n| field(n, "kind").as_str() == Some("text"))
        .expect("the text node");
    assert_eq!(field(node, "text").as_str().unwrap().len(), 400);
    assert_eq!(field(node, "truncated"), &Json::Bool(true));
    assert_eq!(field(node, "bytes"), &Json::Int(1000));
}

#[test]
fn json_select_pages_matches() {
    let t = Temp::new("select");
    let src = page_file(&t, "page.html", PAGE);
    let value = data(&run(&["--json", "select", "li", s(&src), "--limit", "2"]));
    let items = field(&value, "items").as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(field(&value, "total"), &Json::Int(3));
    assert_eq!(field(&value, "more"), &Json::Bool(true));
    assert_eq!(field(&value, "next_offset"), &Json::Int(2));
    assert_eq!(
        field(&items[0], "path").as_str(),
        Some("/html/body/ul/li[1]")
    );
    assert_eq!(field(&items[0], "name").as_str(), Some("li"));
    assert_eq!(field(&items[0], "text").as_str(), Some("a"));
}

#[test]
fn json_select_bad_selector_is_invalid() {
    let t = Temp::new("badsel");
    let src = page_file(&t, "page.html", PAGE);
    let output = run(&["--json", "select", "div[[", s(&src)]);
    assert_eq!(fail(&output, 1, "select").0, "invalid");
}

#[test]
fn json_render_writes_png() {
    let t = Temp::new("render");
    let src = page_file(&t, "page.html", PAGE);
    let out = t.join("page.png");
    let value = data(&run(&[
        "--json",
        "render",
        s(&src),
        "--width",
        "320",
        "--out",
        s(&out),
    ]));
    let bytes = fs::read(&out).unwrap();
    assert_eq!(field(&value, "path").as_str(), Some(fwd(&out).as_str()));
    assert_eq!(field(&value, "bytes"), &Json::Int(bytes.len() as i64));
    assert_eq!(field(&value, "width"), &Json::Int(320));
    assert!(field(&value, "height").as_i64().unwrap() >= 1);
    assert_eq!(bytes[..8], PNG_SIGNATURE);
}

#[test]
fn json_render_out_dash_is_usage() {
    let t = Temp::new("dash");
    let src = page_file(&t, "page.html", PAGE);
    let output = run(&["--json", "render", s(&src), "--out", "-"]);
    let (code, error) = fail(&output, 2, "render");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("use --out FILE.png")
    );
}

#[test]
fn json_missing_file_is_not_found_for_every_tool() {
    let t = Temp::new("missing");
    let missing = t.join("nope.html");
    let out = t.join("page.png");
    let cases: [Vec<&str>; 4] = [
        vec!["--json", "tokens", s(&missing)],
        vec!["--json", "tree", s(&missing)],
        vec!["--json", "select", "p", s(&missing)],
        vec!["--json", "render", s(&missing), "--out", s(&out)],
    ];
    for args in cases {
        let cmd = args[1];
        assert_eq!(fail(&run(&args), 1, cmd).0, "not_found", "{args:?}");
    }
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    let (code, error) = fail(&output, 2, "");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("run: odyssey --describe")
    );
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let folder = t.join("with space");
    fs::create_dir_all(&folder).unwrap();
    let src = page_file(&t, "page.html", PAGE);
    let out = folder.join("page.png");
    let value = data(&run(&["--json", "render", s(&src), "--out", s(&out)]));
    let path = field(&value, "path").as_str().unwrap();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("with space/page.png"), "{path}");
}

#[test]
fn text_mode_unchanged() {
    let t = Temp::new("text");
    let src = page_file(&t, "page.html", PAGE);
    let output = run(&["tokens", s(&src)]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("DOCTYPE html\nSTART html\n"), "{stdout}");
    assert!(stdout.contains("START p class=\"x\"\n"), "{stdout}");
}
