//! Compound command parsers (`if`, `while`/`until`, `for`, `case`, `subshell`, `group`, `func`) and here-doc resolution.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ast::{Command, ParamOp, Part, Pipeline, Redir, RedirOp, Script, Word};
use crate::heredoc::read_pending_heredocs;
use crate::lexer::{is_word_single_lit, Op, Scanner};
use crate::parser::{parse_redirs, parse_script_until, peek_word_lit, skip_linebreak, Stop};
use crate::types::{is_valid_name, ParseError};
use crate::word::{parse_word, TildeMode};

pub(crate) fn parse_func(scanner: &mut Scanner<'_>, name: String) -> Result<Command, ParseError> {
    scanner.take_op(); // '('
    scanner.skip_ws_and_comments();
    if scanner.peek_op() != Some(Op::RParen) {
        let inc = scanner.cursor.peek().is_none();
        return Err(scanner
            .cursor
            .err("expected ')' in function definition", inc));
    }
    scanner.take_op(); // ')'
    skip_linebreak(scanner)?;
    if scanner.cursor.peek().is_none() {
        return Err(scanner.cursor.err("expected function body", true));
    }
    let body = parse_compound_command(scanner)?;
    Ok(Command::Func {
        name,
        body: Box::new(body),
    })
}

fn parse_compound_command(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.skip_ws_and_comments();
    if scanner.peek_op() == Some(Op::LParen) {
        return parse_subshell(scanner);
    }
    match peek_word_lit(scanner).as_deref() {
        Some("{") => {
            parse_word(scanner, TildeMode::None)?;
            parse_group(scanner)
        }
        Some("if") => {
            parse_word(scanner, TildeMode::None)?;
            parse_if(scanner)
        }
        Some("while") => {
            parse_word(scanner, TildeMode::None)?;
            parse_loop(scanner, false)
        }
        Some("until") => {
            parse_word(scanner, TildeMode::None)?;
            parse_loop(scanner, true)
        }
        Some("for") => {
            parse_word(scanner, TildeMode::None)?;
            parse_for(scanner)
        }
        Some("case") => {
            parse_word(scanner, TildeMode::None)?;
            parse_case(scanner)
        }
        _ => Err(scanner.cursor.err("expected compound command", false)),
    }
}

pub(crate) fn parse_subshell(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.take_op(); // '('
    let body = parse_script_until(scanner, Stop::RParen)?;
    if scanner.peek_op() != Some(Op::RParen) {
        return Err(scanner.cursor.err("expected ')'", true));
    }
    scanner.take_op();
    if body.items.is_empty() {
        return Err(scanner.cursor.err("empty subshell", false));
    }
    let redirs = parse_redirs(scanner)?;
    Ok(Command::Subshell(body, redirs))
}

pub(crate) fn parse_group(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    let body = parse_script_until(scanner, Stop::RBrace)?;
    if peek_word_lit(scanner).as_deref() != Some("}") {
        return Err(scanner.cursor.err("expected '}'", true));
    }
    parse_word(scanner, TildeMode::None)?;
    if body.items.is_empty() {
        return Err(scanner.cursor.err("empty brace group", false));
    }
    let redirs = parse_redirs(scanner)?;
    Ok(Command::Group(body, redirs))
}

pub(crate) fn parse_if(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    let mut arms = Vec::new();
    let mut otherwise = None;
    loop {
        let cond = parse_script_until(scanner, Stop::Then)?;
        if peek_word_lit(scanner).as_deref() != Some("then") {
            return Err(scanner.cursor.err("expected 'then'", true));
        }
        parse_word(scanner, TildeMode::None)?;
        if cond.items.is_empty() {
            return Err(scanner.cursor.err("empty 'if' condition", false));
        }
        let body = parse_script_until(scanner, Stop::ElifElseFi)?;
        if body.items.is_empty() {
            return Err(scanner.cursor.err("empty 'if' body", false));
        }
        arms.push((cond, body));

        match peek_word_lit(scanner).as_deref() {
            Some("elif") => {
                parse_word(scanner, TildeMode::None)?;
            }
            Some("else") => {
                parse_word(scanner, TildeMode::None)?;
                let else_body = parse_script_until(scanner, Stop::Fi)?;
                if peek_word_lit(scanner).as_deref() != Some("fi") {
                    return Err(scanner.cursor.err("expected 'fi'", true));
                }
                parse_word(scanner, TildeMode::None)?;
                if else_body.items.is_empty() {
                    return Err(scanner.cursor.err("empty 'else' body", false));
                }
                otherwise = Some(else_body);
                break;
            }
            Some("fi") => {
                parse_word(scanner, TildeMode::None)?;
                break;
            }
            _ => return Err(scanner.cursor.err("expected 'fi'", true)),
        }
    }
    let redirs = parse_redirs(scanner)?;
    Ok(Command::If {
        arms,
        otherwise,
        redirs,
    })
}

