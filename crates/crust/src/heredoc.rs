//! Here-document delimiter and body parsing.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::Part;
use crate::lexer::{PendingHereDoc, Scanner};
use crate::types::ParseError;
use crate::word::{is_word_delim, parse_unquoted_template};

pub(crate) fn parse_heredoc_delim(scanner: &mut Scanner<'_>) -> Result<(String, bool), ParseError> {
    let mut delim = String::new();
    let mut quoted = false;
    let mut any_char = false;

    while let Some(ch) = scanner.cursor.peek() {
        if is_word_delim(ch) {
            break;
        }
        any_char = true;
        match ch {
            '\'' => {
                quoted = true;
                let err = scanner.cursor.err("unterminated single quote", true);
                scanner.cursor.next_char();
                loop {
                    match scanner.cursor.next_raw() {
                        None => return Err(err),
                        Some('\'') => break,
                        Some(c) => delim.push(c),
                    }
                }
            }
            '"' => {
                quoted = true;
                let err = scanner.cursor.err("unterminated double quote", true);
                scanner.cursor.next_char();
                loop {
                    match scanner.cursor.peek() {
                        None => return Err(err),
                        Some('"') => {
                            scanner.cursor.next_char();
                            break;
                        }
                        Some('\\') => {
                            scanner.cursor.next_char();
                            let c = scanner.cursor.next_raw().ok_or_else(|| err.clone())?;
                            if matches!(c, '$' | '`' | '"' | '\\') {
                                delim.push(c);
                            } else {
                                delim.push('\\');
                                delim.push(c);
                            }
                        }
                        Some(c) => {
                            scanner.cursor.next_char();
                            delim.push(c);
                        }
                    }
                }
            }
            '\\' => {
                quoted = true;
                scanner.cursor.next_char();
                let c = scanner.cursor.next_raw().ok_or_else(|| {
                    scanner
                        .cursor
                        .err("trailing backslash in here-doc delimiter", true)
                })?;
                delim.push(c);
            }
            c => {
                scanner.cursor.next_char();
                delim.push(c);
            }
        }
    }

    if !any_char {
        return Err(scanner.cursor.err("missing here-document delimiter", false));
    }
    Ok((delim, quoted))
}

fn ends_with_line_continuation(s: &str) -> bool {
    let trailing_bs = s.bytes().rev().take_while(|&b| b == b'\\').count();
    trailing_bs % 2 == 1
}

pub(crate) fn read_pending_heredocs(scanner: &mut Scanner<'_>) -> Result<(), ParseError> {
    let pending: Vec<PendingHereDoc> = core::mem::take(&mut scanner.pending_heredocs);
    for hd in pending {
        let mut body_str = String::new();
        let mut continuing = false;
        loop {
            if scanner.cursor.peek_raw().is_none() {
                return Err(ParseError {
                    line: hd.line,
                    col: hd.col,
                    msg: String::from("unterminated here-document"),
                    incomplete: true,
                });
            }
            let mut raw_line = String::new();
            let mut had_newline = false;
            while let Some(c) = scanner.cursor.next_raw() {
                if c == '\n' {
                    had_newline = true;
                    break;
                }
                raw_line.push(c);
            }
            let line = if hd.strip_tabs {
                raw_line.trim_start_matches('\t')
            } else {
                raw_line.as_str()
            };
            if !continuing && line == hd.delim {
                break;
            }
            if !had_newline {
                return Err(ParseError {
                    line: hd.line,
                    col: hd.col,
                    msg: String::from("unterminated here-document"),
                    incomplete: true,
                });
            }
            body_str.push_str(line);
            body_str.push('\n');
            continuing = !hd.quoted && ends_with_line_continuation(line);
        }
        let word = if hd.quoted {
            vec![Part::Lit(body_str)]
        } else {
            parse_unquoted_template(&body_str, scanner.depth + 1)?
        };
        if let Some(slot) = scanner.heredocs.get_mut(hd.id) {
            *slot = Some(word);
        }
    }
    Ok(())
}
