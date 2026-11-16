#![cfg(feature = "std")]
#![allow(missing_docs, clippy::unwrap_used)]
// Agent-interface tests for the chronos binary: --describe and --json (docs/ai-computer/ACI.md).

use aci::json::Json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!("chronos-aci-{}-{tag}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn chronos(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chronos"))
        .args(args)
        .current_dir(dir)
        .env("CHRONOS_NAME", "Test User")
        .env("CHRONOS_EMAIL", "test@example.invalid")
        .output()
        .expect("failed to execute chronos")
}

/// Run `chronos --json ARGS` and parse stdout as exactly one validated envelope.
fn cj(dir: &Path, args: &[&str]) -> (Json, Option<i32>) {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    let output = chronos(dir, &all);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(
        stdout.matches('\n').count(),
        1,
        "more than one line: {stdout:?}"
    );
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    (value, output.status.code())
}

/// The data of a successful `chronos --json ARGS`.
fn ok(dir: &Path, args: &[&str]) -> Json {
    let (value, code) = cj(dir, args);
    assert_eq!(
        value.get("ok"),
        Some(&Json::Bool(true)),
        "{args:?}: {value:?}"
    );
    assert_eq!(code, Some(0), "{args:?}");
    value.get("data").unwrap().clone()
}

/// The error code of a failed `chronos --json ARGS`, checked against the exit code.
fn fail(dir: &Path, args: &[&str]) -> (String, Json) {
    let (value, code) = cj(dir, args);
    assert_eq!(
        value.get("ok"),
        Some(&Json::Bool(false)),
        "{args:?}: {value:?}"
    );
    let error = value.get("error").unwrap().clone();
    let name = error.get("code").unwrap().as_str().unwrap().to_string();
    let want = if name == "usage" { 2 } else { 1 };
    assert_eq!(code, Some(want), "{args:?}: exit code for {name}");
    (name, error)
}

fn init(dir: &Path) {
    ok(dir, &["init"]);
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, text).unwrap();
}

fn commit(dir: &Path, msg: &str) -> Json {
    ok(dir, &["commit", "-m", msg])
}

fn items(data: &Json) -> &[Json] {
    data.get("items").unwrap().as_array().unwrap()
}

fn text(v: &Json, key: &str) -> String {
    v.get(key).unwrap().as_str().unwrap().to_string()
}

fn names(list: &Json) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("path").unwrap().as_str().unwrap().to_string())
        .collect()
}

fn strs(list: &Json) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

fn change_of(list: &Json, path: &str) -> String {
    list.as_array()
        .unwrap()
        .iter()
        .find(|c| c.get("path").and_then(Json::as_str) == Some(path))
        .map(|c| text(c, "change"))
        .unwrap_or_else(|| panic!("{path} not in {list:?}"))
}

