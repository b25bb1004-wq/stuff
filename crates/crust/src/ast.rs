//! Abstract syntax tree types for POSIX shell scripts.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// A shell word composed of unquoted, quoted, escaped, and expanded parts.
pub type Word = Vec<Part>;

/// A single component within a shell word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Unquoted literal text (may contain glob characters).
    Lit(String),
    /// Single-quoted literal text (`'...'`).
    Single(String),
    /// Backslash-escaped character outside single quotes.
    Escaped(char),
    /// Double-quoted part (`"..."`), holding only `Lit`, `Escaped`, `Param`, `Cmd`, or `Arith`.
    Double(Vec<Part>),
    /// Parameter expansion (`$x`, `${x}`, `${x:-w}`, ...).
    Param(Param),
    /// Command substitution (`$(...)` or `` `...` ``).
    Cmd(Script),
    /// Arithmetic expansion (`$((...))`), storing raw text expanded at run time.
    Arith(String),
    /// Tilde prefix (`~` or `~user`) at word start or after `=` / `:` in assignments.
    Tilde(String),
}

/// A parameter expansion reference and optional operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Parameter name: identifier, digit(s), or one of `@*#?$!-`.
    pub name: String,
    /// Expansion operator applied to the parameter.
    pub op: ParamOp,
}

/// Operator for `${...}` parameter expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamOp {
    /// Plain value expansion (`$x` or `${x}`).
    Plain,
    /// String length expansion (`${#x}`).
    Length,
    /// Use default value if unset (or null when `bool` is true): `${x-w}` / `${x:-w}`.
    Default(bool, Word),
    /// Assign default value if unset (or null when `bool` is true): `${x=w}` / `${x:=w}`.
    Assign(bool, Word),
    /// Fail with error if unset (or null when `bool` is true): `${x?w}` / `${x:?w}`.
    Error(bool, Word),
    /// Use alternate value if set (and non-null when `bool` is true): `${x+w}` / `${x:+w}`.
    Alt(bool, Word),
    /// Trim shortest (`false`, `%`) or longest (`true`, `%%`) suffix matching pattern.
    TrimSuffix(bool, Word),
    /// Trim shortest (`false`, `#`) or longest (`true`, `##`) prefix matching pattern.
    TrimPrefix(bool, Word),
}

/// Redirection operator kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirOp {
    /// Input redirection (`<`).
    In,
    /// Output redirection (`>`).
    Out,
    /// Append output redirection (`>>`).
    Append,
    /// Clobber output redirection (`>|`).
    Clobber,
    /// Read/write redirection (`<>`).
    ReadWrite,
    /// Duplicate input file descriptor (`<&`).
    DupIn,
    /// Duplicate output file descriptor (`>&`).
    DupOut,
    /// Here-document (`<<` or `<<-`).
    HereDoc {
        /// True for `<<-` (strip leading tabs).
        strip_tabs: bool,
        /// True if delimiter was quoted (`body` is a single unexpanded `Part::Lit`).
        quoted: bool,
        /// Body parts (a single `Part::Lit` when `quoted` is true).
        body: Word,
    },
}

/// A file descriptor redirection attached to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redir {
    /// Optional explicit file descriptor number before the operator.
    pub fd: Option<u32>,
    /// Redirection operator.
    pub op: RedirOp,
    /// Redirection target word (empty for `HereDoc`).
    pub target: Word,
}

/// A simple command with optional variable assignments, words, and redirections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Simple {
    /// Leading `NAME=VALUE` assignments.
    pub assigns: Vec<(String, Word)>,
    /// Command name and arguments.
    pub words: Vec<Word>,
    /// Redirections appearing in the simple command.
    pub redirs: Vec<Redir>,
}

/// A single shell command (simple, compound, or function definition).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A simple command.
    Simple(Simple),
    /// A subshell `( ... )`.
    Subshell(Script, Vec<Redir>),
    /// A brace group `{ ...; }`.
    Group(Script, Vec<Redir>),
    /// An `if ... then ... [elif ... then ...] [else ...] fi` conditional.
    If {
        /// Condition/body pairs for `if` and `elif` branches.
        arms: Vec<(Script, Script)>,
        /// Optional `else` body.
        otherwise: Option<Script>,
        /// Trailing redirections on the `if` command.
        redirs: Vec<Redir>,
    },
    /// A `while` (`until == false`) or `until` (`until == true`) loop.
    Loop {
        /// True for `until`, false for `while`.
        until: bool,
        /// Loop condition script.
        cond: Script,
        /// Loop body script.
        body: Script,
        /// Trailing redirections on the loop.
        redirs: Vec<Redir>,
    },
    /// A `for` loop (`words == None` iterates over `"$@"`).
    For {
        /// Loop variable name.
        var: String,
        /// Optional explicit word list after `in`.
        words: Option<Vec<Word>>,
        /// Loop body script.
        body: Script,
        /// Trailing redirections on the `for` loop.
        redirs: Vec<Redir>,
    },
    /// A `case ... in ... esac` construct.
    Case {
        /// Subject word to match against arm patterns.
        word: Word,
        /// Pattern-list / body arms.
        arms: Vec<(Vec<Word>, Script)>,
        /// Trailing redirections on the `case` command.
        redirs: Vec<Redir>,
    },
    /// A function definition `name() body`.
    Func {
        /// Function name.
        name: String,
        /// Function body compound command.
        body: Box<Command>,
    },
}

/// A pipeline of commands (`[!] cmd1 | cmd2 ...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    /// True if prefixed with `!` to negate exit status.
    pub bang: bool,
    /// Commands connected by `|`.
    pub cmds: Vec<Command>,
}

/// An `&&` / `||` list of pipelines with optional `&` background flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndOr {
    /// First pipeline in the list.
    pub first: Pipeline,
    /// Subsequent `(is_and, pipeline)` entries (`true` = `&&`, `false` = `||`).
    pub rest: Vec<(bool, Pipeline)>,
    /// True if terminated by `&`.
    pub background: bool,
}

/// A sequence of `AndOr` lists representing a complete shell script.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    /// Top-level `AndOr` items in execution order.
    pub items: Vec<AndOr>,
}
