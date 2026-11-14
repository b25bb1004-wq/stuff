//! POSIX shell cursor, operator/comment/here-doc scanner, and line-continuation handling.

use alloc::string::String;
use alloc::vec::Vec;

use crate::ast::{Part, Word};
use crate::types::ParseError;

/// Source cursor tracking 1-based line and column numbers.
#[derive(Clone)]
pub(crate) struct Cursor<'a> {
    pub(crate) src: &'a str,
    pub(crate) pos: usize,
    pub(crate) line: u32,
    pub(crate) col: u32,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub(crate) fn err(&self, msg: &str, incomplete: bool) -> ParseError {
        ParseError {
            line: self.line,
            col: self.col,
            msg: String::from(msg),
            incomplete,
        }
    }

    pub(crate) fn peek_raw(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    pub(crate) fn next_raw(&mut self) -> Option<char> {
        let ch = self.peek_raw()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    /// Skip any `\<newline>` line continuations at the current position.
    pub(crate) fn skip_line_continuations(&mut self) {
        while self.src[self.pos..].starts_with("\\\n") {
            self.pos += 2;
            self.line += 1;
            self.col = 1;
        }
    }

    /// Peek next logical character (skipping `\<newline>` continuations).
    pub(crate) fn peek(&mut self) -> Option<char> {
        self.skip_line_continuations();
        self.peek_raw()
    }

    /// Peek the logical character after the next one (without mutating self).
    pub(crate) fn peek2(&self) -> Option<char> {
        let mut copy = self.clone();
        copy.next_char()?;
        copy.peek()
    }

    /// Consume next logical character (skipping `\<newline>` continuations).
    pub(crate) fn next_char(&mut self) -> Option<char> {
        self.skip_line_continuations();
        self.next_raw()
    }

    pub(crate) fn skip_hspace(&mut self) {
        loop {
            self.skip_line_continuations();
            match self.peek_raw() {
                Some(' ' | '\t') => {
                    self.next_raw();
                }
                _ => break,
            }
        }
    }

    pub(crate) fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while let Some(c) = self.peek_raw() {
                if c == '\n' {
                    break;
                }
                self.next_raw();
            }
        }
    }
}

/// Shell operator tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    AndIf,     // &&
    OrIf,      // ||
    Dsemi,     // ;;
    Semi,      // ;
    Amp,       // &
    Pipe,      // |
    LParen,    // (
    RParen,    // )
    Less,      // <
    Great,     // >
    DGreat,    // >>
    Clobber,   // >|
    LessGreat, // <>
    LessAnd,   // <&
    GreatAnd,  // >&
    DLess,     // <<
    DLessDash, // <<-
}

impl Op {
    pub(crate) fn is_redir(self) -> bool {
        matches!(
            self,
            Self::Less
                | Self::Great
                | Self::DGreat
                | Self::Clobber
                | Self::LessGreat
                | Self::LessAnd
                | Self::GreatAnd
                | Self::DLess
                | Self::DLessDash
        )
    }
}

/// Pending here-document queued on the current line.
#[derive(Debug, Clone)]
pub(crate) struct PendingHereDoc {
    pub(crate) id: usize,
    pub(crate) delim: String,
    pub(crate) strip_tabs: bool,
    pub(crate) quoted: bool,
    pub(crate) line: u32,
    pub(crate) col: u32,
}

