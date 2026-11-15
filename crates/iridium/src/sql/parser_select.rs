//! SELECT statement parser for Iridium SQL.

use crate::sql::ast::{
    FromClause, JoinClause, JoinKind, OrderByItem, SelectItem, SelectStmt, TableRef,
};
use crate::sql::lexer::TokenKind;
use crate::sql::parser::Parser;
use crate::sql::parser_expr::parse_expr;
use crate::Error;

pub(crate) fn parse_select(p: &mut Parser) -> Result<SelectStmt, Error> {
    let distinct = p.eat_kw("DISTINCT");
    let mut items = Vec::new();
    loop {
        items.push(parse_select_item(p)?);
        if !p.eat(TokenKind::Comma) {
            break;
        }
    }

    let from = if p.eat_kw("FROM") {
        let table = parse_table_ref(p)?;
        let mut joins = Vec::new();
        loop {
            let kind = if p.eat_kw("INNER") {
                p.expect_kw("JOIN")?;
                JoinKind::Inner
            } else if p.eat_kw("LEFT") {
                let _ = p.eat_kw("OUTER");
                p.expect_kw("JOIN")?;
                JoinKind::Left
            } else if p.eat_kw("JOIN") {
                JoinKind::Inner
            } else {
                break;
            };
            let right = parse_table_ref(p)?;
            p.expect_kw("ON")?;
            let on = parse_expr(p)?;
            joins.push(JoinClause {
                kind,
                table: right,
                on,
            });
        }
        Some(FromClause { table, joins })
    } else {
        None
    };

    let where_clause = if p.eat_kw("WHERE") {
        Some(parse_expr(p)?)
    } else {
        None
    };

    let group_by = if p.eat_kw("GROUP") {
        p.expect_kw("BY")?;
        let mut exprs = Vec::new();
        loop {
            exprs.push(parse_expr(p)?);
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        exprs
    } else {
        Vec::new()
    };

    let having = if p.eat_kw("HAVING") {
        Some(parse_expr(p)?)
    } else {
        None
    };

    let order_by = if p.eat_kw("ORDER") {
        p.expect_kw("BY")?;
        let mut items = Vec::new();
        loop {
            let expr = parse_expr(p)?;
            let desc = if p.eat_kw("DESC") {
                true
            } else {
                let _ = p.eat_kw("ASC");
                false
            };
            items.push(OrderByItem { expr, desc });
            if !p.eat(TokenKind::Comma) {
                break;
            }
        }
        items
    } else {
        Vec::new()
    };

    let limit = if p.eat_kw("LIMIT") {
        Some(parse_expr(p)?)
    } else {
        None
    };

    let offset = if limit.is_some() && p.eat_kw("OFFSET") {
        Some(parse_expr(p)?)
    } else {
        None
    };

    Ok(SelectStmt {
        distinct,
        items,
        from,
        where_clause,
        group_by,
        having,
        order_by,
        limit,
        offset,
    })
}

fn parse_select_item(p: &mut Parser) -> Result<SelectItem, Error> {
    if p.cur().kind == TokenKind::Star {
        let pos = p.advance().pos;
        return Ok(SelectItem::Star { table: None, pos });
    }
    if matches!(p.cur().kind, TokenKind::Ident(_))
        && p.peek(1).kind == TokenKind::Dot
        && p.peek(2).kind == TokenKind::Star
    {
        let (table, pos) = p.expect_ident()?;
        p.advance();
        p.advance();
        return Ok(SelectItem::Star {
            table: Some(table),
            pos,
        });
    }
    let expr = parse_expr(p)?;
    let alias = if p.eat_kw("AS") || is_select_alias_candidate(p) {
        Some(p.expect_ident()?.0)
    } else {
        None
    };
    Ok(SelectItem::Expr { expr, alias })
}

fn parse_table_ref(p: &mut Parser) -> Result<TableRef, Error> {
    let (name, pos) = p.expect_ident()?;
    let alias = if p.eat_kw("AS") || is_table_alias_candidate(p) {
        Some(p.expect_ident()?.0)
    } else {
        None
    };
    Ok(TableRef { name, alias, pos })
}

fn is_select_alias_candidate(p: &Parser) -> bool {
    let TokenKind::Ident(s) = &p.cur().kind else {
        return false;
    };
    const STOP: &[&str] = &[
        "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "NULL",
    ];
    !STOP.iter().any(|kw| s.eq_ignore_ascii_case(kw))
}

fn is_table_alias_candidate(p: &Parser) -> bool {
    let TokenKind::Ident(s) = &p.cur().kind else {
        return false;
    };
    const STOP: &[&str] = &[
        "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "INNER", "LEFT", "OUTER", "JOIN",
        "ON", "NULL",
    ];
    !STOP.iter().any(|kw| s.eq_ignore_ascii_case(kw))
}
