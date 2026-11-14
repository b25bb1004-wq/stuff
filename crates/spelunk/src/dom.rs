//! DOM node representation and tree utilities.

/// Index of a node inside [`Document::nodes`].
pub type NodeId = usize;

/// Payload of a DOM node.
#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    /// Root document node (`nodes[0]`).
    Document,
    /// Document type declaration.
    Doctype(String),
    /// HTML element with lowercase tag name and `(name, value)` attributes.
    Element {
        /// Lowercase element tag name.
        name: String,
        /// Attribute `(name, value)` pairs in insertion order.
        attrs: Vec<(String, String)>,
    },
    /// Character data node.
    Text(String),
    /// HTML comment node.
    Comment(String),
}

/// A single node in the arena-allocated DOM tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Node payload.
    pub data: Data,
    /// Parent node index, or `None` for the root document.
    pub parent: Option<NodeId>,
    /// Ordered child node indices.
    pub children: Vec<NodeId>,
}

/// Arena-allocated DOM tree (`nodes[0]` is always [`Data::Document`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// All nodes in the arena; index `0` is the Document root.
    pub nodes: Vec<Node>,
}

impl Document {
    /// Create a new empty document with a root `Document` node at index `0`.
    pub(crate) fn new() -> Self {
        Self {
            nodes: vec![Node {
                data: Data::Document,
                parent: None,
                children: Vec::new(),
            }],
        }
    }

    /// Return the lowercase element name for `id`, or `None` if not an element.
    pub fn name(&self, id: NodeId) -> Option<&str> {
        match self.nodes.get(id) {
            Some(Node {
                data: Data::Element { name, .. },
                ..
            }) => Some(name.as_str()),
            _ => None,
        }
    }

    /// Return the value of attribute `name` on element `id`, if present.
    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        match self.nodes.get(id) {
            Some(Node {
                data: Data::Element { attrs, .. },
                ..
            }) => attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    /// Return all element node IDs in document (pre-order) order.
    pub fn elements(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        if self.nodes.is_empty() {
            return out;
        }
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if matches!(node.data, Data::Element { .. }) {
                out.push(id);
            }
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    /// Serialize the tree in html5lib test format.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let Some(root) = self.nodes.first() else {
            return out;
        };
        let mut stack: Vec<(NodeId, usize)> = Vec::new();
        for &child in root.children.iter().rev() {
            stack.push((child, 0));
        }
        while let Some((id, depth)) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            out.push_str("| ");
            for _ in 0..depth {
                out.push_str("  ");
            }
            match &node.data {
                Data::Document => {}
                Data::Doctype(name) => {
                    out.push_str("<!DOCTYPE ");
                    out.push_str(name);
                    out.push_str(">\n");
                }
                Data::Element { name, attrs } => {
                    out.push('<');
                    out.push_str(name);
                    out.push_str(">\n");
                    let mut sorted_attrs: Vec<&(String, String)> = attrs.iter().collect();
                    sorted_attrs.sort_by(|(a, _), (b, _)| a.cmp(b));
                    for (k, v) in sorted_attrs {
                        out.push_str("| ");
                        for _ in 0..=depth {
                            out.push_str("  ");
                        }
                        out.push_str(k);
                        out.push_str("=\"");
                        out.push_str(v);
                        out.push_str("\"\n");
                    }
                }
                Data::Text(text) => {
                    out.push('"');
                    out.push_str(text);
                    out.push_str("\"\n");
                }
                Data::Comment(text) => {
                    out.push_str("<!-- ");
                    out.push_str(text);
                    out.push_str(" -->\n");
                }
            }
            for &child in node.children.iter().rev() {
                stack.push((child, depth + 1));
            }
        }
        out
    }
}