/// Shared scanner state passed between parser and word parser.
pub(crate) struct Scanner<'a> {
    pub(crate) cursor: Cursor<'a>,
    pub(crate) pending_heredocs: Vec<PendingHereDoc>,
    pub(crate) heredocs: Vec<Option<Word>>,
    pub(crate) depth: usize,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Self {
            cursor: Cursor::new(src),
            pending_heredocs: Vec::new(),
            heredocs: Vec::new(),
            depth: 0,
        }
    }

    pub(crate) fn alloc_heredoc(&mut self) -> usize {
        let id = self.heredocs.len();
        self.heredocs.push(None);
        id
    }

    /// Skip horizontal whitespace and `#` comments (not newlines).
    pub(crate) fn skip_ws_and_comments(&mut self) {
        self.cursor.skip_hspace();
        self.cursor.skip_comment();
    }

    /// Try to scan an operator starting at the current logical position.
    pub(crate) fn peek_op(&mut self) -> Option<Op> {
        let mut c = self.cursor.clone();
        c.skip_line_continuations();
        let ch1 = c.next_raw()?;
        let ch2 = c.peek();
        let op = match (ch1, ch2) {
            ('&', Some('&')) => Op::AndIf,
            ('&', _) => Op::Amp,
            ('|', Some('|')) => Op::OrIf,
            ('|', _) => Op::Pipe,
            (';', Some(';')) => Op::Dsemi,
            (';', _) => Op::Semi,
            ('(', _) => Op::LParen,
            (')', _) => Op::RParen,
            ('<', Some('<')) => {
                c.next_char();
                if c.peek() == Some('-') {
                    Op::DLessDash
                } else {
                    Op::DLess
                }
            }
            ('<', Some('>')) => Op::LessGreat,
            ('<', Some('&')) => Op::LessAnd,
            ('<', _) => Op::Less,
            ('>', Some('>')) => Op::DGreat,
            ('>', Some('|')) => Op::Clobber,
            ('>', Some('&')) => Op::GreatAnd,
            ('>', _) => Op::Great,
            _ => return None,
        };
        Some(op)
    }

    /// Consume an operator previously peeked with `peek_op`.
    pub(crate) fn take_op(&mut self) -> Option<Op> {
        let op = self.peek_op()?;
        let count = match op {
            Op::DLessDash => 3,
            Op::AndIf
            | Op::OrIf
            | Op::Dsemi
            | Op::DGreat
            | Op::Clobber
            | Op::LessGreat
            | Op::LessAnd
            | Op::GreatAnd
            | Op::DLess => 2,
            Op::Semi | Op::Amp | Op::Pipe | Op::LParen | Op::RParen | Op::Less | Op::Great => 1,
        };
        for _ in 0..count {
            self.cursor.next_char();
        }
        Some(op)
    }

    /// Check if the current position is an `IO_NUMBER` (all ASCII digits immediately before `<` or `>`).
    pub(crate) fn try_take_io_number(&mut self) -> Result<Option<u32>, ParseError> {
        let mut c = self.cursor.clone();
        let start_line = c.line;
        let start_col = c.col;
        let mut digits = String::new();
        while let Some(ch) = c.peek() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                c.next_char();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return Ok(None);
        }
        match c.peek() {
            Some('<' | '>') => {
                let fd = digits.parse::<u32>().map_err(|_| ParseError {
                    line: start_line,
                    col: start_col,
                    msg: String::from("file descriptor out of range"),
                    incomplete: false,
                })?;
                self.cursor = c;
                Ok(Some(fd))
            }
            _ => Ok(None),
        }
    }

    /// Check if the current position starts with `[A-Za-z_][A-Za-z0-9_]*=`.
    pub(crate) fn try_take_assign_name(&mut self) -> Option<String> {
        let mut c = self.cursor.clone();
        let first = c.peek()?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }
        let mut name = String::new();
        name.push(first);
        c.next_char();
        while let Some(ch) = c.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                name.push(ch);
                c.next_char();
            } else if ch == '=' {
                c.next_char();
                self.cursor = c;
                return Some(name);
            } else {
                return None;
            }
        }
        None
    }

    /// Check if we are at a word boundary / separator / EOF.
    pub(crate) fn at_word_end(&mut self) -> bool {
        match self.cursor.peek() {
            None => true,
            Some(c) => matches!(
                c,
                ' ' | '\t' | '\n' | '&' | '|' | ';' | '(' | ')' | '<' | '>'
            ),
        }
    }
}

pub(crate) fn is_word_single_lit(w: &Word) -> Option<&str> {
    if w.len() == 1 {
        if let Part::Lit(s) = &w[0] {
            return Some(s.as_str());
        }
    }
    None
}
