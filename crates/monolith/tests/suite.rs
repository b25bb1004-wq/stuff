#![allow(clippy::unwrap_used)]
//! Tests of the suite file format and the mechanical checks.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use monolith::suite::{action_line, load, parse, run_checks, Check, CheckResult, Expect, Task};
use monolith::types::{Action, Error, Result};

const EXAMPLE: &str = "\
# a comment
== t01-first-commit
prompt: Make a chronos repository in the folder proj and commit a file
| notes.txt that contains the word hello.
computer: chain1
setup: run chronos init proj
setup: write proj/seed.txt
| first line
| second line
check: chronos log proj => contains hello
check: chronos status proj => lacks untracked
check: chronos fsck proj => exit 0
check: answer => contains 42
solution: write proj/notes.txt
| hello
solution: run chronos add proj notes.txt
solution: done committed
";

/// The error of parsing `text`, which must fail.
fn parse_err(text: &str) -> Error {
    parse(text).unwrap_err()
}

/// Parses `text`, which must be valid, and returns its one task.
fn one_task(text: &str) -> Task {
    let mut tasks = parse(text).unwrap();
    assert_eq!(tasks.len(), 1);
    tasks.remove(0)
}

fn scratch_file(name: &str, text: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "monolith-suite-{}-{name}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("suite.txt");
    std::fs::write(&path, text).unwrap();
    path
}

fn cleanup(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Checks built from the given check lines, in a one-task suite.
fn checks_of(lines: &str) -> Task {
    one_task(&format!("== t\nprompt: p\n{lines}\n"))
}

#[test]
fn the_example_parses_to_the_exact_task() {
    let tasks = parse(EXAMPLE).unwrap();
    assert_eq!(tasks.len(), 1);
    let t = &tasks[0];
    assert_eq!(t.id, "t01-first-commit");
    assert_eq!(
        t.prompt,
        "Make a chronos repository in the folder proj and commit a file\nnotes.txt that contains the word hello."
    );
    assert_eq!(t.computer.as_deref(), Some("chain1"));
    assert_eq!(
        t.setup,
        vec![
            Action::Run {
                line: "chronos init proj".into()
            },
            Action::Write {
                path: "proj/seed.txt".into(),
                content: "first line\nsecond line\n".into()
            },
        ]
    );
    assert_eq!(
        t.checks,
        vec![
            Check::Command {
                line: "chronos log proj".into(),
                expect: Expect::Contains("hello".into())
            },
            Check::Command {
                line: "chronos status proj".into(),
                expect: Expect::Lacks("untracked".into())
            },
            Check::Command {
                line: "chronos fsck proj".into(),
                expect: Expect::Exit(0)
            },
            Check::Answer {
                expect: Expect::Contains("42".into())
            },
        ]
    );
    assert_eq!(
        t.solution,
        vec![
            Action::Write {
                path: "proj/notes.txt".into(),
                content: "hello\n".into()
            },
            Action::Run {
                line: "chronos add proj notes.txt".into()
            },
            Action::Done {
                answer: "committed".into()
            },
        ]
    );
}

#[test]
fn two_tasks_in_one_file() {
    let text = "== a\nprompt: first\ncheck: answer => contains x\n\n== b-2\nprompt: second\ncomputer: c_1\ncheck: answer => lacks y\n";
    let tasks = parse(text).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "a");
    assert_eq!(tasks[0].prompt, "first");
    assert_eq!(tasks[0].computer, None);
    assert_eq!(tasks[1].id, "b-2");
    assert_eq!(tasks[1].computer.as_deref(), Some("c_1"));
    assert_eq!(tasks[1].prompt, "second");
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let text =
        "\n   # indented comment\n== a\n\n  prompt: p\n# between\n\ncheck: answer => lacks y\n\n";
    let t = one_task(text);
    assert_eq!(t.prompt, "p");
    assert_eq!(t.checks.len(), 1);
}

#[test]
fn pipe_lines_keep_hash_and_blank_content() {
    let text = "== a\nprompt: p\n| # not a comment\n|\n| end\ncheck: answer => lacks y\nsetup: write f.txt\n| # heading\n|\n| last\nsolution: done\n";
    let t = one_task(text);
    assert_eq!(t.prompt, "p\n# not a comment\n\nend");
    assert_eq!(
        t.setup,
        vec![Action::Write {
            path: "f.txt".into(),
            content: "# heading\n\nlast\n".into()
        }]
    );
}

