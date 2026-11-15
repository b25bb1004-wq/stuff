//! Validation of Mica IR programs against the RV64 integer subset.

use super::CodegenError;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use mica::ast::BinOp;
use mica::ir::{Body, Func, FuncId, Inst, Label, Lit, Program, Reg, Ty};

/// Maximum number of virtual registers allowed per function frame.
const MAX_REGS_PER_FUNC: usize = 131_072;

/// Validate `p` and resolve `entry`, returning its [`FuncId`].
pub(super) fn validate_program(p: &Program, entry: &str) -> Result<FuncId, CodegenError> {
    let entry_id = find_entry(p, entry)?;
    for (idx, func) in p.funcs.iter().enumerate() {
        validate_func(p, idx, func)?;
    }
    Ok(entry_id)
}

fn find_entry(p: &Program, entry: &str) -> Result<FuncId, CodegenError> {
    if let Some(mid) = p.main {
        if let Some(f) = p.funcs.get(mid as usize) {
            if f.name == entry && f.params == 0 && f.ret == Ty::Int {
                return Ok(mid);
            }
        }
    }
    for (idx, f) in p.funcs.iter().enumerate() {
        if f.name == entry {
            if f.params == 0 && f.ret == Ty::Int {
                return u32::try_from(idx).map_err(|_| CodegenError::NoEntry(String::from(entry)));
            }
            return Err(CodegenError::NoEntry(String::from(entry)));
        }
    }
    Err(CodegenError::NoEntry(String::from(entry)))
}

fn validate_func(p: &Program, fid: usize, f: &Func) -> Result<(), CodegenError> {
    if let Body::Extern(ext) = &f.body {
        return Err(CodegenError::Unsupported(format!(
            "extern function '{ext}' in func #{fid}"
        )));
    }
    if f.params > 8 {
        return Err(CodegenError::Unsupported(format!(
            "function '{}' has {} parameters (max 8)",
            f.name, f.params
        )));
    }
    let params = f.params as usize;
    if params > f.regs.len() {
        return Err(CodegenError::Unsupported(format!(
            "function '{}' has fewer registers than parameters",
            f.name
        )));
    }
    if f.regs.len() > MAX_REGS_PER_FUNC {
        return Err(CodegenError::Unsupported(format!(
            "function '{}' exceeds max register count",
            f.name
        )));
    }
    for (i, ty) in f.regs.iter().enumerate() {
        if i < params {
            validate_param_ty(ty)?;
        } else {
            validate_local_ty(ty)?;
        }
    }
    validate_local_ty(&f.ret)?;

    match &f.body {
        Body::Extern(_) => Ok(()),
        Body::Code { insts, .. } => validate_body(p, f.regs.len(), insts),
    }
}

fn validate_param_ty(ty: &Ty) -> Result<(), CodegenError> {
    match ty {
        Ty::Int | Ty::Bool => Ok(()),
        other => Err(CodegenError::Unsupported(format!(
            "parameter type {}",
            ty_name(other)
        ))),
    }
}

fn validate_local_ty(ty: &Ty) -> Result<(), CodegenError> {
    match ty {
        Ty::Int | Ty::Bool | Ty::Unit => Ok(()),
        other => Err(CodegenError::Unsupported(format!(
            "type {}",
            ty_name(other)
        ))),
    }
}

fn validate_body(p: &Program, num_regs: usize, insts: &[Inst]) -> Result<(), CodegenError> {
    let mut labels = BTreeSet::new();
    for inst in insts {
        if let Inst::Label(lbl) = inst {
            if !labels.insert(*lbl) {
                return Err(CodegenError::Unsupported(format!("duplicate label {lbl}")));
            }
        }
    }
    for inst in insts {
        validate_inst(p, num_regs, &labels, inst)?;
    }
    Ok(())
}

