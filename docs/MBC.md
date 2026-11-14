# MBC: Mica bytecode and the Strata VM (version 1)

The VM is a register machine. It runs untrusted code (smart contracts, downloaded packages), so
three rules shape everything: a module is **verified before it runs**, every operation is
**checked at run time** and fails with a `Trap` instead of a panic, and every run has a **fuel**
limit so it always ends.

## Values
```rust
pub enum Value { Unit, Int(i64), Byte(u8), Bool(bool), Fn(u32), Ref(Handle) }   // Copy
pub struct Handle(u32);                                                          // index into the heap
pub enum Obj {
    Str(String), Bytes(Vec<u8>),          // `str`, `[byte]`
    Array(Vec<Value>),                    // every other `[T]`
    Record(Vec<Value>),                   // records and tuples
    Variant { tag: u32, fields: Vec<Value> },   // variants; `none` = tag 0, `some(v)` = tag 1
}
```

## Module
```rust
pub struct Module { pub consts: Vec<Const>, pub imports: Vec<String>, pub funcs: Vec<Func>, pub entry: u32 }
pub enum Const { Str(String), Bytes(Vec<u8>) }
pub struct Func { pub name: String, pub params: u16, pub regs: u16, pub code: Vec<Op> }
```
`regs` is the frame size; parameters arrive in registers `0..params`; `params <= regs <= 4096`.
`imports` are host function names such as `io.print`. The embedder decides which names it
provides; a module that imports a name the host does not provide is refused at load. This is how
Mica layers are enforced at run time.

## Instructions (`Op`)
`d a b` are register numbers (`u16`), `k` a constant index, `f` a function index, `h` an import
index, `t` a jump target (instruction index inside the same function, `u32`).

| Op | Effect |
|---|---|
| `Int(d, i64)`, `Byte(d, u8)`, `Bool(d, bool)`, `Unit(d)`, `FnRef(d, f)` | load an immediate |
| `Const(d, k)` | allocate a fresh `Str` or `Bytes` object copied from constant `k` |
| `Move(d, a)` | copy |
| `Add Sub Mul (d, a, b)` | checked on `Int` (trap `Overflow`); on `Byte` trap outside 0..=255 |
| `AddW SubW MulW (d, a, b)` | wrapping, `Int` only |
| `Div Rem (d, a, b)` | truncating; trap `DivideByZero`; `MIN / -1` traps `Overflow` |
| `Neg(d, a)` | trap `Overflow` on `i64::MIN` |
| `And Or Xor (d, a, b)` | bitwise on two `Int`, logical on two `Bool` |
| `Shl Shr (d, a, b)` | `Int`; count outside 0..=63 traps `Overflow`; `Shr` is arithmetic |
| `Not(d, a)` | `Bool` |
| `Eq Ne (d, a, b)` | same-kind `Int Byte Bool Unit`, or two `Str`/`Bytes` objects by content |
| `Lt Le Gt Ge (d, a, b)` | two `Int`, two `Byte`, or two `Str` (byte order) |
| `Jump(t)`, `JumpIf(a, t)`, `JumpIfNot(a, t)` | `a` must be `Bool` |
| `Call(d, f, first, n)` | call function `f` with registers `first..first+n`; result in `d` |
| `CallValue(d, a, first, n)` | same, the function is the `Fn` in `a` |
| `CallHost(d, h, first, n)` | call import `h` |
| `Ret(a)` | return the value in `a` |
| `NewArray(d, first, n)` | array of registers `first..first+n`; `Bytes` if n > 0 and all are `Byte` is NOT automatic: see `NewBytes` |
| `NewBytes(d, first, n)` | `Bytes` from `n` `Byte` registers |
| `NewFill(d, a, b)` | array of `b` (Int, 0..=max) copies of `a`; a `Byte` value makes `Bytes` |
| `NewRecord(d, first, n)`, `NewVariant(d, tag, first, n)` | build an object |
| `GetField(d, a, i)`, `SetField(a, i, b)` | field `i` of a `Record` or `Variant` |
| `Tag(d, a)` | variant tag as `Int` |
| `GetIndex(d, a, b)`, `SetIndex(a, b, c)` | element of `Array` or `Bytes` (trap `IndexOutOfRange`) |
| `Len(d, a)` | length of `Str` (bytes), `Bytes`, `Array` |
| `Push(a, b)`, `Pop(d, a)` | grow or shrink; `Pop` gives a `none`/`some` variant |
| `Slice(d, a, lo, hi)` | copy of `lo..hi` from `Str` (must cut on char boundaries, else trap), `Bytes`, `Array` |
| `Concat(d, a, b)` | new object from two `Str`, two `Bytes` or two `Array` |
| `IntToStr(d, a)`, `ParseInt(d, a)` | `ParseInt` gives a `none`/`some` variant |
| `StrToBytes(d, a)`, `BytesToStr(d, a)` | the second gives `none` when not UTF-8 |
| `ToInt(d, a)`, `ToByte(d, a)` | `Byte` to `Int`; `Int` to `Byte` (trap `Overflow`) |
| `Assert(a, k)` | trap `AssertFailed` with constant string `k` when `a` is false |
| `Trap(code)` | stop with `Trap::User(code)` |

