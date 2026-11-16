//! The agent loop: build the prompt, ask the model for one action, run it, repeat.
//! STUB written by the leader: signatures are pinned, bodies belong to worker B1.

use aci::envelope::Code;

use crate::actions::Ctx;
use crate::types::{CmdOut, Error, Outcome, Result, Stop};

/// The system prompt for this context (level, tool list, lessons for `task`).
pub fn system_prompt(_ctx: &Ctx<'_>, _task: &str) -> String {
    String::new()
}

/// Lets the model work on `task` until it says `done`, the step limit is reached or the model
/// server fails. Every turn is journaled.
pub fn run_task(_ctx: &Ctx<'_>, _task: &str) -> Outcome {
    Outcome {
        answer: None,
        stop: Stop::ModelError("agent: not built yet".into()),
        steps: 0,
        invalid_actions: 0,
        failed_actions: 0,
        good_commands: Vec::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        model_ms: 0,
    }
}

/// After a failed task: asks the model for one short lesson (what to do differently), given the
/// task and the session's journal. `None` if the model gives nothing usable.
pub fn reflect(_ctx: &Ctx<'_>, _task: &str, _failed_checks: &[String]) -> Option<String> {
    None
}

/// CLI: `run NAME --task TEXT [--level N] [--model-url URL] [--max-steps N] [--mind DIR]
/// [--freeze] [--seed N] [--temp X] [--thinking]`.
pub fn cmd(_args: &[String]) -> Result<CmdOut> {
    Err(Error::new(Code::Unsupported, "agent: not built yet"))
}
