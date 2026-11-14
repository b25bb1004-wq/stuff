//! Types, and the `uses` layer list shared by function declarations and function types.
use super::P;
use crate::ast::{Ident, Type};
use crate::lexer::Tok;
use crate::span::Diag;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// Names of the builtin types, which the lexer reports as lowercase names.
fn builtin_name(name: &str) -> bool {
    matches!(name, "int" | "bool" | "byte" | "str" | "unit")
}

impl P {
    pub(super) fn ty(&mut self) -> Result<Type, Diag> {
        self.enter()?;
        let r = self.ty_body();
        self.leave();
        r
    }

    fn ty_body(&mut self) -> Result<Type, Diag> {
        let start = self.span();
        let mut t = self.base_type()?;
        while self.eat(&Tok::Question) {
            t = Type::Option {
                inner: Box::new(t),
                span: self.since(start),
            };
        }
        Ok(t)
    }

    fn base_type(&mut self) -> Result<Type, Diag> {
        let start = self.span();
        match self.tok().clone() {
            Tok::LIdent(name) if builtin_name(&name) => {
                let span = self.bump().span;
                Ok(match name.as_str() {
                    "int" => Type::Int { span },
                    "bool" => Type::Bool { span },
                    "byte" => Type::Byte { span },
                    "str" => Type::Str { span },
                    _ => Type::Unit { span },
                })
            }
            Tok::UIdent(name) => {
                let span = self.bump().span;
                Ok(Type::Named(Ident { name, span }))
            }
            Tok::LBracket => {
                self.bump();
                let elem = self.ty()?;
                self.expect(&Tok::RBracket, "']' to close the list type")?;
                Ok(Type::List {
                    elem: Box::new(elem),
                    span: self.since(start),
                })
            }
            Tok::LParen => {
                self.bump();
                let first = self.ty()?;
                self.expect(&Tok::Comma, "',' in the tuple type")?;
                let mut items = alloc::vec![first];
                items.extend(self.list(
                    &Tok::RParen,
                    "')' to close the tuple type",
                    "type",
                    false,
                    false,
                    |p| p.ty(),
                )?);
                Ok(Type::Tuple {
                    items,
                    span: self.since(start),
                })
            }
            Tok::Fn => {
                self.bump();
                self.expect(&Tok::LParen, "'(' after 'fn' in a function type")?;
                let params = self.list(
                    &Tok::RParen,
                    "')' to close the parameter types",
                    "type",
                    false,
                    true,
                    |p| p.ty(),
                )?;
                self.expect(&Tok::Arrow, "'->' in the function type")?;
                let ret = self.ty()?;
                let uses = self.uses_opt()?;
                Ok(Type::Fn {
                    params,
                    ret: Box::new(ret),
                    uses,
                    span: self.since(start),
                })
            }
            _ => Err(self.expected("type")),
        }
    }

    /// Optional `uses a, b`. A comma continues the list only when a layer name follows it
    /// and that name is not a builtin type or the start of a `name: type` parameter.
    pub(super) fn uses_opt(&mut self) -> Result<Vec<Ident>, Diag> {
        let mut out = Vec::new();
        if !self.eat(&Tok::Uses) {
            return Ok(out);
        }
        out.push(self.ident_l("layer name after 'uses'")?);
        while self.tok() == &Tok::Comma && self.continues_uses() {
            self.bump();
            out.push(self.ident_l("layer name")?);
        }
        Ok(out)
    }

    fn continues_uses(&self) -> bool {
        match self.peek(1) {
            Tok::LIdent(name) => !builtin_name(name) && self.peek(2) != &Tok::Colon,
            _ => false,
        }
    }
}
