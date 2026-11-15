//! Host-testable language service for Logos: go to definition, hover, and formatting.

use std::collections::BTreeMap;

use logos::ast::{FnBody, Item, Module, Param, TypeBody};
use logos::Span;

use crate::lang_fmt::{fmt_fn_sig, fmt_type, fmt_type_header, fmt_variant, has_comment};
use crate::lang_walk::{visit_block, visit_expr, visit_type};

#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) def_span: Option<(usize, usize)>,
    pub(crate) hover: String,
}

#[derive(Clone)]
pub(crate) struct TypeEntry {
    pub(crate) entry: Entry,
    pub(crate) fields: BTreeMap<String, Entry>,
}

pub(crate) struct TopEnv {
    pub(crate) types: BTreeMap<String, TypeEntry>,
    pub(crate) variants: BTreeMap<String, Entry>,
    pub(crate) layers: BTreeMap<String, Entry>,
    pub(crate) values: BTreeMap<String, Entry>,
}

pub(crate) struct Resolved {
    pub(crate) def_span: Option<(usize, usize)>,
    pub(crate) hover: Option<String>,
}

/// Return the byte range `(start, end)` of the declaration of the identifier at `offset`.
pub fn definition(src: &str, offset: usize) -> Option<(usize, usize)> {
    resolve_at(src, offset)?.def_span
}

/// Return hover text for the identifier or literal at `offset`.
pub fn hover(src: &str, offset: usize) -> Option<String> {
    resolve_at(src, offset)?.hover
}

/// Format Logos source `src` using the canonical printer, refusing sources with comments.
pub fn format(src: &str) -> Result<String, String> {
    let module = logos::parser::parse(src).map_err(|d| d.message)?;
    if has_comment(src) {
        return Err(String::from("format would drop comments"));
    }
    Ok(logos::print::print(&module))
}

pub(crate) fn span_contains(span: Span, offset: usize) -> bool {
    (span.start as usize) <= offset && offset < (span.end as usize)
}

pub(crate) fn span_range(span: Span) -> (usize, usize) {
    (span.start as usize, span.end as usize)
}

fn resolve_at(src: &str, offset: usize) -> Option<Resolved> {
    if offset >= src.len() {
        return None;
    }
    let module = logos::parser::parse(src).ok()?;
    let top = collect_top(&module);
    let mut scopes = Vec::new();
    for item in &module.items {
        if let Some(res) = visit_item(item, offset, &top, &mut scopes) {
            return Some(res);
        }
    }
    None
}

fn collect_top(module: &Module) -> TopEnv {
    let mut env = TopEnv {
        types: BTreeMap::new(),
        variants: BTreeMap::new(),
        layers: BTreeMap::new(),
        values: BTreeMap::new(),
    };
    for item in &module.items {
        match item {
            Item::Type(t) => {
                let mut fields = BTreeMap::new();
                match &t.body {
                    TypeBody::Record(fs) => {
                        for f in fs {
                            fields.entry(f.name.name.clone()).or_insert_with(|| Entry {
                                def_span: Some(span_range(f.name.span)),
                                hover: format!("field {}: {}", f.name.name, fmt_type(&f.ty)),
                            });
                        }
                    }
                    TypeBody::Variants(vs) => {
                        for v in vs {
                            env.variants
                                .entry(v.name.name.clone())
                                .or_insert_with(|| Entry {
                                    def_span: Some(span_range(v.name.span)),
                                    hover: fmt_variant(v),
                                });
                        }
                    }
                }
                env.types
                    .entry(t.name.name.clone())
                    .or_insert_with(|| TypeEntry {
                        entry: Entry {
                            def_span: Some(span_range(t.name.span)),
                            hover: fmt_type_header(t),
                        },
                        fields,
                    });
            }
            Item::Const(c) => {
                env.values
                    .entry(c.name.name.clone())
                    .or_insert_with(|| Entry {
                        def_span: Some(span_range(c.name.span)),
                        hover: format!("const {}: {}", c.name.name, fmt_type(&c.ty)),
                    });
            }
            Item::Layer(l) => {
                env.layers
                    .entry(l.name.name.clone())
                    .or_insert_with(|| Entry {
                        def_span: Some(span_range(l.name.span)),
                        hover: format!("layer {}", l.name.name),
                    });
            }
            Item::Fn(f) => {
                let def_span = match f.body {
                    FnBody::Block(_) => Some(span_range(f.name.span)),
                    FnBody::Extern { .. } => None,
                };
                env.values
                    .entry(f.name.name.clone())
                    .or_insert_with(|| Entry {
                        def_span,
                        hover: fmt_fn_sig(f),
                    });
            }
            Item::Test(_) | Item::Prop(_) => {}
        }
    }
    env
}

