//! Exam suites: the task file format and the mechanical checks.

use std::fmt::Display;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use aci::envelope::Code;

use crate::types::{Action, Error, Result};

/// What a check expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// The text must contain this.
    Contains(String),
    /// The text must not contain this.
    Lacks(String),
    /// The command must exit with this code (command checks only).
    Exit(i32),
}

/// One check of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Run a Monolith command line in the computer and test its standard output or exit code.
    Command {
        /// The command line, as in [`Action::Run`].
        line: String,
        /// What is expected.
        expect: Expect,
    },
    /// Test the answer the agent gave with `done` (an absent answer is the empty text).
    Answer {
        /// What is expected ([`Expect::Exit`] is not allowed here).
        expect: Expect,
    },
}

/// One exam task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Unique id, for example `t01-init-commit`.
    pub id: String,
    /// What the agent is told to do.
    pub prompt: String,
    /// When set, the task runs on this named computer, kept from earlier tasks with the same
    /// name (memory chains). Otherwise it gets a fresh computer.
    pub computer: Option<String>,
    /// Actions run before the agent starts.
    pub setup: Vec<Action>,
    /// All must pass.
    pub checks: Vec<Check>,
    /// The reference solution, replayed by `eval --selftest`.
    pub solution: Vec<Action>,
}

/// The verdict of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// True when the expectation held.
    pub passed: bool,
    /// One line: the check and, on failure, what was seen instead (capped at 200 characters).
    pub detail: String,
}

/// Where a `| ` line goes: nowhere, the prompt, or the content of a `write` action.
#[derive(Debug, Clone, Copy)]
enum Block {
    None,
    Prompt,
    /// The write action at this index of `setup`.
    Setup(usize),
    /// The write action at this index of `solution`.
    Solution(usize),
}

/// A task still being read.
struct Draft {
    /// Line of its `==` header, for errors about the whole task.
    line: usize,
    id: String,
    prompt: Vec<String>,
    prompt_seen: bool,
    computer: Option<String>,
    setup: Vec<Action>,
    checks: Vec<Check>,
    solution: Vec<Action>,
}

fn invalid(line: usize, msg: impl Display) -> Error {
    Error::new(Code::Invalid, format!("line {line}: {msg}"))
}

/// Puts `line` in front of an error message, keeping its code and hint.
fn at_line(line: usize, e: Error) -> Error {
    Error {
        message: format!("line {line}: {}", e.message),
        ..e
    }
}

