//! Expression parser with SQLite precedence for Iridium SQL.

use crate::sql::ast::{BinaryOp, CallArgs, Expr, ExprKind, Span, UnaryOp};
use crate::sql::lexer::{parse_err, TokenKind};
use crate::sql::parser::Parser;
use crate::Error;

/// Nesting cap: deeper input is an error, so it cannot overflow a 2 MiB thread stack.
// shortcut: fixed cap sized for debug builds on a 2 MiB stack, raise it if the stack grows.
const MAX_DEPTH: usize = 64;

pub(crate) fn parse_expr(p: &mut Parser) -> Result<Expr, Error> {
    nested(p, parse_or)
}

/// Run one recursive expression step, counting its depth against `MAX_DEPTH`.
fn nested(p: &mut Parser, step: fn(&mut Parser) -> Result<Expr, Error>) -> Result<Expr, Error> {
    if p.depth >= MAX_DEPTH {
        return Err(parse_err(p.cur().pos, "expression nested too deeply"));
    }
    p.depth += 1;
    let res = step(p);
    p.depth -= 1;
    res
}

struct DepthGuard<'a> {
    parser: &'a mut Parser,
    saved: usize,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.parser.depth = self.saved;
    }
}

fn bump_chain_depth(p: &mut Parser) -> Result<(), Error> {
    if p.depth >= MAX_DEPTH {
        return Err(parse_err(p.cur().pos, "expression nested too deeply"));
    }
    p.depth += 1;
    Ok(())
}

