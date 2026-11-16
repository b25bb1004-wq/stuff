//! Recursive-descent POSIX shell parser producing `ast::Script`.

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::{AndOr, Command, Part, Pipeline, Redir, RedirOp, Script, Simple, Word};
use crate::compound::{
    parse_case, parse_for, parse_func, parse_group, parse_if, parse_loop, parse_subshell,
    resolve_script,
};
use crate::heredoc::{parse_heredoc_delim, read_pending_heredocs};
use crate::lexer::{is_word_single_lit, Op, PendingHereDoc, Scanner};
use crate::types::{is_valid_name, Escapes, ParseError};
use crate::word::{parse_word, TildeMode};

const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    Eof,
    RParen,
    RBrace,
    Then,
    ElifElseFi,
    Fi,
    Do,
    Done,
    CaseArm,
}

fn is_reserved_word(s: &str) -> bool {
    matches!(
        s,
        "if" | "then"
            | "elif"
            | "else"
            | "fi"
            | "do"
            | "done"
            | "case"
            | "esac"
            | "while"
            | "until"
            | "for"
            | "{"
            | "}"
            | "!"
    )
}

pub(crate) fn peek_word_lit(scanner: &mut Scanner<'_>) -> Option<String> {
    let cp = scanner.cursor.clone();
    scanner.skip_ws_and_comments();
    if scanner.peek_op().is_some() || scanner.cursor.peek() == Some('\n') {
        scanner.cursor = cp;
        return None;
    }
    let mut s = String::new();
    while let Some(c) = scanner.cursor.peek() {
        if matches!(
            c,
            ' ' | '\t' | '\n' | '&' | '|' | ';' | '(' | ')' | '<' | '>'
        ) {
            break;
        }
        if matches!(c, '\'' | '"' | '\\' | '$' | '`' | '~') {
            scanner.cursor = cp;
            return None;
        }
        s.push(c);
        scanner.cursor.next_char();
    }
    scanner.cursor = cp;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub(crate) fn skip_linebreak(scanner: &mut Scanner<'_>) -> Result<(), ParseError> {
    loop {
        scanner.skip_ws_and_comments();
        if scanner.cursor.peek() == Some('\n') {
            scanner.cursor.next_char();
            read_pending_heredocs(scanner)?;
        } else {
            break;
        }
    }
    Ok(())
}

fn at_stop(scanner: &mut Scanner<'_>, stop: Stop) -> bool {
    scanner.skip_ws_and_comments();
    match stop {
        Stop::Eof => scanner.cursor.peek().is_none(),
        Stop::RParen => scanner.peek_op() == Some(Op::RParen),
        Stop::RBrace => peek_word_lit(scanner).as_deref() == Some("}"),
        Stop::Then => peek_word_lit(scanner).as_deref() == Some("then"),
        Stop::ElifElseFi => matches!(
            peek_word_lit(scanner).as_deref(),
            Some("elif" | "else" | "fi")
        ),
        Stop::Fi => peek_word_lit(scanner).as_deref() == Some("fi"),
        Stop::Do => peek_word_lit(scanner).as_deref() == Some("do"),
        Stop::Done => peek_word_lit(scanner).as_deref() == Some("done"),
        Stop::CaseArm => {
            scanner.peek_op() == Some(Op::Dsemi)
                || peek_word_lit(scanner).as_deref() == Some("esac")
        }
    }
}

pub(crate) fn parse_script_until(
    scanner: &mut Scanner<'_>,
    stop: Stop,
) -> Result<Script, ParseError> {
    let mut items = Vec::new();
    loop {
        skip_linebreak(scanner)?;
        if at_stop(scanner, stop) {
            return Ok(Script { items });
        }
        if scanner.cursor.peek().is_none() {
            return Err(scanner
                .cursor
                .err("unexpected end of input", stop != Stop::Eof));
        }

        let item = parse_and_or(scanner)?;
        let bg = item.background;
        items.push(item);

        if bg {
            continue;
        }

        scanner.skip_ws_and_comments();
        match scanner.peek_op() {
            Some(Op::Semi) => {
                scanner.take_op();
                scanner.skip_ws_and_comments();
                if scanner.peek_op() == Some(Op::Semi) {
                    return Err(scanner.cursor.err("unexpected ';'", false));
                }
            }
            Some(Op::Amp) => {
                return Err(scanner.cursor.err("unexpected '&'", false));
            }
            _ => {
                if scanner.cursor.peek() == Some('\n') {
                    scanner.cursor.next_char();
                    read_pending_heredocs(scanner)?;
                } else if at_stop(scanner, stop) {
                    return Ok(Script { items });
                } else if scanner.cursor.peek().is_none() {
                    if stop == Stop::Eof {
                        return Ok(Script { items });
                    }
                    return Err(scanner.cursor.err("unexpected end of input", true));
                } else {
                    let tok = scanner.cursor.peek().unwrap_or(' ');
                    let msg = alloc::format!("syntax error near unexpected token '{tok}'");
                    return Err(scanner.cursor.err(&msg, false));
                }
            }
        }
    }
}

fn parse_and_or(scanner: &mut Scanner<'_>) -> Result<AndOr, ParseError> {
    let first = parse_pipeline(scanner)?;
    let mut rest = Vec::new();
    loop {
        scanner.skip_ws_and_comments();
        let is_and = match scanner.peek_op() {
            Some(Op::AndIf) => true,
            Some(Op::OrIf) => false,
            _ => break,
        };
        scanner.take_op();
        skip_linebreak(scanner)?;
        if scanner.cursor.peek().is_none() {
            return Err(scanner.cursor.err("expected command after '&&'/'||'", true));
        }
        let next_pipe = parse_pipeline(scanner)?;
        rest.push((is_and, next_pipe));
    }
    scanner.skip_ws_and_comments();
    let background = if scanner.peek_op() == Some(Op::Amp) {
        scanner.take_op();
        true
    } else {
        false
    };
    Ok(AndOr {
        first,
        rest,
        background,
    })
}

fn parse_pipeline(scanner: &mut Scanner<'_>) -> Result<Pipeline, ParseError> {
    scanner.skip_ws_and_comments();
    let bang = if peek_word_lit(scanner).as_deref() == Some("!") {
        parse_word(scanner, TildeMode::None)?;
        scanner.skip_ws_and_comments();
        if scanner.cursor.peek().is_none() {
            return Err(scanner.cursor.err("expected command after '!'", true));
        }
        true
    } else {
        false
    };

    let mut cmds = vec![parse_command(scanner)?];
    loop {
        scanner.skip_ws_and_comments();
        if scanner.peek_op() != Some(Op::Pipe) {
            break;
        }
        scanner.take_op();
        skip_linebreak(scanner)?;
        if scanner.cursor.peek().is_none() {
            return Err(scanner.cursor.err("expected command after '|'", true));
        }
        cmds.push(parse_command(scanner)?);
    }
    Ok(Pipeline { bang, cmds })
}

fn parse_command(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.depth += 1;
    if scanner.depth > MAX_DEPTH {
        scanner.depth -= 1;
        return Err(scanner.cursor.err("nesting depth exceeded", false));
    }
    let res = parse_command_inner(scanner);
    scanner.depth -= 1;
    res
}

fn parse_command_inner(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.skip_ws_and_comments();
    if let Some(op) = scanner.peek_op() {
        if op == Op::LParen {
            return parse_subshell(scanner);
        }
        if op.is_redir() {
            return parse_simple(scanner, Vec::new(), Vec::new(), Vec::new());
        }
        return Err(scanner.cursor.err("unexpected operator", false));
    }

    if let Some(fd) = scanner.try_take_io_number()? {
        let redir = parse_redir(scanner, Some(fd))?;
        return parse_simple(scanner, Vec::new(), Vec::new(), vec![redir]);
    }

    if let Some(name) = scanner.try_take_assign_name() {
        let val = parse_word(scanner, TildeMode::Assign)?;
        return parse_simple(scanner, vec![(name, val)], Vec::new(), Vec::new());
    }

    let word_err = scanner.cursor.err("expected command", false);
    let first = parse_word(scanner, TildeMode::Start)?;
    if first.is_empty() {
        return Err(word_err);
    }

    if let Some(kw) = is_word_single_lit(&first) {
        match kw {
            "if" => return parse_if(scanner),
            "while" => return parse_loop(scanner, false),
            "until" => return parse_loop(scanner, true),
            "for" => return parse_for(scanner),
            "case" => return parse_case(scanner),
            "{" => return parse_group(scanner),
            kw if is_reserved_word(kw) => {
                return Err(scanner.cursor.err("unexpected reserved word", false));
            }
            name if is_valid_name(name) => {
                let cp = scanner.cursor.clone();
                scanner.skip_ws_and_comments();
                if scanner.peek_op() == Some(Op::LParen) {
                    return parse_func(scanner, String::from(name));
                }
                scanner.cursor = cp;
            }
            _ => {}
        }
    }

    parse_simple(scanner, Vec::new(), vec![first], Vec::new())
}

fn parse_simple(
    scanner: &mut Scanner<'_>,
    mut assigns: Vec<(String, Word)>,
    mut words: Vec<Word>,
    mut redirs: Vec<Redir>,
) -> Result<Command, ParseError> {
    loop {
        scanner.skip_ws_and_comments();
        if let Some(op) = scanner.peek_op() {
            if op.is_redir() {
                redirs.push(parse_redir(scanner, None)?);
                continue;
            }
            break;
        }
        if scanner.cursor.peek().is_none() || scanner.cursor.peek() == Some('\n') {
            break;
        }
        if let Some(fd) = scanner.try_take_io_number()? {
            redirs.push(parse_redir(scanner, Some(fd))?);
            continue;
        }
        if words.is_empty() {
            if let Some(name) = scanner.try_take_assign_name() {
                let val = parse_word(scanner, TildeMode::Assign)?;
                assigns.push((name, val));
                continue;
            }
        }
        let w = parse_word(scanner, TildeMode::Start)?;
        if w.is_empty() {
            break;
        }
        words.push(w);
    }
    Ok(Command::Simple(Simple {
        assigns,
        words,
        redirs,
    }))
}

fn parse_redir(scanner: &mut Scanner<'_>, fd: Option<u32>) -> Result<Redir, ParseError> {
    let op_line = scanner.cursor.line;
    let op_col = scanner.cursor.col;
    let tok = scanner
        .take_op()
        .ok_or_else(|| scanner.cursor.err("expected redirection operator", false))?;
    scanner.skip_ws_and_comments();

    if matches!(tok, Op::DLess | Op::DLessDash) {
        let strip_tabs = tok == Op::DLessDash;
        let (delim, quoted) = parse_heredoc_delim(scanner)?;
        let id = scanner.alloc_heredoc();
        scanner.pending_heredocs.push(PendingHereDoc {
            id,
            delim,
            strip_tabs,
            quoted,
            line: op_line,
            col: op_col,
        });
        return Ok(Redir {
            fd,
            op: RedirOp::HereDoc {
                strip_tabs,
                quoted,
                body: vec![Part::Lit(id.to_string())],
            },
            target: Vec::new(),
        });
    }

    if scanner.at_word_end() {
        return Err(scanner.cursor.err("missing redirection target", false));
    }
    let target = parse_word(scanner, TildeMode::Start)?;
    if target.is_empty() {
        return Err(scanner.cursor.err("missing redirection target", false));
    }
    let op = match tok {
        Op::Less => RedirOp::In,
        Op::Great => RedirOp::Out,
        Op::DGreat => RedirOp::Append,
        Op::Clobber => RedirOp::Clobber,
        Op::LessGreat => RedirOp::ReadWrite,
        Op::LessAnd => RedirOp::DupIn,
        Op::GreatAnd => RedirOp::DupOut,
        _ => return Err(scanner.cursor.err("invalid redirection operator", false)),
    };
    Ok(Redir { fd, op, target })
}

pub(crate) fn parse_redirs(scanner: &mut Scanner<'_>) -> Result<Vec<Redir>, ParseError> {
    let mut redirs = Vec::new();
    loop {
        scanner.skip_ws_and_comments();
        if let Some(op) = scanner.peek_op() {
            if op.is_redir() {
                redirs.push(parse_redir(scanner, None)?);
                continue;
            }
            break;
        }
        if let Some(fd) = scanner.try_take_io_number()? {
            redirs.push(parse_redir(scanner, Some(fd))?);
            continue;
        }
        break;
    }
    Ok(redirs)
}

pub(crate) fn parse_cmd_subst(scanner: &mut Scanner<'_>) -> Result<Script, ParseError> {
    scanner.depth += 1;
    if scanner.depth > MAX_DEPTH {
        scanner.depth -= 1;
        return Err(scanner.cursor.err("nesting depth exceeded", false));
    }
    let res = (|| {
        let script = parse_script_until(scanner, Stop::RParen)?;
        if scanner.peek_op() != Some(Op::RParen) {
            return Err(scanner.cursor.err("unterminated $(...)", true));
        }
        scanner.take_op();
        Ok(script)
    })();
    scanner.depth -= 1;
    res
}

pub(crate) fn parse_nested(depth: usize, src: &str, esc: Escapes) -> Result<Script, ParseError> {
    if depth > MAX_DEPTH {
        return Err(ParseError {
            line: 1,
            col: 1,
            msg: String::from("nesting depth exceeded"),
            incomplete: false,
        });
    }
    let mut scanner = Scanner::new(src, esc);
    scanner.depth = depth;
    let mut script = parse_script_until(&mut scanner, Stop::Eof)?;
    if let Some(hd) = scanner.pending_heredocs.first() {
        return Err(ParseError {
            line: hd.line,
            col: hd.col,
            msg: String::from("unterminated here-document"),
            incomplete: true,
        });
    }
    resolve_script(&mut script, &mut scanner.heredocs);
    Ok(script)
}

/// Parse a POSIX shell script into an `ast::Script`.
pub fn parse(src: &str, esc: Escapes) -> Result<Script, ParseError> {
    parse_nested(0, src, esc)
}
