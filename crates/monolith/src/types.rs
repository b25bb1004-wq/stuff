//! Shared types. The signatures here are the contract between the modules; change them only with
//! the leader's say.

use std::fmt;
use std::path::PathBuf;

use aci::envelope::Code;
use aci::json::Json;

/// Result with the crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// An error with an ACI code, a message and an optional hint that says how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// ACI error code (`usage`, `not_found`, `denied`, ...).
    pub code: Code,
    /// What went wrong.
    pub message: String,
    /// How to fix it, when known.
    pub hint: Option<String>,
}

impl Error {
    /// An error without a hint.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    /// The same error with a hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " (hint: {h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// One thing the model (or a suite file) asks the harness to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run one Monolith command line, for example `chronos status`. No shell; `--json` is added.
    Run {
        /// The command line: tool name, then its arguments.
        line: String,
    },
    /// Show a tool's commands from its manifest. Topic is `TOOL` or `TOOL COMMAND`.
    Help {
        /// `TOOL` or `TOOL COMMAND`.
        topic: String,
    },
    /// Write a text file inside the computer's `root/`, replacing it.
    Write {
        /// Path relative to `root/`.
        path: String,
        /// The whole new file text.
        content: String,
    },
    /// Read a text file inside the computer's `root/`.
    Read {
        /// Path relative to `root/`.
        path: String,
    },
    /// Store a fact in memory.
    Remember {
        /// The fact.
        text: String,
    },
    /// Search memory.
    Recall {
        /// What to look for.
        query: String,
    },
    /// Finish the task with an answer.
    Done {
        /// The final answer.
        answer: String,
    },
}

impl Action {
    /// The seven verbs, in the order the prompt lists them.
    pub const VERBS: [&'static str; 7] =
        ["run", "help", "write", "read", "remember", "recall", "done"];

    /// Builds an action from a verb, its argument and (for `write`) the file content.
    /// Used for suite lines (`run chronos init`) and for the model's JSON turn.
    pub fn from_parts(verb: &str, arg: &str, content: &str) -> Result<Self> {
        let arg = arg.trim();
        if arg.is_empty() && verb != "done" {
            return Err(Error::new(
                Code::Invalid,
                format!("action `{verb}` needs an argument"),
            ));
        }
        Ok(match verb {
            "run" => Self::Run { line: arg.into() },
            "help" => Self::Help { topic: arg.into() },
            "write" => Self::Write {
                path: arg.into(),
                content: content.into(),
            },
            "read" => Self::Read { path: arg.into() },
            "remember" => Self::Remember { text: arg.into() },
            "recall" => Self::Recall { query: arg.into() },
            "done" => Self::Done { answer: arg.into() },
            other => {
                return Err(
                    Error::new(Code::Invalid, format!("unknown action `{other}`"))
                        .hint("use one of: run, help, write, read, remember, recall, done"),
                )
            }
        })
    }

    /// Parses the model's turn: `{"thought": "...", "action": "run", "arg": "...", "content": "..."}`.
    /// `thought` and `content` may be missing. Returns the thought (may be empty) and the action.
    pub fn from_json(text: &str) -> Result<(String, Self)> {
        let v = Json::parse(text.trim())
            .map_err(|e| Error::new(Code::Invalid, format!("the turn is not JSON: {e}")))?;
        let field = |k: &str| v.get(k).and_then(Json::as_str).unwrap_or("").to_string();
        let verb = field("action");
        if verb.is_empty() {
            return Err(Error::new(Code::Invalid, "the turn has no `action` field"));
        }
        let action = Self::from_parts(&verb, &field("arg"), &field("content"))?;
        Ok((field("thought"), action))
    }

    /// The verb of this action.
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Run { .. } => "run",
            Self::Help { .. } => "help",
            Self::Write { .. } => "write",
            Self::Read { .. } => "read",
            Self::Remember { .. } => "remember",
            Self::Recall { .. } => "recall",
            Self::Done { .. } => "done",
        }
    }

    /// The main argument of this action (command line, path, text, query or answer).
    pub fn arg(&self) -> &str {
        match self {
            Self::Run { line } => line,
            Self::Help { topic } => topic,
            Self::Write { path, .. } | Self::Read { path } => path,
            Self::Remember { text } => text,
            Self::Recall { query } => query,
            Self::Done { answer } => answer,
        }
    }
}

/// What the model reads back after an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// False when the action failed (tool error, refused path, bad command line).
    pub ok: bool,
    /// The text shown to the model.
    pub text: String,
}