#[test]
fn describe_is_a_valid_manifest() {
    let dir = scratch("describe");
    let output = chronos(&dir, &["--describe"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let manifest = Json::parse(stdout.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let mut got: Vec<String> = manifest
        .get("commands")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| text(c, "name"))
        .collect();
    got.sort();
    let mut want = vec![
        "add", "branch", "cat-file", "checkout", "commit", "diff", "export", "import", "init",
        "log", "merge", "mv", "reset", "restore", "rm", "serve", "show", "stash", "status",
        "switch", "tag",
    ];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let dir = scratch("usage");
    let (name, error) = fail(&dir, &["nosuch"]);
    assert_eq!(name, "usage");
    assert!(error.get("hint").is_some());
    let output = chronos(&dir, &["--json", "nosuch"]);
    let value = Json::parse(String::from_utf8_lossy(&output.stdout).trim_end()).unwrap();
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(""));
}

#[test]
fn json_not_a_repository_is_not_found() {
    let dir = scratch("norepo");
    let (name, error) = fail(&dir, &["status"]);
    assert_eq!(name, "not_found");
    assert_eq!(text(&error, "hint"), "run: chronos --json init");
}

#[test]
fn json_commit_without_message_is_usage_and_returns_at_once() {
    let dir = scratch("nomsg");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    ok(&dir, &["add", "a.txt"]);
    let (name, error) = fail(&dir, &["commit"]);
    assert_eq!(name, "usage");
    assert!(text(&error, "hint").contains("-m MSG"));
}

#[test]
fn init_add_commit_show_and_log_shapes() {
    let dir = scratch("shapes");
    let data = ok(&dir, &["init"]);
    assert!(text(&data, "path").ends_with(".chronos"));
    write(&dir, "a.txt", "one\n");
    let added = ok(&dir, &["add", "a.txt"]);
    assert_eq!(added.get("paths").unwrap().as_array().unwrap().len(), 1);
    let made = commit(&dir, "first");
    assert_eq!(text(&made, "id").len(), 64);
    assert_eq!(text(&made, "branch"), "main");
    assert_eq!(made.get("count").and_then(Json::as_i64), Some(1));
    let shown = ok(&dir, &["show"]);
    assert_eq!(text(&shown, "id"), text(&made, "id"));
    assert_eq!(text(&shown, "message"), "first");
    assert_eq!(text(&shown, "author"), "Test User <test@example.invalid>");
    assert!(shown.get("time").and_then(Json::as_i64).is_some());
    assert_eq!(shown.get("parents").unwrap().as_array().unwrap().len(), 0);
    let file = &shown.get("files").unwrap().as_array().unwrap()[0];
    assert_eq!(text(file, "path"), "a.txt");
    assert_eq!(text(file, "change"), "added");
}

#[test]
fn status_lists_each_change_in_the_right_list() {
    let dir = scratch("status");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    write(&dir, "gone.txt", "bye\n");
    ok(&dir, &["add", "a.txt", "gone.txt"]);
    commit(&dir, "base");
    write(&dir, "new.txt", "new\n");
    write(&dir, "src dir/b c.txt", "spaced\n");
    ok(&dir, &["add", "new.txt", "src dir/b c.txt"]);
    ok(&dir, &["rm", "gone.txt"]);
    write(&dir, "a.txt", "two\n");
    write(&dir, "u.txt", "untracked\n");
    let s = ok(&dir, &["status"]);
    assert_eq!(s.get("branch").and_then(Json::as_str), Some("main"));
    assert_eq!(text(&s, "head").len(), 64);
    assert_eq!(s.get("clean").and_then(Json::as_bool), Some(false));
    let staged = s.get("staged").unwrap();
    assert_eq!(change_of(staged, "new.txt"), "added");
    assert_eq!(change_of(staged, "gone.txt"), "deleted");
    assert_eq!(change_of(staged, "src dir/b c.txt"), "added");
    assert_eq!(names(s.get("unstaged").unwrap()), ["a.txt"]);
    assert_eq!(change_of(s.get("unstaged").unwrap(), "a.txt"), "modified");
    let untracked: Vec<&str> = s
        .get("untracked")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(untracked, ["u.txt"]);
    assert_eq!(s.get("unmerged").unwrap().as_array().unwrap().len(), 0);
}

#[test]
fn status_on_a_new_repository_has_no_head() {
    let dir = scratch("newrepo");
    init(&dir);
    let s = ok(&dir, &["status"]);
    assert_eq!(s.get("head"), Some(&Json::Null));
    assert_eq!(s.get("clean").and_then(Json::as_bool), Some(true));
}

#[test]
fn log_pages_with_limit_and_offset() {
    let dir = scratch("log");
    init(&dir);
    for i in 0..5 {
        write(&dir, "f.txt", &format!("{i}\n"));
        ok(&dir, &["add", "f.txt"]);
        commit(&dir, &format!("c{i}"));
    }
    let page = ok(&dir, &["log", "--limit", "2"]);
    assert_eq!(items(&page).len(), 2);
    assert_eq!(page.get("total").and_then(Json::as_i64), Some(5));
    assert_eq!(page.get("more"), Some(&Json::Bool(true)));
    assert_eq!(page.get("next_offset").and_then(Json::as_i64), Some(2));
    // Order is by commit time, ties by id, so the commits made in one second may come in any order.
    let first = &items(&page)[0];
    assert!(text(first, "message").starts_with('c'));
    assert_eq!(first.get("parents").unwrap().as_array().unwrap().len(), 1);
    assert!(first.get("time").and_then(Json::as_i64).is_some());
    let last = ok(&dir, &["log", "--limit", "2", "--offset", "4"]);
    assert_eq!(items(&last).len(), 1);
    assert_eq!(last.get("more"), Some(&Json::Bool(false)));
    assert!(last.get("next_offset").is_none());
}

#[test]
fn diff_lists_files_with_hunks() {
    let dir = scratch("diff");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    ok(&dir, &["add", "a.txt"]);
    commit(&dir, "base");
    write(&dir, "a.txt", "one\ntwo\n");
    let d = ok(&dir, &["diff"]);
    assert_eq!(d.get("total").and_then(Json::as_i64), Some(1));
    assert_eq!(d.get("more"), Some(&Json::Bool(false)));
    let file = &d.get("files").unwrap().as_array().unwrap()[0];
    assert_eq!(text(file, "path"), "a.txt");
    assert_eq!(text(file, "change"), "modified");
    let hunk = &file.get("hunks").unwrap().as_array().unwrap()[0];
    assert_eq!(hunk.get("old_start").and_then(Json::as_i64), Some(1));
    assert_eq!(hunk.get("old_lines").and_then(Json::as_i64), Some(1));
    assert_eq!(hunk.get("new_start").and_then(Json::as_i64), Some(1));
    assert_eq!(hunk.get("new_lines").and_then(Json::as_i64), Some(2));
    let lines: Vec<&str> = hunk
        .get("lines")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(lines, [" one", "+two"]);
    let staged = ok(&dir, &["diff", "--staged"]);
    assert_eq!(staged.get("files").unwrap().as_array().unwrap().len(), 0);
}

#[test]
fn conflicting_merge_is_conflict_and_status_lists_unmerged() {
    let dir = scratch("conflict");
    init(&dir);
    write(&dir, "k.txt", "base\n");
    ok(&dir, &["add", "k.txt"]);
    commit(&dir, "base");
    ok(&dir, &["branch", "cf"]);
    write(&dir, "k.txt", "mainside\n");
    ok(&dir, &["add", "k.txt"]);
    commit(&dir, "mainside");
    ok(&dir, &["switch", "cf"]);
    write(&dir, "k.txt", "cfside\n");
    ok(&dir, &["add", "k.txt"]);
    commit(&dir, "cfside");
    ok(&dir, &["switch", "main"]);
    let (name, error) = fail(&dir, &["merge", "cf"]);
    assert_eq!(name, "conflict");
    assert!(text(&error, "message").contains("k.txt"));
    assert_eq!(
        text(&error, "hint"),
        "fix the files, then run: chronos --json add PATH... and chronos --json commit -m MSG, or chronos --json merge --abort"
    );
    let s = ok(&dir, &["status"]);
    assert_eq!(strs(s.get("unmerged").unwrap()), ["k.txt"]);
    let aborted = ok(&dir, &["merge", "--abort"]);
    assert_eq!(text(&aborted, "status"), "aborted");
}

#[test]
fn fast_forward_merge_and_switch_shapes() {
    let dir = scratch("ff");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    ok(&dir, &["add", "a.txt"]);
    commit(&dir, "base");
    ok(&dir, &["branch", "feat"]);
    let sw = ok(&dir, &["switch", "feat"]);
    assert_eq!(text(&sw, "branch"), "feat");
    write(&dir, "a.txt", "two\n");
    ok(&dir, &["add", "a.txt"]);
    let made = commit(&dir, "feat work");
    ok(&dir, &["switch", "main"]);
    let merged = ok(&dir, &["merge", "feat"]);
    assert_eq!(text(&merged, "status"), "fast_forward");
    assert_eq!(text(&merged, "id"), text(&made, "id"));
}

#[test]
fn refs_stash_and_tag_shapes() {
    let dir = scratch("refs");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    ok(&dir, &["add", "a.txt"]);
    let base = commit(&dir, "base");
    let made = ok(&dir, &["branch", "feat"]);
    assert_eq!(text(&made, "name"), "feat");
    assert_eq!(text(&made, "id"), text(&base, "id"));
    let list = ok(&dir, &["branch"]);
    let current: Vec<&str> = items(&list)
        .iter()
        .filter(|b| b.get("current").and_then(Json::as_bool) == Some(true))
        .map(|b| b.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(current, ["main"]);
    let tag = ok(&dir, &["tag", "v1"]);
    assert_eq!(text(&tag, "name"), "v1");
    assert_eq!(items(&ok(&dir, &["tag"])).len(), 1);
    write(&dir, "a.txt", "two\n");
    let stashed = ok(&dir, &["stash"]);
    assert_eq!(text(&stashed, "id").len(), 64);
    let stack = ok(&dir, &["stash", "list"]);
    assert_eq!(text(&items(&stack)[0], "name"), "stash@{0}");
    let popped = ok(&dir, &["stash", "pop"]);
    assert_eq!(text(&popped, "name"), "stash@{0}");
    assert_eq!(text(&popped, "id"), text(&stashed, "id"));
    let deleted = ok(&dir, &["tag", "-d", "v1"]);
    assert_eq!(text(&deleted, "name"), "v1");
    let gone = ok(&dir, &["branch", "-d", "feat"]);
    assert_eq!(text(&gone, "name"), "feat");
}

#[test]
fn path_moves_report_paths() {
    let dir = scratch("moves");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    write(&dir, "c.txt", "two\n");
    ok(&dir, &["add", "a.txt", "c.txt"]);
    commit(&dir, "base");
    let moved = ok(&dir, &["mv", "a.txt", "b.txt"]);
    assert_eq!(moved.get("paths").unwrap().as_array().unwrap().len(), 2);
    let restored = ok(&dir, &["restore", "--staged", "b.txt"]);
    assert_eq!(restored.get("paths").unwrap().as_array().unwrap().len(), 1);
    let removed = ok(&dir, &["rm", "c.txt"]);
    assert_eq!(removed.get("paths").unwrap().as_array().unwrap().len(), 1);
    let reset = ok(&dir, &["reset"]);
    assert!(reset.get("paths").unwrap().as_array().is_some());
}

#[test]
fn checkout_reports_branch_and_id() {
    let dir = scratch("checkout");
    init(&dir);
    write(&dir, "a.txt", "one\n");
    ok(&dir, &["add", "a.txt"]);
    commit(&dir, "base");
    ok(&dir, &["branch", "feat"]);
    let data = ok(&dir, &["checkout", "feat"]);
    assert_eq!(text(&data, "branch"), "feat");
    assert_eq!(text(&data, "id").len(), 64);
}

#[test]
fn serve_is_unsupported_under_json() {
    let dir = scratch("serve");
    init(&dir);
    let (name, error) = fail(&dir, &["serve"]);
    assert_eq!(name, "unsupported");
    assert!(text(&error, "hint").contains("serve"));
}

#[test]
fn path_with_space_and_backslash_stays_forward_slashed() {
    let dir = scratch("pathspace");
    init(&dir);
    write(&dir, "dir with space/x y.txt", "x\n");
    ok(&dir, &["add", "dir with space/x y.txt"]);
    let s = ok(&dir, &["status"]);
    assert_eq!(names(s.get("staged").unwrap()), ["dir with space/x y.txt"]);
}

#[test]
fn text_status_is_unchanged() {
    let dir = scratch("textstatus");
    let init_out = chronos(&dir, &["init"]);
    assert_eq!(init_out.status.code(), Some(0));
    write(&dir, "u.txt", "untracked\n");
    let out = chronos(&dir, &["status"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "on branch main\nUntracked files:\n? u.txt\n"
    );
}