pub(crate) fn parse_loop(scanner: &mut Scanner<'_>, until: bool) -> Result<Command, ParseError> {
    let cond = parse_script_until(scanner, Stop::Do)?;
    if peek_word_lit(scanner).as_deref() != Some("do") {
        return Err(scanner.cursor.err("expected 'do'", true));
    }
    parse_word(scanner, TildeMode::None)?;
    if cond.items.is_empty() {
        return Err(scanner.cursor.err("empty loop condition", false));
    }
    let body = parse_script_until(scanner, Stop::Done)?;
    if peek_word_lit(scanner).as_deref() != Some("done") {
        return Err(scanner.cursor.err("expected 'done'", true));
    }
    parse_word(scanner, TildeMode::None)?;
    if body.items.is_empty() {
        return Err(scanner.cursor.err("empty loop body", false));
    }
    let redirs = parse_redirs(scanner)?;
    Ok(Command::Loop {
        until,
        cond,
        body,
        redirs,
    })
}

pub(crate) fn parse_for(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.skip_ws_and_comments();
    if scanner.cursor.peek().is_none() {
        return Err(scanner.cursor.err("expected variable name in 'for'", true));
    }
    let var_word = parse_word(scanner, TildeMode::None)?;
    let Some(var_name) = is_word_single_lit(&var_word).filter(|s| is_valid_name(s)) else {
        return Err(scanner.cursor.err("invalid 'for' variable name", false));
    };
    let var = String::from(var_name);

    skip_linebreak(scanner)?;
    let words = if scanner.peek_op() == Some(Op::Semi) {
        scanner.take_op();
        skip_linebreak(scanner)?;
        None
    } else if peek_word_lit(scanner).as_deref() == Some("in") {
        parse_word(scanner, TildeMode::None)?;
        let mut items = Vec::new();
        loop {
            scanner.skip_ws_and_comments();
            if scanner.peek_op() == Some(Op::Semi) {
                scanner.take_op();
                skip_linebreak(scanner)?;
                break;
            }
            if scanner.cursor.peek() == Some('\n') {
                scanner.cursor.next_char();
                read_pending_heredocs(scanner)?;
                skip_linebreak(scanner)?;
                break;
            }
            if scanner.cursor.peek().is_none() {
                return Err(scanner.cursor.err("unterminated 'for' list", true));
            }
            if scanner.peek_op().is_some() {
                return Err(scanner
                    .cursor
                    .err("unexpected operator in 'for' list", false));
            }
            items.push(parse_word(scanner, TildeMode::Start)?);
        }
        Some(items)
    } else {
        None
    };

    if peek_word_lit(scanner).as_deref() != Some("do") {
        let inc = scanner.cursor.peek().is_none();
        return Err(scanner.cursor.err("expected 'do' in 'for'", inc));
    }
    parse_word(scanner, TildeMode::None)?;
    let body = parse_script_until(scanner, Stop::Done)?;
    if peek_word_lit(scanner).as_deref() != Some("done") {
        return Err(scanner.cursor.err("expected 'done'", true));
    }
    parse_word(scanner, TildeMode::None)?;
    if body.items.is_empty() {
        return Err(scanner.cursor.err("empty 'for' body", false));
    }
    let redirs = parse_redirs(scanner)?;
    Ok(Command::For {
        var,
        words,
        body,
        redirs,
    })
}

pub(crate) fn parse_case(scanner: &mut Scanner<'_>) -> Result<Command, ParseError> {
    scanner.skip_ws_and_comments();
    if scanner.cursor.peek().is_none() {
        return Err(scanner.cursor.err("expected word after 'case'", true));
    }
    if scanner.peek_op().is_some() || scanner.cursor.peek() == Some('\n') {
        return Err(scanner.cursor.err("expected word after 'case'", false));
    }
    let word = parse_word(scanner, TildeMode::Start)?;
    skip_linebreak(scanner)?;

    if peek_word_lit(scanner).as_deref() != Some("in") {
        let inc = scanner.cursor.peek().is_none();
        return Err(scanner.cursor.err("expected 'in' in 'case'", inc));
    }
    parse_word(scanner, TildeMode::None)?;

    let mut arms = Vec::new();
    loop {
        skip_linebreak(scanner)?;
        if scanner.cursor.peek().is_none() {
            return Err(scanner.cursor.err("expected 'esac'", true));
        }
        if peek_word_lit(scanner).as_deref() == Some("esac") {
            parse_word(scanner, TildeMode::None)?;
            break;
        }
        if scanner.peek_op() == Some(Op::LParen) {
            scanner.take_op();
        }
        let mut pats = Vec::new();
        loop {
            scanner.skip_ws_and_comments();
            if scanner.cursor.peek().is_none() {
                return Err(scanner.cursor.err("unterminated case pattern", true));
            }
            if scanner.peek_op().is_some() || scanner.cursor.peek() == Some('\n') {
                return Err(scanner.cursor.err("expected case pattern", false));
            }
            pats.push(parse_word(scanner, TildeMode::Start)?);
            scanner.skip_ws_and_comments();
            match scanner.peek_op() {
                Some(Op::Pipe) => {
                    scanner.take_op();
                }
                Some(Op::RParen) => {
                    scanner.take_op();
                    break;
                }
                _ => {
                    let inc = scanner.cursor.peek().is_none();
                    return Err(scanner.cursor.err("expected ')' after case pattern", inc));
                }
            }
        }
        skip_linebreak(scanner)?;
        let arm_body = parse_script_until(scanner, Stop::CaseArm)?;
        arms.push((pats, arm_body));
        if scanner.peek_op() == Some(Op::Dsemi) {
            scanner.take_op();
        } else if peek_word_lit(scanner).as_deref() != Some("esac") {
            let inc = scanner.cursor.peek().is_none();
            return Err(scanner.cursor.err("expected ';;' or 'esac'", inc));
        }
    }

    let redirs = parse_redirs(scanner)?;
    Ok(Command::Case { word, arms, redirs })
}

