//! Linear-time regex compiler and matcher with Python `re` (ASCII) semantics.

mod ast;
mod class;
mod compile;
mod vm;

use alloc::vec::Vec;

use compile::Prog;

/// Error returned when a regex pattern is syntactically invalid or exceeds limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexError {
    /// Byte offset in the pattern where the error was detected.
    pub pos: usize,
    /// Short static description of the syntax or limit error.
    pub msg: &'static str,
}

/// Regex: compile once, run in guaranteed linear time (Thompson NFA / Pike VM, no backtracking).
#[derive(Debug, Clone)]
pub struct Regex {
    prog: Prog,
}

impl Regex {
    /// Compile `pattern` into a linear-time NFA program.
    pub fn new(pattern: &str) -> Result<Regex, RegexError> {
        let parsed = ast::parse_pattern(pattern)?;
        let prog = compile::compile(&parsed.ast, parsed.num_captures)?;
        Ok(Regex { prog })
    }

    /// Leftmost match starting at or after `start`, with Python `re` semantics for which match
    /// wins (leftmost, then the preference order of alternation and greedy/lazy repetition).
    /// Returns byte ranges of the whole match and of each capture group (`None` if did not take part).
    pub fn find_at(&self, text: &str, start: usize) -> Option<Vec<Option<(usize, usize)>>> {
        vm::find_at(&self.prog, text, start, usize::MAX)
    }

    /// Returns `true` if the regex matches anywhere in `text`.
    pub fn is_match(&self, text: &str) -> bool {
        self.find_at(text, 0).is_some()
    }

    /// Return all non-overlapping leftmost match spans in `text`, following Python 3.7+ `re.finditer` rules.
    pub fn find_iter(&self, text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut no_empty_at = usize::MAX;
        while pos <= text.len() {
            // After an empty match at `pos`, retry at the same spot with only empty matches there refused.
            let Some(groups) = vm::find_at(&self.prog, text, pos, no_empty_at) else {
                break;
            };
            let Some(Some((start, end))) = groups.first().copied() else {
                break;
            };
            out.push((start, end));
            pos = end;
            no_empty_at = if end == start { end } else { usize::MAX };
        }
        out
    }
}
