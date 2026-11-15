//! Reference interpreter for Logos's typed intermediate representation.

mod vm;

#[cfg(test)]
mod tests;

use crate::ir::{self, Program, Ty, TypeKind};
use crate::span::Span;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use vm::{Rng, Val, Vm};

/// Runtime trap raised by the Logos reference interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Trap {
    /// Signed integer or byte arithmetic overflow, or out-of-range shift/conversion.
    Overflow,
    /// Division or remainder by zero.
    DivideByZero,
    /// Array/slice index out of range or invalid UTF-8 slice boundary.
    IndexOutOfRange,
    /// Failed `assert` statement with its message.
    AssertFailed(String),
    /// Instruction fuel exhausted.
    OutOfFuel,
    /// Call stack depth exceeded 10,000 frames.
    StackOverflow,
    /// Host extern function not provided by the reference interpreter.
    NoExtern(String),
}

/// Result of running a Logos entry point, test, or property.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// Captured `io.print` output (and property failure summary, if any).
    pub output: String,
    /// Completion status or trapping instruction span.
    pub result: Result<(), (Trap, Span)>,
}

/// Runs `main` of `p` with `input` lines for `io.read_line` and an instruction `fuel` budget.
pub fn run_main(p: &ir::Program, input: &str, fuel: u64) -> Outcome {
    let Some(fid) = p.main else {
        return Outcome {
            output: String::new(),
            result: Ok(()),
        };
    };
    let mut vm = Vm::new(p, input, fuel);
    let result = vm.run_func(fid, &[]).map(|_| ());
    Outcome {
        output: vm.output,
        result,
    }
}

/// Runs unit test `index` of `p` with an instruction `fuel` budget.
pub fn run_test(p: &ir::Program, index: usize, fuel: u64) -> Outcome {
    let Some((_, fid)) = p.tests.get(index) else {
        return Outcome {
            output: String::new(),
            result: Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 })),
        };
    };
    let mut vm = Vm::new(p, "", fuel);
    let result = vm.run_func(*fid, &[]).map(|_| ());
    Outcome {
        output: vm.output,
        result,
    }
}

/// Runs property test `index` of `p` for up to `cases` seeded inputs with `fuel` per case.
pub fn run_prop(p: &ir::Program, index: usize, seed: u64, cases: u32, fuel: u64) -> Outcome {
    let Some(prop) = p.props.get(index) else {
        return Outcome {
            output: String::new(),
            result: Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 })),
        };
    };
    let Some(func) = p.funcs.get(prop.func as usize) else {
        return Outcome {
            output: String::new(),
            result: Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 })),
        };
    };
    let param_tys = func.regs.get(..func.params as usize).unwrap_or(&[]);
    let mut master = Rng::new(seed);
    let mut output = String::new();

    for case in 0..cases {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let args: Vec<Val> = param_tys
            .iter()
            .map(|ty| gen_val(p, ty, &mut rng, 0))
            .collect();

        if let Some(guard_fid) = prop.guard {
            let guard_args: Vec<Val> = args.iter().map(Val::deep_clone).collect();
            let mut gvm = Vm::new(p, "", fuel);
            match gvm.run_func(guard_fid, &guard_args) {
                Ok(Val::Bool(true)) => {}
                Ok(Val::Bool(false)) => continue,
                Ok(_) => {
                    return Outcome {
                        output,
                        result: Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 })),
                    };
                }
                Err((trap, span)) => {
                    output.push_str(&format!(
                        "prop '{}' guard failed on case {} (seed {:#018x}): {:?}\n",
                        prop.name, case, seed, trap
                    ));
                    return Outcome {
                        output,
                        result: Err((trap, span)),
                    };
                }
            }
        }

        let mut vm = Vm::new(p, "", fuel);
        if let Err((trap, span)) = vm.run_func(prop.func, &args) {
            output.push_str(&vm.output);
            output.push_str(&format!(
                "prop '{}' failed on case {} (seed {:#018x}, case_seed {:#018x}): {:?}\n",
                prop.name, case, seed, case_seed, trap
            ));
            return Outcome {
                output,
                result: Err((trap, span)),
            };
        }
    }

    Outcome {
        output,
        result: Ok(()),
    }
}

fn gen_val(p: &Program, ty: &Ty, rng: &mut Rng, depth: u32) -> Val {
    match ty {
        Ty::Int => if rng.chance(1, 2) {
            (rng.range(0, 2001) as i64) - 1000
        } else {
            rng.u64() as i64
        }
        .into_val(),
        Ty::Byte => Val::Byte(rng.u64() as u8),
        Ty::Bool => Val::Bool(rng.chance(1, 2)),
        Ty::Unit => Val::Unit,
        Ty::Str => {
            let len = if depth > 4 { 0 } else { rng.below(17) };
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                let ch = (b' ' + (rng.below(95) as u8)) as char;
                s.push(ch);
            }
            Val::Str(Rc::new(s))
        }
        Ty::Array(elem) => {
            let len = if depth > 3 { 0 } else { rng.below(13) };
            let items = (0..len).map(|_| gen_val(p, elem, rng, depth + 1)).collect();
            Val::Array(Rc::new(RefCell::new(items)))
        }
        Ty::Tuple(elems) => {
            let items = elems
                .iter()
                .map(|t| gen_val(p, t, rng, depth + 1))
                .collect();
            Val::Tuple(Rc::new(items))
        }
        Ty::Opt(inner) => {
            if depth > 4 || rng.chance(1, 4) {
                Val::Opt(None)
            } else {
                Val::Opt(Some(Box::new(gen_val(p, inner, rng, depth + 1))))
            }
        }
        Ty::Named(tid) => match p.types.get(*tid as usize).map(|t| &t.kind) {
            Some(TypeKind::Record(fields)) => {
                let vals = fields
                    .iter()
                    .map(|(_, ft)| gen_val(p, ft, rng, depth + 1))
                    .collect();
                Val::Record(Rc::new(RefCell::new(vals)))
            }
            Some(TypeKind::Variants(vars)) if !vars.is_empty() => {
                let idx = if depth > 4 {
                    vars.iter().position(|(_, f)| f.is_empty()).unwrap_or(0)
                } else {
                    rng.below(vars.len())
                };
                let vals = vars
                    .get(idx)
                    .map(|(_, f)| f.iter().map(|ft| gen_val(p, ft, rng, depth + 1)).collect())
                    .unwrap_or_default();
                Val::Variant(idx as u32, Rc::new(vals))
            }
            _ => Val::Unit,
        },
        Ty::Fn {
            params,
            ret,
            layers,
        } => {
            let matched = p.funcs.iter().enumerate().find(|(_, f)| {
                let f_params = f.regs.get(..f.params as usize).unwrap_or(&[]);
                f_params == params.as_slice() && f.ret == **ret && (f.layers & !layers) == 0
            });
            Val::Fn(matched.map_or(0, |(i, _)| i as u32))
        }
    }
}

trait IntoVal {
    fn into_val(self) -> Val;
}

impl IntoVal for i64 {
    fn into_val(self) -> Val {
        Val::Int(self)
    }
}
