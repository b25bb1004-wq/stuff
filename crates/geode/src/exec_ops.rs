//! Scalar arithmetic, bitwise, shift, comparison, unary and conversion helpers.

use crate::exec_obj::alloc_gc;
use crate::heap::Heap;
use crate::op::Op;
use crate::value::{Obj, Trap, Value};
use crate::vm::{Limits, Vm};
use alloc::{format, string::String, vec, vec::Vec};

pub(crate) fn get_reg(reg_stack: &[Value], bp: usize, r: u16) -> Result<Value, Trap> {
    reg_stack
        .get(bp + usize::from(r))
        .copied()
        .ok_or(Trap::TypeError)
}

pub(crate) fn set_reg(reg_stack: &mut [Value], bp: usize, r: u16, v: Value) -> Result<(), Trap> {
    let slot = reg_stack
        .get_mut(bp + usize::from(r))
        .ok_or(Trap::TypeError)?;
    *slot = v;
    Ok(())
}

pub(crate) fn check_fuel(used: u64, cost: u64, limit: u64) -> Result<(), Trap> {
    if used.checked_add(cost).is_none_or(|n| n > limit) {
        Err(Trap::OutOfFuel)
    } else {
        Ok(())
    }
}

pub(crate) fn spend(used: &mut u64, cost: u64, limit: u64) -> Result<(), Trap> {
    check_fuel(*used, cost, limit)?;
    *used += cost;
    Ok(())
}

pub(crate) fn eval_checked_arith(op: &Op, va: Value, vb: Value) -> Result<Value, Trap> {
    match (va, vb) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Op::Add(..) => x.checked_add(y),
                Op::Sub(..) => x.checked_sub(y),
                _ => x.checked_mul(y),
            };
            r.map(Value::Int).ok_or(Trap::Overflow)
        }
        (Value::Byte(x), Value::Byte(y)) => {
            let r = match op {
                Op::Add(..) => x.checked_add(y),
                Op::Sub(..) => x.checked_sub(y),
                _ => x.checked_mul(y),
            };
            r.map(Value::Byte).ok_or(Trap::Overflow)
        }
        _ => Err(Trap::TypeError),
    }
}

pub(crate) fn eval_wrap_arith(op: &Op, va: Value, vb: Value) -> Result<Value, Trap> {
    let (Value::Int(x), Value::Int(y)) = (va, vb) else {
        return Err(Trap::TypeError);
    };
    let r = match op {
        Op::AddW(..) => x.wrapping_add(y),
        Op::SubW(..) => x.wrapping_sub(y),
        _ => x.wrapping_mul(y),
    };
    Ok(Value::Int(r))
}

pub(crate) fn eval_div_rem(op: &Op, va: Value, vb: Value) -> Result<Value, Trap> {
    match (va, vb) {
        (Value::Int(x), Value::Int(y)) => {
            if y == 0 {
                return Err(Trap::DivideByZero);
            }
            let r = if matches!(op, Op::Div(..)) {
                x.checked_div(y)
            } else {
                x.checked_rem(y)
            };
            r.map(Value::Int).ok_or(Trap::Overflow)
        }
        (Value::Byte(x), Value::Byte(y)) => {
            if y == 0 {
                return Err(Trap::DivideByZero);
            }
            let r = if matches!(op, Op::Div(..)) {
                x / y
            } else {
                x % y
            };
            Ok(Value::Byte(r))
        }
        _ => Err(Trap::TypeError),
    }
}

pub(crate) fn eval_bit_logic(op: &Op, va: Value, vb: Value) -> Result<Value, Trap> {
    match (va, vb) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Op::And(..) => x & y,
                Op::Or(..) => x | y,
                _ => x ^ y,
            };
            Ok(Value::Int(r))
        }
        (Value::Bool(x), Value::Bool(y)) => {
            let r = match op {
                Op::And(..) => x & y,
                Op::Or(..) => x | y,
                _ => x ^ y,
            };
            Ok(Value::Bool(r))
        }
        _ => Err(Trap::TypeError),
    }
}