/// How much the harness helps the model. Each level includes the ones below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 0: tool names and summaries; actions run, write, read, done; raw results; no memory.
    Baseline,
    /// 1: help cards, teaching errors, shaped output, history compaction, one worked example.
    Interface,
    /// 2: memory actions, lessons and skills put in the prompt, reflection.
    Memory,
}

impl Level {
    /// Parses `0`, `1` or `2`.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "0" => Ok(Self::Baseline),
            "1" => Ok(Self::Interface),
            "2" => Ok(Self::Memory),
            _ => Err(Error::new(
                Code::Usage,
                format!("level must be 0, 1 or 2, not `{s}`"),
            )),
        }
    }

    /// The level as a number.
    pub fn number(self) -> u8 {
        self as u8
    }
}

/// Settings for one agent run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunConfig {
    /// How much the harness helps.
    pub level: Level,
    /// Base URL of the model server, loopback only, for example `http://127.0.0.1:8080`.
    pub model_url: String,
    /// Model name sent in the request (servers with one model ignore it).
    pub model: String,
    /// Most actions per task.
    pub max_steps: u32,
    /// Most tokens the model may write per turn.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f64,
    /// Sampling seed.
    pub seed: i64,
    /// Let the model think before each turn (slower).
    pub thinking: bool,
    /// Seconds before a tool is killed.
    pub tool_timeout_secs: u64,
    /// Most bytes of one observation shown to the model.
    pub obs_cap_bytes: usize,
    /// Level 2: lessons and skills are read but never written (the model's own `remember`
    /// facts still go to its computer's memory).
    pub freeze_memory: bool,
    /// Memory store to use instead of the computer's own `memory/` (a shared mind).
    pub mind_dir: Option<PathBuf>,
    /// Extra lessons to put in the prompt for this run only (used for a verified retry).
    pub extra_lessons: Vec<String>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            level: Level::Memory,
            model_url: "http://127.0.0.1:8080".into(),
            model: "local".into(),
            max_steps: 16,
            max_tokens: 400,
            temperature: 0.6,
            seed: 1,
            thinking: false,
            tool_timeout_secs: 60,
            obs_cap_bytes: 2000,
            freeze_memory: false,
            mind_dir: None,
            extra_lessons: Vec::new(),
        }
    }
}

/// Why a run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The model said `done`.
    Done,
    /// The step limit was reached.
    StepLimit,
    /// The model server failed; the text says how.
    ModelError(String),
}

/// The result of one agent run on one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The answer given with `done`, if any.
    pub answer: Option<String>,
    /// Why the run stopped.
    pub stop: Stop,
    /// Actions taken.
    pub steps: u32,
    /// Turns that were not a valid action.
    pub invalid_actions: u32,
    /// Actions whose observation was not ok.
    pub failed_actions: u32,
    /// The `run` command lines that succeeded, in order (the raw material for a skill).
    pub good_commands: Vec<String>,
    /// Prompt tokens used, summed over turns.
    pub prompt_tokens: u64,
    /// Completion tokens used, summed over turns.
    pub completion_tokens: u64,
    /// Milliseconds spent waiting for the model.
    pub model_ms: u64,
}

/// What a CLI subcommand returns: the JSON `data`, the plain text for text mode and the exit code.
#[derive(Debug, Clone, PartialEq)]
pub struct CmdOut {
    /// The `data` object of the `--json` envelope.
    pub data: Json,
    /// What text mode prints.
    pub text: String,
    /// Process exit code.
    pub exit: u8,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn json_turn_becomes_an_action() {
        let (thought, a) =
            Action::from_json(r#"{"thought":"look","action":"run","arg":" chronos status "}"#)
                .unwrap();
        assert_eq!(thought, "look");
        assert_eq!(
            a,
            Action::Run {
                line: "chronos status".into()
            }
        );
        let (_, w) =
            Action::from_json(r#"{"action":"write","arg":"a.txt","content":"x\ny\n"}"#).unwrap();
        assert_eq!(
            w,
            Action::Write {
                path: "a.txt".into(),
                content: "x\ny\n".into()
            }
        );
        assert_eq!(w.verb(), "write");
        assert_eq!(w.arg(), "a.txt");
    }

    #[test]
    fn bad_turns_are_invalid_with_a_reason() {
        for bad in [
            "not json",
            r#"{"arg":"x"}"#,
            r#"{"action":"fly","arg":"x"}"#,
            r#"{"action":"run","arg":" "}"#,
        ] {
            let e = Action::from_json(bad).unwrap_err();
            assert_eq!(e.code, Code::Invalid, "{bad}");
        }
        assert!(Action::from_json(r#"{"action":"done","arg":""}"#).is_ok());
    }
}
