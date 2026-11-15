//! Inline flow: flattening of inline children, whitespace collapsing, word wrapping and lines.

use crate::dom::{Data, NodeId};
use crate::style::{Computed, Display};

use super::{BoxRect, Ctx, TextRun, MAX_DEPTH};

/// A child of a block container after inline elements are flattened.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Item {
    /// A block-level element, hoisted out of any inline elements around it.
    Block(NodeId),
    /// A text node; `style` is the computed style of the element that holds it.
    Text { node: NodeId, style: Computed },
}

/// The flow items of `parent`, with inline elements replaced by their children.
pub(crate) fn flatten(ctx: &Ctx, parent: NodeId) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(st) = ctx.style(parent) {
        push_children(ctx, parent, st, &mut out, 0);
    }
    out
}

fn push_children(ctx: &Ctx, id: NodeId, st: Computed, out: &mut Vec<Item>, depth: usize) {
    if depth >= MAX_DEPTH {
        return;
    }
    let Some(node) = ctx.doc.nodes.get(id) else {
        return;
    };
    for &c in &node.children {
        let Some(child) = ctx.doc.nodes.get(c) else {
            continue;
        };
        match &child.data {
            Data::Text(_) => out.push(Item::Text { node: c, style: st }),
            Data::Element { .. } => {
                let Some(cs) = ctx.style(c) else {
                    continue;
                };
                match cs.display {
                    Display::None => {}
                    Display::Block | Display::Flex => out.push(Item::Block(c)),
                    Display::Inline => push_children(ctx, c, cs, out, depth + 1),
                }
            }
            _ => {}
        }
    }
}

/// Font scale for a font size: `max(1, round(font_px / 16))`, halves rounding up.
fn scale_of(px: i32) -> u32 {
    let p = u32::try_from(px.max(0)).unwrap_or(0);
    (p.saturating_add(8) / 16).max(1)
}

/// Place inline `items` in a content box at `cx` of width `cw`, starting at `top`.
/// Appends the word runs to `ctx.runs` and returns the height of the line boxes.
pub(crate) fn lines(ctx: &mut Ctx, items: &[Item], cx: i32, top: i32, cw: i32) -> i32 {
    let doc = ctx.doc;
    let mut flow = Flow {
        cx,
        cw,
        start: top,
        y: top,
        words: Vec::new(),
        width: 0,
        scale: 0,
        space: None,
    };
    for item in items {
        let Item::Text { node, style } = *item else {
            continue;
        };
        let Some(Data::Text(text)) = doc.nodes.get(node).map(|n| &n.data) else {
            continue;
        };
        let scale = scale_of(style.font_px);
        let ws = |c: char| c.is_ascii_whitespace();
        if text.starts_with(ws) {
            flow.space = Some(scale);
        }
        for (i, word) in text.split_ascii_whitespace().enumerate() {
            if i > 0 {
                flow.space = Some(scale);
            }
            flow.place(&mut ctx.runs, node, word, scale);
        }
        if text.ends_with(ws) {
            flow.space = Some(scale);
        }
    }
    flow.finish(&mut ctx.runs);
    flow.y.saturating_sub(flow.start)
}

/// State of the line being filled.
struct Flow {
    cx: i32,
    cw: i32,
    start: i32,
    /// Top of the next line.
    y: i32,
    /// Runs on the current line; `rect.y` is filled in when the line closes.
    words: Vec<TextRun>,
    /// Right edge of the last word on the current line, relative to `cx`.
    width: i32,
    /// Largest scale on the current line.
    scale: u32,
    /// Scale of a pending collapsed space before the next word.
    space: Option<u32>,
}

impl Flow {
    fn place(&mut self, runs: &mut Vec<TextRun>, node: NodeId, word: &str, scale: u32) {
        let (w, h) = pigment::measure(word, scale);
        let w = i32::try_from(w).unwrap_or(i32::MAX);
        let h = i32::try_from(h).unwrap_or(i32::MAX);
        let gap = self.space.take().map_or(0, space_px);
        let (x_off, fits) = if self.words.is_empty() {
            (0, true)
        } else {
            let x = self.width.saturating_add(gap);
            (x, x.saturating_add(w) <= self.cw)
        };
        let x_off = if fits {
            x_off
        } else {
            self.close(runs);
            0
        };
        self.words.push(TextRun {
            node,
            text: word.to_string(),
            rect: BoxRect {
                x: self.cx.saturating_add(x_off),
                y: 0,
                w,
                h,
            },
            scale,
        });
        self.width = x_off.saturating_add(w);
        self.scale = self.scale.max(scale);
    }

    /// Close the current line: fix the runs' top, advance the line top by `16 * scale`.
    fn close(&mut self, runs: &mut Vec<TextRun>) {
        if self.words.is_empty() {
            return;
        }
        let y = self.y;
        let line_h = i32::try_from(16u32.saturating_mul(self.scale)).unwrap_or(i32::MAX);
        for mut run in self.words.drain(..) {
            run.rect.y = y;
            runs.push(run);
        }
        self.y = y.saturating_add(line_h);
        self.width = 0;
        self.scale = 0;
    }

    fn finish(&mut self, runs: &mut Vec<TextRun>) {
        self.close(runs);
    }
}

/// Width of a collapsed space at `scale`: 8 pixels per scale unit.
fn space_px(scale: u32) -> i32 {
    i32::try_from(8u32.saturating_mul(scale)).unwrap_or(i32::MAX)
}
