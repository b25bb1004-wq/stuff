//! Recursive-descent SQL statement parser for Iridium.

use crate::sql::ast::{
    ColType, ColumnDefAst, CreateIndexStmt, CreateTableStmt, DeleteStmt, InsertStmt, Stmt,
    UpdateStmt,
};
use crate::sql::lexer::{parse_err, tokenize, Token, TokenKind};
use crate::sql::parser_expr::parse_expr;
use crate::sql::parser_select::parse_select;
use crate::Error;

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) pos: usize,
    pub(crate) depth: usize,
}

pub(crate) fn parse_statement(sql: &str) -> Result<Stmt, Error> {
    let tokens = tokenize(sql)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    if p.at_eof() {
        return Err(parse_err(p.cur().pos, "empty SQL statement"));
    }
    let stmt = if p.eat_kw("EXPLAIN") {
        p.expect_kw("SELECT")?;
        Stmt::Explain(parse_select(&mut p)?)
    } else if p.eat_kw("SELECT") {
        Stmt::Select(parse_select(&mut p)?)
    } else if p.eat_kw("CREATE") {
        parse_create(&mut p)?
    } else if p.eat_kw("DROP") {
        parse_drop(&mut p)?
    } else if p.eat_kw("INSERT") {
        Stmt::Insert(parse_insert(&mut p)?)
    } else if p.eat_kw("UPDATE") {
        Stmt::Update(parse_update(&mut p)?)
    } else if p.eat_kw("DELETE") {
        Stmt::Delete(parse_delete(&mut p)?)
    } else if p.eat_kw("BEGIN") {
        let _ = p.eat_kw("TRANSACTION");
        Stmt::Begin
    } else if p.eat_kw("COMMIT") {
        let _ = p.eat_kw("TRANSACTION");
        Stmt::Commit
    } else if p.eat_kw("ROLLBACK") {
        let _ = p.eat_kw("TRANSACTION");
        Stmt::Rollback
    } else {
        return Err(parse_err(p.cur().pos, "expected SQL statement"));
    };

    let _ = p.eat(TokenKind::Semi);
    if !p.at_eof() {
        return Err(parse_err(p.cur().pos, "unexpected trailing tokens"));
    }
    Ok(stmt)
}

