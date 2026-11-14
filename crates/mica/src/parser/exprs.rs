//! Expressions: precedence climbing for the binary levels, then unary, postfix and primary.
use super::P;
use crate::ast::{Arm, BinOp, Expr, Ident, RecordField, UnOp};
use crate::lexer::Tok;
use crate::span::{Diag, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

/// The literal 9223372036854775808, legal only as the operand of unary minus.
const MIN_LITERAL: u64 = 1 << 63;

enum Op {
    Bin(BinOp),
    Range,
}

/// Binary operator for a token, with its precedence level (1 = loosest, 10 = tightest).
fn binop(t: &Tok) -> Option<(u8, Op)> {
    use BinOp as B;
    Some(match t {
        Tok::OrOr => (1, Op::Bin(B::Or)),
        Tok::AndAnd => (2, Op::Bin(B::And)),
        Tok::EqEq => (3, Op::Bin(B::Eq)),
        Tok::NotEq => (3, Op::Bin(B::Ne)),
        Tok::Lt => (3, Op::Bin(B::Lt)),
        Tok::Le => (3, Op::Bin(B::Le)),
        Tok::Gt => (3, Op::Bin(B::Gt)),
        Tok::Ge => (3, Op::Bin(B::Ge)),
        Tok::DotDot => (4, Op::Range),
        Tok::Bar => (5, Op::Bin(B::BitOr)),
        Tok::Caret => (6, Op::Bin(B::BitXor)),
        Tok::Amp => (7, Op::Bin(B::BitAnd)),
        Tok::Shl => (8, Op::Bin(B::Shl)),
        Tok::Shr => (8, Op::Bin(B::Shr)),
        Tok::Plus => (9, Op::Bin(B::Add)),
        Tok::Minus => (9, Op::Bin(B::Sub)),
        Tok::PlusWrap => (9, Op::Bin(B::AddWrap)),
        Tok::MinusWrap => (9, Op::Bin(B::SubWrap)),
        Tok::Star => (10, Op::Bin(B::Mul)),
        Tok::Slash => (10, Op::Bin(B::Div)),
        Tok::Percent => (10, Op::Bin(B::Rem)),
        Tok::StarWrap => (10, Op::Bin(B::MulWrap)),
        _ => return None,
    })
}

fn is_postfix_start(t: &Tok) -> bool {
    matches!(t, Tok::LParen | Tok::LBracket | Tok::Dot)
}

impl P {
    /// A full expression, the top of the grammar.
    pub(super) fn expr(&mut self) -> Result<Expr, Diag> {
        self.enter()?;
        let r = self.expr_body();
        self.leave();
        r
    }

    /// An expression in the head of `if`, `while`, `for` or `match`, or in a `where` clause.
    pub(super) fn head_expr(&mut self) -> Result<Expr, Diag> {
        self.enter()?;
        let saved = self.no_rec;
        self.no_rec = true;
        let r = self.expr_body();
        self.no_rec = saved;
        self.leave();
        r
    }

    /// Expression without the depth guard; statements call this so a block costs one level.
    pub(super) fn expr_body(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        let mut lhs = self.binary(1)?;
        while self.eat(&Tok::PipeArrow) {
            let rhs = self.postfix()?;
            lhs = Expr::Pipe {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: self.since(start),
            };
        }
        Ok(lhs)
    }

    fn binary(&mut self, min: u8) -> Result<Expr, Diag> {
        let start = self.span();
        let mut lhs = self.unary()?;
        // Comparisons and ranges do not chain: a second operator of the same level stops here.
        let mut chained: Option<u8> = None;
        while let Some((prec, op)) = binop(self.tok()) {
            if prec < min || chained == Some(prec) {
                break;
            }
            self.bump();
            let rhs = self.binary(prec + 1)?;
            let span = self.since(start);
            lhs = match op {
                Op::Bin(b) => Expr::Binary {
                    op: b,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                },
                Op::Range => Expr::Range {
                    start: Box::new(lhs),
                    end: Box::new(rhs),
                    span,
                },
            };
            if prec == 3 || prec == 4 {
                chained = Some(prec);
            }
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        let op = match *self.tok() {
            Tok::Minus => Some(UnOp::Neg),
            Tok::Bang => Some(UnOp::Not),
            _ => None,
        };
        let Some(op) = op else {
            return self.postfix();
        };
        self.enter()?;
        self.bump();
        if op == UnOp::Neg
            && *self.tok() == Tok::Int(MIN_LITERAL)
            && !is_postfix_start(self.peek(1))
        {
            self.bump();
            self.leave();
            return Ok(Expr::Int {
                value: i64::MIN,
                span: self.since(start),
            });
        }
        let operand = self.unary();
        self.leave();
        let operand = operand?;
        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
            span: self.since(start),
        })
    }

    fn postfix(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        let e = self.primary()?;
        if !is_postfix_start(self.tok()) {
            return Ok(e);
        }
        self.postfix_rest(start, e)
    }

    fn postfix_rest(&mut self, start: Span, mut e: Expr) -> Result<Expr, Diag> {
        loop {
            match *self.tok() {
                Tok::LParen => {
                    self.bump();
                    let args = self.normal(|p| {
                        p.list(
                            &Tok::RParen,
                            "')' to close the call",
                            "expression",
                            true,
                            true,
                            |p| p.expr(),
                        )
                    })?;
                    e = Expr::Call {
                        callee: Box::new(e),
                        args,
                        span: self.since(start),
                    };
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.normal(|p| p.expr())?;
                    self.expect(&Tok::RBracket, "']' to close the index")?;
                    e = Expr::Index {
                        base: Box::new(e),
                        index: Box::new(index),
                        span: self.since(start),
                    };
                }
                Tok::Dot => {
                    self.bump();
                    match self.tok().clone() {
                        Tok::LIdent(name) => {
                            let t = self.bump();
                            e = Expr::Field {
                                base: Box::new(e),
                                name: Ident { name, span: t.span },
                                span: self.since(start),
                            };
                        }
                        Tok::Int(v) => {
                            let t = self.bump();
                            let index = u32::try_from(v)
                                .map_err(|_| self.fail(t.span, "tuple index is out of range"))?;
                            e = Expr::TupleField {
                                base: Box::new(e),
                                index,
                                span: self.since(start),
                            };
                        }
                        _ => return Err(self.expected("field name or tuple index after '.'")),
                    }
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Diag> {
        match self.tok() {
            Tok::Int(_) | Tok::Byte(_) | Tok::Str(_) | Tok::True | Tok::False | Tok::None => {
                self.primary_lit()
            }
            Tok::Some => self.primary_some(),
            Tok::LIdent(_) => self.primary_lident(),
            Tok::UIdent(_) => self.primary_uident(),
            Tok::LParen => {
                let start = self.bump().span;
                self.normal(|p| p.paren_rest(start))
            }
            Tok::LBracket => {
                let start = self.bump().span;
                self.normal(|p| p.list_expr(start))
            }
            Tok::If => self.if_expr(),
            Tok::Match => self.match_expr(),
            Tok::LBrace => Ok(Expr::Block(self.block()?)),
            _ => Err(self.expected("expression")),
        }
    }

    fn primary_lit(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        match self.tok().clone() {
            Tok::Int(v) => {
                if v > i64::MAX as u64 {
                    return Err(self.fail(
                        start,
                        &format!("integer literal {} does not fit in int (i64)", v),
                    ));
                }
                self.bump();
                Ok(Expr::Int {
                    value: v as i64,
                    span: self.since(start),
                })
            }
            Tok::Byte(value) => {
                self.bump();
                Ok(Expr::Byte {
                    value,
                    span: self.since(start),
                })
            }
            Tok::Str(value) => {
                self.bump();
                Ok(Expr::Str {
                    value,
                    span: self.since(start),
                })
            }
            Tok::True | Tok::False => {
                let value = *self.tok() == Tok::True;
                self.bump();
                Ok(Expr::Bool {
                    value,
                    span: self.since(start),
                })
            }
            Tok::None => {
                self.bump();
                Ok(Expr::None {
                    span: self.since(start),
                })
            }
            _ => Err(self.expected("expression")),
        }
    }

    fn primary_some(&mut self) -> Result<Expr, Diag> {
        let start = self.bump().span;
        self.expect(&Tok::LParen, "'(' after 'some'")?;
        let value = self.normal(|p| p.expr())?;
        self.expect(&Tok::RParen, "')' to close 'some'")?;
        Ok(Expr::Some {
            value: Box::new(value),
            span: self.since(start),
        })
    }

    fn primary_lident(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        if let Tok::LIdent(name) = self.tok().clone() {
            if name == "_" {
                return Err(self.fail(start, "'_' is only allowed in patterns"));
            }
            self.bump();
            Ok(Expr::Name(Ident { name, span: start }))
        } else {
            Err(self.expected("expression"))
        }
    }

    fn primary_uident(&mut self) -> Result<Expr, Diag> {
        let start = self.span();
        let Tok::UIdent(name) = self.tok().clone() else {
            return Err(self.expected("expression"));
        };
        self.bump();
        let name = Ident { name, span: start };
        if self.eat(&Tok::LParen) {
            let args = self.normal(|p| {
                p.list(
                    &Tok::RParen,
                    "')' to close the value",
                    "expression",
                    true,
                    false,
                    |p| p.expr(),
                )
            })?;
            Ok(Expr::Variant {
                name,
                args,
                span: self.since(start),
            })
        } else if !self.no_rec && *self.tok() == Tok::LBrace {
            self.bump();
            let fields = self.normal(|p| {
                p.list(
                    &Tok::RBrace,
                    "'}' to close the record",
                    "field name",
                    true,
                    false,
                    |p| p.record_field(),
                )
            })?;
            Ok(Expr::Record {
                name,
                fields,
                span: self.since(start),
            })
        } else {
            Ok(Expr::Variant {
                name,
                args: Vec::new(),
                span: self.since(start),
            })
        }
    }

    fn record_field(&mut self) -> Result<RecordField, Diag> {
        let start = self.span();
        let name = self.ident_l("field name")?;
        self.expect(&Tok::Colon, "':' after the field name")?;
        let value = self.expr()?;
        Ok(RecordField {
            name,
            value,
            span: self.since(start),
        })
    }

    /// After `(`: unit, a group, or a tuple of two or more.
    fn paren_rest(&mut self, start: Span) -> Result<Expr, Diag> {
        if self.eat(&Tok::RParen) {
            return Ok(Expr::Unit {
                span: self.since(start),
            });
        }
        let first = self.expr()?;
        if self.eat(&Tok::RParen) {
            return Ok(first);
        }
        self.expect(&Tok::Comma, "',' or ')' to close the group")?;
        let mut items = alloc::vec![first];
        loop {
            items.push(self.expr()?);
            if self.eat(&Tok::RParen) {
                break;
            }
            self.expect(&Tok::Comma, "',' or ')' to close the tuple")?;
        }
        Ok(Expr::Tuple {
            items,
            span: self.since(start),
        })
    }

    /// After `[`: an empty list, a list, or a repeat `[value; count]`.
    fn list_expr(&mut self, start: Span) -> Result<Expr, Diag> {
        if self.eat(&Tok::RBracket) {
            return Ok(Expr::List {
                items: Vec::new(),
                span: self.since(start),
            });
        }
        let first = self.expr()?;
        if self.eat(&Tok::Semi) {
            let count = self.expr()?;
            self.expect(&Tok::RBracket, "']' to close the repeat")?;
            return Ok(Expr::Repeat {
                value: Box::new(first),
                count: Box::new(count),
                span: self.since(start),
            });
        }
        let mut items = alloc::vec![first];
        if !self.eat(&Tok::RBracket) {
            self.expect(&Tok::Comma, "',' or ']' to close the list")?;
            let rest = self.list(
                &Tok::RBracket,
                "']' to close the list",
                "expression",
                true,
                true,
                |p| p.expr(),
            )?;
            items.extend(rest);
        }
        Ok(Expr::List {
            items,
            span: self.since(start),
        })
    }

    fn if_expr(&mut self) -> Result<Expr, Diag> {
        let start = self.bump().span;
        let cond = self.head_expr()?;
        let then = self.block()?;
        let els = if self.eat(&Tok::Else) {
            let e = if *self.tok() == Tok::If {
                self.enter()?;
                let r = self.if_expr();
                self.leave();
                r?
            } else {
                Expr::Block(self.block()?)
            };
            Some(Box::new(e))
        } else {
            None
        };
        Ok(Expr::If {
            cond: Box::new(cond),
            then,
            els,
            span: self.since(start),
        })
    }

    fn match_expr(&mut self) -> Result<Expr, Diag> {
        let start = self.bump().span;
        let subject = self.head_expr()?;
        self.expect(&Tok::LBrace, "'{' to open the match arms")?;
        if *self.tok() == Tok::RBrace {
            return Err(self.expected("pattern"));
        }
        let arms = self.normal(|p| {
            p.list(
                &Tok::RBrace,
                "'}' to close the match",
                "pattern",
                true,
                false,
                |p| p.arm(),
            )
        })?;
        Ok(Expr::Match {
            subject: Box::new(subject),
            arms,
            span: self.since(start),
        })
    }

    fn arm(&mut self) -> Result<Arm, Diag> {
        let start = self.span();
        let pattern = self.pattern()?;
        self.expect(&Tok::FatArrow, "'=>' after the pattern")?;
        let body = self.expr()?;
        Ok(Arm {
            pattern,
            body,
            span: self.since(start),
        })
    }
}
