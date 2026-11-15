//! CSS Selectors Level 4 matching engine.

use std::collections::{HashMap, HashSet};

use crate::dom::{Data, Document, NodeId};

use super::parser::{
    parse_selectors, AttrOp, Combinator, Compound, Selector, SelectorError, Simple, TypeSel,
};

/// Sibling positions of one element: 1-based index and count among all element siblings,
/// and the same among siblings with the same tag name. Zero when the element has no parent.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Sib {
    idx: i32,
    total: i32,
    type_idx: i32,
    type_total: i32,
}

/// Sibling positions of every node, indexed by `NodeId`, computed in one pass over the tree.
pub(crate) fn sibling_table(doc: &Document) -> Vec<Sib> {
    let mut table = vec![Sib::default(); doc.nodes.len()];
    for node in &doc.nodes {
        let kids: Vec<(NodeId, &str)> = node
            .children
            .iter()
            .filter_map(|&cid| match doc.nodes.get(cid).map(|n| &n.data) {
                Some(Data::Element { name, .. }) => Some((cid, name.as_str())),
                _ => None,
            })
            .collect();
        let mut counts: HashMap<&str, i32> = HashMap::new();
        for (_, name) in &kids {
            *counts.entry(name).or_insert(0) += 1;
        }
        let total = kids.len() as i32;
        let mut seen: HashMap<&str, i32> = HashMap::new();
        for (i, (cid, name)) in kids.iter().enumerate() {
            let run = seen.entry(name).or_insert(0);
            *run += 1;
            if let Some(slot) = table.get_mut(*cid) {
                *slot = Sib {
                    idx: i as i32 + 1,
                    total,
                    type_idx: *run,
                    type_total: counts[name],
                };
            }
        }
    }
    table
}

/// Failed (combinator index, node) pairs for one match call; each pair is memoised once.
type Failed = HashSet<(usize, NodeId)>;

impl Selector {
    /// Return `true` if element `el` in `doc` matches this selector.
    pub fn matches(&self, doc: &Document, el: NodeId) -> bool {
        self.matches_with(doc, el, None)
    }

    /// Match with sibling positions precomputed by [`sibling_table`] (`None` scans siblings).
    pub(crate) fn matches_with(&self, doc: &Document, el: NodeId, sib: Option<&[Sib]>) -> bool {
        if !matches!(
            doc.nodes.get(el).map(|n| &n.data),
            Some(Data::Element { .. })
        ) {
            return false;
        }
        let len = self.rest.len();
        let last_comp = if len == 0 {
            &self.first
        } else {
            &self.rest[len - 1].1
        };
        if !matches_compound(doc, el, last_comp, sib) {
            return false;
        }
        if len == 0 {
            return true;
        }
        let mut failed = Failed::new();
        chain_matches(doc, self, len - 1, el, sib, &mut failed)
    }
}

/// Return `true` if the compound left of combinator `rest[k]` can be matched by a node
/// reached from `cur` through that combinator, with everything to its left also matching.
/// The result for a given `(k, cur)` never depends on the caller, so failures are memoised.
/// Recursion depth is at most the compound cap enforced by the parser.
fn chain_matches(
    doc: &Document,
    sel: &Selector,
    k: usize,
    cur: NodeId,
    sib: Option<&[Sib]>,
    failed: &mut Failed,
) -> bool {
    if failed.contains(&(k, cur)) {
        return false;
    }
    let found = match sel.rest[k].0 {
        Combinator::Child => {
            element_parent(doc, cur).is_some_and(|p| step(doc, sel, k, p, sib, failed))
        }
        Combinator::NextSibling => {
            prev_element_sibling(doc, cur).is_some_and(|p| step(doc, sel, k, p, sib, failed))
        }
        Combinator::Descendant => {
            // Every ancestor of a failed ancestor has already been ruled out, so the walk
            // stops there and the nodes walked on this failure are recorded as failed too.
            let mut walked = Vec::new();
            let mut hit = false;
            let mut p = element_parent(doc, cur);
            while let Some(anc) = p {
                if step(doc, sel, k, anc, sib, failed) {
                    hit = true;
                    break;
                }
                walked.push(anc);
                if failed.contains(&(k, anc)) {
                    break;
                }
                p = element_parent(doc, anc);
            }
            if !hit {
                failed.extend(walked.into_iter().map(|w| (k, w)));
            }
            hit
        }
        Combinator::SubsequentSibling => {
            let mut walked = Vec::new();
            let mut hit = false;
            let mut p = prev_element_sibling(doc, cur);
            while let Some(s) = p {
                if step(doc, sel, k, s, sib, failed) {
                    hit = true;
                    break;
                }
                walked.push(s);
                if failed.contains(&(k, s)) {
                    break;
                }
                p = prev_element_sibling(doc, s);
            }
            if !hit {
                failed.extend(walked.into_iter().map(|w| (k, w)));
            }
            hit
        }
    };
    if !found {
        failed.insert((k, cur));
    }
    found
}

