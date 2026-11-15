//! Flex container and item properties: `display: flex` lives in `Computed`, the rest here.

use crate::dom::Document;

use super::cascade;

/// Main axis of a flex container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDir {
    /// Items side by side, left to right.
    Row,
    /// Items stacked top to bottom.
    Column,
}

/// Placement of leftover main-axis space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexJustify {
    /// Items packed at the start.
    Start,
    /// Items packed in the centre.
    Center,
    /// Items packed at the end.
    End,
    /// Leftover space spread between the items.
    SpaceBetween,
}

/// Cross-axis placement of items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexAlign {
    /// Items at the cross start.
    Start,
    /// Items centred on the cross axis.
    Center,
    /// Items at the cross end.
    End,
    /// Auto-size items fill the cross axis.
    Stretch,
}

/// Flex properties of one element. None of them inherit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flex {
    /// Main axis (container property).
    pub dir: FlexDir,
    /// Main-axis distribution (container property).
    pub justify: FlexJustify,
    /// Cross-axis placement (container property).
    pub align: FlexAlign,
    /// Share of free main-axis space (item property).
    pub grow: u32,
    /// Space between items in pixels (container property).
    pub gap: i32,
}

/// Initial flex values for every element.
pub(crate) const FLEX_INITIAL: Flex = Flex {
    dir: FlexDir::Row,
    justify: FlexJustify::Start,
    align: FlexAlign::Stretch,
    grow: 0,
    gap: 0,
};

/// Compute the flex properties of every element. Indexed by `NodeId`; `None` for non-elements.
pub fn compute_flex(doc: &Document) -> Vec<Option<Flex>> {
    cascade::compute_all(doc)
        .into_iter()
        .map(|c| c.map(|(_, f)| f))
        .collect()
}
