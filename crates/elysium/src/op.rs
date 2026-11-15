//! The instruction set, in the order of the table in docs/MBC.md.
//! Register operands are `u16`; constant, function, import, tag and jump-target operands are `u32`;
//! counts `n` are `u16`. Field indexes are `u16`.

/// One instruction. `d` is the destination register, `a`, `b`, `c` are source registers.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Load a signed integer immediate into `d`.
    Int(u16, i64),
    /// Load a byte immediate into `d`.
    Byte(u16, u8),
    /// Load a boolean immediate into `d`.
    Bool(u16, bool),
    /// Load unit into `d`.
    Unit(u16),
    /// Load a function reference (function index) into `d`.
    FnRef(u16, u32),
    /// Allocate a fresh `Str` or `Bytes` object copied from constant `k`.
    Const(u16, u32),
    /// Copy register `a` into `d`.
    Move(u16, u16),
    /// Checked addition on `Int` (or `Byte`, trapping outside 0..=255).
    Add(u16, u16, u16),
    /// Checked subtraction, as `Add`.
    Sub(u16, u16, u16),
    /// Checked multiplication, as `Add`.
    Mul(u16, u16, u16),
    /// Wrapping addition on `Int`.
    AddW(u16, u16, u16),
    /// Wrapping subtraction on `Int`.
    SubW(u16, u16, u16),
    /// Wrapping multiplication on `Int`.
    MulW(u16, u16, u16),
    /// Truncating division; traps on zero and on `MIN / -1`.
    Div(u16, u16, u16),
    /// Truncating remainder; traps on zero.
    Rem(u16, u16, u16),
    /// Negation on `Int`; traps on `i64::MIN`.
    Neg(u16, u16),
    /// Bitwise and on `Int`, logical and on `Bool`.
    And(u16, u16, u16),
    /// Bitwise or on `Int`, logical or on `Bool`.
    Or(u16, u16, u16),
    /// Bitwise xor on `Int`, logical xor on `Bool`.
    Xor(u16, u16, u16),
    /// Shift left on `Int`; count outside 0..=63 traps.
    Shl(u16, u16, u16),
    /// Arithmetic shift right on `Int`; count outside 0..=63 traps.
    Shr(u16, u16, u16),
    /// Logical not on `Bool`.
    Not(u16, u16),
    /// Equality on same-kind scalars, or on strings or byte objects by content.
    Eq(u16, u16, u16),
    /// Inequality, as `Eq`.
    Ne(u16, u16, u16),
    /// Less than on `Int`, `Byte` or `Str`.
    Lt(u16, u16, u16),
    /// Less or equal, as `Lt`.
    Le(u16, u16, u16),
    /// Greater than, as `Lt`.
    Gt(u16, u16, u16),
    /// Greater or equal, as `Lt`.
    Ge(u16, u16, u16),
    /// Jump to instruction `t`.
    Jump(u32),
    /// Jump to instruction `t` when `a` is true.
    JumpIf(u16, u32),
    /// Jump to instruction `t` when `a` is false.
    JumpIfNot(u16, u32),
    /// Call function `f` with registers `first..first+n`; result in `d`.
    Call(u16, u32, u16, u16),
    /// Call the `Fn` held in register `a` with registers `first..first+n`; result in `d`.
    CallValue(u16, u16, u16, u16),
    /// Call import `h` with registers `first..first+n`; result in `d`.
    CallHost(u16, u32, u16, u16),
    /// Return the value in `a`.
    Ret(u16),
    /// Build an array from registers `first..first+n`.
    NewArray(u16, u16, u16),
    /// Build a byte array from `n` `Byte` registers `first..first+n`.
    NewBytes(u16, u16, u16),
    /// Build an array of `b` copies of `a`; a `Byte` value builds `Bytes`.
    NewFill(u16, u16, u16),
    /// Build a record from registers `first..first+n`.
    NewRecord(u16, u16, u16),
    /// Build a variant with tag `tag` from registers `first..first+n`.
    NewVariant(u16, u32, u16, u16),
    /// Read field `i` of the record or variant in `a`.
    GetField(u16, u16, u16),
    /// Write register `b` into field `i` of the record or variant in `a`.
    SetField(u16, u16, u16),
    /// Read the variant tag of `a` as `Int`.
    Tag(u16, u16),
    /// Read element `b` of the array or byte array `a`.
    GetIndex(u16, u16, u16),
    /// Write `c` into element `b` of the array or byte array `a`.
    SetIndex(u16, u16, u16),
    /// Length of a string (bytes), byte array or array.
    Len(u16, u16),
    /// Append `b` to the array `a`.
    Push(u16, u16),
    /// Remove the last element of the array `a`; gives `none` or `some`.
    Pop(u16, u16),
    /// Copy of the range `lo..hi` of a string, byte array or array.
    Slice(u16, u16, u16, u16),
    /// Concatenate two strings, two byte arrays or two arrays.
    Concat(u16, u16, u16),
    /// Decimal text of an `Int`.
    IntToStr(u16, u16),
    /// Parse an `Int` from a string; gives `none` or `some`.
    ParseInt(u16, u16),
    /// UTF-8 bytes of a string.
    StrToBytes(u16, u16),
    /// String from UTF-8 bytes; gives `none` when not UTF-8.
    BytesToStr(u16, u16),
    /// Widen a `Byte` to `Int`.
    ToInt(u16, u16),
    /// Narrow an `Int` to `Byte`; traps outside 0..=255.
    ToByte(u16, u16),
    /// Trap `AssertFailed` with constant string `k` when `a` is false.
    Assert(u16, u32),
    /// Stop with `Trap::User(code)`.
    Trap(u32),
}