fn parse_or(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_and(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    while guard.parser.eat_kw("OR") {
        bump_chain_depth(guard.parser)?;
        let right = parse_and(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_and(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_not(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    while guard.parser.eat_kw("AND") {
        bump_chain_depth(guard.parser)?;
        let right = parse_not(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_not(p: &mut Parser) -> Result<Expr, Error> {
    if p.is_kw("NOT") {
        let tok = p.advance();
        let inner = nested(p, parse_not)?;
        let end = inner.span.end;
        return Ok(Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(inner),
            },
            span: Span {
                start: tok.pos,
                end,
            },
        });
    }
    parse_eq(p)
}

fn parse_eq(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_cmp(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    loop {
        if guard.parser.eat_kw("IS") {
            bump_chain_depth(guard.parser)?;
            let negated = guard.parser.eat_kw("NOT");
            let null_tok = guard.parser.expect_kw("NULL")?;
            let span = Span {
                start: left.span.start,
                end: null_tok.end,
            };
            left = Expr {
                kind: ExprKind::IsNull {
                    expr: Box::new(left),
                    negated,
                },
                span,
            };
            continue;
        }
        let op = match guard.parser.cur().kind {
            TokenKind::Eq | TokenKind::EqEq => BinaryOp::Eq,
            TokenKind::Ne => BinaryOp::Ne,
            _ => break,
        };
        bump_chain_depth(guard.parser)?;
        guard.parser.advance();
        let right = parse_cmp(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_cmp(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_concat(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    loop {
        if guard.parser.is_kw("NOT")
            && (guard.parser.is_kw_at(1, "IN")
                || guard.parser.is_kw_at(1, "BETWEEN")
                || guard.parser.is_kw_at(1, "LIKE"))
        {
            bump_chain_depth(guard.parser)?;
            guard.parser.advance();
            left = parse_predicate(guard.parser, left, true)?;
            continue;
        }
        if guard.parser.is_kw("IN") || guard.parser.is_kw("BETWEEN") || guard.parser.is_kw("LIKE") {
            bump_chain_depth(guard.parser)?;
            left = parse_predicate(guard.parser, left, false)?;
            continue;
        }
        let op = match guard.parser.cur().kind {
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Ge => BinaryOp::Ge,
            _ => break,
        };
        bump_chain_depth(guard.parser)?;
        guard.parser.advance();
        let right = parse_concat(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_predicate(p: &mut Parser, left: Expr, negated: bool) -> Result<Expr, Error> {
    if p.eat_kw("IN") {
        p.expect(TokenKind::LParen, "'('")?;
        let mut list = Vec::new();
        if p.cur().kind != TokenKind::RParen {
            loop {
                list.push(parse_expr(p)?);
                if !p.eat(TokenKind::Comma) {
                    break;
                }
            }
        }
        let rparen = p.expect(TokenKind::RParen, "')'")?;
        let span = Span {
            start: left.span.start,
            end: rparen.end,
        };
        Ok(Expr {
            kind: ExprKind::InList {
                expr: Box::new(left),
                list,
                negated,
            },
            span,
        })
    } else if p.eat_kw("BETWEEN") {
        let low = parse_concat(p)?;
        p.expect_kw("AND")?;
        let high = parse_concat(p)?;
        let span = Span {
            start: left.span.start,
            end: high.span.end,
        };
        Ok(Expr {
            kind: ExprKind::Between {
                expr: Box::new(left),
                low: Box::new(low),
                high: Box::new(high),
                negated,
            },
            span,
        })
    } else {
        p.expect_kw("LIKE")?;
        let pattern = parse_concat(p)?;
        let span = Span {
            start: left.span.start,
            end: pattern.span.end,
        };
        Ok(Expr {
            kind: ExprKind::Like {
                expr: Box::new(left),
                pattern: Box::new(pattern),
                negated,
            },
            span,
        })
    }
}

fn parse_concat(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_add(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    while guard.parser.eat(TokenKind::Concat) {
        bump_chain_depth(guard.parser)?;
        let right = parse_add(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Concat,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_add(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_mul(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    loop {
        let op = match guard.parser.cur().kind {
            TokenKind::Plus => BinaryOp::Add,
            TokenKind::Minus => BinaryOp::Sub,
            _ => break,
        };
        bump_chain_depth(guard.parser)?;
        guard.parser.advance();
        let right = parse_mul(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_mul(p: &mut Parser) -> Result<Expr, Error> {
    let mut left = parse_unary(p)?;
    let guard = DepthGuard {
        saved: p.depth,
        parser: p,
    };
    loop {
        let op = match guard.parser.cur().kind {
            TokenKind::Star => BinaryOp::Mul,
            TokenKind::Slash => BinaryOp::Div,
            TokenKind::Percent => BinaryOp::Mod,
            _ => break,
        };
        bump_chain_depth(guard.parser)?;
        guard.parser.advance();
        let right = parse_unary(guard.parser)?;
        let span = Span {
            start: left.span.start,
            end: right.span.end,
        };
        left = Expr {
            kind: ExprKind::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        };
    }
    Ok(left)
}

fn parse_unary(p: &mut Parser) -> Result<Expr, Error> {
    match p.cur().kind {
        TokenKind::Minus => {
            let tok = p.advance();
            let inner = nested(p, parse_unary)?;
            let end = inner.span.end;
            Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(inner),
                },
                span: Span {
                    start: tok.pos,
                    end,
                },
            })
        }
        TokenKind::Plus => {
            let tok = p.advance();
            let inner = nested(p, parse_unary)?;
            let end = inner.span.end;
            Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(inner),
                },
                span: Span {
                    start: tok.pos,
                    end,
                },
            })
        }
        _ => parse_primary(p),
    }
}

fn parse_primary(p: &mut Parser) -> Result<Expr, Error> {
    let tok = p.cur().clone();
    match tok.kind {
        TokenKind::Int(digits) => {
            p.advance();
            Ok(Expr {
                kind: ExprKind::IntLit(digits),
                span: Span {
                    start: tok.pos,
                    end: tok.end,
                },
            })
        }
        TokenKind::Real(digits) => {
            p.advance();
            Ok(Expr {
                kind: ExprKind::RealLit(digits),
                span: Span {
                    start: tok.pos,
                    end: tok.end,
                },
            })
        }
        TokenKind::Text(s) => {
            p.advance();
            Ok(Expr {
                kind: ExprKind::Text(s),
                span: Span {
                    start: tok.pos,
                    end: tok.end,
                },
            })
        }
        TokenKind::Blob(b) => {
            p.advance();
            Ok(Expr {
                kind: ExprKind::Blob(b),
                span: Span {
                    start: tok.pos,
                    end: tok.end,
                },
            })
        }
        TokenKind::LParen => {
            p.advance();
            let inner = parse_expr(p)?;
            let rparen = p.expect(TokenKind::RParen, "')'")?;
            Ok(Expr {
                kind: ExprKind::Paren(Box::new(inner)),
                span: Span {
                    start: tok.pos,
                    end: rparen.end,
                },
            })
        }
        TokenKind::Ident(ref name) if name.eq_ignore_ascii_case("NULL") => {
            p.advance();
            Ok(Expr {
                kind: ExprKind::Null,
                span: Span {
                    start: tok.pos,
                    end: tok.end,
                },
            })
        }
        TokenKind::Ident(ref name) if name.eq_ignore_ascii_case("CASE") => parse_case(p),
        TokenKind::Ident(name) => {
            p.advance();
            if p.eat(TokenKind::LParen) {
                parse_call(p, name, tok.pos)
            } else if p.eat(TokenKind::Dot) {
                let (col, _) = p.expect_ident()?;
                let end = p.tokens[p.pos - 1].end;
                Ok(Expr {
                    kind: ExprKind::Column {
                        table: Some(name),
                        name: col,
                        pos: tok.pos,
                    },
                    span: Span {
                        start: tok.pos,
                        end,
                    },
                })
            } else {
                Ok(Expr {
                    kind: ExprKind::Column {
                        table: None,
                        name,
                        pos: tok.pos,
                    },
                    span: Span {
                        start: tok.pos,
                        end: tok.end,
                    },
                })
            }
        }
        _ => Err(parse_err(tok.pos, "expected expression")),
    }
}

fn parse_call(p: &mut Parser, name: String, start: usize) -> Result<Expr, Error> {
    let args = if p.eat(TokenKind::Star) {
        CallArgs::Star
    } else if p.cur().kind == TokenKind::RParen {
        CallArgs::List(Vec::new())
    } else {
        let mut list = Vec::new();
        loop {
            list.push(parse_expr(p)?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        CallArgs::List(list)
    };
    let rparen = p.expect(TokenKind::RParen, "')'")?;
    Ok(Expr {
        kind: ExprKind::Call {
            name,
            args,
            pos: start,
        },
        span: Span {
            start,
            end: rparen.end,
        },
    })
}

fn parse_case(p: &mut Parser) -> Result<Expr, Error> {
    let start = p.advance().pos;
    let operand = if !p.is_kw("WHEN") {
        Some(Box::new(parse_expr(p)?))
    } else {
        None
    };
    let mut whens = Vec::new();
    while p.eat_kw("WHEN") {
        let cond = parse_expr(p)?;
        p.expect_kw("THEN")?;
        let val = parse_expr(p)?;
        whens.push((cond, val));
    }
    if whens.is_empty() {
        return Err(parse_err(p.cur().pos, "CASE requires at least one WHEN"));
    }
    let else_expr = if p.eat_kw("ELSE") {
        Some(Box::new(parse_expr(p)?))
    } else {
        None
    };
    let end_tok = p.expect_kw("END")?;
    Ok(Expr {
        kind: ExprKind::Case {
            operand,
            whens,
            else_expr,
        },
        span: Span {
            start,
            end: end_tok.end,
        },
    })
}
