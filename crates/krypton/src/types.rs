//! Error types, environment trait, and identifier validation helpers.

use alloc::string::String;
use alloc::vec::Vec;

use crate::ast;

/// Error returned when parsing a shell script fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number where the error was detected.
    pub line: u32,
    /// 1-based column number where the error was detected.
    pub col: u32,
    /// Human-readable error message.
    pub msg: String,
    /// True when input ended inside an incomplete construct.
    pub incomplete: bool,
}

/// Error returned during word or arithmetic expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// Parameter unset or null in `${x?msg}` / `${x:?msg}`.
    Unset(String, String),
    /// Invalid parameter substitution syntax or invalid assignment target.
    BadSubst(String),
    /// Arithmetic syntax error or division by zero.
    Arith(String),
}

/// External environment interface used by expansion without performing direct I/O.
pub trait Env {
    /// Look up a shell variable or special scalar parameter (`?`, `$`, `!`, `-`, `0`).
    fn get(&self, name: &str) -> Option<String>;
    /// Assign a shell variable (used by `${x=w}` and arithmetic assignments).
    fn set(&mut self, name: &str, value: &str);
    /// Return current positional parameters (`$1`, `$2`, ...).
    fn positional(&self) -> Vec<String>;
    /// Execute a command substitution and return its stdout.
    fn command(&mut self, s: &ast::Script) -> String;
    /// Resolve home directory for `user` (`""` means current user).
    fn home(&self, user: &str) -> Option<String>;
    /// List directory entry names (without `.` or `..`) in any order.
    fn list_dir(&self, dir: &str) -> Vec<String>;
    /// Return whether `set -u` (nounset) is active.
    fn nounset(&self) -> bool {
        false
    }
}

/// Check if `name` is a valid POSIX shell identifier (`[A-Za-z_][A-Za-z0-9_]*`).
pub(crate) fn is_valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Check if `name` is assignable (a valid identifier, not a special/positional parameter).
pub(crate) fn is_assignable_name(name: &str) -> bool {
    is_valid_name(name)
}
