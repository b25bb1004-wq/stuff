//! Heap object construction, indexing, mutation, slicing and concatenation.

use crate::exec_ops::{check_fuel, get_reg, set_reg, spend};
use crate::heap::obj_size;
use crate::op::Op;
use crate::value::{Handle, Obj, Trap, Value};
use crate::vm::{Limits, Vm};
use alloc::{string::String, vec, vec::Vec};

pub(crate) fn alloc_gc(
    vm: &mut Vm,
    obj: Obj,
    roots: &[Value],
    extra: Option<Value>,
) -> Result<Handle, Trap> {
    let size = obj_size(&obj);
    vm.heap.ensure_space(size, roots, extra)?;
    vm.heap.alloc_with_size(obj, size)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_new_seq(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    first: u16,
    n: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let start = bp + usize::from(first);
    let slice = reg_stack
        .get(start..start + usize::from(n))
        .ok_or(Trap::TypeError)?;
    if matches!(op, Op::NewBytes(..)) {
        let cost = 1u64.saturating_add(u64::from(n).div_ceil(8));
        check_fuel(vm.fuel_used, cost, limits.fuel)?;
        let mut bytes = Vec::with_capacity(usize::from(n));
        for &v in slice {
            let Value::Byte(b) = v else {
                return Err(Trap::TypeError);
            };
            bytes.push(b);
        }
        let h = alloc_gc(vm, Obj::Bytes(bytes), reg_stack, None)?;
        vm.fuel_used += cost;
        set_reg(reg_stack, bp, d, Value::Ref(h))
    } else {
        let cost = 1u64.saturating_add(u64::from(n));
        check_fuel(vm.fuel_used, cost, limits.fuel)?;
        let elems = slice.to_vec();
        let obj = match *op {
            Op::NewArray(..) => Obj::Array(elems),
            Op::NewRecord(..) => Obj::Record(elems),
            Op::NewVariant(_, tag, ..) => Obj::Variant { tag, fields: elems },
            _ => return Err(Trap::TypeError),
        };
        let h = alloc_gc(vm, obj, reg_stack, None)?;
        vm.fuel_used += cost;
        set_reg(reg_stack, bp, d, Value::Ref(h))
    }
}

pub(crate) fn exec_new_fill(
    vm: &mut Vm,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    a: u16,
    b: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let val = get_reg(reg_stack, bp, a)?;
    let Value::Int(cnt) = get_reg(reg_stack, bp, b)? else {
        return Err(Trap::TypeError);
    };
    if cnt < 0 {
        return Err(Trap::IndexOutOfRange);
    }
    let cnt_u64 = cnt as u64;
    let extra = if matches!(val, Value::Byte(_)) {
        cnt_u64.div_ceil(8)
    } else {
        cnt_u64
    };
    let cost = 1u64.saturating_add(extra);
    check_fuel(vm.fuel_used, cost, limits.fuel)?;

    let count = usize::try_from(cnt_u64).map_err(|_| Trap::OutOfMemory)?;
    let size = if matches!(val, Value::Byte(_)) {
        16usize.checked_add(count).ok_or(Trap::OutOfMemory)?
    } else {
        count
            .checked_mul(16)
            .and_then(|p| p.checked_add(16))
            .ok_or(Trap::OutOfMemory)?
    };
    vm.heap.ensure_space(size, reg_stack, None)?;
    let obj = if let Value::Byte(byte) = val {
        Obj::Bytes(vec![byte; count])
    } else {
        Obj::Array(vec![val; count])
    };
    let h = vm.heap.alloc_with_size(obj, size)?;
    vm.fuel_used += cost;
    set_reg(reg_stack, bp, d, Value::Ref(h))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_field(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    r1: u16,
    r2: u16,
    i: u16,
    limits: Limits,
) -> Result<(), Trap> {
    if matches!(op, Op::GetField(..)) {
        let (d, a) = (r1, r2);
        let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
            return Err(Trap::TypeError);
        };
        let val = match vm.heap.get(h)? {
            Obj::Record(v) | Obj::Variant { fields: v, .. } => {
                *v.get(usize::from(i)).ok_or(Trap::IndexOutOfRange)?
            }
            _ => return Err(Trap::TypeError),
        };
        spend(&mut vm.fuel_used, 1, limits.fuel)?;
        set_reg(reg_stack, bp, d, val)
    } else {
        let (a, b) = (r1, r2);
        let val = get_reg(reg_stack, bp, b)?;
        let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
            return Err(Trap::TypeError);
        };
        match vm.heap.get_mut(h)? {
            Obj::Record(v) | Obj::Variant { fields: v, .. } => {
                let slot = v.get_mut(usize::from(i)).ok_or(Trap::IndexOutOfRange)?;
                *slot = val;
            }
            _ => return Err(Trap::TypeError),
        }
        vm.heap.sync_dirty();
        spend(&mut vm.fuel_used, 1, limits.fuel)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_index(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    r1: u16,
    r2: u16,
    r3: u16,
    limits: Limits,
) -> Result<(), Trap> {
    if matches!(op, Op::GetIndex(..)) {
        let (d, a, b) = (r1, r2, r3);
        let (Value::Ref(h), Value::Int(idx)) =
            (get_reg(reg_stack, bp, a)?, get_reg(reg_stack, bp, b)?)
        else {
            return Err(Trap::TypeError);
        };
        let idx_u = usize::try_from(idx).map_err(|_| Trap::IndexOutOfRange)?;
        let val = match vm.heap.get(h)? {
            Obj::Array(v) => *v.get(idx_u).ok_or(Trap::IndexOutOfRange)?,
            Obj::Bytes(v) => Value::Byte(*v.get(idx_u).ok_or(Trap::IndexOutOfRange)?),
            _ => return Err(Trap::TypeError),
        };
        spend(&mut vm.fuel_used, 1, limits.fuel)?;
        set_reg(reg_stack, bp, d, val)
    } else {
        let (a, b, c) = (r1, r2, r3);
        let (va, vb, vc) = (
            get_reg(reg_stack, bp, a)?,
            get_reg(reg_stack, bp, b)?,
            get_reg(reg_stack, bp, c)?,
        );
        let (Value::Ref(h), Value::Int(idx)) = (va, vb) else {
            return Err(Trap::TypeError);
        };
        let res = match vm.heap.get_mut(h)? {
            Obj::Array(v) => {
                let idx_u = usize::try_from(idx).map_err(|_| Trap::IndexOutOfRange)?;
                v.get_mut(idx_u)
                    .map(|s| *s = vc)
                    .ok_or(Trap::IndexOutOfRange)
            }
            Obj::Bytes(v) => {
                let Value::Byte(byte) = vc else {
                    vm.heap.sync_dirty();
                    return Err(Trap::TypeError);
                };
                let idx_u = usize::try_from(idx).map_err(|_| Trap::IndexOutOfRange)?;
                v.get_mut(idx_u)
                    .map(|s| *s = byte)
                    .ok_or(Trap::IndexOutOfRange)
            }
            _ => Err(Trap::TypeError),
        };
        vm.heap.sync_dirty();
        res?;
        spend(&mut vm.fuel_used, 1, limits.fuel)
    }
}

pub(crate) fn exec_push_pop(
    vm: &mut Vm,
    op: &Op,
    reg_stack: &mut [Value],
    bp: usize,
    r1: u16,
    r2: u16,
    limits: Limits,
) -> Result<(), Trap> {
    if matches!(op, Op::Push(..)) {
        let (va, vb) = (get_reg(reg_stack, bp, r1)?, get_reg(reg_stack, bp, r2)?);
        let Value::Ref(h) = va else {
            return Err(Trap::TypeError);
        };
        let add_bytes = match vm.heap.get(h)? {
            Obj::Array(_) => 16usize,
            Obj::Bytes(_) if matches!(vb, Value::Byte(_)) => 1usize,
            _ => return Err(Trap::TypeError),
        };
        check_fuel(vm.fuel_used, 2, limits.fuel)?;
        vm.heap.ensure_space(add_bytes, reg_stack, None)?;
        match vm.heap.get_mut(h)? {
            Obj::Array(v) => v.push(vb),
            Obj::Bytes(v) => {
                if let Value::Byte(b) = vb {
                    v.push(b);
                }
            }
            _ => return Err(Trap::TypeError),
        }
        vm.heap.sync_dirty();
        vm.fuel_used += 2;
        Ok(())
    } else {
        let (d, a) = (r1, r2);
        let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
            return Err(Trap::TypeError);
        };
        let is_empty = match vm.heap.get(h)? {
            Obj::Array(v) => v.is_empty(),
            Obj::Bytes(v) => v.is_empty(),
            _ => return Err(Trap::TypeError),
        };
        let cost = if is_empty { 1 } else { 2 };
        check_fuel(vm.fuel_used, cost, limits.fuel)?;
        let popped = match vm.heap.get_mut(h)? {
            Obj::Array(v) => v.pop(),
            Obj::Bytes(v) => v.pop().map(Value::Byte),
            _ => return Err(Trap::TypeError),
        };
        vm.heap.sync_dirty();
        let var_obj = match popped {
            Some(v) => Obj::Variant {
                tag: 1,
                fields: vec![v],
            },
            None => Obj::Variant {
                tag: 0,
                fields: Vec::new(),
            },
        };
        match alloc_gc(vm, var_obj, reg_stack, popped) {
            Ok(var_h) => {
                vm.fuel_used += cost;
                set_reg(reg_stack, bp, d, Value::Ref(var_h))
            }
            Err(e) => {
                if let Some(v) = popped {
                    if let Ok(Obj::Array(vec)) = vm.heap.get_mut(h) {
                        vec.push(v);
                    } else if let (Ok(Obj::Bytes(vec)), Value::Byte(b)) = (vm.heap.get_mut(h), v) {
                        vec.push(b);
                    }
                    vm.heap.sync_dirty();
                }
                Err(e)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exec_slice(
    vm: &mut Vm,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    a: u16,
    lo: u16,
    hi: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let Value::Ref(h) = get_reg(reg_stack, bp, a)? else {
        return Err(Trap::TypeError);
    };
    let (Value::Int(lo_i), Value::Int(hi_i)) =
        (get_reg(reg_stack, bp, lo)?, get_reg(reg_stack, bp, hi)?)
    else {
        return Err(Trap::TypeError);
    };
    if !matches!(vm.heap.get(h)?, Obj::Str(_) | Obj::Bytes(_) | Obj::Array(_)) {
        return Err(Trap::TypeError);
    }
    if lo_i < 0 || hi_i < lo_i {
        return Err(Trap::IndexOutOfRange);
    }
    let lo_u = usize::try_from(lo_i).map_err(|_| Trap::IndexOutOfRange)?;
    let hi_u = usize::try_from(hi_i).map_err(|_| Trap::IndexOutOfRange)?;
    let slen = hi_u - lo_u;

    let (cost, size) = match vm.heap.get(h)? {
        Obj::Str(s) if hi_u <= s.len() && s.is_char_boundary(lo_u) && s.is_char_boundary(hi_u) => {
            (1u64.saturating_add((slen as u64).div_ceil(8)), 16 + slen)
        }
        Obj::Bytes(b) if hi_u <= b.len() => {
            (1u64.saturating_add((slen as u64).div_ceil(8)), 16 + slen)
        }
        Obj::Array(v) if hi_u <= v.len() => (1u64.saturating_add(slen as u64), 16 + slen * 16),
        _ => return Err(Trap::IndexOutOfRange),
    };
    check_fuel(vm.fuel_used, cost, limits.fuel)?;
    vm.heap.ensure_space(size, reg_stack, None)?;
    let new_obj = match vm.heap.get(h)? {
        Obj::Str(s) => Obj::Str(String::from(&s[lo_u..hi_u])),
        Obj::Bytes(b) => Obj::Bytes(b[lo_u..hi_u].to_vec()),
        Obj::Array(v) => Obj::Array(v[lo_u..hi_u].to_vec()),
        _ => return Err(Trap::TypeError),
    };
    let new_h = vm.heap.alloc_with_size(new_obj, size)?;
    vm.fuel_used += cost;
    set_reg(reg_stack, bp, d, Value::Ref(new_h))
}

pub(crate) fn exec_concat(
    vm: &mut Vm,
    reg_stack: &mut [Value],
    bp: usize,
    d: u16,
    a: u16,
    b: u16,
    limits: Limits,
) -> Result<(), Trap> {
    let (Value::Ref(ha), Value::Ref(hb)) = (get_reg(reg_stack, bp, a)?, get_reg(reg_stack, bp, b)?)
    else {
        return Err(Trap::TypeError);
    };
    let (cost, size) = match (vm.heap.get(ha)?, vm.heap.get(hb)?) {
        (Obj::Str(sa), Obj::Str(sb)) => {
            let n = sa.len().checked_add(sb.len()).ok_or(Trap::OutOfMemory)?;
            let sz = 16usize.checked_add(n).ok_or(Trap::OutOfMemory)?;
            (1u64.saturating_add((n as u64).div_ceil(8)), sz)
        }
        (Obj::Bytes(ba), Obj::Bytes(bb)) => {
            let n = ba.len().checked_add(bb.len()).ok_or(Trap::OutOfMemory)?;
            let sz = 16usize.checked_add(n).ok_or(Trap::OutOfMemory)?;
            (1u64.saturating_add((n as u64).div_ceil(8)), sz)
        }
        (Obj::Array(va), Obj::Array(vb)) => {
            let n = va.len().checked_add(vb.len()).ok_or(Trap::OutOfMemory)?;
            let sz = n
                .checked_mul(16)
                .and_then(|p| p.checked_add(16))
                .ok_or(Trap::OutOfMemory)?;
            (1u64.saturating_add(n as u64), sz)
        }
        _ => return Err(Trap::TypeError),
    };
    check_fuel(vm.fuel_used, cost, limits.fuel)?;
    vm.heap.ensure_space(size, reg_stack, None)?;
    let new_obj = match (vm.heap.get(ha)?, vm.heap.get(hb)?) {
        (Obj::Str(sa), Obj::Str(sb)) => {
            let mut s = String::with_capacity(sa.len() + sb.len());
            s.push_str(sa);
            s.push_str(sb);
            Obj::Str(s)
        }
        (Obj::Bytes(ba), Obj::Bytes(bb)) => {
            let mut v = Vec::with_capacity(ba.len() + bb.len());
            v.extend_from_slice(ba);
            v.extend_from_slice(bb);
            Obj::Bytes(v)
        }
        (Obj::Array(va), Obj::Array(vb)) => {
            let mut v = Vec::with_capacity(va.len() + vb.len());
            v.extend_from_slice(va);
            v.extend_from_slice(vb);
            Obj::Array(v)
        }
        _ => return Err(Trap::TypeError),
    };
    let new_h = vm.heap.alloc_with_size(new_obj, size)?;
    vm.fuel_used += cost;
    set_reg(reg_stack, bp, d, Value::Ref(new_h))
}
