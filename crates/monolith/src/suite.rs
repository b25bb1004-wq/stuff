//! Exam suites: the task file format and the mechanical checks.
//! STUB written by the leader: signatures are pinned, bodies belong to worker A3.

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

fn stub<T>() -> Result<T> {
    Err(Error::new(Code::Unsupported, "suite: not built yet"))
}

/// Parses a suite file's text. `invalid` names the line number of the first problem.
pub fn parse(_text: &str) -> Result<Vec<Task>> {
    stub()
}

/// Reads and parses a suite file. `not_found` if it is missing.
pub fn load(_path: &Path) -> Result<Vec<Task>> {
    stub()
}

/// Runs every check of `task`. `run` executes one command line in the task's computer and
/// returns its exit code and standard output; a `run` error fails that check with the error text.
pub fn run_checks(
    _task: &Task,
    _answer: Option<&str>,
    _run: &mut dyn FnMut(&str) -> Result<(i32, String)>,
) -> Vec<CheckResult> {
    Vec::new()
}
