//! Typed intermediate representation for Logos programs.
use crate::ast;
use crate::span::Span;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Virtual register index within a function frame.
pub type Reg = u32;
/// Basic-block label index within a function body.
pub type Label = u32;
/// Function index in [`Program::funcs`].
pub type FuncId = u32;
/// Nominal type index in [`Program::types`].
pub type TypeId = u32;

/// Resolved value type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// 64-bit signed integer (`int`).
    Int,
    /// 8-bit unsigned integer (`byte`).
    Byte,
    /// Boolean (`bool`).
    Bool,
    /// Immutable UTF-8 string (`str`).
    Str,
    /// Unit type (`unit`).
    Unit,
    /// Growable array (`[T]`).
    Array(Box<Ty>),
    /// Tuple of two or more elements (`(A, B, ..)`).
    Tuple(Vec<Ty>),
    /// Optional value (`T?`).
    Opt(Box<Ty>),
    /// User-defined record or variant type.
    Named(TypeId),
    /// First-class function signature (`fn(A) -> B uses l`).
    Fn {
        /// Parameter types in order.
        params: Vec<Ty>,
        /// Return type.
        ret: Box<Ty>,
        /// Bitmask of used layers.
        layers: u64,
    },
}

/// Layout of a user-defined type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    /// Record fields `(name, ty)` in declaration order.
    Record(Vec<(String, Ty)>),
    /// Sum-type variants `(name, payload_tys)` in declaration order.
    Variants(Vec<(String, Vec<Ty>)>),
}

/// Metadata for a user-defined record or variant type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    /// Declared type name.
    pub name: String,
    /// Record fields or variant cases.
    pub kind: TypeKind,
}

/// Immediate literal constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lit {
    /// 64-bit signed integer literal.
    Int(i64),
    /// 8-bit unsigned byte literal.
    Byte(u8),
    /// Boolean literal.
    Bool(bool),
    /// Unit literal `()`.
    Unit,
    /// String literal.
    Str(String),
    /// Reference to a top-level function.
    Fn(FuncId),
}

/// Built-in operation with no layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// `len(x)` on `str` or `[T]`.
    Len,
    /// `push(xs, v)` on `[T]`.
    Push,
    /// `pop(xs) -> T?` on `[T]`.
    Pop,
    /// `xs[a..b]` slice copy on `[T]` or `str`.
    Slice,
    /// `a + b` concatenation on `str` or `[T]`.
    Concat,
    /// `str(int) -> str`.
    IntToStr,
    /// `parse_int(str) -> int?`.
    ParseInt,
    /// `bytes(str) -> [byte]`.
    Bytes,
    /// `text([byte]) -> str?`.
    Text,
    /// `int(byte) -> int`.
    ToInt,
    /// `byte(int) -> byte`.
    ToByte,
    /// `min(a, b)` on `int`.
    Min,
    /// `max(a, b)` on `int`.
    Max,
    /// `abs(a)` on `int`.
    Abs,
}

/// Three-address register instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    /// Load literal into destination register.
    Lit(Reg, Lit),
    /// Copy source register into destination register.
    Move(Reg, Reg),
    /// Unary operation `dst = op src`.
    Un(Reg, ast::UnOp, Reg),
    /// Binary operation `dst = op(lhs, rhs)` (never `And` or `Or`).
    Bin(Reg, ast::BinOp, Reg, Reg),
    /// Direct call `dst = func(args)`.
    Call(Reg, FuncId, Vec<Reg>),
    /// Indirect call through function register `dst = callee(args)`.
    CallValue(Reg, Reg, Vec<Reg>),
    /// Built-in call `dst = builtin(args)`.
    Builtin(Reg, Builtin, Vec<Reg>),
    /// Construct array from element registers.
    NewArray(Reg, Vec<Reg>),
    /// Construct repeated array `[value; count]`: `NewFill(dst, value, count)`.
    NewFill(Reg, Reg, Reg),
    /// Construct tuple from element registers.
    NewTuple(Reg, Vec<Reg>),
    /// Construct record of `TypeId` from field registers in declaration order.
    NewRecord(Reg, TypeId, Vec<Reg>),
    /// Construct variant `tag` of `TypeId` from payload registers.
    NewVariant(Reg, TypeId, u32, Vec<Reg>),
    /// Construct `some(value)`.
    NewSome(Reg, Reg),
    /// Construct `none`.
    NewNone(Reg),
    /// Read field `idx` of a record, tuple, variant payload, or `some`: `GetField(dst, base, idx)`.
    GetField(Reg, Reg, u32),
    /// Write field `idx` of a record: `SetField(base, idx, value)`.
    SetField(Reg, u32, Reg),
    /// Read variant or option tag as `int`: `Tag(dst, src)`.
    Tag(Reg, Reg),
    /// Read array element `dst = base[index]`.
    GetIndex(Reg, Reg, Reg),
    /// Write array element `base[index] = value`.
    SetIndex(Reg, Reg, Reg),
    /// Define a branch target label.
    Label(Label),
    /// Unconditional jump to label.
    Jump(Label),
    /// Conditional branch `Branch(cond, if_true, if_false)`.
    Branch(Reg, Label, Label),
    /// Return register from function.
    Ret(Reg),
    /// Trap with message if condition register is false.
    Assert(Reg, String),
}

/// Function implementation: compiled IR instructions or host import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Compiled instruction stream with parallel source spans.
    Code {
        /// Instructions in order.
        insts: Vec<Inst>,
        /// Source span of each instruction (`spans[i]` is the source of `insts[i]`).
        spans: Vec<Span>,
    },
    /// Host import name.
    Extern(String),
}

/// A named `let` or `var` binding and the instruction range where it is in scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Name as written in the source.
    pub name: String,
    /// Register that holds the binding.
    pub reg: Reg,
    /// Index of the first instruction after the binding is defined (inclusive).
    pub from: u32,
    /// Index of the first instruction after the scope ends (exclusive).
    pub to: u32,
}

/// Checked and lowered function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Func {
    /// Function name.
    pub name: String,
    /// Number of parameters (`regs[0..params]` are the parameters).
    pub params: u32,
    /// Return type.
    pub ret: Ty,
    /// Bitmask of layers used by this function.
    pub layers: u64,
    /// Types of all registers in this function frame.
    pub regs: Vec<Ty>,
    /// `let` and `var` bindings with their scopes; used by debugging, not by execution.
    pub bindings: Vec<Binding>,
    /// Code body or extern import name.
    pub body: Body,
}

/// Checked property test entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    /// Property description.
    pub name: String,
    /// Property body function.
    pub func: FuncId,
    /// Optional `where` guard function taking the same parameters and returning `bool`.
    pub guard: Option<FuncId>,
}

/// Whole checked and lowered Logos program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Declared nominal types indexed by [`TypeId`].
    pub types: Vec<TypeInfo>,
    /// Layer names indexed by bit position (`0..=6` are built-in, followed by declared layers).
    pub layers: Vec<String>,
    /// All functions (user functions, tests, property bodies and guards) indexed by [`FuncId`].
    pub funcs: Vec<Func>,
    /// Unit tests `(name, func_id)`.
    pub tests: Vec<(String, FuncId)>,
    /// Property tests.
    pub props: Vec<Prop>,
    /// Entry-point `main` function, if present.
    pub main: Option<FuncId>,
}
