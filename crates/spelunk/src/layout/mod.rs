//! Block, inline and flex layout over computed styles. Grid is not handled here.

mod block;
mod flex;
mod inline;

use crate::dom::{Document, NodeId};
use crate::style::{compute_flex, Computed, Display, Flex, Len, FLEX_INITIAL};

// shortcut: boxes nested deeper than 256 levels are not laid out, which keeps recursion bounded
// on hostile input; upgrade to an explicit work stack if deep documents must render.
const MAX_DEPTH: usize = 256;

/// A rectangle in layout pixels (border box for boxes, glyph box for text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxRect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

/// One word fragment placed on a line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// The text node the word came from.
    pub node: NodeId,
    /// The word.
    pub text: String,
    /// Where the word is drawn.
    pub rect: BoxRect,
    /// Text scale, `max(1, round(font_px / 16))`.
    pub scale: u32,
}

/// The result of laying out a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Border box of every element with display block, in document order.
    pub boxes: Vec<(NodeId, BoxRect)>,
    /// Every word fragment placed on a line, in order.
    pub runs: Vec<TextRun>,
    /// Border-box height of the root element.
    pub height: i32,
}

/// Shared state while laying out one document.
pub(crate) struct Ctx<'a> {
    pub(crate) doc: &'a Document,
    styles: &'a [Option<Computed>],
    flexes: Vec<Option<Flex>>,
    pub(crate) boxes: Vec<(NodeId, BoxRect)>,
    pub(crate) runs: Vec<TextRun>,
    pub(crate) depth: usize,
}

impl Ctx<'_> {
    /// The computed style of element `id`, if it has one.
    pub(crate) fn style(&self, id: NodeId) -> Option<Computed> {
        self.styles.get(id).copied().flatten()
    }

    /// The flex properties of element `id` (initial values for non-elements).
    pub(crate) fn flex_of(&self, id: NodeId) -> Flex {
        self.flexes
            .get(id)
            .copied()
            .flatten()
            .unwrap_or(FLEX_INITIAL)
    }
}

/// Lay out `doc` in a viewport `viewport_w` pixels wide, using the styles from `style::compute`.
pub fn layout(doc: &Document, styles: &[Option<Computed>], viewport_w: i32) -> Layout {
    let mut ctx = Ctx {
        doc,
        styles,
        flexes: compute_flex(doc),
        boxes: Vec::new(),
        runs: Vec::new(),
        depth: 0,
    };
    let mut height = 0;
    if let Some(root) = root_element(doc) {
        if let Some(st) = ctx.style(root) {
            if st.display != Display::None {
                let top = margin_px(st.margin[0]);
                height = block::block(&mut ctx, root, 0, viewport_w, top);
            }
        }
    }
    Layout {
        boxes: ctx.boxes,
        runs: ctx.runs,
        height,
    }
}

/// The `html` element, or else the first element child of the document.
fn root_element(doc: &Document) -> Option<NodeId> {
    let els: Vec<NodeId> = doc
        .nodes
        .first()?
        .children
        .iter()
        .copied()
        .filter(|&c| doc.name(c).is_some())
        .collect();
    els.iter()
        .copied()
        .find(|&c| doc.name(c) == Some("html"))
        .or_else(|| els.first().copied())
}

/// A vertical or horizontal margin in pixels; `auto` counts as 0 here.
pub(crate) fn margin_px(l: Len) -> i32 {
    match l {
        Len::Px(p) => p,
        Len::Auto => 0,
    }
}