/// `a-z`, `0-9`, `-` and `_`, 1 to `max` characters.
fn valid_name(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.chars().count())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Parses a suite file's text. `invalid` names the line number of the first problem.
pub fn parse(text: &str) -> Result<Vec<Task>> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut block = Block::None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let s = raw.trim_start();
        if let Some(rest) = s.strip_prefix('|') {
            let content = match rest.strip_prefix(' ') {
                Some(content) => content,
                None if rest.is_empty() => "",
                None => {
                    return Err(invalid(n, "a `|` line is `| ` and text, or just `|`"));
                }
            };
            let Some(d) = draft.as_mut() else {
                return Err(invalid(n, "`|` line before any task"));
            };
            match block {
                Block::Prompt => d.prompt.push(content.to_string()),
                Block::Setup(k) => push_content(&mut d.setup, k, content),
                Block::Solution(k) => push_content(&mut d.solution, k, content),
                Block::None => {
                    return Err(invalid(
                        n,
                        "`|` lines only follow `prompt:` or a `write` action",
                    ));
                }
            }
            continue;
        }
        // Any line that is not a `|` line ends the open block.
        block = Block::None;
        let t = s.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix("==") {
            if let Some(done) = draft.take() {
                tasks.push(finish(done)?);
            }
            let id = rest.trim();
            if !valid_name(id, 60) {
                return Err(invalid(
                    n,
                    format!("bad task id `{id}`: use 1 to 60 of a-z, 0-9, - and _"),
                ));
            }
            if tasks.iter().any(|task| task.id == id) {
                return Err(invalid(n, format!("duplicate task id `{id}`")));
            }
            draft = Some(Draft {
                line: n,
                id: id.to_string(),
                prompt: Vec::new(),
                prompt_seen: false,
                computer: None,
                setup: Vec::new(),
                checks: Vec::new(),
                solution: Vec::new(),
            });
            continue;
        }
        let Some(d) = draft.as_mut() else {
            return Err(invalid(n, "text before the first `==` task line"));
        };
        let Some((key, value)) = s.split_once(": ") else {
            return Err(invalid(n, "expected `key: value`"));
        };
        let value = value.trim();
        match key {
            "prompt" => {
                if d.prompt_seen {
                    return Err(invalid(n, "a task has one `prompt:`"));
                }
                d.prompt_seen = true;
                if !value.is_empty() {
                    d.prompt.push(value.to_string());
                }
                block = Block::Prompt;
            }
            "computer" => {
                if d.computer.is_some() {
                    return Err(invalid(n, "a task has one `computer:`"));
                }
                if !valid_name(value, 40) {
                    return Err(invalid(
                        n,
                        format!("bad computer name `{value}`: use 1 to 40 of a-z, 0-9, - and _"),
                    ));
                }
                d.computer = Some(value.to_string());
            }
            "setup" | "solution" => {
                let (verb, arg) = value.split_once(' ').unwrap_or((value, ""));
                let action = Action::from_parts(verb, arg, "").map_err(|e| at_line(n, e))?;
                let is_write = matches!(action, Action::Write { .. });
                let list = if key == "setup" {
                    &mut d.setup
                } else {
                    &mut d.solution
                };
                list.push(action);
                if is_write {
                    let k = list.len() - 1;
                    block = if key == "setup" {
                        Block::Setup(k)
                    } else {
                        Block::Solution(k)
                    };
                }
            }
            "check" => {
                let (left, right) = value.rsplit_once(" => ").ok_or_else(|| {
                    invalid(
                        n,
                        "a check is `COMMAND => EXPECT`, with ` => ` between them",
                    )
                })?;
                let expect = parse_expect(right.trim()).map_err(|m| invalid(n, m))?;
                let left = left.trim();
                if left == "answer" {
                    if matches!(expect, Expect::Exit(_)) {
                        return Err(invalid(n, "`answer` checks cannot use `exit`"));
                    }
                    d.checks.push(Check::Answer { expect });
                } else if left.is_empty() {
                    return Err(invalid(n, "a check has no command before ` => `"));
                } else {
                    d.checks.push(Check::Command {
                        line: left.to_string(),
                        expect,
                    });
                }
            }
            other => return Err(invalid(n, format!("unknown key `{other}`"))),
        }
    }
    if let Some(done) = draft.take() {
        tasks.push(finish(done)?);
    }
    Ok(tasks)
}

/// Reads and parses a suite file. `not_found` if it is missing.
pub fn load(path: &Path) -> Result<Vec<Task>> {
    let text = fs::read_to_string(path).map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            Error::new(
                Code::NotFound,
                format!("no suite file at {}", path.display()),
            )
        } else {
            Error::new(Code::Io, format!("cannot read {}: {e}", path.display()))
        }
    })?;
    parse(&text).map_err(|e| Error {
        message: format!("{}: {}", path.display(), e.message),
        ..e
    })
}

/// The line form of an action, as a suite file holds it. `write` is `write PATH` followed by
/// one `| ` line per line of its content.
pub fn action_line(a: &Action) -> String {
    match a {
        Action::Write { path, content } => {
            let mut out = format!("write {path}");
            if !content.is_empty() {
                for line in content.strip_suffix('\n').unwrap_or(content).split('\n') {
                    out.push('\n');
                    if line.is_empty() {
                        out.push('|');
                    } else {
                        out.push_str("| ");
                        out.push_str(line);
                    }
                }
            }
            out
        }
        other => {
            if other.arg().is_empty() {
                other.verb().to_string()
            } else {
                format!("{} {}", other.verb(), other.arg())
            }
        }
    }
}