pub(crate) fn entry_to_resolved(entry: &Entry) -> Resolved {
    Resolved {
        def_span: entry.def_span,
        hover: Some(entry.hover.clone()),
    }
}

pub(crate) fn lookup_value(
    name: &str,
    top: &TopEnv,
    scopes: &[BTreeMap<String, Entry>],
) -> Option<Resolved> {
    for scope in scopes.iter().rev() {
        if let Some(entry) = scope.get(name) {
            return Some(entry_to_resolved(entry));
        }
    }
    top.values.get(name).map(entry_to_resolved)
}

fn bind_param(scope: &mut BTreeMap<String, Entry>, p: &Param) -> Entry {
    let entry = Entry {
        def_span: Some(span_range(p.name.span)),
        hover: format!("param {}: {}", p.name.name, fmt_type(&p.ty)),
    };
    scope.insert(p.name.name.clone(), entry.clone());
    entry
}

fn visit_item(
    item: &Item,
    offset: usize,
    top: &TopEnv,
    scopes: &mut Vec<BTreeMap<String, Entry>>,
) -> Option<Resolved> {
    match item {
        Item::Type(t) => {
            if !span_contains(t.span, offset) {
                return None;
            }
            if span_contains(t.name.span, offset) {
                return top
                    .types
                    .get(&t.name.name)
                    .map(|te| entry_to_resolved(&te.entry));
            }
            match &t.body {
                TypeBody::Record(fields) => {
                    for f in fields {
                        if span_contains(f.name.span, offset) {
                            return Some(Resolved {
                                def_span: Some(span_range(f.name.span)),
                                hover: Some(format!("field {}: {}", f.name.name, fmt_type(&f.ty))),
                            });
                        }
                        if let Some(r) = visit_type(&f.ty, offset, top) {
                            return Some(r);
                        }
                    }
                }
                TypeBody::Variants(variants) => {
                    for v in variants {
                        if span_contains(v.name.span, offset) {
                            return top.variants.get(&v.name.name).map(entry_to_resolved);
                        }
                        for ty in &v.fields {
                            if let Some(r) = visit_type(ty, offset, top) {
                                return Some(r);
                            }
                        }
                    }
                }
            }
            None
        }
        Item::Const(c) => {
            if !span_contains(c.span, offset) {
                return None;
            }
            if span_contains(c.name.span, offset) {
                return top.values.get(&c.name.name).map(entry_to_resolved);
            }
            visit_type(&c.ty, offset, top).or_else(|| visit_expr(&c.value, offset, top, scopes))
        }
        Item::Layer(l) => {
            if span_contains(l.name.span, offset) {
                return top.layers.get(&l.name.name).map(entry_to_resolved);
            }
            None
        }
        Item::Fn(f) => {
            if !span_contains(f.span, offset) {
                return None;
            }
            if span_contains(f.name.span, offset) {
                return top.values.get(&f.name.name).map(entry_to_resolved);
            }
            let mut fn_scope = BTreeMap::new();
            for p in &f.params {
                let entry = bind_param(&mut fn_scope, p);
                if span_contains(p.name.span, offset) {
                    return Some(entry_to_resolved(&entry));
                }
                if let Some(r) = visit_type(&p.ty, offset, top) {
                    return Some(r);
                }
            }
            if let Some(ret) = &f.ret {
                if let Some(r) = visit_type(ret, offset, top) {
                    return Some(r);
                }
            }
            for u in &f.uses {
                if span_contains(u.span, offset) {
                    return top.layers.get(&u.name).map(entry_to_resolved);
                }
            }
            if let FnBody::Block(block) = &f.body {
                scopes.push(fn_scope);
                let res = visit_block(block, offset, top, scopes);
                scopes.pop();
                return res;
            }
            None
        }
        Item::Test(t) => {
            if !span_contains(t.span, offset) {
                return None;
            }
            visit_block(&t.body, offset, top, scopes)
        }
        Item::Prop(p) => {
            if !span_contains(p.span, offset) {
                return None;
            }
            let mut prop_scope = BTreeMap::new();
            for param in &p.params {
                let entry = bind_param(&mut prop_scope, param);
                if span_contains(param.name.span, offset) {
                    return Some(entry_to_resolved(&entry));
                }
                if let Some(r) = visit_type(&param.ty, offset, top) {
                    return Some(r);
                }
            }
            scopes.push(prop_scope);
            let res = p
                .cond
                .as_ref()
                .and_then(|c| visit_expr(c, offset, top, scopes))
                .or_else(|| visit_block(&p.body, offset, top, scopes));
            scopes.pop();
            res
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn layer_const_variant_and_record_field_resolve() {
        let src = r#"layer io
const limit: int = 10
type Color = Red | Rgb(int, int, int)
type Pt = { x: int, y: int }

fn draw(c: Color) -> int uses io {
  var p: Pt = Pt { x: limit, y: 1 }
  let flag = true
  let b = b'a'
  match c {
    Red => p.x,
    Rgb(r, _, _) => if flag { r } else { 1 },
  }
}
"#;
        logos::parser::parse(src).unwrap();
        let io_decl = src.find("io\n").unwrap();
        let io_use = src.find("uses io").unwrap() + 5;
        assert_eq!(definition(src, io_use), Some((io_decl, io_decl + 2)));
        assert_eq!(hover(src, io_use).as_deref(), Some("layer io"));

        let limit_decl = src.find("limit").unwrap();
        let limit_use = src.rfind("limit").unwrap();
        assert_eq!(
            definition(src, limit_use),
            Some((limit_decl, limit_decl + 5))
        );
        assert_eq!(hover(src, limit_use).as_deref(), Some("const limit: int"));

        let rgb_use = src.rfind("Rgb(").unwrap();
        assert_eq!(
            hover(src, rgb_use).as_deref(),
            Some("variant Rgb(int, int, int)")
        );

        let var_p_use = src.find("p.x").unwrap();
        assert_eq!(hover(src, var_p_use).as_deref(), Some("var p: Pt"));

        let rec_field_use = src.find("x: limit").unwrap();
        let rec_field_decl = src.find("x: int").unwrap();
        assert_eq!(
            definition(src, rec_field_use),
            Some((rec_field_decl, rec_field_decl + 1))
        );
        assert_eq!(hover(src, rec_field_use).as_deref(), Some("field x: int"));

        let bool_pos = src.find("true").unwrap();
        assert_eq!(hover(src, bool_pos).as_deref(), Some("bool literal"));
        assert_eq!(definition(src, bool_pos), None);

        let byte_pos = src.find("b'a'").unwrap();
        assert_eq!(hover(src, byte_pos).as_deref(), Some("byte literal"));
    }

    #[test]
    fn comment_detection_ignores_strings_and_bytes() {
        assert!(!has_comment("fn f() -> str { \"// not a comment\" }"));
        assert!(!has_comment("fn f() -> str { \"\\\"//\\\"\" }"));
        assert!(!has_comment("fn f() -> byte { b'/' }"));
        assert!(has_comment("fn f() {} // trailing"));
        assert!(has_comment("\"ok\" // real comment"));
    }
}
