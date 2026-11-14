//! POSIX `test` and `[` builtin evaluator.

use std::format;
use std::fs;
use std::path::{Path, PathBuf};
use std::prelude::rust_2021::*;

use super::redir::{FdTable, IoStreams};

fn resolve(cwd: &Path, s: &str) -> PathBuf {
    let p = Path::new(s);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn is_unary_op(s: &str) -> bool {
    matches!(
        s,
        "-z" | "-n"
            | "-e"
            | "-f"
            | "-d"
            | "-r"
            | "-w"
            | "-x"
            | "-s"
            | "-L"
            | "-h"
            | "-p"
            | "-b"
            | "-c"
            | "-S"
    )
}

fn is_binary_op(s: &str) -> bool {
    matches!(
        s,
        "=" | "==" | "!=" | "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge"
    )
}

fn eval_unary(op: &str, arg: &str, cwd: &Path) -> Result<bool, String> {
    match op {
        "-z" => Ok(arg.is_empty()),
        "-n" => Ok(!arg.is_empty()),
        "-e" => Ok(fs::metadata(resolve(cwd, arg)).is_ok()),
        "-f" => Ok(fs::metadata(resolve(cwd, arg)).is_ok_and(|m| m.is_file())),
        "-d" => Ok(fs::metadata(resolve(cwd, arg)).is_ok_and(|m| m.is_dir())),
        "-s" => Ok(fs::metadata(resolve(cwd, arg)).is_ok_and(|m| m.len() > 0)),
        "-L" | "-h" => {
            Ok(fs::symlink_metadata(resolve(cwd, arg)).is_ok_and(|m| m.file_type().is_symlink()))
        }
        "-r" => Ok(fs::File::open(resolve(cwd, arg)).is_ok()),
        "-w" => Ok(fs::metadata(resolve(cwd, arg)).is_ok_and(|m| !m.permissions().readonly())),
        "-x" => {
            let p = resolve(cwd, arg);
            let Ok(m) = fs::metadata(&p) else {
                return Ok(false);
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                Ok((m.permissions().mode() & 0o111) != 0)
            }
            #[cfg(not(unix))]
            {
                let _ = m;
                Ok(true)
            }
        }
        "-p" | "-b" | "-c" | "-S" => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let Ok(m) = fs::metadata(resolve(cwd, arg)) else {
                    return Ok(false);
                };
                let ft = m.file_type();
                Ok(match op {
                    "-p" => ft.is_fifo(),
                    "-b" => ft.is_block_device(),
                    "-c" => ft.is_char_device(),
                    "-S" => ft.is_socket(),
                    _ => false,
                })
            }
            #[cfg(not(unix))]
            {
                let _ = (op, arg, cwd);
                Ok(false)
            }
        }
        _ => Err(format!("unknown unary operator: {op}")),
    }
}

fn parse_int(s: &str) -> Result<i64, String> {
    s.trim()
        .parse::<i64>()
        .map_err(|_| format!("integer expression expected: {s}"))
}

fn eval_binary(lhs: &str, op: &str, rhs: &str) -> Result<bool, String> {
    match op {
        "=" | "==" => Ok(lhs == rhs),
        "!=" => Ok(lhs != rhs),
        "-eq" => Ok(parse_int(lhs)? == parse_int(rhs)?),
        "-ne" => Ok(parse_int(lhs)? != parse_int(rhs)?),
        "-lt" => Ok(parse_int(lhs)? < parse_int(rhs)?),
        "-le" => Ok(parse_int(lhs)? <= parse_int(rhs)?),
        "-gt" => Ok(parse_int(lhs)? > parse_int(rhs)?),
        "-ge" => Ok(parse_int(lhs)? >= parse_int(rhs)?),
        _ => Err(format!("unknown binary operator: {op}")),
    }
}

struct Parser<'a> {
    args: &'a [String],
    pos: usize,
    cwd: &'a Path,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&str> {
        self.args.get(self.pos).map(String::as_str)
    }

    fn peek2(&self) -> Option<&str> {
        self.args.get(self.pos + 1).map(String::as_str)
    }

    fn next(&mut self) -> Option<&str> {
        let s = self.args.get(self.pos).map(String::as_str)?;
        self.pos += 1;
        Some(s)
    }

    fn parse_or(&mut self) -> Result<bool, String> {
        let mut val = self.parse_and()?;
        while self.peek() == Some("-o") {
            self.next();
            let rhs = self.parse_and()?;
            val = val || rhs;
        }
        Ok(val)
    }

    fn parse_and(&mut self) -> Result<bool, String> {
        let mut val = self.parse_not()?;
        while self.peek() == Some("-a") {
            self.next();
            let rhs = self.parse_not()?;
            val = val && rhs;
        }
        Ok(val)
    }

    fn parse_not(&mut self) -> Result<bool, String> {
        if self.peek() == Some("!") {
            self.next();
            Ok(!self.parse_not()?)
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<bool, String> {
        let Some(tok) = self.peek() else {
            return Err("argument expected".to_string());
        };
        if tok == "(" {
            self.next();
            let val = self.parse_or()?;
            if self.next() != Some(")") {
                return Err("missing ')'".to_string());
            }
            return Ok(val);
        }
        if let Some(op) = self.peek2() {
            if is_binary_op(op) {
                let lhs = self.next().unwrap_or_default().to_string();
                let bin = self.next().unwrap_or_default().to_string();
                let Some(rhs) = self.next() else {
                    return Err(format!("missing operand after {bin}"));
                };
                return eval_binary(&lhs, &bin, rhs);
            }
        }
        if is_unary_op(tok) {
            let op = self.next().unwrap_or_default().to_string();
            let cwd = self.cwd;
            let Some(arg) = self.next() else {
                return Err(format!("missing operand after {op}"));
            };
            return eval_unary(&op, arg, cwd);
        }
        let s = self.next().unwrap_or_default();
        Ok(!s.is_empty())
    }
}

fn eval_slice(args: &[String], cwd: &Path) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [a] => Ok(!a.is_empty()),
        [a, b] if a == "!" => Ok(b.is_empty()),
        [op, arg] if is_unary_op(op) => eval_unary(op, arg, cwd),
        [lhs, op, rhs] if is_binary_op(op) => eval_binary(lhs, op, rhs),
        [a, b, c] if a == "!" => Ok(!eval_slice(&[b.clone(), c.clone()], cwd)?),
        [a, b, c] if a == "(" && c == ")" => Ok(!b.is_empty()),
        _ => {
            let mut p = Parser { args, pos: 0, cwd };
            let res = p.parse_or()?;
            if let Some(extra) = p.peek() {
                return Err(format!("unexpected argument: {extra}"));
            }
            Ok(res)
        }
    }
}

/// Execute `test` or `[` builtin.
pub(crate) fn builtin_test(
    args: &[String],
    is_bracket: bool,
    cwd: &Path,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let slice = if is_bracket {
        if args.last().map(String::as_str) != Some("]") {
            fds.write_err(io, "crust: [: missing ']'\n");
            return 2;
        }
        &args[..args.len().saturating_sub(1)]
    } else {
        args
    };

    match eval_slice(slice, cwd) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            let cmd = if is_bracket { "[" } else { "test" };
            fds.write_err(io, &format!("crust: {cmd}: {e}\n"));
            2
        }
    }
}
