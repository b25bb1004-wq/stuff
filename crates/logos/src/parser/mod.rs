//! Recursive-descent parser: tokens to a syntax tree. It stops at the first error.
use crate::ast::{Ident, Module};
use crate::lexer::{fixed_text, lex, Tok, Token};
use crate::span::{Diag, Span};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod exprs;
mod items;
mod patterns;
mod stmts;
mod types;

#[cfg(test)]
mod tests;

/// Deepest allowed nesting of expressions, blocks, types and patterns.
const MAX_DEPTH: u32 = 200;

/// Parses a whole Logos source file. Never panics; nesting past 200 levels is a `Diag`.
pub fn parse(src: &str) -> Result<Module, Diag> {
    let toks = lex(src)?;
    let end = toks.last().map_or(Span { start: 0, end: 0 }, |t| t.span);
    let mut p = P {
        toks,
        pos: 0,
        depth: 0,
        no_rec: false,
        eof: Token {
            tok: Tok::Eof,
            span: end,
        },
    };
    p.module()
}

/// Parser state: the token cursor, the nesting depth and the record-head flag.
struct P {
    toks: Vec<Token>,
    pos: usize,
    depth: u32,
    /// True while parsing the head of `if`, `while`, `for`, `match`, where a bare record is refused.
    no_rec: bool,
    eof: Token,
}

/// Describes a token for an error message.
fn describe(t: &Tok) -> String {
    match t {
        Tok::Newline => String::from("newline"),
        Tok::Eof => String::from("end of input"),
        Tok::LIdent(s) | Tok::UIdent(s) => format!("name '{}'", s),
        Tok::Int(v) => format!("integer {}", v),
        Tok::Byte(_) => String::from("byte literal"),
        Tok::Str(_) => String::from("string literal"),
        other => match fixed_text(other) {
            Some(s) => format!("'{}'", s),
            None => String::from("token"),
        },
    }
}

impl P {
    fn module(&mut self) -> Result<Module, Diag> {
        let mut items = Vec::new();
        loop {
            self.skip_terms();
            if *self.tok() == Tok::Eof {
                break;
            }
            items.push(self.item()?);
            match self.tok() {
                Tok::Newline | Tok::Semi | Tok::Eof => {}
                _ => return Err(self.expected("newline after the item")),
            }
        }
        Ok(Module { items })
    }

    fn cur(&self) -> &Token {
        self.toks.get(self.pos).unwrap_or(&self.eof)
    }

    fn tok(&self) -> &Tok {
        &self.cur().tok
    }

    fn span(&self) -> Span {
        self.cur().span
    }

    fn peek(&self, n: usize) -> &Tok {
        self.toks
            .get(self.pos + n)
            .map_or(&self.eof.tok, |t| &t.tok)
    }

    fn bump(&mut self) -> Token {
        let t = self.cur().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    /// Span from `start` to the end of the last consumed token.
    fn since(&self, start: Span) -> Span {
        let end = self
            .pos
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(start.end, |t| t.span.end);
        Span {
            start: start.start,
            end: end.max(start.start),
        }
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.tok() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<Span, Diag> {
        if self.tok() == t {
            Ok(self.bump().span)
        } else {
            Err(self.expected(what))
        }
    }

    /// Error "expected WHAT, found X" at the current token.
    fn expected(&self, what: &str) -> Diag {
        Diag {
            span: self.span(),
            message: format!("expected {}, found {}", what, describe(self.tok())),
        }
    }

    fn fail(&self, span: Span, message: &str) -> Diag {
        Diag {
            span,
            message: String::from(message),
        }
    }

    fn enter(&mut self) -> Result<(), Diag> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.fail(self.span(), "nesting too deep (limit is 200 levels)"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn skip_terms(&mut self) {
        while matches!(self.tok(), Tok::Newline | Tok::Semi) {
            self.bump();
        }
    }

    /// Runs `f` with the record-head restriction lifted (inside brackets and blocks).
    fn normal<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Diag>) -> Result<T, Diag> {
        let saved = self.no_rec;
        self.no_rec = false;
        let r = f(self);
        self.no_rec = saved;
        r
    }

    fn ident_l(&mut self, what: &str) -> Result<Ident, Diag> {
        match self.tok().clone() {
            Tok::LIdent(name) => {
                let t = self.bump();
                Ok(Ident { name, span: t.span })
            }
            _ => Err(self.expected(what)),
        }
    }

    fn ident_u(&mut self, what: &str) -> Result<Ident, Diag> {
        match self.tok().clone() {
            Tok::UIdent(name) => {
                let t = self.bump();
                Ok(Ident { name, span: t.span })
            }
            _ => Err(self.expected(what)),
        }
    }

    fn string_lit(&mut self, what: &str) -> Result<(String, Span), Diag> {
        match self.tok().clone() {
            Tok::Str(s) => {
                let t = self.bump();
                Ok((s, t.span))
            }
            _ => Err(self.expected(what)),
        }
    }

    /// Comma-separated items up to `close` (consumed). `what` names the closer for the
    /// message, `item_what` names an item. A trailing comma is accepted when `trailing`.
    fn list<T>(
        &mut self,
        close: &Tok,
        what: &str,
        item_what: &str,
        trailing: bool,
        allow_empty: bool,
        mut f: impl FnMut(&mut Self) -> Result<T, Diag>,
    ) -> Result<Vec<T>, Diag> {
        let mut out = Vec::new();
        if self.tok() == close {
            if allow_empty {
                self.bump();
                return Ok(out);
            }
            return Err(self.expected(item_what));
        }
        loop {
            out.push(f(self)?);
            if self.eat(close) {
                break;
            }
            if !self.eat(&Tok::Comma) {
                return Err(self.expected(&format!("',' or {}", what)));
            }
            if trailing && self.tok() == close {
                self.bump();
                break;
            }
        }
        Ok(out)
    }
}
