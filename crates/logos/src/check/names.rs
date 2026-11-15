//! Name resolution, type/layer/function/constant tables, and constant folding.
use super::{ConstEntry, Env, FuncEntry, VariantEntry, MAX_DEPTH};
use crate::ast::{
    BinOp, ConstDef, Expr, FnBody, FnDef, Ident, Item, Module, Type, TypeBody, TypeDef, UnOp,
};
use crate::ir::{Builtin, FuncId, Lit, Ty, TypeId, TypeInfo, TypeKind};
use crate::span::{Diag, Span};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub(super) const BUILTIN_LAYERS: [&str; 7] = ["io", "fs", "net", "time", "rand", "chain", "sys"];
const MAX_CONST_STR_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_CONST_STR_BYTES: usize = 16 * 1024 * 1024;
type ConstRes = Result<(Lit, Ty), Diag>;

pub(super) fn resolve_builtin(name: &str) -> Option<Builtin> {
    Some(match name {
        "len" => Builtin::Len,
        "push" => Builtin::Push,
        "pop" => Builtin::Pop,
        "str" => Builtin::IntToStr,
        "parse_int" => Builtin::ParseInt,
        "bytes" => Builtin::Bytes,
        "text" => Builtin::Text,
        "int" => Builtin::ToInt,
        "byte" => Builtin::ToByte,
        "min" => Builtin::Min,
        "max" => Builtin::Max,
        "abs" => Builtin::Abs,
        _ => return None,
    })
}

pub(super) fn diag(span: Span, msg: impl Into<String>) -> Diag {
    Diag {
        span,
        message: msg.into(),
    }
}

