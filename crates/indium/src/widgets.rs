//! Pure state machines for list, scroll, tabs, menu and text field widgets.
//! No drawing and no floats. Every `-> bool` method returns whether the state changed.

mod field;
mod menu;

pub use field::FieldState;
pub use menu::{MenuItem, MenuState};

/// Navigation key shared by list and menu widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// Previous row or item.
    Up,
    /// Next row or item.
    Down,
    /// One page up.
    PageUp,
    /// One page down.
    PageDown,
    /// First row or item.
    Home,
    /// Last row or item.
    End,
}

/// Selection and scroll offset of a list with `visible` rows on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListState {
    /// Number of rows.
    pub len: usize,
    /// Selected row, if any.
    pub selected: Option<usize>,
    /// First visible row.
    pub offset: usize,
    /// Rows on screen.
    pub visible: usize,
}

impl ListState {
    /// New list with `len` rows and `visible` rows on screen; nothing selected.
    pub fn new(len: usize, visible: usize) -> ListState {
        ListState {
            len,
            selected: None,
            offset: 0,
            visible,
        }
    }

    /// Apply navigation key `n`; returns whether the state changed.
    pub fn nav(&mut self, n: Nav) -> bool {
        if self.len == 0 {
            return false;
        }
        let before = *self;
        let last = self.len - 1;
        let page = self.page();
        self.selected = Some(match self.selected {
            None if n == Nav::End => last,
            None => 0,
            Some(s) => match n {
                Nav::Up => s.saturating_sub(1),
                Nav::Down => s.saturating_add(1).min(last),
                Nav::PageUp => s.saturating_sub(page),
                Nav::PageDown => s.saturating_add(page).min(last),
                Nav::Home => 0,
                Nav::End => last,
            },
        });
        // the brief moves the offset only when the selection changes
        if self.selected != before.selected {
            self.reveal();
        }
        *self != before
    }

    /// Select visible row `row` (relative to the offset); returns whether the state changed.
    pub fn click_row(&mut self, row: usize) -> bool {
        let before = *self;
        let index = self.offset.saturating_add(row);
        if index < self.len {
            self.selected = Some(index);
        }
        *self != before
    }

    /// Set the row count, clamping selection and offset; returns whether the state changed.
    pub fn set_len(&mut self, len: usize) -> bool {
        let before = *self;
        self.len = len;
        self.selected = match self.selected {
            Some(s) if len > 0 => Some(s.min(len - 1)),
            _ => None,
        };
        self.offset = self.offset.min(len.saturating_sub(self.page()));
        self.reveal();
        *self != before
    }

    /// Rows on screen, never less than one.
    fn page(&self) -> usize {
        self.visible.max(1)
    }

    /// Move the offset the least amount that keeps the selection on screen.
    fn reveal(&mut self) {
        let page = self.page();
        if let Some(s) = self.selected {
            if s < self.offset {
                self.offset = s;
            } else if s >= self.offset.saturating_add(page) {
                self.offset = s.saturating_add(1).saturating_sub(page);
            }
        }
    }
}

/// Scroll position of content of height `content` shown in a view of height `view`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    /// Content height.
    pub content: u32,
    /// View height.
    pub view: u32,
    /// Current scroll position.
    pub pos: u32,
}

impl ScrollState {
    /// Largest scroll position (content minus view, saturating).
    pub fn max(&self) -> u32 {
        self.content.saturating_sub(self.view)
    }

    /// Scroll by `d` (clamped to `0..=max`); returns whether the position changed.
    pub fn scroll_by(&mut self, d: i32) -> bool {
        let before = *self;
        let next = i64::from(self.pos) + i64::from(d);
        self.pos = u32::try_from(next.clamp(0, i64::from(self.max()))).unwrap_or(0);
        *self != before
    }

    /// Scrollbar thumb on a track of `track` pixels, as `(start, length)`.
    pub fn thumb(&self, track: u32) -> (u32, u32) {
        if self.content <= self.view {
            return (0, track);
        }
        let max = u64::from(self.max());
        let t = u64::from(track);
        let len = (t * u64::from(self.view) / u64::from(self.content))
            .max(8)
            .min(t);
        let pos = u64::from(self.pos).min(max);
        let start = (t - len) * pos / max;
        (
            u32::try_from(start).unwrap_or(0),
            u32::try_from(len).unwrap_or(0),
        )
    }
}

/// Active tab among `count` tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsState {
    /// Number of tabs.
    pub count: usize,
    /// Index of the active tab.
    pub active: usize,
}

impl TabsState {
    /// Activate the next tab, wrapping around; returns whether the state changed.
    #[allow(clippy::should_implement_trait)] // name fixed by the T62 brief
    pub fn next(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        let before = *self;
        self.active = self.active.saturating_add(1) % self.count;
        *self != before
    }

    /// Activate the previous tab, wrapping around; returns whether the state changed.
    pub fn prev(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        let before = *self;
        self.active = self.active.saturating_add(self.count - 1) % self.count;
        *self != before
    }

    /// Activate tab `i` if it exists; returns whether the state changed.
    pub fn select(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false;
        }
        let before = *self;
        self.active = i;
        *self != before
    }
}