pub(crate) fn eval_shift(op: &Op, va: Value, vb: Value) -> Result<Value, Trap> {
    let (Value::Int(x), Value::Int(y)) = (va, vb) else {
        return Err(Trap::TypeError);
    };
    let Ok(shift) = u32::try_from(y) else {
        return Err(Trap::Overflow);
    };
    if shift > 63 {
        return Err(Trap::Overflow);
    }
    let res = if matches!(op, Op::Shl(..)) {
        x << shift
    } else {
        x >> shift
    };
    Ok(Value::Int(res))
}

pub(crate) fn eval_eq(heap: &Heap, va: Value, vb: Value) -> Result<(bool, u64), Trap> {
    match (va, vb) {
        (Value::Unit, Value::Unit) => Ok((true, 0)),
        (Value::Int(x), Value::Int(y)) => Ok((x == y, 0)),
        (Value::Byte(x), Value::Byte(y)) => Ok((x == y, 0)),
        (Value::Bool(x), Value::Bool(y)) => Ok((x == y, 0)),
        (Value::Ref(ha), Value::Ref(hb)) => match (heap.get(ha)?, heap.get(hb)?) {
            (Obj::Str(sa), Obj::Str(sb)) => {
                let extra = (sa.len().min(sb.len()) as u64).div_ceil(8);
                Ok((sa == sb, extra))
            }
            (Obj::Bytes(ba), Obj::Bytes(bb)) => {
                let extra = (ba.len().min(bb.len()) as u64).div_ceil(8);
                Ok((ba == bb, extra))
            }
            _ => Err(Trap::TypeError),
        },
        _ => Err(Trap::TypeError),
    }
}