/// Runs every check of `task`. `run` executes one command line in the task's computer and
/// returns its exit code and standard output; a `run` error fails that check with the error text.
pub fn run_checks(
    task: &Task,
    answer: Option<&str>,
    run: &mut dyn FnMut(&str) -> Result<(i32, String)>,
) -> Vec<CheckResult> {
    task.checks
        .iter()
        .map(|check| {
            let written = written(check);
            match check {
                Check::Command { line, expect } => match run(line) {
                    Err(e) => verdict(false, &written, &e.to_string()),
                    Ok((exit, out)) => match expect {
                        Expect::Exit(n) => verdict(exit == *n, &written, &format!("exit {exit}")),
                        Expect::Contains(t) => verdict(out.contains(t.as_str()), &written, &out),
                        Expect::Lacks(t) => verdict(!out.contains(t.as_str()), &written, &out),
                    },
                },
                Check::Answer { expect } => {
                    let got = answer.unwrap_or("");
                    let lower = got.to_lowercase();
                    match expect {
                        Expect::Contains(t) => {
                            verdict(lower.contains(&t.to_lowercase()), &written, got)
                        }
                        Expect::Lacks(t) => {
                            verdict(!lower.contains(&t.to_lowercase()), &written, got)
                        }
                        Expect::Exit(_) => verdict(false, &written, "an exit code, not text"),
                    }
                }
            }
        })
        .collect()
}

/// Turns a finished draft into a task, or reports what it lacks at its header line.
fn finish(d: Draft) -> Result<Task> {
    if d.prompt.is_empty() {
        return Err(invalid(d.line, format!("task `{}` has no prompt", d.id)));
    }
    if d.checks.is_empty() {
        return Err(invalid(d.line, format!("task `{}` has no check", d.id)));
    }
    Ok(Task {
        id: d.id,
        prompt: d.prompt.join("\n"),
        computer: d.computer,
        setup: d.setup,
        checks: d.checks,
        solution: d.solution,
    })
}

/// Adds a `| ` line to the write action at `k`: its content gets the line and a newline.
fn push_content(list: &mut [Action], k: usize, line: &str) {
    if let Some(Action::Write { content, .. }) = list.get_mut(k) {
        content.push_str(line);
        content.push('\n');
    }
}

/// Parses `contains TEXT`, `lacks TEXT` or `exit N`.
fn parse_expect(s: &str) -> std::result::Result<Expect, String> {
    let (word, arg) = s.split_once(' ').unwrap_or((s, ""));
    let arg = arg.trim();
    match word {
        "contains" | "lacks" if arg.is_empty() => Err(format!("`{word}` needs some text")),
        "contains" => Ok(Expect::Contains(arg.to_string())),
        "lacks" => Ok(Expect::Lacks(arg.to_string())),
        "exit" => arg
            .parse::<i32>()
            .map(Expect::Exit)
            .map_err(|_| format!("`exit` needs a number, not `{arg}`")),
        _ => Err(format!(
            "the expectation is `contains TEXT`, `lacks TEXT` or `exit N`, not `{s}`"
        )),
    }
}

/// The check as the suite file writes it, `LEFT => EXPECT`.
fn written(check: &Check) -> String {
    match check {
        Check::Command { line, expect } => format!("{line} => {}", expect_text(expect)),
        Check::Answer { expect } => format!("answer => {}", expect_text(expect)),
    }
}

fn expect_text(expect: &Expect) -> String {
    match expect {
        Expect::Contains(t) => format!("contains {t}"),
        Expect::Lacks(t) => format!("lacks {t}"),
        Expect::Exit(n) => format!("exit {n}"),
    }
}

/// The result of one check. `got` is what was seen, shown only on failure.
fn verdict(passed: bool, written: &str, got: &str) -> CheckResult {
    let detail = if passed {
        format!("PASS {written}")
    } else {
        let seen: String = got.replace(['\r', '\n'], " ").chars().take(200).collect();
        let seen = if seen.is_empty() {
            "(nothing)".to_string()
        } else {
            seen
        };
        format!("FAIL {written}: got {seen}")
    };
    CheckResult { passed, detail }
}
