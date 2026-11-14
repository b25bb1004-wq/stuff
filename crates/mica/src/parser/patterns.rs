//! Match patterns.
use super::P;
use crate::ast::{Ident, Pattern};
use crate::lexer::Tok;
use crate::span::Diag;
use alloc::boxed::Box;
use alloc::vec::Vec;

impl P {
    pub(super) fn pattern(&mut self) -> Result<Pattern, Diag> {
        self.enter()?;
        let r = self.pattern_body();
        self.leave();
        r
    }

    fn pattern_body(&mut self) -> Result<Pattern, Diag> {
        let start = self.span();
        match self.tok().clone() {
            Tok::LIdent(name) => {
                self.bump();
                if name == "_" {
                    Ok(Pattern::Wild { span: start })
                } else {
                    Ok(Pattern::Bind(Ident { name, span: start }))
                }
            }
            Tok::Int(v) => {
                if v > i64::MAX as u64 {
                    return Err(self.fail(
                        start,
                        "integer literal does not fit in int (i64); it cannot be a pattern",
                    ));
                }
                self.bump();
                Ok(Pattern::Int {
                    value: v as i64,
                    span: start,
                })
            }
            Tok::Byte(value) => {
                self.bump();
                Ok(Pattern::Byte { value, span: start })
            }
            Tok::Str(value) => {
                self.bump();
                Ok(Pattern::Str { value, span: start })
            }
            Tok::True | Tok::False => {
                let value = *self.tok() == Tok::True;
                self.bump();
                Ok(Pattern::Bool { value, span: start })
            }
            Tok::None => {
                self.bump();
                Ok(Pattern::None { span: start })
            }
            Tok::Some => {
                self.bump();
                self.expect(&Tok::LParen, "'(' after 'some' in a pattern")?;
                let inner = self.pattern()?;
                self.expect(&Tok::RParen, "')' to close the 'some' pattern")?;
                Ok(Pattern::Some {
                    inner: Box::new(inner),
                    span: self.since(start),
                })
            }
            Tok::UIdent(name) => {
                self.bump();
                let args = if self.eat(&Tok::LParen) {
                    self.list(
                        &Tok::RParen,
                        "')' to close the pattern",
                        "pattern",
                        false,
                        false,
                        |p| p.pattern(),
                    )?
                } else {
                    Vec::new()
                };
                Ok(Pattern::Variant {
                    name: Ident { name, span: start },
                    args,
                    span: self.since(start),
                })
            }
            Tok::LParen => {
                self.bump();
                let first = self.pattern()?;
                self.expect(&Tok::Comma, "',' in the tuple pattern")?;
                let mut items = alloc::vec![first];
                items.extend(self.list(
                    &Tok::RParen,
                    "')' to close the tuple pattern",
                    "pattern",
                    false,
                    false,
                    |p| p.pattern(),
                )?);
                Ok(Pattern::Tuple {
                    items,
                    span: self.since(start),
                })
            }
            _ => Err(self.expected("pattern")),
        }
    }
}
