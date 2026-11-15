//! Cascade: collect the declarations that match each element and keep one winner per slot.

use crate::css::{parse_declarations, parse_stylesheet, Declaration, Rule, Token, Value};
use crate::dom::{Data, Document, NodeId};
use crate::select::matcher::{sibling_table, Sib};
use crate::select::{parse_selectors, Selector};

use super::flex::{Flex, FLEX_INITIAL};
use super::values::{parse_decl, Prop, SLOTS};
use super::{Computed, INITIAL};

/// The user-agent sheet (origin 0).
const UA_SHEET: &str = "html, body, div, p, h1, h2, h3, ul, ol, li, section, header, footer, nav, \
main, article, pre, blockquote { display: block } head, style, script, title { display: none } \
body { margin: 8px } p { margin: 16px 0 } h1 { font-size: 32px; margin: 21px 0 } \
h2 { font-size: 24px; margin: 19px 0 } ul, ol { margin: 16px 0; padding-left: 40px }";

/// Recursion bound for serializing selector text.
const MAX_DEPTH: usize = 256;

/// Cascade priority, compared as a tuple: important, origin (2 style attribute, 1 author,
/// 0 user agent), specificity, sheet-order index, declaration index. Greater wins.
type Key = (bool, u8, (u32, u32, u32), usize, usize);

/// One style rule with its selectors parsed.
struct Entry {
    origin: u8,
    selectors: Vec<Selector>,
    decls: Vec<Declaration>,
}

type Winners = [Option<(Key, Prop)>; SLOTS];

/// Cascade every element of `doc` and apply inheritance (pre-order, so parents come first).
/// Returns the computed style and the flex properties of each element.
pub(crate) fn compute_all(doc: &Document) -> Vec<Option<(Computed, Flex)>> {
    let mut entries = Vec::new();
    collect(UA_SHEET, 0, &mut entries);
    for id in doc.elements() {
        if doc.name(id) == Some("style") {
            collect(&child_text(doc, id), 1, &mut entries);
        }
    }
    let sib = sibling_table(doc);
    let mut out: Vec<Option<(Computed, Flex)>> = vec![None; doc.nodes.len()];
    for id in doc.elements() {
        let winners = winners(doc, id, &entries, &sib);
        let parent = doc
            .nodes
            .get(id)
            .and_then(|n| n.parent)
            .and_then(|p| out.get(p).copied().flatten())
            .map(|(c, _)| c);
        let style = apply(&winners, parent);
        let flex = apply_flex(&winners);
        if let Some(slot) = out.get_mut(id) {
            *slot = Some((style, flex));
        }
    }
    out
}

/// Concatenate the text children of element `id` (the body of a `<style>` element).
fn child_text(doc: &Document, id: NodeId) -> String {
    let mut s = String::new();
    if let Some(node) = doc.nodes.get(id) {
        for &c in &node.children {
            if let Some(Data::Text(t)) = doc.nodes.get(c).map(|x| &x.data) {
                s.push_str(t);
            }
        }
    }
    s
}

/// Parse a sheet and append its style rules, with selectors parsed, in order.
fn collect(text: &str, origin: u8, entries: &mut Vec<Entry>) {
    for rule in parse_stylesheet(text) {
        let Rule::Style {
            prelude,
            declarations,
        } = rule
        else {
            continue;
        };
        let mut sel = String::new();
        serialize(&prelude, &mut sel, 0);
        if let Ok(selectors) = parse_selectors(&sel) {
            entries.push(Entry {
                origin,
                selectors,
                decls: declarations,
            });
        }
    }
}

/// Keep, per slot, the declaration with the highest cascade key among those that parse.
fn winners(doc: &Document, id: NodeId, entries: &[Entry], sib: &[Sib]) -> Winners {
    let mut best: Winners = [None; SLOTS];
    for (ei, entry) in entries.iter().enumerate() {
        let spec = entry
            .selectors
            .iter()
            .filter(|s| s.matches_with(doc, id, Some(sib)))
            .map(Selector::specificity)
            .max();
        let Some(spec) = spec else {
            continue;
        };
        for (di, d) in entry.decls.iter().enumerate() {
            offer(&mut best, d, (d.important, entry.origin, spec, ei, di));
        }
    }
    if let Some(attr) = doc.attr(id, "style") {
        for (di, d) in parse_declarations(attr).iter().enumerate() {
            offer(&mut best, d, (d.important, 2, (0, 0, 0), 0, di));
        }
    }
    best
}

