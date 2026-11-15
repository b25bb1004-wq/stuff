//! Application trait, effects, shared filesystem handle, and 8x16 cell text-grid helpers.

use std::cell::RefCell;
use std::rc::Rc;

use hades::{Fs, MemDisk};
use indium::Event;
use pigment::{Canvas, Rect, Rgba, GLYPH_H, GLYPH_W};

/// Shared in-memory `hades` filesystem handle used by [`Desktop`](crate::Desktop) and its apps.
pub type SharedFs = Rc<RefCell<Fs<MemDisk>>>;

/// Width in pixels of one fixed-grid character cell.
pub(crate) const CELL_W: u32 = GLYPH_W;

/// Height in pixels of one fixed-grid character cell.
pub(crate) const CELL_H: u32 = GLYPH_H;

/// Action requested by an [`App`] after handling an input event.
pub enum Effect {
    /// Keep running normally.
    None,
    /// Close this application window.
    Close,
    /// Open a child application in a new window.
    Open(Box<dyn App>),
}

/// Interactive application hosted inside a [`Desktop`](crate::Desktop) window.
pub trait App {
    /// Window title shown by the desktop or app state.
    fn title(&self) -> &str;

    /// Handle a routed window input event at timestamp `now_ms`.
    fn event(&mut self, ev: Event, now_ms: u64) -> Effect;

    /// Paint window content onto `c` (sized to the content area), returning local damage or `None`.
    fn paint(&mut self, c: &mut Canvas) -> Option<Rect>;
}

/// Number of `(cols, rows)` of 8x16 character cells that fit on `c`.
pub(crate) fn grid_dims(c: &Canvas) -> (usize, usize) {
    let cols = (c.width / CELL_W) as usize;
    let rows = (c.height / CELL_H) as usize;
    (cols, rows)
}

/// Fill the entire canvas with `bg`.
pub(crate) fn fill_bg(c: &mut Canvas, bg: Rgba) {
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w: c.width,
            h: c.height,
        },
        bg,
    );
}

/// Paint one 8x16 text-grid row at `row`, filling its strip with `bg` and clipping to `max_cols`.
pub(crate) fn paint_row(
    c: &mut Canvas,
    row: usize,
    text: &str,
    fg: Rgba,
    bg: Rgba,
    max_cols: usize,
) {
    let y = i32::try_from(row.saturating_mul(CELL_H as usize)).unwrap_or(i32::MAX);
    c.fill_rect(
        Rect {
            x: 0,
            y,
            w: c.width,
            h: CELL_H,
        },
        bg,
    );
    if max_cols == 0 || text.is_empty() {
        return;
    }
    let mut clipped = String::with_capacity(text.len().min(max_cols));
    for ch in text.chars().take(max_cols) {
        if ch == '\t' || ch.is_control() {
            clipped.push(' ');
        } else {
            clipped.push(ch);
        }
    }
    c.draw_text(0, y, &clipped, fg, 1, u32::MAX);
}

/// Paint a 2x16 vertical cursor bar at `(row, col)` in cell coordinates.
pub(crate) fn paint_cursor_bar(c: &mut Canvas, row: usize, col: usize, color: Rgba) {
    let x = i32::try_from(col.saturating_mul(CELL_W as usize)).unwrap_or(i32::MAX);
    let y = i32::try_from(row.saturating_mul(CELL_H as usize)).unwrap_or(i32::MAX);
    c.fill_rect(
        Rect {
            x,
            y,
            w: 2,
            h: CELL_H,
        },
        color,
    );
}
