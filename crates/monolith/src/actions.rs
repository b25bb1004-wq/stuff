//! Executing one action on a computer and rendering what the model reads back.
//! STUB written by the leader: signatures are pinned, bodies belong to worker B1.

use aci::envelope::Code;

use crate::computer::Computer;
use crate::memory::Store;
use crate::types::{Action, Error, Observation, Result, RunConfig};

/// Everything an action needs.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// The computer being driven.
    pub computer: &'a Computer,
    /// The memory store in use (the computer's own, or a shared mind).
    pub mind: &'a Store,
    /// Run settings.
    pub cfg: &'a RunConfig,
    /// The session id written to the journal.
    pub session: &'a str,
}

/// Runs one Monolith command line in `c`: split, fence check, add `--json` as the first
/// argument, execute. Returns the exit code and standard output as text (lossy UTF-8).
/// Used by `run` actions, suite setup and suite checks.
pub fn run_line(_c: &Computer, _line: &str, _timeout_secs: u64) -> Result<(i32, String)> {
    Err(Error::new(Code::Unsupported, "actions: not built yet"))
}

/// Executes one action and records it in the journal. Never fails: an error becomes an
/// observation with `ok: false` whose text says what went wrong and, from level 1, how to fix it.
pub fn execute(_ctx: &Ctx<'_>, _action: &Action) -> Observation {
    Observation {
        ok: false,
        text: "actions: not built yet".into(),
    }
}

/// The tool list for the system prompt: one line per tool, `name: summary`, read from each
/// tool's `--describe` (cached in the computer's folder).
pub fn tool_index(_c: &Computer) -> Result<String> {
    Err(Error::new(Code::Unsupported, "actions: not built yet"))
}

/// A compact card for `TOOL` (its commands, one line each) or `TOOL COMMAND` (arguments,
/// what it returns, an example), built from the tool's manifest.
pub fn help_card(_c: &Computer, _topic: &str) -> Result<String> {
    Err(Error::new(Code::Unsupported, "actions: not built yet"))
}
