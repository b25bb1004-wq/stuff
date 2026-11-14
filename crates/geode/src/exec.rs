//! The iterative bytecode interpreter loop and frame management.

use crate::exec_obj::{
    alloc_gc, exec_concat, exec_field, exec_index, exec_new_fill, exec_new_seq, exec_push_pop,
    exec_slice,
};
use crate::exec_ops::{
    check_fuel, eval_bit_logic, eval_checked_arith, eval_cmp, eval_div_rem, eval_eq, eval_shift,
    eval_wrap_arith, exec_conv, exec_unary_misc, get_reg, set_reg, spend,
};
use crate::module::Const;
use crate::op::Op;
use crate::value::{Obj, Trap, Value};
use crate::vm::{Host, Limits, Vm};
use alloc::{vec, vec::Vec};

/// One active call frame on the VM's iterative frame stack.
struct Frame {
    func: u32,
    pc: usize,
    bp: usize,
    ret_reg: u16,
}

/// Run `entry_func` with `args` under `limits`.
pub(crate) fn execute(
    vm: &mut Vm,
    entry_func: u32,
    args: &[Value],
    host: &mut dyn Host,
    limits: Limits,
) -> Result<Value, Trap> {
    if limits.max_depth == 0 {
        vm.trap_site = Some((entry_func, 0));
        return Err(Trap::StackOverflow);
    }
    let init_regs = vm
        .module
        .funcs
        .get(entry_func as usize)
        .ok_or(Trap::TypeError)?
        .regs;
    let mut reg_stack = vec![Value::Unit; usize::from(init_regs)];
    if let Some(dst) = reg_stack.get_mut(..args.len()) {
        dst.copy_from_slice(args);
    }

    let mut frames = Vec::new();
    frames.push(Frame {
        func: entry_func,
        pc: 0,
        bp: 0,
        ret_reg: 0,
    });

    while let Some(frame) = frames.last_mut() {
        let func_idx = frame.func as usize;
        let pc = frame.pc;
        let bp = frame.bp;
        vm.trap_site = Some((frame.func, pc as u32));
        if vm.fuel_used >= limits.fuel {
            return Err(Trap::OutOfFuel);
        }
        let op = vm
            .module
            .funcs
            .get(func_idx)
            .and_then(|f| f.code.get(pc))
            .ok_or(Trap::TypeError)?
            .clone();
        frame.pc = pc.saturating_add(1);

        match op {
            Op::Int(d, v) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, Value::Int(v))?;
            }
            Op::Byte(d, v) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, Value::Byte(v))?;
            }
            Op::Bool(d, v) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, Value::Bool(v))?;
            }
            Op::Unit(d) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, Value::Unit)?;
            }
            Op::FnRef(d, f) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, Value::Fn(f))?;
            }
            Op::Const(d, k) => {
                let (obj, len) = match vm.module.consts.get(k as usize).ok_or(Trap::TypeError)? {
                    Const::Str(s) => (Obj::Str(s.clone()), s.len()),
                    Const::Bytes(b) => (Obj::Bytes(b.clone()), b.len()),
                };
                let cost = 1u64.saturating_add((len as u64).div_ceil(8));
                check_fuel(vm.fuel_used, cost, limits.fuel)?;
                let h = alloc_gc(vm, obj, &reg_stack, None)?;
                vm.fuel_used += cost;
                set_reg(&mut reg_stack, bp, d, Value::Ref(h))?;
            }
            Op::Move(d, a) => {
                let va = get_reg(&reg_stack, bp, a)?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, va)?;
            }
            Op::Add(d, a, b) | Op::Sub(d, a, b) | Op::Mul(d, a, b) => {
                let res = eval_checked_arith(
                    &op,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::AddW(d, a, b) | Op::SubW(d, a, b) | Op::MulW(d, a, b) => {
                let res = eval_wrap_arith(
                    &op,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::Div(d, a, b) | Op::Rem(d, a, b) => {
                let res = eval_div_rem(
                    &op,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::And(d, a, b) | Op::Or(d, a, b) | Op::Xor(d, a, b) => {
                let res = eval_bit_logic(
                    &op,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::Shl(d, a, b) | Op::Shr(d, a, b) => {
                let res = eval_shift(
                    &op,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::Neg(d, a)
            | Op::Not(d, a)
            | Op::Tag(d, a)
            | Op::Len(d, a)
            | Op::ToInt(d, a)
            | Op::ToByte(d, a) => {
                exec_unary_misc(vm, &op, &mut reg_stack, bp, d, a, limits)?;
            }
            Op::Eq(d, a, b) | Op::Ne(d, a, b) => {
                let (eq, extra) = eval_eq(
                    &vm.heap,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1u64.saturating_add(extra), limits.fuel)?;
                let res = if matches!(op, Op::Eq(..)) { eq } else { !eq };
                set_reg(&mut reg_stack, bp, d, Value::Bool(res))?;
            }
            Op::Lt(d, a, b) | Op::Le(d, a, b) | Op::Gt(d, a, b) | Op::Ge(d, a, b) => {
                let (ord, extra) = eval_cmp(
                    &vm.heap,
                    get_reg(&reg_stack, bp, a)?,
                    get_reg(&reg_stack, bp, b)?,
                )?;
                spend(&mut vm.fuel_used, 1u64.saturating_add(extra), limits.fuel)?;
                let res = match op {
                    Op::Lt(..) => ord.is_lt(),
                    Op::Le(..) => ord.is_le(),
                    Op::Gt(..) => ord.is_gt(),
                    _ => ord.is_ge(),
                };
                set_reg(&mut reg_stack, bp, d, Value::Bool(res))?;
            }
            Op::Jump(t) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                if let Some(top) = frames.last_mut() {
                    top.pc = t as usize;
                }
            }
            Op::JumpIf(a, t) | Op::JumpIfNot(a, t) => {
                let Value::Bool(cond) = get_reg(&reg_stack, bp, a)? else {
                    return Err(Trap::TypeError);
                };
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                let take = if matches!(op, Op::JumpIf(..)) {
                    cond
                } else {
                    !cond
                };
                if take {
                    if let Some(top) = frames.last_mut() {
                        top.pc = t as usize;
                    }
                }
            }
            Op::Call(d, f, first, n) => {
                push_call_frame(vm, &mut frames, &mut reg_stack, bp, d, f, first, n, limits)?;
            }
            Op::CallValue(d, a, first, n) => {
                let Value::Fn(f) = get_reg(&reg_stack, bp, a)? else {
                    return Err(Trap::TypeError);
                };
                push_call_frame(vm, &mut frames, &mut reg_stack, bp, d, f, first, n, limits)?;
            }
            Op::CallHost(d, h, first, n) => {
                check_fuel(vm.fuel_used, 1, limits.fuel)?;
                let name = vm
                    .module
                    .imports
                    .get(h as usize)
                    .ok_or(Trap::TypeError)?
                    .clone();
                let start = bp + usize::from(first);
                let call_args = reg_stack
                    .get(start..start + usize::from(n))
                    .ok_or(Trap::TypeError)?
                    .to_vec();
                let res = host.call(&name, &call_args, &mut vm.heap)?;
                vm.heap.sync_dirty();
                vm.fuel_used += 1;
                set_reg(&mut reg_stack, bp, d, res)?;
            }
            Op::Ret(a) => {
                let ret_val = get_reg(&reg_stack, bp, a)?;
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                let popped = frames.pop().ok_or(Trap::TypeError)?;
                reg_stack.truncate(popped.bp);
                if let Some(caller) = frames.last() {
                    set_reg(&mut reg_stack, caller.bp, popped.ret_reg, ret_val)?;
                } else {
                    vm.trap_site = None;
                    return Ok(ret_val);
                }
            }
            Op::NewArray(d, first, n)
            | Op::NewRecord(d, first, n)
            | Op::NewVariant(d, _, first, n)
            | Op::NewBytes(d, first, n) => {
                exec_new_seq(vm, &op, &mut reg_stack, bp, d, first, n, limits)?;
            }
            Op::NewFill(d, a, b) => {
                exec_new_fill(vm, &mut reg_stack, bp, d, a, b, limits)?;
            }
            Op::GetField(d, a, i) | Op::SetField(d, i, a) => {
                exec_field(vm, &op, &mut reg_stack, bp, d, a, i, limits)?;
            }
            Op::GetIndex(d, a, b) | Op::SetIndex(d, a, b) => {
                exec_index(vm, &op, &mut reg_stack, bp, d, a, b, limits)?;
            }
            Op::Push(a, b) | Op::Pop(a, b) => {
                exec_push_pop(vm, &op, &mut reg_stack, bp, a, b, limits)?;
            }
            Op::Slice(d, a, lo, hi) => {
                exec_slice(vm, &mut reg_stack, bp, d, a, lo, hi, limits)?;
            }
            Op::Concat(d, a, b) => {
                exec_concat(vm, &mut reg_stack, bp, d, a, b, limits)?;
            }
            Op::IntToStr(d, a)
            | Op::ParseInt(d, a)
            | Op::StrToBytes(d, a)
            | Op::BytesToStr(d, a) => {
                exec_conv(vm, &op, &mut reg_stack, bp, d, a, limits)?;
            }
            Op::Assert(a, k) => {
                let Value::Bool(cond) = get_reg(&reg_stack, bp, a)? else {
                    return Err(Trap::TypeError);
                };
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                if !cond {
                    let Some(Const::Str(msg)) = vm.module.consts.get(k as usize) else {
                        return Err(Trap::TypeError);
                    };
                    return Err(Trap::AssertFailed(msg.clone()));
                }
            }
            Op::Trap(code) => {
                spend(&mut vm.fuel_used, 1, limits.fuel)?;
                return Err(Trap::User(code));
            }
        }
    }
    Err(Trap::TypeError)
}

#[allow(clippy::too_many_arguments)]
fn push_call_frame(
    vm: &mut Vm,
    frames: &mut Vec<Frame>,
    reg_stack: &mut Vec<Value>,
    caller_bp: usize,
    ret_reg: u16,
    callee_idx: u32,
    first: u16,
    n: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let callee = vm
        .module
        .funcs
        .get(callee_idx as usize)
        .ok_or(Trap::TypeError)?;
    if callee.params != n {
        return Err(Trap::TypeError);
    }
    if frames.len() >= limits.max_depth as usize {
        return Err(Trap::StackOverflow);
    }
    spend(&mut vm.fuel_used, 1, limits.fuel)?;
    let new_bp = reg_stack.len();
    let callee_regs = usize::from(callee.regs);
    reg_stack.resize(new_bp + callee_regs, Value::Unit);
    let src = caller_bp + usize::from(first);
    let n_u = usize::from(n);
    reg_stack.copy_within(src..src + n_u, new_bp);
    frames.push(Frame {
        func: callee_idx,
        pc: 0,
        bp: new_bp,
        ret_reg,
    });
    Ok(())
}
