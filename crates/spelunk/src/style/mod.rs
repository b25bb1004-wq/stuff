//! Computed style: the cascade (UA sheet, `<style>` sheets, `style` attributes) with inheritance.

mod cascade;
mod values;

use crate::dom::Document;

/// Outer display type of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// Block-level box.
    Block,
    /// Inline-level box.
    Inline,
    /// Not rendered, with its subtree.
    None,
}

/// A length in CSS pixels, or `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Len {
    /// A pixel length.
    Px(i32),
    /// The `auto` keyword.
    Auto,
}

/// The computed values this engine supports for one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Computed {
    /// Display type.
    pub display: Display,
    /// Width (`auto` or a pixel length).
    pub width: Len,
    /// Height (`auto` or a pixel length).
    pub height: Len,
    /// Margins in top, right, bottom, left order.
    pub margin: [Len; 4],
    /// Padding in top, right, bottom, left order.
    pub padding: [i32; 4],
    /// Border widths in top, right, bottom, left order.
    pub border: [i32; 4],
    /// Text colour as RGBA.
    pub color: [u8; 4],
    /// Background colour as RGBA.
    pub background: [u8; 4],
    /// Font size in pixels.
    pub font_px: i32,
}

/// Initial values for every property (before inheritance).
pub(crate) const INITIAL: Computed = Computed {
    display: Display::Inline,
    width: Len::Auto,
    height: Len::Auto,
    margin: [Len::Px(0); 4],
    padding: [0; 4],
    border: [0; 4],
    color: [0, 0, 0, 255],
    background: [0, 0, 0, 0],
    font_px: 16,
};

/// Compute the style of every element. Indexed by `NodeId`; `None` for non-elements.
pub fn compute(doc: &Document) -> Vec<Option<Computed>> {
    cascade::compute(doc)
}