#[test]
fn last_separator_splits_the_check() {
    let t = checks_of(r#"check: krypton run "echo a => b" => contains b"#);
    assert_eq!(
        t.checks,
        vec![Check::Command {
            line: r#"krypton run "echo a => b""#.into(),
            expect: Expect::Contains("b".into())
        }]
    );
}

#[test]
fn write_with_no_pipe_lines_has_empty_content() {
    let t = one_task("== a\nprompt: p\nsetup: write empty.txt\ncheck: answer => lacks y\n");
    assert_eq!(
        t.setup,
        vec![Action::Write {
            path: "empty.txt".into(),
            content: String::new()
        }]
    );
}

#[test]
fn every_error_names_its_line() {
    let long_id = format!(
        "== {}\nprompt: p\ncheck: answer => lacks y\n",
        "a".repeat(61)
    );
    let e = parse_err(&long_id);
    assert!(e.message.starts_with("line 1: ") && e.message.contains("bad task id"));
    let cases: [(&str, usize, &str); 19] = [
        (
            "hello\n== a\nprompt: p\ncheck: answer => lacks y\n",
            1,
            "before the first",
        ),
        (
            "== a\nprompt: p\nfoo: bar\ncheck: answer => lacks y\n",
            3,
            "unknown key `foo`",
        ),
        (
            "== a\nprompt p\ncheck: answer => lacks y\n",
            2,
            "key: value",
        ),
        (
            "== T 1\nprompt: p\ncheck: answer => lacks y\n",
            1,
            "bad task id",
        ),
        (
            "== a\nprompt: p\ncheck: answer => lacks y\n== a\n",
            4,
            "duplicate task id",
        ),
        ("== a\ncheck: answer => lacks y\n", 1, "no prompt"),
        ("== a\nprompt: p\n", 1, "no check"),
        (
            "== a\nprompt: p\nsetup: fly away\ncheck: answer => lacks y\n",
            3,
            "unknown action",
        ),
        (
            "== a\nprompt: p\ncheck: chronos log proj contains x\n",
            3,
            "=>",
        ),
        (
            "== a\nprompt: p\ncheck: chronos log => maybe x\n",
            3,
            "expectation",
        ),
        ("== a\nprompt: p\ncheck: x => exit abc\n", 3, "number"),
        (
            "== a\nprompt: p\ncheck: x => lacks \n",
            3,
            "needs some text",
        ),
        (
            "== a\nprompt: p\ncheck: answer => exit 0\n",
            3,
            "cannot use `exit`",
        ),
        (
            "== a\nprompt: p\nsetup: run x y\n| oops\ncheck: answer => lacks z\n",
            4,
            "`|` lines only follow",
        ),
        (
            "== a\nprompt: p\nsolution: done\n| oops\ncheck: answer => lacks z\n",
            4,
            "`|` lines only follow",
        ),
        (
            "== a\nprompt: p\ncomputer: Chain 1\ncheck: answer => lacks z\n",
            3,
            "bad computer name",
        ),
        (
            "== a\nprompt: p\n|oops\ncheck: answer => lacks z\n",
            3,
            "a `|` line is",
        ),
        (
            "== a\nprompt: p\nprompt: q\ncheck: answer => lacks z\n",
            3,
            "one `prompt:`",
        ),
        (
            "== a\nprompt: p\ncheck: answer => lacks z\nsetup: run\n",
            4,
            "needs an argument",
        ),
    ];
    for (text, line, fragment) in cases {
        let e = parse_err(text);
        assert_eq!(e.code, Code::Invalid, "{text:?}");
        assert!(
            e.message.starts_with(&format!("line {line}: ")),
            "{text:?} gave {}",
            e.message
        );
        assert!(e.message.contains(fragment), "{text:?} gave {}", e.message);
    }
}

#[test]
fn load_of_a_missing_file_is_not_found() {
    let path = std::env::temp_dir().join(format!(
        "monolith-suite-missing-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert_eq!(load(&path).unwrap_err().code, Code::NotFound);
}

#[test]
fn load_reads_a_file_and_prefixes_errors_with_its_name() {
    let good = scratch_file("good", EXAMPLE);
    assert_eq!(load(&good).unwrap().len(), 1);
    cleanup(&good);

    let bad = scratch_file("bad", "== a\nprompt: p\nnonsense\n");
    let e = load(&bad).unwrap_err();
    assert_eq!(e.code, Code::Invalid);
    assert!(e.message.contains("suite.txt: line 3:"), "{}", e.message);
    cleanup(&bad);
}

#[test]
fn action_lines_round_trip_for_all_seven_verbs() {
    let actions = vec![
        Action::from_parts("run", "chronos status", "").unwrap(),
        Action::from_parts("help", "chronos", "").unwrap(),
        Action::from_parts("write", "proj/a.txt", "x\n\n# y\n").unwrap(),
        Action::from_parts("read", "proj/a.txt", "").unwrap(),
        Action::from_parts("remember", "the server uses 8473", "").unwrap(),
        Action::from_parts("recall", "server port", "").unwrap(),
        Action::from_parts("done", "42", "").unwrap(),
        Action::from_parts("done", "", "").unwrap(),
    ];
    let mut text = String::from("== rt\nprompt: p\n");
    for a in &actions {
        text.push_str(&format!("setup: {}\n", action_line(a)));
    }
    text.push_str("check: answer => lacks z\n");
    let t = one_task(&text);
    assert_eq!(t.setup, actions);
    assert_eq!(action_line(&actions[2]), "write proj/a.txt\n| x\n|\n| # y");
    assert_eq!(action_line(&actions[6]), "done 42");
    assert_eq!(action_line(&actions[7]), "done");
}

#[test]
fn a_rendered_task_parses_back_to_the_same_task() {
    let t = one_task(EXAMPLE);
    let mut text = format!("== {}\nprompt: {}\n", t.id, t.prompt.replace('\n', "\n| "));
    for a in &t.solution {
        text.push_str(&format!("solution: {}\n", action_line(a)));
    }
    for c in &t.checks {
        let line = match c {
            Check::Command { line, expect } => format!("{line} => {}", expect_word(expect)),
            Check::Answer { expect } => format!("answer => {}", expect_word(expect)),
        };
        text.push_str(&format!("check: {line}\n"));
    }
    let back = one_task(&text);
    assert_eq!(back.prompt, t.prompt);
    assert_eq!(back.solution, t.solution);
    assert_eq!(back.checks, t.checks);
}

fn expect_word(e: &Expect) -> String {
    match e {
        Expect::Contains(t) => format!("contains {t}"),
        Expect::Lacks(t) => format!("lacks {t}"),
        Expect::Exit(n) => format!("exit {n}"),
    }
}

/// A canned command runner: `chronos log` prints a line, `chronos status` lists an untracked
/// file, `chronos fsck` exits 1, `krypton` fails.
fn canned(line: &str) -> Result<(i32, String)> {
    match line {
        "chronos log proj" => Ok((0, "commit: hello world".into())),
        "chronos status proj" => Ok((0, "untracked: notes.txt".into())),
        "chronos fsck proj" => Ok((1, String::new())),
        "chronos clean proj" => Ok((0, "a\nb".into())),
        _ => Err(Error::new(Code::NotFound, "no such tool: krypton")),
    }
}

fn run_all(task: &Task, answer: Option<&str>) -> Vec<CheckResult> {
    let mut run = |line: &str| canned(line);
    run_checks(task, answer, &mut run)
}

#[test]
fn command_checks_pass_and_fail_as_specified() {
    let t = checks_of(
        "check: chronos log proj => contains hello\n\
         check: chronos log proj => contains goodbye\n\
         check: chronos log proj => lacks goodbye\n\
         check: chronos status proj => lacks untracked\n\
         check: chronos fsck proj => exit 0\n\
         check: chronos fsck proj => exit 2",
    );
    let r = run_all(&t, None);
    assert_eq!(
        r.iter().map(|c| c.passed).collect::<Vec<_>>(),
        vec![true, false, true, false, false, false]
    );
    assert_eq!(r[0].detail, "PASS chronos log proj => contains hello");
    assert_eq!(
        r[1].detail,
        "FAIL chronos log proj => contains goodbye: got commit: hello world"
    );
    assert_eq!(
        r[3].detail,
        "FAIL chronos status proj => lacks untracked: got untracked: notes.txt"
    );
    assert_eq!(r[4].detail, "FAIL chronos fsck proj => exit 0: got exit 1");
    assert_eq!(r[5].detail, "FAIL chronos fsck proj => exit 2: got exit 1");
}

#[test]
fn a_run_error_fails_the_check_with_its_text() {
    let t = checks_of("check: krypton run x => contains y");
    let r = run_all(&t, None);
    assert!(!r[0].passed);
    assert!(r[0]
        .detail
        .starts_with("FAIL krypton run x => contains y: got "));
    assert!(
        r[0].detail.contains("no such tool: krypton"),
        "{}",
        r[0].detail
    );
}

#[test]
fn answer_checks_are_case_insensitive_and_none_is_empty() {
    let t = checks_of(
        "check: answer => contains forty-two\n\
         check: answer => lacks yes\n\
         check: answer => contains yes\n\
         check: answer => lacks forty",
    );
    let some = run_all(&t, Some("The answer: Forty-Two"));
    assert_eq!(
        some.iter().map(|c| c.passed).collect::<Vec<_>>(),
        vec![true, true, false, false]
    );
    let none = run_all(&t, None);
    assert_eq!(
        none.iter().map(|c| c.passed).collect::<Vec<_>>(),
        vec![false, true, false, true]
    );
    assert_eq!(
        none[0].detail,
        "FAIL answer => contains forty-two: got (nothing)"
    );
}

#[test]
fn newlines_become_spaces_and_the_seen_text_is_capped() {
    let t = checks_of("check: chronos clean proj => contains z");
    let r = run_all(&t, None);
    assert_eq!(
        r[0].detail,
        "FAIL chronos clean proj => contains z: got a b"
    );

    let mut long_run = |_: &str| -> Result<(i32, String)> { Ok((0, "x".repeat(500))) };
    let t = checks_of("check: any => contains y");
    let r = run_checks(&t, None, &mut long_run);
    let prefix = "FAIL any => contains y: got ";
    assert!(r[0].detail.starts_with(prefix));
    assert_eq!(r[0].detail[prefix.len()..], "x".repeat(200));
}
