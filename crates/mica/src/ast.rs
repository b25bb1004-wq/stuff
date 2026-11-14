//! Syntax tree. Every node carries the byte span of its source text.
use crate::span::Span;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// A name with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    /// The name as written.
    pub name: String,
    /// Where the name is.
    pub span: Span,
}

/// A whole source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Top-level items in source order.
    pub items: Vec<Item>,
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A record or variant type.
    Type(TypeDef),
    /// A constant.
    Const(ConstDef),
    /// A layer declaration.
    Layer(LayerDef),
    /// A function.
    Fn(FnDef),
    /// A unit test.
    Test(TestDef),
    /// A property test.
    Prop(PropDef),
}

/// `type Name = { .. }` or `type Name = A | B(..)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    /// The type name.
    pub name: Ident,
    /// Record fields or variants.
    pub body: TypeBody,
    /// Whole declaration.
    pub span: Span,
}

/// Body of a type declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeBody {
    /// Record fields, in order.
    Record(Vec<Field>),
    /// Variants separated by `|`.
    Variants(Vec<Variant>),
}

/// One record field `name: type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Field name.
    pub name: Ident,
    /// Field type.
    pub ty: Type,
    /// Whole field.
    pub span: Span,
}

/// One variant `Name` or `Name(types)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Variant name.
    pub name: Ident,
    /// Payload types; empty when the variant has no parentheses.
    pub fields: Vec<Type>,
    /// Whole variant.
    pub span: Span,
}

/// `const name: type = expr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDef {
    /// Constant name.
    pub name: Ident,
    /// Declared type.
    pub ty: Type,
    /// Initialiser.
    pub value: Expr,
    /// Whole declaration.
    pub span: Span,
}

/// `layer name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerDef {
    /// Layer name.
    pub name: Ident,
    /// Whole declaration.
    pub span: Span,
}

/// A function parameter `name: type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Parameter name.
    pub name: Ident,
    /// Parameter type.
    pub ty: Type,
    /// Whole parameter.
    pub span: Span,
}

/// A function declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDef {
    /// Function name.
    pub name: Ident,
    /// Parameters in order.
    pub params: Vec<Param>,
    /// Return type, when written.
    pub ret: Option<Type>,
    /// Layers listed after `uses`.
    pub uses: Vec<Ident>,
    /// Block body or extern binding.
    pub body: FnBody,
    /// Whole declaration.
    pub span: Span,
}

/// Body of a function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FnBody {
    /// An ordinary block.
    Block(Block),
    /// `= extern "path"`.
    Extern {
        /// The import name.
        path: String,
        /// Where the string is.
        span: Span,
    },
}

/// `test "name" { .. }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDef {
    /// Test name.
    pub name: String,
    /// Test body.
    pub body: Block,
    /// Whole declaration.
    pub span: Span,
}

/// `prop "name" (params) where cond { .. }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropDef {
    /// Property name.
    pub name: String,
    /// Generated parameters.
    pub params: Vec<Param>,
    /// Optional precondition.
    pub cond: Option<Expr>,
    /// Property body.
    pub body: Block,
    /// Whole declaration.
    pub span: Span,
}