pub(super) fn err<T>(span: Span, msg: impl Into<String>) -> Result<T, Diag> {
    Err(diag(span, msg))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstState {
    Unvisited,
    Visiting,
    Done,
}

impl Env {
    pub(super) fn collect(m: &Module) -> Result<Self, Diag> {
        let mut env = Self {
            layers: BUILTIN_LAYERS.iter().map(|s| String::from(*s)).collect(),
            types: Vec::new(),
            type_names: Vec::new(),
            type_map: BTreeMap::new(),
            variants: Vec::new(),
            variant_map: BTreeMap::new(),
            consts: Vec::new(),
            const_map: BTreeMap::new(),
            funcs: Vec::new(),
            func_map: BTreeMap::new(),
            const_str_bytes: 0,
            main: None,
        };
        let (mut type_defs, mut const_defs, mut fn_defs) = (Vec::new(), Vec::new(), Vec::new());
        for item in &m.items {
            match item {
                Item::Layer(l) => {
                    if env.layers.iter().any(|e| e == &l.name.name) {
                        return err(l.name.span, format!("duplicate layer '{}'", l.name.name));
                    }
                    if env.layers.len() >= 64 {
                        return err(l.span, "too many layers (maximum is 64)");
                    }
                    env.layers.push(l.name.name.clone());
                }
                Item::Type(t) => {
                    if env.find_type(&t.name.name).is_some() {
                        return err(t.name.span, format!("duplicate type '{}'", t.name.name));
                    }
                    let id = env.types.len() as TypeId;
                    env.type_names.push((t.name.name.clone(), id));
                    env.type_map.insert(t.name.name.clone(), id);
                    let kind = TypeKind::Record(Vec::new());
                    env.types.push(TypeInfo {
                        name: t.name.name.clone(),
                        kind,
                    });
                    type_defs.push(t);
                }
                Item::Const(c) => {
                    env.check_top_value_name(&c.name)?;
                    let name = c.name.name.clone();
                    let idx = env.consts.len();
                    env.const_map.insert(name, idx);
                    env.consts.push(ConstEntry {
                        ty: Ty::Unit,
                        lit: Lit::Unit,
                    });
                    const_defs.push(c);
                }
                Item::Fn(f) => {
                    env.check_top_value_name(&f.name)?;
                    let (id, is_extern) = (
                        env.funcs.len() as FuncId,
                        matches!(f.body, FnBody::Extern { .. }),
                    );
                    let name = f.name.name.clone();
                    env.func_map.insert(name.clone(), id as usize);
                    env.funcs.push(FuncEntry {
                        name,
                        id,
                        params: Vec::new(),
                        ret: Ty::Unit,
                        layers: 0,
                        is_extern,
                    });
                    fn_defs.push(f);
                }
                Item::Test(_) | Item::Prop(_) => {}
            }
        }
        env.resolve_all_types(&type_defs)?;
        env.resolve_all_funcs(&fn_defs)?;
        let mut states = alloc::vec![ConstState::Unvisited; const_defs.len()];
        for i in 0..const_defs.len() {
            env.resolve_const_at(i, &const_defs, &mut states, 0)?;
        }
        Ok(env)
    }

    fn resolve_all_types(&mut self, type_defs: &[&TypeDef]) -> Result<(), Diag> {
        for (i, t) in type_defs.iter().enumerate() {
            let tid = i as TypeId;
            let kind = match &t.body {
                TypeBody::Record(fields) => {
                    let (mut seen_fields, mut out) =
                        (BTreeSet::new(), Vec::with_capacity(fields.len()));
                    for f in fields {
                        if !seen_fields.insert(f.name.name.as_str()) {
                            return err(f.name.span, format!("duplicate field '{}'", f.name.name));
                        }
                        out.push((f.name.name.clone(), self.resolve_ty(&f.ty, 0)?));
                    }
                    TypeKind::Record(out)
                }
                TypeBody::Variants(vars) => {
                    let mut out = Vec::with_capacity(vars.len());
                    for (tag, v) in vars.iter().enumerate() {
                        if self.find_variant(&v.name.name).is_some() {
                            return err(
                                v.name.span,
                                format!("duplicate variant '{}'", v.name.name),
                            );
                        }
                        let mut f_tys = Vec::with_capacity(v.fields.len());
                        for fty in &v.fields {
                            f_tys.push(self.resolve_ty(fty, 0)?);
                        }
                        let name = v.name.name.clone();
                        let ve = VariantEntry {
                            type_id: tid,
                            tag: tag as u32,
                            fields: f_tys.clone(),
                        };
                        let v_idx = self.variants.len();
                        self.variant_map.insert(name.clone(), v_idx);
                        self.variants.push(ve);
                        out.push((name, f_tys));
                    }
                    TypeKind::Variants(out)
                }
            };
            self.types[i].kind = kind;
        }
        Ok(())
    }

    fn resolve_all_funcs(&mut self, fn_defs: &[&FnDef]) -> Result<(), Diag> {
        for (i, f) in fn_defs.iter().enumerate() {
            let (mut seen, mut params) = (BTreeSet::new(), Vec::with_capacity(f.params.len()));
            for p in &f.params {
                if p.name.name != "_" && !seen.insert(p.name.name.as_str()) {
                    return err(
                        p.name.span,
                        format!("duplicate parameter '{}'", p.name.name),
                    );
                }
                params.push(self.resolve_ty(&p.ty, 0)?);
            }
            let ret = f
                .ret
                .as_ref()
                .map(|t| self.resolve_ty(t, 0))
                .transpose()?
                .unwrap_or(Ty::Unit);
            let layers = self.resolve_uses(&f.uses)?;
            if f.name.name == "main" {
                if !params.is_empty() {
                    return err(f.span, "'main' must not have parameters");
                }
                self.main = Some(i as FuncId);
            }
            self.funcs[i].params = params;
            self.funcs[i].ret = ret;
            self.funcs[i].layers = layers;
        }
        Ok(())
    }

    fn check_top_value_name(&self, id: &Ident) -> Result<(), Diag> {
        if resolve_builtin(&id.name).is_some() {
            return err(
                id.span,
                format!("'{}' conflicts with a built-in function", id.name),
            );
        }
        if self.find_const(&id.name).is_some() || self.find_func(&id.name).is_some() {
            return err(id.span, format!("duplicate definition '{}'", id.name));
        }
        Ok(())
    }

    pub(super) fn resolve_ty(&self, ty: &Type, depth: u32) -> Result<Ty, Diag> {
        if depth > MAX_DEPTH {
            return err(type_span(ty), "type nesting is too deep");
        }
        match ty {
            Type::Int { .. } => Ok(Ty::Int),
            Type::Bool { .. } => Ok(Ty::Bool),
            Type::Byte { .. } => Ok(Ty::Byte),
            Type::Str { .. } => Ok(Ty::Str),
            Type::Unit { .. } => Ok(Ty::Unit),
            Type::Named(id) => self
                .find_type(&id.name)
                .map(Ty::Named)
                .ok_or_else(|| diag(id.span, format!("unknown type '{}'", id.name))),
            Type::List { elem, .. } => Ok(Ty::Array(Box::new(self.resolve_ty(elem, depth + 1)?))),
            Type::Option { inner, .. } => Ok(Ty::Opt(Box::new(self.resolve_ty(inner, depth + 1)?))),
            Type::Tuple { items, .. } => {
                let mut out = Vec::with_capacity(items.len());
                for e in items {
                    out.push(self.resolve_ty(e, depth + 1)?);
                }
                Ok(Ty::Tuple(out))
            }
            Type::Fn {
                params, ret, uses, ..
            } => {
                let mut p_tys = Vec::with_capacity(params.len());
                for p in params {
                    p_tys.push(self.resolve_ty(p, depth + 1)?);
                }
                let r_ty = Box::new(self.resolve_ty(ret, depth + 1)?);
                Ok(Ty::Fn {
                    params: p_tys,
                    ret: r_ty,
                    layers: self.resolve_uses(uses)?,
                })
            }
        }
    }

    pub(super) fn resolve_uses(&self, uses: &[Ident]) -> Result<u64, Diag> {
        let mut mask = 0u64;
        for u in uses {
            let Some(bit) = self.layers.iter().position(|l| l == &u.name) else {
                return err(u.span, format!("unknown layer '{}'", u.name));
            };
            let flag = 1u64 << bit;
            if (mask & flag) != 0 {
                return err(u.span, format!("duplicate layer '{}'", u.name));
            }
            mask |= flag;
        }
        Ok(mask)
    }

    fn resolve_const_at(
        &mut self,
        i: usize,
        defs: &[&ConstDef],
        st: &mut [ConstState],
        d: u32,
    ) -> ConstRes {
        if d > MAX_DEPTH {
            return err(defs[i].span, "constant nesting is too deep");
        }
        match st[i] {
            ConstState::Done => return Ok((self.consts[i].lit.clone(), self.consts[i].ty.clone())),
            ConstState::Visiting => {
                return err(
                    defs[i].span,
                    format!("cycle in constant '{}'", defs[i].name.name),
                )
            }
            ConstState::Unvisited => st[i] = ConstState::Visiting,
        }
        let exp_ty = self.resolve_ty(&defs[i].ty, d + 1)?;
        let (lit, act_ty) = self.eval_const_expr(&defs[i].value, defs, st, d + 1)?;
        if act_ty != exp_ty {
            let n = &defs[i].name.name;
            return err(
                defs[i].span,
                format!("type mismatch in constant '{n}': expected {exp_ty:?}, found {act_ty:?}"),
            );
        }
        self.consts[i].ty = exp_ty.clone();
        self.consts[i].lit = lit.clone();
        st[i] = ConstState::Done;
        Ok((lit, exp_ty))
    }

    fn eval_const_expr(
        &mut self,
        e: &Expr,
        defs: &[&ConstDef],
        st: &mut [ConstState],
        d: u32,
    ) -> ConstRes {
        if d > MAX_DEPTH {
            return err(expr_span(e), "constant expression nesting is too deep");
        }
        match e {
            Expr::Int { value, .. } => Ok((Lit::Int(*value), Ty::Int)),
            Expr::Byte { value, .. } => Ok((Lit::Byte(*value), Ty::Byte)),
            Expr::Bool { value, .. } => Ok((Lit::Bool(*value), Ty::Bool)),
            Expr::Str { value, .. } => Ok((Lit::Str(value.clone()), Ty::Str)),
            Expr::Unit { .. } => Ok((Lit::Unit, Ty::Unit)),
            Expr::Name(id) => {
                let Some(&c_idx) = self.const_map.get(&id.name) else {
                    return err(
                        id.span,
                        format!("'{}' is not a constant in constant expression", id.name),
                    );
                };
                self.resolve_const_at(c_idx, defs, st, d + 1)
            }
            Expr::Unary { op, operand, span } => {
                let (v, ty) = self.eval_const_expr(operand, defs, st, d + 1)?;
                match (op, v) {
                    (UnOp::Neg, Lit::Int(n)) => n
                        .checked_neg()
                        .map(|r| (Lit::Int(r), Ty::Int))
                        .ok_or_else(|| diag(*span, "constant arithmetic overflow")),
                    (UnOp::Not, Lit::Bool(b)) => Ok((Lit::Bool(!b), Ty::Bool)),
                    _ => err(*span, format!("invalid unary operator {op:?} on {ty:?}")),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => {
                let (l, lty) = self.eval_const_expr(lhs, defs, st, d + 1)?;
                let (r, rty) = self.eval_const_expr(rhs, defs, st, d + 1)?;
                eval_const_bin(*op, l, lty, r, rty, *span, &mut self.const_str_bytes)
            }
            _ => err(
                expr_span(e),
                "constant initialiser must be a literal or constant arithmetic",
            ),
        }
    }
}

fn eval_const_bin(
    op: BinOp,
    l: Lit,
    lty: Ty,
    r: Lit,
    rty: Ty,
    span: Span,
    const_str_bytes: &mut usize,
) -> ConstRes {
    if lty != rty {
        return err(span, "type mismatch in constant binary expression");
    }
    let boolean = |b: bool| Ok((Lit::Bool(b), Ty::Bool));
    match (l, r) {
        (Lit::Int(a), Lit::Int(b)) => eval_const_int(op, a, b, span),
        (Lit::Byte(a), Lit::Byte(b)) => eval_const_byte(op, a, b, span),
        (Lit::Bool(a), Lit::Bool(b)) => match op {
            BinOp::And => boolean(a && b),
            BinOp::Or => boolean(a || b),
            BinOp::Eq => boolean(a == b),
            BinOp::Ne => boolean(a != b),
            _ => err(span, "invalid operator on bool in constant"),
        },
        (Lit::Str(a), Lit::Str(b)) => match op {
            BinOp::Add => {
                let new_len = a.len().saturating_add(b.len());
                if new_len > MAX_CONST_STR_BYTES
                    || const_str_bytes.saturating_add(new_len) > MAX_TOTAL_CONST_STR_BYTES
                {
                    return err(span, "constant string exceeds size limit");
                }
                *const_str_bytes += new_len;
                Ok((Lit::Str(format!("{a}{b}")), Ty::Str))
            }
            BinOp::Eq => boolean(a == b),
            BinOp::Ne => boolean(a != b),
            BinOp::Lt => boolean(a < b),
            BinOp::Le => boolean(a <= b),
            BinOp::Gt => boolean(a > b),
            BinOp::Ge => boolean(a >= b),
            _ => err(span, "invalid operator on str in constant"),
        },
        (Lit::Unit, Lit::Unit) => match op {
            BinOp::Eq => boolean(true),
            BinOp::Ne => boolean(false),
            _ => err(span, "invalid operator on unit in constant"),
        },
        _ => err(span, "invalid constant binary expression"),
    }
}

fn eval_const_int(op: BinOp, a: i64, b: i64, span: Span) -> ConstRes {
    let int = |v: Option<i64>| {
        v.map(|n| (Lit::Int(n), Ty::Int))
            .ok_or_else(|| diag(span, "constant arithmetic overflow"))
    };
    let boolean = |v: bool| Ok((Lit::Bool(v), Ty::Bool));
    match op {
        BinOp::Add => int(a.checked_add(b)),
        BinOp::Sub => int(a.checked_sub(b)),
        BinOp::Mul => int(a.checked_mul(b)),
        BinOp::Div if b == 0 => err(span, "division by zero in constant"),
        BinOp::Div => int(a.checked_div(b)),
        BinOp::Rem if b == 0 => err(span, "division by zero in constant"),
        BinOp::Rem => int(a.checked_rem(b)),
        BinOp::AddWrap => int(Some(a.wrapping_add(b))),
        BinOp::SubWrap => int(Some(a.wrapping_sub(b))),
        BinOp::MulWrap => int(Some(a.wrapping_mul(b))),
        BinOp::BitOr => int(Some(a | b)),
        BinOp::BitXor => int(Some(a ^ b)),
        BinOp::BitAnd => int(Some(a & b)),
        BinOp::Shl if (0..=63).contains(&b) => int(Some(a.wrapping_shl(b as u32))),
        BinOp::Shr if (0..=63).contains(&b) => int(Some(a.wrapping_shr(b as u32))),
        BinOp::Shl | BinOp::Shr => err(span, "shift count out of range in constant"),
        BinOp::Eq => boolean(a == b),
        BinOp::Ne => boolean(a != b),
        BinOp::Lt => boolean(a < b),
        BinOp::Le => boolean(a <= b),
        BinOp::Gt => boolean(a > b),
        BinOp::Ge => boolean(a >= b),
        BinOp::And | BinOp::Or => err(span, "logical operator requires bool operands"),
    }
}

fn eval_const_byte(op: BinOp, a: u8, b: u8, span: Span) -> ConstRes {
    let byte = |v: Option<u8>| {
        v.map(|n| (Lit::Byte(n), Ty::Byte))
            .ok_or_else(|| diag(span, "constant byte arithmetic overflow"))
    };
    let boolean = |v: bool| Ok((Lit::Bool(v), Ty::Bool));
    match op {
        BinOp::Add => byte(a.checked_add(b)),
        BinOp::Sub => byte(a.checked_sub(b)),
        BinOp::Mul => byte(a.checked_mul(b)),
        BinOp::Div | BinOp::Rem if b == 0 => err(span, "division by zero in constant"),
        BinOp::Div => byte(Some(a / b)),
        BinOp::Rem => byte(Some(a % b)),
        BinOp::Eq => boolean(a == b),
        BinOp::Ne => boolean(a != b),
        BinOp::Lt => boolean(a < b),
        BinOp::Le => boolean(a <= b),
        BinOp::Gt => boolean(a > b),
        BinOp::Ge => boolean(a >= b),
        _ => err(span, "invalid operator on byte in constant"),
    }
}

pub(super) fn type_span(ty: &Type) -> Span {
    match ty {
        Type::Int { span } | Type::Bool { span } | Type::Byte { span } => *span,
        Type::Str { span } | Type::Unit { span } | Type::List { span, .. } => *span,
        Type::Tuple { span, .. } | Type::Option { span, .. } | Type::Fn { span, .. } => *span,
        Type::Named(id) => id.span,
    }
}

pub(super) fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Int { span, .. } | Expr::Byte { span, .. } | Expr::Str { span, .. } => *span,
        Expr::Bool { span, .. } | Expr::None { span } | Expr::Some { span, .. } => *span,
        Expr::Unit { span } | Expr::Range { span, .. } | Expr::Variant { span, .. } => *span,
        Expr::Record { span, .. } | Expr::List { span, .. } | Expr::Repeat { span, .. } => *span,
        Expr::Tuple { span, .. } | Expr::Unary { span, .. } | Expr::Binary { span, .. } => *span,
        Expr::Pipe { span, .. } | Expr::Call { span, .. } | Expr::Index { span, .. } => *span,
        Expr::Field { span, .. } | Expr::TupleField { span, .. } => *span,
        Expr::If { span, .. } | Expr::Match { span, .. } => *span,
        Expr::Name(id) => id.span,
        Expr::Block(b) => b.span,
    }
}