pub(crate) fn eval_cmp(
    heap: &Heap,
    va: Value,
    vb: Value,
) -> Result<(core::cmp::Ordering, u64), Trap> {
    match (va, vb) {
        (Value::Int(x), Value::Int(y)) => Ok((x.cmp(&y), 0)),
        (Value::Byte(x), Value::Byte(y)) => Ok((x.cmp(&y), 0)),
        (Value::Ref(ha), Value::Ref(hb)) => match (heap.get(ha)?, heap.get(hb)?) {
            (Obj::Str(sa), Obj::Str(sb)) => {
                let extra = (sa.len().min(sb.len()) as u64).div_ceil(8);
                Ok((sa.as_bytes().cmp(sb.as_bytes()), extra))
            }
            _ => Err(Trap::TypeError),
        },
        _ => Err(Trap::TypeError),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_unary_misc(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    a: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let va = get_reg(reg_stack, bp, a)?;
    let res = match op {
        Op::Neg(..) => {
            let Value::Int(x) = va else {
                return Err(Trap::TypeError);
            };
            Value::Int(x.checked_neg().ok_or(Trap::Overflow)?)
        }
        Op::Not(..) => {
            let Value::Bool(b) = va else {
                return Err(Trap::TypeError);
            };
            Value::Bool(!b)
        }
        Op::Tag(..) => {
            let Value::Ref(h) = va else {
                return Err(Trap::TypeError);
            };
            let Obj::Variant { tag, .. } = vm.heap.get(h)? else {
                return Err(Trap::TypeError);
            };
            Value::Int(i64::from(*tag))
        }
        Op::Len(..) => {
            let Value::Ref(h) = va else {
                return Err(Trap::TypeError);
            };
            let len = match vm.heap.get(h)? {
                Obj::Str(s) => s.len(),
                Obj::Bytes(b) => b.len(),
                Obj::Array(v) => v.len(),
                _ => return Err(Trap::TypeError),
            };
            Value::Int(len as i64)
        }
        Op::ToInt(..) => {
            let Value::Byte(b) = va else {
                return Err(Trap::TypeError);
            };
            Value::Int(i64::from(b))
        }
        _ => {
            let Value::Int(x) = va else {
                return Err(Trap::TypeError);
            };
            Value::Byte(u8::try_from(x).map_err(|_| Trap::Overflow)?)
        }
    };
    spend(&mut vm.fuel_used, 1, limits.fuel)?;
    set_reg(reg_stack, bp, d, res)
}

pub(crate) fn exec_conv(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    a: u16,
    limits: Limits,
) -> Result<(), Trap> {
    match op {
        Op::IntToStr(..) => {
            let Value::Int(x) = get_reg(reg_stack, bp, a)? else {
                return Err(Trap::TypeError);
            };
            let s = format!("{x}");
            let cost = 1u64.saturating_add((s.len() as u64).div_ceil(8));
            check_fuel(vm.fuel_used, cost, limits.fuel)?;
            let h = alloc_gc(vm, Obj::Str(s), reg_stack, None)?;
            vm.fuel_used += cost;
            set_reg(reg_stack, bp, d, Value::Ref(h))
        }
        Op::ParseInt(..) => {
            let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
                return Err(Trap::TypeError);
            };
            let Obj::Str(s) = vm.heap.get(h)? else {
                return Err(Trap::TypeError);
            };
            let (cost, var_obj) = match parse_int_strict(s) {
                Some(n) => (
                    2u64,
                    Obj::Variant {
                        tag: 1,
                        fields: vec![Value::Int(n)],
                    },
                ),
                None => (
                    1u64,
                    Obj::Variant {
                        tag: 0,
                        fields: Vec::new(),
                    },
                ),
            };
            check_fuel(vm.fuel_used, cost, limits.fuel)?;
            let var_h = alloc_gc(vm, var_obj, reg_stack, None)?;
            vm.fuel_used += cost;
            set_reg(reg_stack, bp, d, Value::Ref(var_h))
        }
        Op::StrToBytes(..) => {
            let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
                return Err(Trap::TypeError);
            };
            let Obj::Str(s) = vm.heap.get(h)? else {
                return Err(Trap::TypeError);
            };
            let len = s.len();
            let cost = 1u64.saturating_add((len as u64).div_ceil(8));
            check_fuel(vm.fuel_used, cost, limits.fuel)?;
            let size = 16 + len;
            vm.heap.ensure_space(size, reg_stack, None)?;
            let Obj::Str(s) = vm.heap.get(h)? else {
                return Err(Trap::TypeError);
            };
            let bytes = s.as_bytes().to_vec();
            let new_h = vm.heap.alloc_with_size(Obj::Bytes(bytes), size)?;
            vm.fuel_used += cost;
            set_reg(reg_stack, bp, d, Value::Ref(new_h))
        }
        _ => {
            let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
                return Err(Trap::TypeError);
            };
            let Obj::Bytes(b) = vm.heap.get(h)? else {
                return Err(Trap::TypeError);
            };
            if core::str::from_utf8(b).is_ok() {
                let len = b.len();
                let cost = 1u64.saturating_add((len as u64).div_ceil(8));
                check_fuel(vm.fuel_used, cost, limits.fuel)?;
                let str_size = 16 + len;
                let total_size = str_size.checked_add(32).ok_or(Trap::OutOfMemory)?;
                vm.heap.ensure_space(total_size, reg_stack, None)?;
                let Obj::Bytes(b) = vm.heap.get(h)? else {
                    return Err(Trap::TypeError);
                };
                let s = String::from_utf8(b.clone()).map_err(|_| Trap::TypeError)?;
                let str_h = vm.heap.alloc_with_size(Obj::Str(s), str_size)?;
                let var_h = vm.heap.alloc_with_size(
                    Obj::Variant {
                        tag: 1,
                        fields: vec![Value::Ref(str_h)],
                    },
                    32,
                )?;
                vm.fuel_used += cost;
                set_reg(reg_stack, bp, d, Value::Ref(var_h))
            } else {
                check_fuel(vm.fuel_used, 1, limits.fuel)?;
                let var_h = alloc_gc(
                    vm,
                    Obj::Variant {
                        tag: 0,
                        fields: Vec::new(),
                    },
                    reg_stack,
                    None,
                )?;
                vm.fuel_used += 1;
                set_reg(reg_stack, bp, d, Value::Ref(var_h))
            }
        }
    }
}

fn parse_int_strict(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let digits = if bytes.first() == Some(&b'-') {
        &bytes[1..]
    } else {
        bytes
    };
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}