/// A braced sequence of statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Statements in order.
    pub stmts: Vec<Stmt>,
    /// Including the braces.
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `let` or `var` binding.
    Let {
        /// True for `var`.
        mutable: bool,
        /// Bound name.
        name: Ident,
        /// Annotation, when written.
        ty: Option<Type>,
        /// Initial value.
        value: Expr,
        /// Whole statement.
        span: Span,
    },
    /// `return` with an optional value.
    Return {
        /// The returned value, if any.
        value: Option<Expr>,
        /// Whole statement.
        span: Span,
    },
    /// `break`.
    Break {
        /// Whole statement.
        span: Span,
    },
    /// `continue`.
    Continue {
        /// Whole statement.
        span: Span,
    },
    /// `while cond { .. }`.
    While {
        /// Loop condition.
        cond: Expr,
        /// Loop body.
        body: Block,
        /// Whole statement.
        span: Span,
    },
    /// `for x in iter { .. }`.
    For {
        /// Loop variable.
        var: Ident,
        /// Iterated expression or range.
        iter: Expr,
        /// Loop body.
        body: Block,
        /// Whole statement.
        span: Span,
    },
    /// `assert cond, msg`.
    Assert {
        /// Asserted condition.
        cond: Expr,
        /// Optional failure message.
        msg: Option<Expr>,
        /// Whole statement.
        span: Span,
    },
    /// `place = value`.
    Assign {
        /// Assigned place: a name, field, tuple field or index.
        target: Expr,
        /// New value.
        value: Expr,
        /// Whole statement.
        span: Span,
    },
    /// An expression used as a statement.
    Expr {
        /// The expression.
        expr: Expr,
        /// Whole statement.
        span: Span,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Arithmetic negation `-`.
    Neg,
    /// Logical not `!`.
    Not,
}

