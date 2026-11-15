//! The verifier: every rule of the "Verifier" section of docs/MBC.md.

use crate::module::{Const, Func, Module, MAX_CODE, MAX_NAME, MAX_REGS, MAX_TABLE};
use crate::op::Op;

/// Why a module is refused. Instruction-level errors name the function and instruction index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// More than 65 536 functions.
    TooManyFuncs,
    /// More than 65 536 constants.
    TooManyConsts,
    /// More than 65 536 imports.
    TooManyImports,
    /// A function name longer than 255 bytes.
    FuncNameTooLong {
        /// The function index.
        func: u32,
    },
    /// An import name longer than 255 bytes.
    ImportNameTooLong {
        /// The import index.
        import: u32,
    },
    /// `entry` is not a function index.
    EntryOutOfRange {
        /// The entry index.
        entry: u32,
    },
    /// `regs` is above 4096.
    FrameTooLarge {
        /// The function index.
        func: u32,
    },
    /// `params` is above `regs`.
    ParamsExceedRegs {
        /// The function index.
        func: u32,
    },
    /// More than 1 048 576 instructions.
    TooManyInstructions {
        /// The function index.
        func: u32,
    },
    /// The function has no instructions.
    EmptyCode {
        /// The function index.
        func: u32,
    },
    /// The last instruction is not `Ret`, `Jump` or `Trap`.
    BadLastInstruction {
        /// The function index.
        func: u32,
    },
    /// A register operand is not below `regs`.
    RegisterOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending register.
        reg: u16,
    },
    /// A register span `first..first+n` goes past `regs`.
    RegisterRangeOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
    },
    /// A constant index is not in the pool.
    ConstOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending index.
        index: u32,
    },
    /// A function index is not in the module.
    FuncOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending index.
        index: u32,
    },
    /// An import index is not in the module.
    ImportOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending index.
        index: u32,
    },
    /// A jump target is not below the code length.
    JumpOutOfRange {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending target.
        target: u32,
    },
    /// A `Call` passes a number of arguments other than the callee's `params`.
    ArgCountMismatch {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The callee index.
        callee: u32,
        /// The callee's `params`.
        expected: u16,
        /// The argument count given.
        got: u16,
    },
    /// An `Assert` message constant is not a `Str`.
    AssertNotString {
        /// The function index.
        func: u32,
        /// The instruction index.
        at: u32,
        /// The offending constant index.
        index: u32,
    },
}

/// Check every verifier rule. A module that passes is safe to run.
pub fn verify(module: &Module) -> Result<(), VerifyError> {
    if module.funcs.len() > MAX_TABLE {
        return Err(VerifyError::TooManyFuncs);
    }
    if module.consts.len() > MAX_TABLE {
        return Err(VerifyError::TooManyConsts);
    }
    if module.imports.len() > MAX_TABLE {
        return Err(VerifyError::TooManyImports);
    }
    for (i, name) in module.imports.iter().enumerate() {
        if name.len() > MAX_NAME {
            return Err(VerifyError::ImportNameTooLong { import: idx(i) });
        }
    }
    for (f, func) in module.funcs.iter().enumerate() {
        if func.name.len() > MAX_NAME {
            return Err(VerifyError::FuncNameTooLong { func: idx(f) });
        }
    }
    if module.funcs.get(module.entry as usize).is_none() {
        return Err(VerifyError::EntryOutOfRange {
            entry: module.entry,
        });
    }
    for (f, func) in module.funcs.iter().enumerate() {
        verify_func(module, idx(f), func)?;
    }
    Ok(())
}

/// Convert a table position to an index. Positions come from a table capped at 65 536, so this never truncates.
fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn verify_func(m: &Module, f: u32, func: &Func) -> Result<(), VerifyError> {
    if func.regs > MAX_REGS {
        return Err(VerifyError::FrameTooLarge { func: f });
    }
    if func.params > func.regs {
        return Err(VerifyError::ParamsExceedRegs { func: f });
    }
    if func.code.len() > MAX_CODE {
        return Err(VerifyError::TooManyInstructions { func: f });
    }
    let last = func.code.last().ok_or(VerifyError::EmptyCode { func: f })?;
    if !matches!(last, Op::Ret(_) | Op::Jump(_) | Op::Trap(_)) {
        return Err(VerifyError::BadLastInstruction { func: f });
    }
    let cx = Cx {
        m,
        f,
        regs: func.regs,
        len: func.code.len(),
    };
    for (at, op) in func.code.iter().enumerate() {
        cx.check(idx(at), op)?;
    }
    Ok(())
}

/// Context for checking one function's instructions.
struct Cx<'a> {
    m: &'a Module,
    f: u32,
    regs: u16,
    len: usize,
}