fn validate_inst(
    p: &Program,
    num_regs: usize,
    labels: &BTreeSet<Label>,
    inst: &Inst,
) -> Result<(), CodegenError> {
    match inst {
        Inst::Lit(d, lit) => {
            check_reg(*d, num_regs)?;
            match lit {
                Lit::Int(_) | Lit::Bool(_) | Lit::Unit => Ok(()),
                Lit::Byte(_) => Err(CodegenError::Unsupported(String::from("Lit::Byte"))),
                Lit::Str(_) => Err(CodegenError::Unsupported(String::from("Lit::Str"))),
                Lit::Fn(_) => Err(CodegenError::Unsupported(String::from("Lit::Fn"))),
            }
        }
        Inst::Move(d, s) | Inst::Un(d, _, s) => {
            check_reg(*d, num_regs)?;
            check_reg(*s, num_regs)
        }
        Inst::Bin(d, op, a, b) => {
            check_reg(*d, num_regs)?;
            check_reg(*a, num_regs)?;
            check_reg(*b, num_regs)?;
            validate_binop(*op)
        }
        Inst::Call(d, callee, args) => {
            check_reg(*d, num_regs)?;
            let target = p
                .funcs
                .get(*callee as usize)
                .ok_or_else(|| CodegenError::Unsupported(format!("unknown FuncId {callee}")))?;
            if args.len() != target.params as usize {
                return Err(CodegenError::Unsupported(format!(
                    "call arity mismatch for '{}'",
                    target.name
                )));
            }
            for &arg in args {
                check_reg(arg, num_regs)?;
            }
            Ok(())
        }
        Inst::Label(_) => Ok(()),
        Inst::Jump(lbl) => check_label(*lbl, labels),
        Inst::Branch(cond, t_lbl, f_lbl) => {
            check_reg(*cond, num_regs)?;
            check_label(*t_lbl, labels)?;
            check_label(*f_lbl, labels)
        }
        Inst::Ret(r) => check_reg(*r, num_regs),
        Inst::Assert(cond, _) => check_reg(*cond, num_regs),
        other => Err(CodegenError::Unsupported(format!(
            "instruction {}",
            inst_name(other)
        ))),
    }
}

fn validate_binop(op: BinOp) -> Result<(), CodegenError> {
    match op {
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or => Ok(()),
        other => Err(CodegenError::Unsupported(format!("BinOp::{other:?}"))),
    }
}

fn check_reg(r: Reg, num_regs: usize) -> Result<(), CodegenError> {
    if (r as usize) >= num_regs {
        return Err(CodegenError::Unsupported(format!(
            "register r{r} out of bounds"
        )));
    }
    Ok(())
}

fn check_label(lbl: Label, labels: &BTreeSet<Label>) -> Result<(), CodegenError> {
    if !labels.contains(&lbl) {
        return Err(CodegenError::Unsupported(format!("undefined label {lbl}")));
    }
    Ok(())
}

fn ty_name(ty: &Ty) -> &'static str {
    match ty {
        Ty::Int => "Int",
        Ty::Byte => "Byte",
        Ty::Bool => "Bool",
        Ty::Str => "Str",
        Ty::Unit => "Unit",
        Ty::Array(_) => "Array",
        Ty::Tuple(_) => "Tuple",
        Ty::Opt(_) => "Opt",
        Ty::Named(_) => "Named",
        Ty::Fn { .. } => "Fn",
    }
}

fn inst_name(inst: &Inst) -> &'static str {
    match inst {
        Inst::Lit(..) => "Lit",
        Inst::Move(..) => "Move",
        Inst::Un(..) => "Un",
        Inst::Bin(..) => "Bin",
        Inst::Call(..) => "Call",
        Inst::CallValue(..) => "CallValue",
        Inst::Builtin(..) => "Builtin",
        Inst::NewArray(..) => "NewArray",
        Inst::NewFill(..) => "NewFill",
        Inst::NewTuple(..) => "NewTuple",
        Inst::NewRecord(..) => "NewRecord",
        Inst::NewVariant(..) => "NewVariant",
        Inst::NewSome(..) => "NewSome",
        Inst::NewNone(..) => "NewNone",
        Inst::GetField(..) => "GetField",
        Inst::SetField(..) => "SetField",
        Inst::Tag(..) => "Tag",
        Inst::GetIndex(..) => "GetIndex",
        Inst::SetIndex(..) => "SetIndex",
        Inst::Label(..) => "Label",
        Inst::Jump(..) => "Jump",
        Inst::Branch(..) => "Branch",
        Inst::Ret(..) => "Ret",
        Inst::Assert(..) => "Assert",
    }
}
