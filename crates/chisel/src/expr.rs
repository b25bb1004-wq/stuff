//! Expression AST and recursive-descent parser with LLVM/GNU precedence.

use crate::expr_tok::{tokenize, Tok};
use crate::AsmError;

/// Relocation modifier wrapping an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelocKind {
    Hi,
    Lo,
    PcrelHi,
    PcrelLo,
}

/// Parsed expression AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expr {
    Num(i64),
    Ident(String),
    NumLabel {
        num: u64,
        forward: bool,
    },
    Unary {
        op: UnaryOp,
        inner: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Reloc {
        kind: RelocKind,
        inner: Box<Expr>,
    },
}

/// Unary expression operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Plus,
    Neg,
    BitNot,
}

/// Binary expression operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitOr,
    BitAnd,
    BitXor,
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<&Tok> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_add_sub(&mut self) -> Result<Expr, AsmError> {
        let mut lhs = self.parse_bitwise()?;
        while let Some(op) = match self.peek() {
            Some(Tok::Plus) => Some(BinOp::Add),
            Some(Tok::Minus) => Some(BinOp::Sub),
            _ => None,
        } {
            self.next();
            let rhs = self.parse_bitwise()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bitwise(&mut self) -> Result<Expr, AsmError> {
        let mut lhs = self.parse_mul_shift()?;
        while let Some(op) = match self.peek() {
            Some(Tok::Pipe) => Some(BinOp::BitOr),
            Some(Tok::Amp) => Some(BinOp::BitAnd),
            Some(Tok::Caret) => Some(BinOp::BitXor),
            _ => None,
        } {
            self.next();
            let rhs = self.parse_mul_shift()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_mul_shift(&mut self) -> Result<Expr, AsmError> {
        let mut lhs = self.parse_unary()?;
        while let Some(op) = match self.peek() {
            Some(Tok::Star) => Some(BinOp::Mul),
            Some(Tok::Slash) => Some(BinOp::Div),
            Some(Tok::Percent) => Some(BinOp::Rem),
            Some(Tok::Shl) => Some(BinOp::Shl),
            Some(Tok::Shr) => Some(BinOp::Shr),
            _ => None,
        } {
            self.next();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, AsmError> {
        let op = match self.peek() {
            Some(Tok::Plus) => Some(UnaryOp::Plus),
            Some(Tok::Minus) => Some(UnaryOp::Neg),
            Some(Tok::Tilde) => Some(UnaryOp::BitNot),
            _ => None,
        };
        if let Some(op) = op {
            self.next();
            let inner = self.parse_unary()?;
            return Ok(Expr::Unary {
                op,
                inner: Box::new(inner),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, AsmError> {
        let Some(tok) = self.next().cloned() else {
            return Err(AsmError::new(self.line, "expected expression"));
        };
        match tok {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Ident(name) => Ok(Expr::Ident(name)),
            Tok::NumLabel { num, forward } => Ok(Expr::NumLabel { num, forward }),
            Tok::LParen => {
                let inner = self.parse_add_sub()?;
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err(AsmError::new(self.line, "missing ')' in expression"));
                }
                Ok(inner)
            }
            Tok::Reloc(kind) => {
                if !matches!(self.next(), Some(Tok::LParen)) {
                    return Err(AsmError::new(self.line, "expected '(' after relocation"));
                }
                let inner = self.parse_add_sub()?;
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err(AsmError::new(self.line, "missing ')' after relocation"));
                }
                Ok(Expr::Reloc {
                    kind,
                    inner: Box::new(inner),
                })
            }
            _ => Err(AsmError::new(self.line, "unexpected token in expression")),
        }
    }
}

/// Parse an expression string into an AST.
pub(crate) fn parse_expr(s: &str, line: usize) -> Result<Expr, AsmError> {
    let toks = tokenize(s, line)?;
    if toks.is_empty() {
        return Err(AsmError::new(line, "empty expression"));
    }
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        line,
    };
    let expr = p.parse_add_sub()?;
    if p.pos != toks.len() {
        return Err(AsmError::new(
            line,
            "unexpected trailing tokens in expression",
        ));
    }
    Ok(expr)
}
