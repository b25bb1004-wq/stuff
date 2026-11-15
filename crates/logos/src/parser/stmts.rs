//! Blocks and statements.
use super::P;
use crate::ast::{Block, Expr, Ident, Stmt};
use crate::lexer::Tok;
use crate::span::Diag;
use alloc::vec::Vec;

fn is_place(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Name(_) | Expr::Field { .. } | Expr::TupleField { .. } | Expr::Index { .. }
    )
}

impl P {
    pub(super) fn block(&mut self) -> Result<Block, Diag> {
        self.enter()?;
        let r = self.block_body();
        self.leave();
        r
    }

    fn block_body(&mut self) -> Result<Block, Diag> {
        let start = self.span();
        self.expect(&Tok::LBrace, "'{' to open a block")?;
        let saved = self.no_rec;
        self.no_rec = false;
        let mut stmts = Vec::new();
        let res = loop {
            self.skip_terms();
            if self.eat(&Tok::RBrace) {
                break Ok(());
            }
            if *self.tok() == Tok::Eof {
                break Err(self.expected("'}' to close the block"));
            }
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(e) => break Err(e),
            }
            match self.tok() {
                Tok::Newline | Tok::Semi | Tok::RBrace => {}
                _ => break Err(self.expected("newline or '}' after the statement")),
            }
        };
        self.no_rec = saved;
        res?;
        Ok(Block {
            stmts,
            span: self.since(start),
        })
    }

    pub(super) fn stmt(&mut self) -> Result<Stmt, Diag> {
        match *self.tok() {
            Tok::Let => self.let_stmt(false),
            Tok::Var => self.let_stmt(true),
            Tok::Return => self.return_stmt(),
            Tok::Break => {
                let start = self.bump().span;
                Ok(Stmt::Break {
                    span: self.since(start),
                })
            }
            Tok::Continue => {
                let start = self.bump().span;
                Ok(Stmt::Continue {
                    span: self.since(start),
                })
            }
            Tok::While => self.while_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Assert => self.assert_stmt(),
            _ => self.expr_or_assign_stmt(),
        }
    }

    fn return_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.bump().span;
        let value = if matches!(
            self.tok(),
            Tok::Newline | Tok::Semi | Tok::RBrace | Tok::Eof
        ) {
            None
        } else {
            Some(self.expr()?)
        };
        Ok(Stmt::Return {
            value,
            span: self.since(start),
        })
    }

    fn while_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.bump().span;
        let cond = self.head_expr()?;
        let body = self.block()?;
        Ok(Stmt::While {
            cond,
            body,
            span: self.since(start),
        })
    }

    fn for_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.bump().span;
        let var: Ident = self.ident_l("loop variable after 'for'")?;
        self.expect(&Tok::In, "'in' after the loop variable")?;
        let iter = self.head_expr()?;
        let body = self.block()?;
        Ok(Stmt::For {
            var,
            iter,
            body,
            span: self.since(start),
        })
    }

    fn assert_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.bump().span;
        let cond = self.expr()?;
        let msg = if self.eat(&Tok::Comma) {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Stmt::Assert {
            cond,
            msg,
            span: self.since(start),
        })
    }

    fn expr_or_assign_stmt(&mut self) -> Result<Stmt, Diag> {
        let start = self.span();
        let target = self.expr_body()?;
        let span = self.since(start);
        if self.eat(&Tok::Eq) {
            if !is_place(&target) {
                return Err(self.fail(
                    span,
                    "left side of '=' must be a variable, a field or an index",
                ));
            }
            let value = self.expr()?;
            Ok(Stmt::Assign {
                target,
                value,
                span: self.since(start),
            })
        } else {
            Ok(Stmt::Expr { expr: target, span })
        }
    }

    fn let_stmt(&mut self, mutable: bool) -> Result<Stmt, Diag> {
        let start = self.bump().span;
        let name = self.ident_l("variable name after 'let' or 'var'")?;
        let ty = if self.eat(&Tok::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect(&Tok::Eq, "'=' in the binding")?;
        let value = self.expr()?;
        Ok(Stmt::Let {
            mutable,
            name,
            ty,
            value,
            span: self.since(start),
        })
    }
}