impl Parser {
    pub(crate) fn cur(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    pub(crate) fn peek(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    pub(crate) fn at_eof(&self) -> bool {
        matches!(self.cur().kind, TokenKind::Eof)
    }

    pub(crate) fn advance(&mut self) -> Token {
        let t = self.cur().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    pub(crate) fn is_kw(&self, kw: &str) -> bool {
        matches!(&self.cur().kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    pub(crate) fn is_kw_at(&self, offset: usize, kw: &str) -> bool {
        matches!(&self.peek(offset).kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    pub(crate) fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect_kw(&mut self, kw: &str) -> Result<Token, Error> {
        if self.is_kw(kw) {
            Ok(self.advance())
        } else {
            Err(parse_err(self.cur().pos, format!("expected keyword {kw}")))
        }
    }

    pub(crate) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.cur().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: TokenKind, what: &str) -> Result<Token, Error> {
        if self.cur().kind == kind {
            Ok(self.advance())
        } else {
            Err(parse_err(self.cur().pos, format!("expected {what}")))
        }
    }

    pub(crate) fn expect_ident(&mut self) -> Result<(String, usize), Error> {
        let t = self.cur().clone();
        match t.kind {
            TokenKind::Ident(name) => {
                if name.eq_ignore_ascii_case("NULL") {
                    return Err(parse_err(t.pos, "expected identifier, found NULL"));
                }
                self.advance();
                Ok((name, t.pos))
            }
            _ => Err(parse_err(t.pos, "expected identifier")),
        }
    }
}

fn parse_create(p: &mut Parser) -> Result<Stmt, Error> {
    if p.eat_kw("TABLE") {
        let if_not_exists = if p.eat_kw("IF") {
            p.expect_kw("NOT")?;
            p.expect_kw("EXISTS")?;
            true
        } else {
            false
        };
        let (name, name_pos) = p.expect_ident()?;
        p.expect(TokenKind::LParen, "'('")?;
        let mut columns = Vec::new();
        loop {
            columns.push(parse_column_def(p)?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        p.expect(TokenKind::RParen, "')'")?;
        let _ = p.eat_kw("STRICT");
        Ok(Stmt::CreateTable(CreateTableStmt {
            name,
            name_pos,
            if_not_exists,
            columns,
        }))
    } else {
        let unique = p.eat_kw("UNIQUE");
        p.expect_kw("INDEX")?;
        let if_not_exists = if p.eat_kw("IF") {
            p.expect_kw("NOT")?;
            p.expect_kw("EXISTS")?;
            true
        } else {
            false
        };
        let (name, name_pos) = p.expect_ident()?;
        p.expect_kw("ON")?;
        let (table, _) = p.expect_ident()?;
        p.expect(TokenKind::LParen, "'('")?;
        let mut columns = Vec::new();
        loop {
            columns.push(p.expect_ident()?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        p.expect(TokenKind::RParen, "')'")?;
        Ok(Stmt::CreateIndex(CreateIndexStmt {
            name,
            name_pos,
            table,
            unique,
            if_not_exists,
            columns,
        }))
    }
}

fn parse_column_def(p: &mut Parser) -> Result<ColumnDefAst, Error> {
    let (name, name_pos) = p.expect_ident()?;
    let (ty_name, ty_pos) = p.expect_ident()?;
    let (ty, is_integer_keyword) = if ty_name.eq_ignore_ascii_case("INTEGER") {
        (ColType::Integer, true)
    } else if ty_name.eq_ignore_ascii_case("INT") {
        (ColType::Integer, false)
    } else if ty_name.eq_ignore_ascii_case("TEXT") {
        (ColType::Text, false)
    } else if ty_name.eq_ignore_ascii_case("BLOB") {
        (ColType::Blob, false)
    } else {
        return Err(parse_err(
            ty_pos,
            format!("unsupported column type {ty_name}"),
        ));
    };
    let mut primary_key = false;
    let mut not_null = false;
    let mut unique = false;
    loop {
        if p.eat_kw("PRIMARY") {
            p.expect_kw("KEY")?;
            primary_key = true;
        } else if p.eat_kw("NOT") {
            p.expect_kw("NULL")?;
            not_null = true;
        } else if p.eat_kw("UNIQUE") {
            unique = true;
        } else {
            break;
        }
    }
    Ok(ColumnDefAst {
        name,
        name_pos,
        ty,
        is_integer_keyword,
        primary_key,
        not_null,
        unique,
    })
}

fn parse_drop(p: &mut Parser) -> Result<Stmt, Error> {
    if p.eat_kw("TABLE") {
        let if_exists = if p.eat_kw("IF") {
            p.expect_kw("EXISTS")?;
            true
        } else {
            false
        };
        let (name, _) = p.expect_ident()?;
        Ok(Stmt::DropTable { name, if_exists })
    } else if p.eat_kw("INDEX") {
        let if_exists = if p.eat_kw("IF") {
            p.expect_kw("EXISTS")?;
            true
        } else {
            false
        };
        let (name, _) = p.expect_ident()?;
        Ok(Stmt::DropIndex { name, if_exists })
    } else {
        Err(parse_err(p.cur().pos, "expected TABLE or INDEX after DROP"))
    }
}

fn parse_insert(p: &mut Parser) -> Result<InsertStmt, Error> {
    p.expect_kw("INTO")?;
    let (table, _) = p.expect_ident()?;
    let columns = if p.eat(TokenKind::LParen) {
        let mut cols = Vec::new();
        loop {
            cols.push(p.expect_ident()?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        p.expect(TokenKind::RParen, "')'")?;
        Some(cols)
    } else {
        None
    };
    p.expect_kw("VALUES")?;
    let mut rows = Vec::new();
    loop {
        p.expect(TokenKind::LParen, "'('")?;
        let mut vals = Vec::new();
        loop {
            vals.push(parse_expr(p)?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        p.expect(TokenKind::RParen, "')'")?;
        rows.push(vals);
        if !p.eat(TokenKind::Comma) {
            break;
        }
    }
    Ok(InsertStmt {
        table,
        columns,
        rows,
    })
}

fn parse_update(p: &mut Parser) -> Result<UpdateStmt, Error> {
    let (table, _) = p.expect_ident()?;
    p.expect_kw("SET")?;
    let mut assignments = Vec::new();
    loop {
        let (col, col_pos) = p.expect_ident()?;
        p.expect(TokenKind::Eq, "'='")?;
        let expr = parse_expr(p)?;
        assignments.push((col, col_pos, expr));
        if !p.eat(TokenKind::Comma) {
            break;
        }
    }
    let where_clause = if p.eat_kw("WHERE") {
        Some(parse_expr(p)?)
    } else {
        None
    };
    Ok(UpdateStmt {
        table,
        assignments,
        where_clause,
    })
}

fn parse_delete(p: &mut Parser) -> Result<DeleteStmt, Error> {
    p.expect_kw("FROM")?;
    let (table, _) = p.expect_ident()?;
    let where_clause = if p.eat_kw("WHERE") {
        Some(parse_expr(p)?)
    } else {
        None
    };
    Ok(DeleteStmt {
        table,
        where_clause,
    })
}