impl Cx<'_> {
    fn reg(&self, at: u32, r: u16) -> Result<(), VerifyError> {
        if r < self.regs {
            Ok(())
        } else {
            Err(VerifyError::RegisterOutOfRange {
                func: self.f,
                at,
                reg: r,
            })
        }
    }

    fn span(&self, at: u32, first: u16, n: u16) -> Result<(), VerifyError> {
        if u32::from(first) + u32::from(n) <= u32::from(self.regs) {
            Ok(())
        } else {
            Err(VerifyError::RegisterRangeOutOfRange { func: self.f, at })
        }
    }

    fn konst(&self, at: u32, k: u32) -> Result<&Const, VerifyError> {
        usize::try_from(k)
            .ok()
            .and_then(|i| self.m.consts.get(i))
            .ok_or(VerifyError::ConstOutOfRange {
                func: self.f,
                at,
                index: k,
            })
    }

    fn func(&self, at: u32, f: u32) -> Result<&Func, VerifyError> {
        usize::try_from(f)
            .ok()
            .and_then(|i| self.m.funcs.get(i))
            .ok_or(VerifyError::FuncOutOfRange {
                func: self.f,
                at,
                index: f,
            })
    }

    fn import(&self, at: u32, h: u32) -> Result<(), VerifyError> {
        let ok = usize::try_from(h).is_ok_and(|i| i < self.m.imports.len());
        if ok {
            Ok(())
        } else {
            Err(VerifyError::ImportOutOfRange {
                func: self.f,
                at,
                index: h,
            })
        }
    }

    fn target(&self, at: u32, t: u32) -> Result<(), VerifyError> {
        let ok = usize::try_from(t).is_ok_and(|i| i < self.len);
        if ok {
            Ok(())
        } else {
            Err(VerifyError::JumpOutOfRange {
                func: self.f,
                at,
                target: t,
            })
        }
    }

    fn check(&self, at: u32, op: &Op) -> Result<(), VerifyError> {
        match *op {
            Op::Int(d, _) | Op::Byte(d, _) | Op::Bool(d, _) | Op::Unit(d) => self.reg(at, d)?,
            Op::FnRef(d, f) => {
                self.reg(at, d)?;
                self.func(at, f).map(|_| ())?;
            }
            Op::Const(d, k) => {
                self.reg(at, d)?;
                self.konst(at, k).map(|_| ())?;
            }
            Op::Move(d, a)
            | Op::Neg(d, a)
            | Op::Not(d, a)
            | Op::Len(d, a)
            | Op::Tag(d, a)
            | Op::Pop(d, a)
            | Op::IntToStr(d, a)
            | Op::ParseInt(d, a)
            | Op::StrToBytes(d, a)
            | Op::BytesToStr(d, a)
            | Op::ToInt(d, a)
            | Op::ToByte(d, a)
            | Op::Push(d, a)
            | Op::GetField(d, a, _)
            | Op::SetField(d, _, a) => {
                self.reg(at, d)?;
                self.reg(at, a)?;
            }
            Op::Add(d, a, b)
            | Op::Sub(d, a, b)
            | Op::Mul(d, a, b)
            | Op::AddW(d, a, b)
            | Op::SubW(d, a, b)
            | Op::MulW(d, a, b)
            | Op::Div(d, a, b)
            | Op::Rem(d, a, b)
            | Op::And(d, a, b)
            | Op::Or(d, a, b)
            | Op::Xor(d, a, b)
            | Op::Shl(d, a, b)
            | Op::Shr(d, a, b)
            | Op::Eq(d, a, b)
            | Op::Ne(d, a, b)
            | Op::Lt(d, a, b)
            | Op::Le(d, a, b)
            | Op::Gt(d, a, b)
            | Op::Ge(d, a, b)
            | Op::Concat(d, a, b)
            | Op::GetIndex(d, a, b)
            | Op::NewFill(d, a, b) => {
                self.reg(at, d)?;
                self.reg(at, a)?;
                self.reg(at, b)?;
            }
            Op::SetIndex(a, b, c) => {
                self.reg(at, a)?;
                self.reg(at, b)?;
                self.reg(at, c)?;
            }
            Op::Slice(d, a, lo, hi) => {
                for r in [d, a, lo, hi] {
                    self.reg(at, r)?;
                }
            }
            Op::Jump(t) => self.target(at, t)?,
            Op::JumpIf(a, t) | Op::JumpIfNot(a, t) => {
                self.reg(at, a)?;
                self.target(at, t)?;
            }
            Op::Call(d, f, first, n) => {
                self.reg(at, d)?;
                let callee = self.func(at, f)?;
                if callee.params != n {
                    return Err(VerifyError::ArgCountMismatch {
                        func: self.f,
                        at,
                        callee: f,
                        expected: callee.params,
                        got: n,
                    });
                }
                self.span(at, first, n)?;
            }
            Op::CallValue(d, a, first, n) => {
                self.reg(at, d)?;
                self.reg(at, a)?;
                self.span(at, first, n)?;
            }
            Op::CallHost(d, h, first, n) => {
                self.reg(at, d)?;
                self.import(at, h)?;
                self.span(at, first, n)?;
            }
            Op::Ret(a) => self.reg(at, a)?,
            Op::NewArray(d, first, n)
            | Op::NewBytes(d, first, n)
            | Op::NewRecord(d, first, n)
            | Op::NewVariant(d, _, first, n) => {
                self.reg(at, d)?;
                self.span(at, first, n)?;
            }
            Op::Assert(a, k) => {
                self.reg(at, a)?;
                if !matches!(self.konst(at, k)?, Const::Str(_)) {
                    return Err(VerifyError::AssertNotString {
                        func: self.f,
                        at,
                        index: k,
                    });
                }
            }
            Op::Trap(_) => {}
        }
        Ok(())
    }
}
