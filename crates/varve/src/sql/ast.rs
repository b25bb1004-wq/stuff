//! Abstract syntax tree for the Varve SQL subset.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stmt {
    CreateTable(CreateTableStmt),
    DropTable { name: String, if_exists: bool },
    CreateIndex(CreateIndexStmt),
    DropIndex { name: String, if_exists: bool },
    Insert(InsertStmt),
    Update(UpdateStmt),
    Delete(DeleteStmt),
    Begin,
    Commit,
    Rollback,
    Select(SelectStmt),
    Explain(SelectStmt),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColType {
    Integer,
    Text,
    Blob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnDefAst {
    pub name: String,
    pub name_pos: usize,
    pub ty: ColType,
    pub is_integer_keyword: bool,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateTableStmt {
    pub name: String,
    pub name_pos: usize,
    pub if_not_exists: bool,
    pub columns: Vec<ColumnDefAst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateIndexStmt {
    pub name: String,
    pub name_pos: usize,
    pub table: String,
    pub unique: bool,
    pub if_not_exists: bool,
    pub columns: Vec<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InsertStmt {
    pub table: String,
    pub columns: Option<Vec<(String, usize)>>,
    pub rows: Vec<Vec<Expr>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateStmt {
    pub table: String,
    pub assignments: Vec<(String, usize, Expr)>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteStmt {
    pub table: String,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStmt {
    pub distinct: bool,
    pub items: Vec<SelectItem>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub order_by: Vec<OrderByItem>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectItem {
    Star { table: Option<String>, pos: usize },
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FromClause {
    pub table: TableRef,
    pub joins: Vec<JoinClause>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableRef {
    pub name: String,
    pub alias: Option<String>,
    pub pos: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinKind {
    Inner,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinClause {
    pub kind: JoinKind,
    pub table: TableRef,
    pub on: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderByItem {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExprKind {
    Null,
    IntLit(String),
    Text(String),
    Blob(Vec<u8>),
    Column {
        table: Option<String>,
        name: String,
        pos: usize,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        negated: bool,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        negated: bool,
    },
    Case {
        operand: Option<Box<Expr>>,
        whens: Vec<(Expr, Expr)>,
        else_expr: Option<Box<Expr>>,
    },
    Call {
        name: String,
        args: CallArgs,
        pos: usize,
    },
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallArgs {
    Star,
    List(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}
