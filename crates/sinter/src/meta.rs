//! Compact binary encoding of nominal types and function signatures in `Const::Bytes`.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use mica::ir::{Program, Ty, TypeInfo, TypeKind};

const MAGIC: &[u8] = b"SINTER_META1";

pub(crate) type FuncSig = (Vec<Ty>, Ty, u64);

pub(crate) fn encode_meta(p: &Program) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, p.types.len() as u32);
    for t in &p.types {
        put_str(&mut out, &t.name);
        match &t.kind {
            TypeKind::Record(fields) => {
                out.push(0);
                put_u32(&mut out, fields.len() as u32);
                for (fname, fty) in fields {
                    put_str(&mut out, fname);
                    put_ty(&mut out, fty);
                }
            }
            TypeKind::Variants(vars) => {
                out.push(1);
                put_u32(&mut out, vars.len() as u32);
                for (vname, vtys) in vars {
                    put_str(&mut out, vname);
                    put_u32(&mut out, vtys.len() as u32);
                    for vty in vtys {
                        put_ty(&mut out, vty);
                    }
                }
            }
        }
    }
    put_u32(&mut out, p.funcs.len() as u32);
    for f in &p.funcs {
        let params = f.regs.get(..f.params as usize).unwrap_or(&[]);
        put_u32(&mut out, params.len() as u32);
        for pt in params {
            put_ty(&mut out, pt);
        }
        put_ty(&mut out, &f.ret);
        out.extend_from_slice(&f.layers.to_le_bytes());
    }
    out
}

pub(crate) fn decode_meta(consts: &[geode::Const]) -> (Vec<TypeInfo>, Vec<FuncSig>) {
    for c in consts {
        if let geode::Const::Bytes(b) = c {
            if let Some(rest) = b.strip_prefix(MAGIC) {
                if let Some(parsed) = parse_meta(rest) {
                    return parsed;
                }
            }
        }
    }
    (Vec::new(), Vec::new())
}

fn parse_meta(mut s: &[u8]) -> Option<(Vec<TypeInfo>, Vec<FuncSig>)> {
    let n_types = get_u32(&mut s)? as usize;
    let mut types = Vec::with_capacity(n_types);
    for _ in 0..n_types {
        let name = get_str(&mut s)?;
        let tag = get_u8(&mut s)?;
        let kind = if tag == 0 {
            let nf = get_u32(&mut s)? as usize;
            let mut fields = Vec::with_capacity(nf);
            for _ in 0..nf {
                fields.push((get_str(&mut s)?, get_ty(&mut s)?));
            }
            TypeKind::Record(fields)
        } else {
            let nv = get_u32(&mut s)? as usize;
            let mut vars = Vec::with_capacity(nv);
            for _ in 0..nv {
                let vname = get_str(&mut s)?;
                let np = get_u32(&mut s)? as usize;
                let mut vtys = Vec::with_capacity(np);
                for _ in 0..np {
                    vtys.push(get_ty(&mut s)?);
                }
                vars.push((vname, vtys));
            }
            TypeKind::Variants(vars)
        };
        types.push(TypeInfo { name, kind });
    }
    let n_funcs = get_u32(&mut s)? as usize;
    let mut funcs = Vec::with_capacity(n_funcs);
    for _ in 0..n_funcs {
        let np = get_u32(&mut s)? as usize;
        let mut params = Vec::with_capacity(np);
        for _ in 0..np {
            params.push(get_ty(&mut s)?);
        }
        let ret = get_ty(&mut s)?;
        let layers = get_u64(&mut s)?;
        funcs.push((params, ret, layers));
    }
    Some((types, funcs))
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

fn put_ty(out: &mut Vec<u8>, ty: &Ty) {
    match ty {
        Ty::Int => out.push(0),
        Ty::Byte => out.push(1),
        Ty::Bool => out.push(2),
        Ty::Str => out.push(3),
        Ty::Unit => out.push(4),
        Ty::Array(el) => {
            out.push(5);
            put_ty(out, el);
        }
        Ty::Tuple(items) => {
            out.push(6);
            put_u32(out, items.len() as u32);
            for it in items {
                put_ty(out, it);
            }
        }
        Ty::Opt(inner) => {
            out.push(7);
            put_ty(out, inner);
        }
        Ty::Named(tid) => {
            out.push(8);
            put_u32(out, *tid);
        }
        Ty::Fn {
            params,
            ret,
            layers,
        } => {
            out.push(9);
            put_u32(out, params.len() as u32);
            for p in params {
                put_ty(out, p);
            }
            put_ty(out, ret);
            out.extend_from_slice(&layers.to_le_bytes());
        }
    }
}

fn get_u8(s: &mut &[u8]) -> Option<u8> {
    let (&b, rest) = s.split_first()?;
    *s = rest;
    Some(b)
}

fn get_u32(s: &mut &[u8]) -> Option<u32> {
    if s.len() < 4 {
        return None;
    }
    let (head, rest) = s.split_at(4);
    *s = rest;
    Some(u32::from_le_bytes([head[0], head[1], head[2], head[3]]))
}

fn get_u64(s: &mut &[u8]) -> Option<u64> {
    if s.len() < 8 {
        return None;
    }
    let (head, rest) = s.split_at(8);
    *s = rest;
    let arr: [u8; 8] = head.try_into().ok()?;
    Some(u64::from_le_bytes(arr))
}

fn get_str(s: &mut &[u8]) -> Option<String> {
    let len = get_u32(s)? as usize;
    if s.len() < len {
        return None;
    }
    let (head, rest) = s.split_at(len);
    *s = rest;
    core::str::from_utf8(head).ok().map(String::from)
}

fn get_ty(s: &mut &[u8]) -> Option<Ty> {
    match get_u8(s)? {
        0 => Some(Ty::Int),
        1 => Some(Ty::Byte),
        2 => Some(Ty::Bool),
        3 => Some(Ty::Str),
        4 => Some(Ty::Unit),
        5 => Some(Ty::Array(Box::new(get_ty(s)?))),
        6 => {
            let n = get_u32(s)? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(get_ty(s)?);
            }
            Some(Ty::Tuple(items))
        }
        7 => Some(Ty::Opt(Box::new(get_ty(s)?))),
        8 => Some(Ty::Named(get_u32(s)?)),
        9 => {
            let n = get_u32(s)? as usize;
            let mut params = Vec::with_capacity(n);
            for _ in 0..n {
                params.push(get_ty(s)?);
            }
            let ret = Box::new(get_ty(s)?);
            let layers = get_u64(s)?;
            Some(Ty::Fn {
                params,
                ret,
                layers,
            })
        }
        _ => None,
    }
}
