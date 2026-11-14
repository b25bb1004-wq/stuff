//! Runtime values, heap objects and traps. The data model only; the interpreter is a later task.

use alloc::{string::String, vec::Vec};

/// A runtime value. Copy-cheap: heap objects are reached through `Ref`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// The unit value.
    Unit,
    /// A 64-bit signed integer.
    Int(i64),
    /// An unsigned byte.
    Byte(u8),
    /// A boolean.
    Bool(bool),
    /// A function, by index into `Module::funcs`.
    Fn(u32),
    /// A reference to a heap object.
    Ref(Handle),
}

/// Index into the heap's slot table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle(pub u32);

/// A heap object.
#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    /// A `str`.
    Str(String),
    /// A `[byte]`.
    Bytes(Vec<u8>),
    /// Every other `[T]`.
    Array(Vec<Value>),
    /// A record or tuple.
    Record(Vec<Value>),
    /// A variant: `none` is tag 0, `some(v)` is tag 1.
    Variant {
        /// The variant tag.
        tag: u32,
        /// The payload fields.
        fields: Vec<Value>,
    },
}

/// Why a run stopped without returning a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Trap {
    /// Integer overflow, or a shift count outside 0..=63.
    Overflow,
    /// Division or remainder by zero.
    DivideByZero,
    /// An index past the end of an array, byte array or slice.
    IndexOutOfRange,
    /// An `assert` failed, with its message.
    AssertFailed(String),
    /// An operand had the wrong kind.
    TypeError,
    /// The fuel limit was reached.
    OutOfFuel,
    /// The heap limit was reached after a collection.
    OutOfMemory,
    /// The call depth limit was reached.
    StackOverflow,
    /// A host function failed, with its message.
    Host(String),
    /// An explicit `trap` with a user code.
    User(u32),
}
