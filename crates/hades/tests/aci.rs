// Agent-interface tests for the hades binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_hades");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("hades_aci_{name}_{}_{id}", std::process::id()));
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

/// The error code of a failed envelope, checking the exit code and that the command name is `cmd`.
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

/// A new image with 256 blocks, made through --json.
fn image(t: &Temp, name: &str) -> PathBuf {
    let img = t.join(name);
    data(&run(&["--json", "mkfs", s(&img), "--blocks", "256"]));
    img
}

fn host_file(t: &Temp, name: &str, bytes: &[u8]) -> PathBuf {
    let path = t.join(name);
    fs::write(&path, bytes).unwrap();
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
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    let subcommands = ["mkfs", "ls", "cat", "put", "rm", "mkdir", "fsck"];
    assert_eq!(names, subcommands);
    for name in subcommands {
        assert!(
            help.contains(&format!("hades {name} ")),
            "{name} not in --help"
        );
    }
}

#[test]
fn json_mkfs_returns_the_image_path() {
    let t = Temp::new("mkfs");
    let img = t.join("disk.img");
    let value = data(&run(&["--json", "mkfs", s(&img), "--blocks", "256"]));
    assert_eq!(field(&value, "path").as_str(), Some(fwd(&img).as_str()));
    assert_eq!(field(&value, "blocks").as_i64(), Some(256));
    assert!(img.exists());
}

#[test]
fn json_ls_pages_a_folder_of_three() {
    let t = Temp::new("ls");
    let img = image(&t, "disk.img");
    data(&run(&["--json", "mkdir", s(&img), "/d"]));
    for name in ["a.txt", "b.txt", "c.txt"] {
        let src = host_file(&t, name, b"hello");
        data(&run(&[
            "--json",
            "put",
            s(&img),
            s(&src),
            &format!("/d/{name}"),
        ]));
    }
    let first = data(&run(&["--json", "ls", s(&img), "/d", "--limit", "2"]));
    assert_eq!(field(&first, "items").as_array().unwrap().len(), 2);
    assert_eq!(field(&first, "total").as_i64(), Some(3));
    assert_eq!(field(&first, "more"), &Json::Bool(true));
    assert_eq!(field(&first, "next_offset").as_i64(), Some(2));
    let item = &field(&first, "items").as_array().unwrap()[0];
    assert_eq!(field(item, "kind").as_str(), Some("file"));
    assert_eq!(field(item, "size").as_i64(), Some(5));

    let last = data(&run(&["--json", "ls", s(&img), "/d", "--offset", "2"]));
    assert_eq!(field(&last, "items").as_array().unwrap().len(), 1);
    assert_eq!(field(&last, "more"), &Json::Bool(false));
    assert!(last.get("next_offset").is_none());
}

#[test]
fn json_put_rm_mkdir_report_their_paths() {
    let t = Temp::new("put");
    let img = image(&t, "disk.img");
    let src = host_file(&t, "src.txt", b"12345678");
    let put = data(&run(&["--json", "put", s(&img), s(&src), "/note.txt"]));
    assert_eq!(field(&put, "path").as_str(), Some("/note.txt"));
    assert_eq!(field(&put, "bytes").as_i64(), Some(8));
    let mkdir = data(&run(&["--json", "mkdir", s(&img), "/dir"]));
    assert_eq!(field(&mkdir, "path").as_str(), Some("/dir"));
    let rm = data(&run(&["--json", "rm", s(&img), "/note.txt"]));
    assert_eq!(field(&rm, "path").as_str(), Some("/note.txt"));
}

#[test]
fn json_cat_caps_text_at_16384_bytes() {
    let t = Temp::new("cat");
    let img = image(&t, "disk.img");
    let big = host_file(&t, "big.txt", &[b'x'; 20000]);
    data(&run(&["--json", "put", s(&img), s(&big), "/big.txt"]));
    let value = data(&run(&["--json", "cat", s(&img), "/big.txt"]));
    assert_eq!(field(&value, "text").as_str().unwrap().len(), 16384);
    assert_eq!(field(&value, "truncated"), &Json::Bool(true));
    assert_eq!(field(&value, "bytes").as_i64(), Some(20000));
    assert_eq!(field(&value, "path").as_str(), Some("/big.txt"));
}

#[test]
fn json_cat_out_writes_every_byte() {
    let t = Temp::new("catout");
    let img = image(&t, "disk.img");
    let big = host_file(&t, "big.txt", &[b'y'; 20000]);
    data(&run(&["--json", "put", s(&img), s(&big), "/big.txt"]));
    let out = t.join("copy.bin");
    let value = data(&run(&[
        "--json",
        "cat",
        s(&img),
        "/big.txt",
        "--out",
        s(&out),
    ]));
    assert_eq!(field(&value, "bytes").as_i64(), Some(20000));
    assert_eq!(fs::read(&out).unwrap().len(), 20000);
    assert_eq!(field(&value, "path").as_str(), Some(fwd(&out).as_str()));
}

#[test]
fn json_fsck_clean_image() {
    let t = Temp::new("fsck");
    let img = image(&t, "disk.img");
    let value = data(&run(&["--json", "fsck", s(&img)]));
    assert_eq!(field(&value, "clean"), &Json::Bool(true));
    assert_eq!(field(&value, "problems").as_array().unwrap().len(), 0);
    assert_eq!(field(&value, "count").as_i64(), Some(0));
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let t = Temp::new("bad");
    let img = image(&t, "disk.img");
    let missing = t.join("nope.img");
    assert_eq!(
        fail(&run(&["--json", "ls", s(&missing), "/"]), 1, "ls"),
        "not_found"
    );
    assert_eq!(
        fail(&run(&["--json", "ls", s(&img), "/nope"]), 1, "ls"),
        "not_found"
    );
    data(&run(&["--json", "mkdir", s(&img), "/dir"]));
    assert_eq!(
        fail(&run(&["--json", "mkdir", s(&img), "/dir"]), 1, "mkdir"),
        "exists"
    );
    assert_eq!(
        fail(
            &run(&["--json", "mkfs", s(&t.join("tiny.img")), "--blocks", "4"]),
            2,
            "mkfs"
        ),
        "invalid"
    );
    assert_eq!(
        fail(&run(&["--json", "fsck", s(&missing)]), 1, "fsck"),
        "not_found"
    );
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    assert_eq!(fail(&output, 2, ""), "usage");
    let value = envelope(&output);
    let hint = value.get("error").unwrap().get("hint").unwrap();
    assert_eq!(hint.as_str(), Some("run: hades --describe"));
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let dir = t.join("a b");
    fs::create_dir_all(&dir).unwrap();
    let img = dir.join("disk.img");
    let value = data(&run(&["--json", "mkfs", s(&img), "--blocks", "256"]));
    let path = field(&value, "path").as_str().unwrap();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("a b/disk.img"), "{path}");
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let img = t.join("disk.img");
    let out = run(&["mkfs", s(&img), "--blocks", "256"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("mkfs {} (256 blocks)\n", img.display())
    );
    // The JSON-only flags stay usage errors in text mode.
    let limited = run(&["ls", s(&img), "/", "--limit", "2"]);
    assert_eq!(limited.status.code(), Some(2));
    assert!(limited.stdout.is_empty());
}