pub(crate) fn resolve_script(script: &mut Script, heredocs: &mut [Option<Word>]) {
    for item in &mut script.items {
        resolve_pipeline(&mut item.first, heredocs);
        for (_, p) in &mut item.rest {
            resolve_pipeline(p, heredocs);
        }
    }
}

fn resolve_pipeline(p: &mut Pipeline, heredocs: &mut [Option<Word>]) {
    for cmd in &mut p.cmds {
        resolve_command(cmd, heredocs);
    }
}

fn resolve_command(cmd: &mut Command, heredocs: &mut [Option<Word>]) {
    match cmd {
        Command::Simple(s) => {
            for (_, w) in &mut s.assigns {
                resolve_word(w, heredocs);
            }
            for w in &mut s.words {
                resolve_word(w, heredocs);
            }
            resolve_redirs(&mut s.redirs, heredocs);
        }
        Command::Subshell(s, r) | Command::Group(s, r) => {
            resolve_script(s, heredocs);
            resolve_redirs(r, heredocs);
        }
        Command::If {
            arms,
            otherwise,
            redirs,
        } => {
            for (c, b) in arms {
                resolve_script(c, heredocs);
                resolve_script(b, heredocs);
            }
            if let Some(o) = otherwise {
                resolve_script(o, heredocs);
            }
            resolve_redirs(redirs, heredocs);
        }
        Command::Loop {
            cond, body, redirs, ..
        } => {
            resolve_script(cond, heredocs);
            resolve_script(body, heredocs);
            resolve_redirs(redirs, heredocs);
        }
        Command::For {
            words,
            body,
            redirs,
            ..
        } => {
            if let Some(ws) = words {
                for w in ws {
                    resolve_word(w, heredocs);
                }
            }
            resolve_script(body, heredocs);
            resolve_redirs(redirs, heredocs);
        }
        Command::Case { word, arms, redirs } => {
            resolve_word(word, heredocs);
            for (pats, body) in arms {
                for p in pats {
                    resolve_word(p, heredocs);
                }
                resolve_script(body, heredocs);
            }
            resolve_redirs(redirs, heredocs);
        }
        Command::Func { body, .. } => resolve_command(body, heredocs),
    }
}

fn resolve_redirs(redirs: &mut [Redir], heredocs: &mut [Option<Word>]) {
    for r in redirs {
        resolve_word(&mut r.target, heredocs);
        if let RedirOp::HereDoc { body, .. } = &mut r.op {
            if let Some(id_str) = is_word_single_lit(body) {
                if let Ok(id) = id_str.parse::<usize>() {
                    if let Some(slot) = heredocs.get_mut(id) {
                        if let Some(resolved) = slot.take() {
                            *body = resolved;
                        }
                    }
                }
            }
            resolve_word(body, heredocs);
        }
    }
}

pub(crate) fn resolve_word(word: &mut Word, heredocs: &mut [Option<Word>]) {
    for part in word {
        match part {
            Part::Double(parts) => resolve_word(parts, heredocs),
            Part::Cmd(s) => resolve_script(s, heredocs),
            Part::Param(p) => match &mut p.op {
                ParamOp::Plain | ParamOp::Length => {}
                ParamOp::Default(_, w)
                | ParamOp::Assign(_, w)
                | ParamOp::Error(_, w)
                | ParamOp::Alt(_, w)
                | ParamOp::TrimSuffix(_, w)
                | ParamOp::TrimPrefix(_, w) => resolve_word(w, heredocs),
            },
            Part::Lit(_) | Part::Single(_) | Part::Escaped(_) | Part::Arith(_) | Part::Tilde(_) => {
            }
        }
    }
}
