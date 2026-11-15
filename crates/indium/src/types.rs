//! Public types, styles, events, actions, and themes for `indium`.

use pigment::Rgba;

/// Stable identifier of a node in a [`Ui`](crate::Ui) tree.
pub type NodeId = usize;

/// Main-axis layout direction of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Lay out children left to right.
    Row,
    /// Lay out children top to bottom.
    Column,
}

/// Main-axis distribution of extra space when no child grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    /// Pack children at the start of the main axis.
    Start,
    /// Center children along the main axis.
    Center,
    /// Pack children at the end of the main axis.
    End,
    /// Distribute free space evenly between children.
    SpaceBetween,
}

/// Cross-axis alignment of children inside a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Align children to the start of the cross axis.
    Start,
    /// Center children along the cross axis.
    Center,
    /// Align children to the end of the cross axis.
    End,
    /// Stretch children to fill the container's inner cross size.
    Stretch,
}

/// Padding insets in pixels for all four edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    /// Top padding in pixels.
    pub top: u32,
    /// Right padding in pixels.
    pub right: u32,
    /// Bottom padding in pixels.
    pub bottom: u32,
    /// Left padding in pixels.
    pub left: u32,
}

/// Layout style properties for a widget or container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Main-axis direction for children of a `Box`.
    pub dir: Dir,
    /// Main-axis justification for children when none grow.
    pub justify: Justify,
    /// Cross-axis alignment for children of a `Box`.
    pub align: Align,
    /// Spacing in pixels between adjacent children.
    pub gap: u32,
    /// Inner padding between a node's rect and its content or children.
    pub padding: Insets,
    /// Flex grow factor along the parent's main axis.
    pub grow: u32,
    /// Explicit base width override in pixels.
    pub width: Option<u32>,
    /// Explicit base height override in pixels.
    pub height: Option<u32>,
    /// Minimum width in pixels.
    pub min_w: u32,
    /// Minimum height in pixels.
    pub min_h: u32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            dir: Dir::Column,
            justify: Justify::Start,
            align: Align::Stretch,
            gap: 0,
            padding: Insets::default(),
            grow: 0,
            width: None,
            height: None,
            min_w: 0,
            min_h: 0,
        }
    }
}

/// Widget kind and text payload for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Container node that lays out its children.
    Box,
    /// Non-interactive text label.
    Label(String),
    /// Focusable, clickable push button.
    Button(String),
    /// Focusable, editable single-line text field.
    Field(String),
}

/// Logical key press event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Move focus forward.
    Tab,
    /// Move focus backward.
    ShiftTab,
    /// Activate focused button or submit focused field.
    Enter,
    /// Activate focused button.
    Space,
    /// Delete the last character in a focused field.
    Backspace,
    /// Clear focus.
    Escape,
}

/// Input event delivered to [`Ui::event`](crate::Ui::event).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Pointer moved to `(x, y)`.
    MouseMove {
        /// Pointer x coordinate.
        x: i32,
        /// Pointer y coordinate.
        y: i32,
    },
    /// Primary mouse button pressed at `(x, y)`.
    MouseDown {
        /// Pointer x coordinate.
        x: i32,
        /// Pointer y coordinate.
        y: i32,
    },
    /// Primary mouse button released at `(x, y)`.
    MouseUp {
        /// Pointer x coordinate.
        x: i32,
        /// Pointer y coordinate.
        y: i32,
    },
    /// Logical key press.
    Key(Key),
    /// Character text input.
    Text(char),
}

/// Action emitted by [`Ui::event`](crate::Ui::event).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A button was clicked.
    Clicked(NodeId),
    /// A text field's contents changed.
    Changed(NodeId),
    /// A text field was submitted with Enter.
    Submitted(NodeId),
    /// Focus moved to a node or was cleared (`None`).
    Focused(Option<NodeId>),
}

/// Visual theme colours and text scale for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Background fill colour.
    pub bg: Rgba,
    /// Foreground text colour.
    pub fg: Rgba,
    /// Default button fill colour.
    pub button: Rgba,
    /// Hovered button fill colour.
    pub button_hover: Rgba,
    /// Pressed button fill colour.
    pub button_pressed: Rgba,
    /// Text field background fill colour.
    pub field_bg: Rgba,
    /// Focus ring stroke colour.
    pub focus_ring: Rgba,
    /// Integer bitmap font scale (1 or greater).
    pub text_scale: u32,
}

impl Theme {
    /// Default light theme with opaque colours and scale 1.
    pub fn light() -> Theme {
        Theme {
            bg: Rgba(245, 245, 245, 255),
            fg: Rgba(20, 20, 20, 255),
            button: Rgba(220, 220, 225, 255),
            button_hover: Rgba(200, 205, 220, 255),
            button_pressed: Rgba(175, 185, 210, 255),
            field_bg: Rgba(255, 255, 255, 255),
            focus_ring: Rgba(30, 110, 240, 255),
            text_scale: 1,
        }
    }

    /// Default dark theme with opaque colours and scale 1.
    pub fn dark() -> Theme {
        Theme {
            bg: Rgba(24, 24, 28, 255),
            fg: Rgba(235, 235, 240, 255),
            button: Rgba(55, 58, 66, 255),
            button_hover: Rgba(75, 80, 92, 255),
            button_pressed: Rgba(95, 102, 118, 255),
            field_bg: Rgba(38, 40, 46, 255),
            focus_ring: Rgba(90, 165, 255, 255),
            text_scale: 1,
        }
    }
}
