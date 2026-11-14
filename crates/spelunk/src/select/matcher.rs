//! CSS Selectors Level 4 matching engine.

use crate::dom::{Data, Document, NodeId};

use super::parser::{
    parse_selectors, AttrOp, Combinator, Compound, Selector, SelectorError, Simple, TypeSel,
};

impl Selector {
    /// Return `true` if element `el` in `doc` matches this selector.
    pub fn matches(&self, doc: &Document, el: NodeId) -> bool {
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
        if !matches_compound(doc, el, last_comp) {
            return false;
        }
        if len == 0 {
            return true;
        }

        // Explicit backtracking stack: (combinator_index_to_resolve, current_node)
        let mut stack: Vec<(usize, NodeId)> = vec![(len - 1, el)];
        while let Some((comb_idx, cur)) = stack.pop() {
            let (comb, _) = &self.rest[comb_idx];
            let target_comp = if comb_idx == 0 {
                &self.first
            } else {
                &self.rest[comb_idx - 1].1
            };

            match comb {
                Combinator::Child => {
                    if let Some(parent) = element_parent(doc, cur) {
                        if matches_compound(doc, parent, target_comp) {
                            if comb_idx == 0 {
                                return true;
                            }
                            stack.push((comb_idx - 1, parent));
                        }
                    }
                }
                Combinator::NextSibling => {
                    if let Some(prev) = prev_element_sibling(doc, cur) {
                        if matches_compound(doc, prev, target_comp) {
                            if comb_idx == 0 {
                                return true;
                            }
                            stack.push((comb_idx - 1, prev));
                        }
                    }
                }
                Combinator::Descendant => {
                    let mut p = element_parent(doc, cur);
                    let mut candidates = Vec::new();
                    while let Some(anc) = p {
                        if matches_compound(doc, anc, target_comp) {
                            if comb_idx == 0 {
                                return true;
                            }
                            candidates.push((comb_idx - 1, anc));
                        }
                        p = element_parent(doc, anc);
                    }
                    for c in candidates.into_iter().rev() {
                        stack.push(c);
                    }
                }
                Combinator::SubsequentSibling => {
                    let mut p = prev_element_sibling(doc, cur);
                    let mut candidates = Vec::new();
                    while let Some(sib) = p {
                        if matches_compound(doc, sib, target_comp) {
                            if comb_idx == 0 {
                                return true;
                            }
                            candidates.push((comb_idx - 1, sib));
                        }
                        p = prev_element_sibling(doc, sib);
                    }
                    for c in candidates.into_iter().rev() {
                        stack.push(c);
                    }
                }
            }
        }

        false
    }
}

/// Return all element node IDs in `doc` matching any selector in `selectors`, in document order.
pub fn select(doc: &Document, selectors: &str) -> Result<Vec<NodeId>, SelectorError> {
    let parsed = parse_selectors(selectors)?;
    let mut out = Vec::new();
    for el in doc.elements() {
        if parsed.iter().any(|s| s.matches(doc, el)) {
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

fn matches_compound(doc: &Document, el: NodeId, comp: &Compound) -> bool {
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
        if !matches_simple(doc, el, name, simple) {
            return false;
        }
    }
    true
}

fn matches_simple(doc: &Document, el: NodeId, el_name: &str, simple: &Simple) -> bool {
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
        Simple::FirstChild => sibling_index(doc, el, None).0 == 1,
        Simple::LastChild => {
            let (idx, total) = sibling_index(doc, el, None);
            idx > 0 && idx == total
        }
        Simple::OnlyChild => {
            let (idx, total) = sibling_index(doc, el, None);
            idx == 1 && total == 1
        }
        Simple::FirstOfType => sibling_index(doc, el, Some(el_name)).0 == 1,
        Simple::LastOfType => {
            let (idx, total) = sibling_index(doc, el, Some(el_name));
            idx > 0 && idx == total
        }
        Simple::Empty => is_empty_element(doc, el),
        Simple::NthChild(a, b) => {
            let (idx, _) = sibling_index(doc, el, None);
            matches_an_plus_b(idx, *a, *b)
        }
        Simple::NthLastChild(a, b) => {
            let (idx, total) = sibling_index(doc, el, None);
            idx > 0 && matches_an_plus_b(total - idx + 1, *a, *b)
        }
        Simple::NthOfType(a, b) => {
            let (idx, _) = sibling_index(doc, el, Some(el_name));
            matches_an_plus_b(idx, *a, *b)
        }
        Simple::NthLastOfType(a, b) => {
            let (idx, total) = sibling_index(doc, el, Some(el_name));
            idx > 0 && matches_an_plus_b(total - idx + 1, *a, *b)
        }
        Simple::Not(inner) => !matches_compound(doc, el, inner),
    }
}

fn sibling_index(doc: &Document, el: NodeId, of_type: Option<&str>) -> (i32, i32) {
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
