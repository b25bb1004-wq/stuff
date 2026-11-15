//! SQL engine over the Iridium B+tree and transactional pager.

mod ast;
mod catalog;
mod codec;
mod db;
mod dml;
mod expr;
mod functions;
mod lexer;
mod parser;
mod parser_expr;
mod parser_select;
mod planner;
mod probe;
mod select;
mod select_group;
mod validate;

pub use codec::{decode_record, encode_key, encode_record};
pub use db::Db;

/// SQL runtime value matching SQLite STRICT storage classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// SQL NULL value.
    Null,
    /// 64-bit signed integer.
    Int(i64),
    /// UTF-8 text string.
    Text(String),
    /// Raw byte sequence.
    Blob(Vec<u8>),
}

/// Result of executing a single SQL statement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rows {
    /// Result column names (empty for non-query statements).
    pub columns: Vec<String>,
    /// Result rows, each having `columns.len()` values.
    pub rows: Vec<Vec<Value>>,
    /// Number of rows inserted, updated, or deleted.
    pub changes: u64,
}