fn offer(best: &mut Winners, d: &Declaration, key: Key) {
    for prop in parse_decl(&d.name, &d.value) {
        let Some(slot) = best.get_mut(prop.slot()) else {
            continue;
        };
        let wins = match slot {
            Some((k, _)) => key > *k,
            None => true,
        };
        if wins {
            *slot = Some((key, prop));
        }
    }
}

/// Start from the initial values, inherit `color` and `font-size`, then apply the winners.
fn apply(winners: &Winners, parent: Option<Computed>) -> Computed {
    let mut c = INITIAL;
    if let Some(p) = parent {
        c.color = p.color;
        c.font_px = p.font_px;
    }
    for (_, prop) in winners.iter().flatten() {
        match *prop {
            Prop::Display(d) => c.display = d,
            Prop::Width(l) => c.width = l,
            Prop::Height(l) => c.height = l,
            Prop::Margin(i, l) => set(&mut c.margin, i, l),
            Prop::Padding(i, p) => set(&mut c.padding, i, p),
            Prop::Border(i, b) => set(&mut c.border, i, b),
            Prop::Color(col) => c.color = col,
            Prop::Background(col) => c.background = col,
            Prop::FontPx(f) => c.font_px = f,
            Prop::FlexDir(_)
            | Prop::FlexJustify(_)
            | Prop::FlexAlign(_)
            | Prop::FlexGrow(_)
            | Prop::Gap(_) => {}
        }
    }
    c
}

/// Apply the flex winners to the initial flex values (none of them inherit).
fn apply_flex(winners: &Winners) -> Flex {
    let mut f = FLEX_INITIAL;
    for (_, prop) in winners.iter().flatten() {
        match *prop {
            Prop::FlexDir(d) => f.dir = d,
            Prop::FlexJustify(j) => f.justify = j,
            Prop::FlexAlign(a) => f.align = a,
            Prop::FlexGrow(g) => f.grow = g,
            Prop::Gap(g) => f.gap = g,
            _ => {}
        }
    }
    f
}

fn set<T>(arr: &mut [T; 4], i: usize, v: T) {
    if let Some(x) = arr.get_mut(i) {
        *x = v;
    }
}

/// Rebuild selector text from prelude values so it can be parsed by the selector parser.
fn serialize(values: &[Value], out: &mut String, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for v in values {
        match v {
            Value::Token(t) => token_text(t, out),
            Value::Block { open, items } => {
                let close = match open {
                    '[' => ']',
                    '(' => ')',
                    _ => '}',
                };
                out.push(*open);
                serialize(items, out, depth + 1);
                out.push(close);
            }
            Value::Function { name, args } => {
                out.push_str(name);
                out.push('(');
                serialize(args, out, depth + 1);
                out.push(')');
            }
        }
    }
}

fn token_text(t: &Token, out: &mut String) {
    match t {
        Token::Ident(s) => out.push_str(s),
        Token::Hash { value, .. } => {
            out.push('#');
            out.push_str(value);
        }
        Token::Str(s) => {
            out.push('"');
            out.push_str(s);
            out.push('"');
        }
        Token::Delim(c) => out.push(*c),
        Token::Number { value, int } => {
            // A plus sign keeps `2n+1` style selectors intact after the number is re-read.
            if *value > 0.0 {
                out.push('+');
            }
            out.push_str(&number(*value, *int));
        }
        Token::Dimension { value, int, unit } => {
            out.push_str(&number(*value, *int));
            out.push_str(unit);
        }
        Token::Percentage(v) => {
            out.push_str(&number(*v, false));
            out.push('%');
        }
        Token::Whitespace => out.push(' '),
        Token::Colon => out.push(':'),
        Token::Comma => out.push(','),
        Token::OpenSquare => out.push('['),
        Token::CloseSquare => out.push(']'),
        _ => {}
    }
}

fn number(v: f64, int: bool) -> String {
    if int {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