/// Binary operators (ranges and pipes are separate nodes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `||`.
    Or,
    /// `&&`.
    And,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `|`.
    BitOr,
    /// `^`.
    BitXor,
    /// `&`.
    BitAnd,
    /// `<<`.
    Shl,
    /// `>>`.
    Shr,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `+%`.
    AddWrap,
    /// `-%`.
    SubWrap,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Rem,
    /// `*%`.
    MulWrap,
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Integer literal.
    Int {
        /// Value (the minimum int comes from `-9223372036854775808`).
        value: i64,
        /// Source range.
        span: Span,
    },
    /// Byte literal.
    Byte {
        /// Byte value.
        value: u8,
        /// Source range.
        span: Span,
    },
    /// String literal.
    Str {
        /// Decoded text.
        value: String,
        /// Source range.
        span: Span,
    },
    /// `true` or `false`.
    Bool {
        /// The value.
        value: bool,
        /// Source range.
        span: Span,
    },
    /// `none`.
    None {
        /// Source range.
        span: Span,
    },
    /// `some(expr)`.
    Some {
        /// Wrapped value.
        value: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// A lowercase name.
    Name(Ident),
    /// A variant value, with arguments when written with parentheses.
    Variant {
        /// Variant name.
        name: Ident,
        /// Arguments; empty when there are no parentheses.
        args: Vec<Expr>,
        /// Source range.
        span: Span,
    },
    /// A record value `Name { f: e, .. }`.
    Record {
        /// Record type name.
        name: Ident,
        /// Field initialisers.
        fields: Vec<RecordField>,
        /// Source range.
        span: Span,
    },
    /// An array literal `[a, b]`.
    List {
        /// Elements.
        items: Vec<Expr>,
        /// Source range.
        span: Span,
    },
    /// A repeated array `[value; count]`.
    Repeat {
        /// Element value.
        value: Box<Expr>,
        /// Number of copies.
        count: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// A tuple of two or more values.
    Tuple {
        /// Components.
        items: Vec<Expr>,
        /// Source range.
        span: Span,
    },
    /// The unit value `()`.
    Unit {
        /// Source range.
        span: Span,
    },
    /// A prefix operator.
    Unary {
        /// Operator.
        op: UnOp,
        /// Operand.
        operand: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// An infix operator.
    Binary {
        /// Operator.
        op: BinOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// `lhs |> rhs`, kept as written (not desugared).
    Pipe {
        /// Piped value.
        lhs: Box<Expr>,
        /// Postfix expression receiving it.
        rhs: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// A range `start .. end`.
    Range {
        /// First value.
        start: Box<Expr>,
        /// Value after the last one.
        end: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// A call `callee(args)`.
    Call {
        /// Called expression.
        callee: Box<Expr>,
        /// Arguments.
        args: Vec<Expr>,
        /// Source range.
        span: Span,
    },
    /// An index `base[index]`.
    Index {
        /// Indexed expression.
        base: Box<Expr>,
        /// Index or slice range.
        index: Box<Expr>,
        /// Source range.
        span: Span,
    },
    /// A field access `base.name`.
    Field {
        /// Record expression.
        base: Box<Expr>,
        /// Field name.
        name: Ident,
        /// Source range.
        span: Span,
    },
    /// A tuple access `base.0`.
    TupleField {
        /// Tuple expression.
        base: Box<Expr>,
        /// Component index.
        index: u32,
        /// Source range.
        span: Span,
    },
    /// `if cond { .. } else ..`.
    If {
        /// Condition.
        cond: Box<Expr>,
        /// Then branch.
        then: Block,
        /// Else branch: an `Expr::Block` or a chained `Expr::If`.
        els: Option<Box<Expr>>,
        /// Source range.
        span: Span,
    },
    /// `match subject { arms }`.
    Match {
        /// Matched expression.
        subject: Box<Expr>,
        /// Arms in order.
        arms: Vec<Arm>,
        /// Source range.
        span: Span,
    },
    /// A block used as an expression.
    Block(Block),
}

/// One `field: value` in a record value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordField {
    /// Field name.
    pub name: Ident,
    /// Field value.
    pub value: Expr,
    /// Whole field.
    pub span: Span,
}

/// One `pattern => body` arm of a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    /// Pattern tested.
    pub pattern: Pattern,
    /// Value when the pattern matches.
    pub body: Expr,
    /// Whole arm.
    pub span: Span,
}

/// A pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `_`.
    Wild {
        /// Source range.
        span: Span,
    },
    /// A binding name.
    Bind(Ident),
    /// Integer literal.
    Int {
        /// Value.
        value: i64,
        /// Source range.
        span: Span,
    },
    /// Byte literal.
    Byte {
        /// Value.
        value: u8,
        /// Source range.
        span: Span,
    },
    /// String literal.
    Str {
        /// Value.
        value: String,
        /// Source range.
        span: Span,
    },
    /// `true` or `false`.
    Bool {
        /// Value.
        value: bool,
        /// Source range.
        span: Span,
    },
    /// `none`.
    None {
        /// Source range.
        span: Span,
    },
    /// `some(pattern)`.
    Some {
        /// Inner pattern.
        inner: Box<Pattern>,
        /// Source range.
        span: Span,
    },
    /// A variant pattern, with arguments when written with parentheses.
    Variant {
        /// Variant name.
        name: Ident,
        /// Argument patterns; empty without parentheses.
        args: Vec<Pattern>,
        /// Source range.
        span: Span,
    },
    /// A tuple pattern of two or more.
    Tuple {
        /// Component patterns.
        items: Vec<Pattern>,
        /// Source range.
        span: Span,
    },
}

/// A type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// `int`.
    Int {
        /// Source range.
        span: Span,
    },
    /// `bool`.
    Bool {
        /// Source range.
        span: Span,
    },
    /// `byte`.
    Byte {
        /// Source range.
        span: Span,
    },
    /// `str`.
    Str {
        /// Source range.
        span: Span,
    },
    /// `unit`.
    Unit {
        /// Source range.
        span: Span,
    },
    /// A named record or variant type.
    Named(Ident),
    /// `[T]`.
    List {
        /// Element type.
        elem: Box<Type>,
        /// Source range.
        span: Span,
    },
    /// `(A, B, ..)`.
    Tuple {
        /// Component types.
        items: Vec<Type>,
        /// Source range.
        span: Span,
    },
    /// `T?`.
    Option {
        /// Wrapped type.
        inner: Box<Type>,
        /// Source range.
        span: Span,
    },
    /// `fn(A) -> B uses l`.
    Fn {
        /// Parameter types.
        params: Vec<Type>,
        /// Return type.
        ret: Box<Type>,
        /// Layers after `uses`.
        uses: Vec<Ident>,
        /// Source range.
        span: Span,
    },
}