## Traps
```rust
pub enum Trap { Overflow, DivideByZero, IndexOutOfRange, AssertFailed(String), TypeError,
                OutOfFuel, OutOfMemory, StackOverflow, Host(String), User(u32) }
```
A wrong operand kind is `TypeError`. A call depth above the limit is `StackOverflow`.

## Verifier (`verify(&Module) -> Result<(), VerifyError>`); refuse unless all hold
- `entry` and every `f`, `k`, `h` index is in range; `Call` argument count equals the callee's
  `params`; `Assert` constants are `Str`.
- Every register operand is below the function's `regs`; `first + n <= regs`; `params <= regs`.
- Every jump target is below the code length; code is not empty and its last instruction is
  `Ret`, `Jump` or `Trap`.
- Limits: at most 65 536 functions, constants and imports each; at most 1 048 576 instructions
  per function; names at most 255 bytes.

## Running
```rust
pub trait Host { fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap>; }
pub struct Limits { pub fuel: u64, pub max_heap_bytes: usize, pub max_depth: u32 }
pub struct Vm;   // Vm::new(module, host_names) -> Result<Vm, LoadError>   (verifies, binds imports)
                 // vm.run(&mut host, limits) -> Result<Value, Trap>         (calls `entry` with no arguments)
                 // vm.call(func_name, &[Value], &mut host, limits) -> Result<Value, Trap>
                 // vm.heap() / vm.fuel_used()
```
- **Fuel:** every instruction costs 1; instructions that allocate or copy cost 1 more per 8
  bytes or per element touched. When fuel reaches zero the run stops with `OutOfFuel`. Fuel use
  must be identical on every machine: it is part of blockchain consensus.
- **Heap and garbage collector:** a mark and sweep collector over a slot table with a free list.
  Roots are every register of every live frame. It runs when the bytes allocated since the last
  collection pass a threshold, and before reporting `OutOfMemory`. Handles of freed slots are
  reused; a collection never moves objects. `Heap` exposes `alloc`, `get`, `get_mut`,
  `live_objects()`, `live_bytes()` for hosts and tests.
- The interpreter loop is iterative (its own frame stack), never recursive, so deep Mica
  recursion cannot overflow the Rust stack.

## Binary format (`encode(&Module) -> Vec<u8>`, `decode(&[u8]) -> Result<Module, DecodeError>`)
Magic `MBC1`, then unsigned LEB128 for every count, index and register, signed LEB128 (zig-zag)
for `Int` immediates, a one-byte opcode per instruction in the order of the table above, and
length-prefixed UTF-8 for strings. `decode(encode(m)) == m` for every module, and `decode` of any
byte string returns `Ok` or `Err`, never panics and never allocates more than the input length
times a small constant.
