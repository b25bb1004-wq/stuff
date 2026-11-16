//! Exams: run suites with the model or with the reference solutions, and report.
//! STUB written by the leader: signatures are pinned, bodies belong to worker C1.

use aci::envelope::Code;

use crate::types::{CmdOut, Error, Result};

/// CLI: `eval --suite FILE [--suite FILE]... --out DIR [--level N] [--selftest] [--mind DIR]
/// [--freeze] [--only ID,ID] [--limit N] [--model-url URL] [--seed N] [--temp X] [--thinking]`.
pub fn cmd(_args: &[String]) -> Result<CmdOut> {
    Err(Error::new(Code::Unsupported, "eval: not built yet"))
}