/// Check candidate `cand` against the compound left of `rest[k]`, then the chain beyond it.
fn step(
    doc: &Document,
    sel: &Selector,
    k: usize,
    cand: NodeId,
    sib: Option<&[Sib]>,
    failed: &mut Failed,
) -> bool {
    let target = if k == 0 {
        &sel.first
    } else {
        &sel.rest[k - 1].1
    };
    matches_compound(doc, cand, target, sib)
        && (k == 0 || chain_matches(doc, sel, k - 1, cand, sib, failed))
}

/// Return all element node IDs in `doc` matching any selector in `selectors`, in document order.
pub fn select(doc: &Document, selectors: &str) -> Result<Vec<NodeId>, SelectorError> {
    let parsed = parse_selectors(selectors)?;
    let sib = sibling_table(doc);
    let mut out = Vec::new();
    for el in doc.elements() {
        if parsed.iter().any(|s| s.matches_with(doc, el, Some(&sib))) {
            out.push(el);
        }
    }
    Ok(out)
}

fn element_parent(doc: &Document, el: NodeId) -> Option<NodeId> {
    let pid = doc.nodes.get(el)?.parent?;
    match doc.nodes.get(pid)?.data {
        Data::Element { .. } => Some(pid),
        _ => None,
    }
}

fn prev_element_sibling(doc: &Document, el: NodeId) -> Option<NodeId> {
    let pid = doc.nodes.get(el)?.parent?;
    let children = &doc.nodes.get(pid)?.children;
    let pos = children.iter().position(|&c| c == el)?;
    for &c in children[..pos].iter().rev() {
        if matches!(
            doc.nodes.get(c).map(|n| &n.data),
            Some(Data::Element { .. })
        ) {
            return Some(c);
        }
    }
    None
}

fn matches_compound(doc: &Document, el: NodeId, comp: &Compound, sib: Option<&[Sib]>) -> bool {
    let Some(node) = doc.nodes.get(el) else {
        return false;
    };
    let Data::Element { name, .. } = &node.data else {
        return false;
    };
    if let Some(TypeSel::Tag(tag)) = &comp.type_sel {
        if name != tag {
            return false;
        }
    }
    for simple in &comp.simples {
        if !matches_simple(doc, el, name, simple, sib) {
            return false;
        }
    }
    true
}

