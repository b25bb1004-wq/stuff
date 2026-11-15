//! Widget tree, flex layout, input events, focus ring, and software rendering.
#![forbid(unsafe_code)]

pub mod anim;
mod events;
mod layout;
pub mod motion;
mod render;
mod types;
pub mod widgets;

pub use pigment::{Canvas, Rect, Rgba};
pub use types::{Action, Align, Dir, Event, Insets, Justify, Key, Kind, NodeId, Style, Theme};

/// Internal node record stored in `Ui::nodes`.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Parent node id (`None` only for the root).
    pub(crate) parent: Option<NodeId>,
    /// Ordered list of child node ids.
    pub(crate) children: Vec<NodeId>,
    /// Widget kind and text payload.
    pub(crate) kind: Kind,
    /// Layout style properties.
    pub(crate) style: Style,
    /// Computed rectangle from the most recent [`Ui::layout`] call.
    pub(crate) rect: Option<Rect>,
}

/// Immediate-style retained widget tree with flex layout, focus, and rendering.
#[derive(Debug, Clone)]
pub struct Ui {
    pub(crate) nodes: Vec<Option<Node>>,
    pub(crate) theme: Theme,
    pub(crate) focused: Option<NodeId>,
    pub(crate) pressed: Option<NodeId>,
    pub(crate) hovered: Option<NodeId>,
}

impl Ui {
    /// Create a new `Ui` with `theme` and a root `Box` at node `0`.
    pub fn new(theme: Theme) -> Ui {
        let root = Node {
            parent: None,
            children: Vec::new(),
            kind: Kind::Box,
            style: Style::default(),
            rect: None,
        };
        Ui {
            nodes: vec![Some(root)],
            theme,
            focused: None,
            pressed: None,
            hovered: None,
        }
    }

    /// Return the root node id (`0`).
    pub fn root(&self) -> NodeId {
        0
    }

    /// Add a child node under `parent`, returning `None` unless `parent` is a live `Box`.
    pub fn add(&mut self, parent: NodeId, kind: Kind, style: Style) -> Option<NodeId> {
        let is_live_box = self
            .node(parent)
            .is_some_and(|n| matches!(n.kind, Kind::Box));
        if !is_live_box {
            return None;
        }
        let id = self.nodes.len();
        self.nodes.push(Some(Node {
            parent: Some(parent),
            children: Vec::new(),
            kind,
            style,
            rect: None,
        }));
        if let Some(Some(p)) = self.nodes.get_mut(parent) {
            p.children.push(id);
        }
        Some(id)
    }

    /// Remove `id` and its subtree; returns `false` for unknown ids or the root.
    pub fn remove(&mut self, id: NodeId) -> bool {
        if id == 0 {
            return false;
        }
        let Some(parent_id) = self.node(id).and_then(|n| n.parent) else {
            return false;
        };
        if let Some(Some(parent)) = self.nodes.get_mut(parent_id) {
            parent.children.retain(|&c| c != id);
        }
        let mut stack = vec![id];
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.nodes.get_mut(nid).and_then(Option::take) {
                if self.focused == Some(nid) {
                    self.focused = None;
                }
                if self.pressed == Some(nid) {
                    self.pressed = None;
                }
                if self.hovered == Some(nid) {
                    self.hovered = None;
                }
                stack.extend(node.children);
            }
        }
        true
    }

    /// Return the children of `id`, or an empty slice for unknown ids.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.node(id).map_or(&[], |n| n.children.as_slice())
    }

    /// Return the text of a `Label`, `Button`, or `Field`, or `None` for `Box` or unknown ids.
    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id)?.kind {
            Kind::Box => None,
            Kind::Label(s) | Kind::Button(s) | Kind::Field(s) => Some(s.as_str()),
        }
    }

    /// Set the text of a `Label`, `Button`, or `Field`; returns `false` for `Box` or unknown ids.
    pub fn set_text(&mut self, id: NodeId, text: &str) -> bool {
        let Some(Some(node)) = self.nodes.get_mut(id) else {
            return false;
        };
        match &mut node.kind {
            Kind::Box => false,
            Kind::Label(s) | Kind::Button(s) | Kind::Field(s) => {
                s.clear();
                s.push_str(text);
                true
            }
        }
    }

    /// Return the rectangle of `id` from the last [`Ui::layout`] call, or `None`.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        self.node(id).and_then(|n| n.rect)
    }

    /// Return the currently focused node id, if any.
    pub fn focus(&self) -> Option<NodeId> {
        self.focused
    }

    /// Look up a live node by id.
    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id).and_then(Option::as_ref)
    }
}
