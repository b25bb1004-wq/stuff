//! Formatting and comment-detection helpers for `athena::lang`.

use std::fmt::Write;

use logos::ast::{FnDef, Type, TypeBody, TypeDef, Variant};

pub(crate) fn has_comment(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                    } else if b[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'b' if b.get(i + 1) == Some(&b'\'')
                && (i == 0 || !is_ident_byte(b[i.saturating_sub(1)])) =>
            {
                i += 2;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                    } else if b[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => return true,
            _ => i += 1,
        }
    }
    false
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

pub(crate) fn fmt_fn_sig(f: &FnDef) -> String {
    let mut out = String::new();
    let _ = write!(out, "fn {}(", f.name.name);
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}: {}", p.name.name, fmt_type(&p.ty));
    }
    out.push(')');
    if let Some(ret) = &f.ret {
        let _ = write!(out, " -> {}", fmt_type(ret));
    }
    if !f.uses.is_empty() {
        out.push_str(" uses ");
        for (i, u) in f.uses.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&u.name);
        }
    }
    out
}

pub(crate) fn fmt_type_header(t: &TypeDef) -> String {
    let mut out = format!("type {} = ", t.name.name);
    match &t.body {
        TypeBody::Record(fields) => {
            out.push_str("{ ");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{}: {}", f.name.name, fmt_type(&f.ty));
            }
            out.push_str(" }");
        }
        TypeBody::Variants(variants) => {
            for (i, v) in variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(" | ");
                }
                out.push_str(&v.name.name);
                if !v.fields.is_empty() {
                    out.push('(');
                    for (j, ty) in v.fields.iter().enumerate() {
                        if j > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(&fmt_type(ty));
                    }
                    out.push(')');
                }
            }
        }
    }
    out
}

pub(crate) fn fmt_variant(v: &Variant) -> String {
    let mut out = format!("variant {}", v.name.name);
    if !v.fields.is_empty() {
        out.push('(');
        for (i, ty) in v.fields.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&fmt_type(ty));
        }
        out.push(')');
    }
    out
}

pub(crate) fn fmt_type(t: &Type) -> String {
    match t {
        Type::Int { .. } => String::from("int"),
        Type::Bool { .. } => String::from("bool"),
        Type::Byte { .. } => String::from("byte"),
        Type::Str { .. } => String::from("str"),
        Type::Unit { .. } => String::from("unit"),
        Type::Named(id) => id.name.clone(),
        Type::List { elem, .. } => format!("[{}]", fmt_type(elem)),
        Type::Tuple { items, .. } => {
            let mut out = String::from("(");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&fmt_type(item));
            }
            out.push(')');
            out
        }
        Type::Option { inner, .. } => format!("{}?", fmt_type(inner)),
        Type::Fn {
            params, ret, uses, ..
        } => {
            let mut out = String::from("fn(");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&fmt_type(p));
            }
            let _ = write!(out, ") -> {}", fmt_type(ret));
            if !uses.is_empty() {
                out.push_str(" uses ");
                for (i, u) in uses.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&u.name);
                }
            }
            out
        }
    }
}