fn matches_simple(
    doc: &Document,
    el: NodeId,
    el_name: &str,
    simple: &Simple,
    sib: Option<&[Sib]>,
) -> bool {
    match simple {
        Simple::Id(id) => doc.attr(el, "id") == Some(id.as_str()),
        Simple::Class(cls) => doc
            .attr(el, "class")
            .is_some_and(|v| v.split_ascii_whitespace().any(|c| c == cls)),
        Simple::AttrPresence(attr) => doc.attr(el, attr).is_some(),
        Simple::AttrMatch { name, op, value } => {
            let Some(actual) = doc.attr(el, name) else {
                return false;
            };
            match op {
                AttrOp::Eq => actual == value,
                AttrOp::Includes => {
                    !value.is_empty()
                        && !value.bytes().any(|b| b.is_ascii_whitespace())
                        && actual.split_ascii_whitespace().any(|w| w == value)
                }
                AttrOp::DashMatch => {
                    actual == value
                        || (actual.starts_with(value.as_str())
                            && actual.as_bytes().get(value.len()) == Some(&b'-'))
                }
                AttrOp::Prefix => !value.is_empty() && actual.starts_with(value.as_str()),
                AttrOp::Suffix => !value.is_empty() && actual.ends_with(value.as_str()),
                AttrOp::Substring => !value.is_empty() && actual.contains(value.as_str()),
            }
        }
        Simple::Root => doc.nodes.get(el).and_then(|n| n.parent) == Some(0),
        Simple::FirstChild => sibling_index(doc, el, None, sib).0 == 1,
        Simple::LastChild => {
            let (idx, total) = sibling_index(doc, el, None, sib);
            idx > 0 && idx == total
        }
        Simple::OnlyChild => {
            let (idx, total) = sibling_index(doc, el, None, sib);
            idx == 1 && total == 1
        }
        Simple::FirstOfType => sibling_index(doc, el, Some(el_name), sib).0 == 1,
        Simple::LastOfType => {
            let (idx, total) = sibling_index(doc, el, Some(el_name), sib);
            idx > 0 && idx == total
        }
        Simple::Empty => is_empty_element(doc, el),
        Simple::NthChild(a, b) => {
            let (idx, _) = sibling_index(doc, el, None, sib);
            matches_an_plus_b(idx, *a, *b)
        }
        Simple::NthLastChild(a, b) => {
            let (idx, total) = sibling_index(doc, el, None, sib);
            idx > 0 && matches_an_plus_b(total - idx + 1, *a, *b)
        }
        Simple::NthOfType(a, b) => {
            let (idx, _) = sibling_index(doc, el, Some(el_name), sib);
            matches_an_plus_b(idx, *a, *b)
        }
        Simple::NthLastOfType(a, b) => {
            let (idx, total) = sibling_index(doc, el, Some(el_name), sib);
            idx > 0 && matches_an_plus_b(total - idx + 1, *a, *b)
        }
        Simple::Not(inner) => !matches_compound(doc, el, inner, sib),
    }
}

/// Return `(index, count)` of `el` among its element siblings (same tag only when `of_type`).
/// Uses the precomputed table when given, otherwise scans the siblings.
fn sibling_index(
    doc: &Document,
    el: NodeId,
    of_type: Option<&str>,
    sib: Option<&[Sib]>,
) -> (i32, i32) {
    if let Some(table) = sib {
        let Some(s) = table.get(el) else {
            return (0, 0);
        };
        return if of_type.is_some() {
            (s.type_idx, s.type_total)
        } else {
            (s.idx, s.total)
        };
    }
    let Some(pid) = doc.nodes.get(el).and_then(|n| n.parent) else {
        return (0, 0);
    };
    let Some(parent) = doc.nodes.get(pid) else {
        return (0, 0);
    };
    let mut idx = 0i32;
    let mut total = 0i32;
    for &cid in &parent.children {
        if let Some(Data::Element { name, .. }) = doc.nodes.get(cid).map(|n| &n.data) {
            if of_type.is_none_or(|t| name == t) {
                total += 1;
                if cid == el {
                    idx = total;
                }
            }
        }
    }
    (idx, total)
}

fn is_empty_element(doc: &Document, el: NodeId) -> bool {
    let Some(node) = doc.nodes.get(el) else {
        return false;
    };
    for &cid in &node.children {
        if let Some(child) = doc.nodes.get(cid) {
            match &child.data {
                Data::Element { .. } => return false,
                Data::Text(t) if !t.bytes().all(|b| b.is_ascii_whitespace()) => return false,
                _ => {}
            }
        }
    }
    true
}

fn matches_an_plus_b(idx: i32, a: i32, b: i32) -> bool {
    if idx <= 0 {
        return false;
    }
    if a == 0 {
        return idx == b;
    }
    let diff = idx - b;
    diff % a == 0 && diff / a >= 0
}
