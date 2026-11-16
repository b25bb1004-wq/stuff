//! AI computers: their folders, the path fence, and running Monolith tools inside them.
//! STUB written by the leader: signatures are pinned, bodies belong to worker A1.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aci::envelope::Code;

use crate::types::{CmdOut, Error, Result};

/// A computer: `dir` is `$MONOLITH_HOME/computers/NAME`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computer {
    /// Its name (`a-z`, `0-9`, `-`, `_`; 1 to 40 characters).
    pub name: String,
    /// Its folder.
    pub dir: PathBuf,
}

/// What a tool run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// Exit code (-1 when killed).
    pub exit: i32,
    /// Standard output, capped.
    pub stdout: Vec<u8>,
    /// Standard error, capped.
    pub stderr: Vec<u8>,
    /// True when the timeout killed it.
    pub timed_out: bool,
    /// Wall time in milliseconds.
    pub ms: u64,
}

fn stub<T>() -> Result<T> {
    Err(Error::new(Code::Unsupported, "computer: not built yet"))
}

/// `$MONOLITH_HOME`, or `~/.monolith`.
pub fn home() -> PathBuf {
    PathBuf::new()
}

/// Where the Monolith tool binaries are: `$MONOLITH_BIN`, or the folder of the running program.
pub fn tool_dir() -> PathBuf {
    PathBuf::new()
}

/// Splits a command line into arguments without a shell: spaces separate, single and double
/// quotes group, a backslash escapes the next character outside single quotes.
pub fn split_command_line(_line: &str) -> Result<Vec<String>> {
    stub()
}

impl Computer {
    /// Makes a new computer under `home`. `exists` if the name is taken, `invalid` for a bad name.
    pub fn create(_home: &Path, _name: &str) -> Result<Self> {
        stub()
    }

    /// Opens an existing computer. `not_found` if there is none.
    pub fn open(_home: &Path, _name: &str) -> Result<Self> {
        stub()
    }

    /// Names of all computers under `home`, sorted.
    pub fn list(_home: &Path) -> Result<Vec<String>> {
        stub()
    }

    /// Deletes a computer's folder.
    pub fn remove(_home: &Path, _name: &str) -> Result<()> {
        stub()
    }

    /// Its `root/` folder (the tools' working directory).
    pub fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// Turns a path relative to `root/` into a host path, or `denied` if it leaves `root/`.
    pub fn resolve(&self, _rel: &str) -> Result<PathBuf> {
        stub()
    }

    /// Checks a tool's arguments against the fence: the tool must be a Monolith tool, and no
    /// word may leave `root/` or name a host outside loopback. `denied` says which word and why.
    pub fn check_args(&self, _argv: &[String]) -> Result<()> {
        stub()
    }

    /// Runs `argv[0]` (a Monolith tool) with the rest as arguments, cwd `root/`, after
    /// [`Computer::check_args`]. Kills it at `timeout`. Output is capped at 256 KiB per stream.
    pub fn exec(&self, _argv: &[String], _timeout: Duration) -> Result<ExecResult> {
        stub()
    }
}

/// CLI: `new NAME`, `list`, `rm NAME`, `exec NAME -- TOOL ARGS...`, `log NAME [--limit N] [--offset N]`.
pub fn cmd(_sub: &str, _args: &[String]) -> Result<CmdOut> {
    stub()
}
