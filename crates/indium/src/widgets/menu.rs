//! Drop-down menu state with hover movement over enabled items.

use super::Nav;

/// One menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Text shown for the entry.
    pub label: String,
    /// Disabled items can be hovered past but never chosen.
    pub enabled: bool,
}

/// Menu with open state and the hovered item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuState {
    /// Entries in display order.
    pub items: Vec<MenuItem>,
    /// Whether the menu is showing.
    pub open: bool,
    /// Hovered item index, if any.
    pub hover: Option<usize>,
}

impl MenuState {
    /// Open the menu and hover the first enabled item (none if there is none).
    pub fn open(&mut self) {
        self.open = true;
        self.hover = self.edge(true);
    }

    /// Close the menu and clear the hover.
    pub fn close(&mut self) {
        self.open = false;
        self.hover = None;
    }

    /// Move the hover for navigation key `k` while open; returns whether it changed.
    pub fn key(&mut self, k: Nav) -> bool {
        if !self.open {
            return false;
        }
        let before = self.hover;
        self.hover = match k {
            Nav::Down => self.step(self.hover, true),
            Nav::Up => self.step(self.hover, false),
            Nav::Home | Nav::PageUp => self.edge(true),
            Nav::End | Nav::PageDown => self.edge(false),
        };
        self.hover != before
    }

    /// Choose the hovered item: returns its index and closes the menu, or `None`.
    pub fn enter(&mut self) -> Option<usize> {
        if !self.open {
            return None;
        }
        let h = self.hover?;
        if !self.is_enabled(h) {
            return None;
        }
        self.close();
        Some(h)
    }

    /// Whether item `i` exists and is enabled.
    fn is_enabled(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|it| it.enabled)
    }

    /// First (`forward`) or last enabled item.
    fn edge(&self, forward: bool) -> Option<usize> {
        if forward {
            self.items.iter().position(|it| it.enabled)
        } else {
            self.items.iter().rposition(|it| it.enabled)
        }
    }

    /// Next or previous enabled item from `from`, wrapping; from `None` gives the first or last.
    fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let n = self.items.len();
        if n == 0 {
            return None;
        }
        for k in 1..=n {
            let idx = match (from, forward) {
                (None, true) => k - 1,
                (None, false) => n - k,
                (Some(h), true) => h.saturating_add(k) % n,
                (Some(h), false) => h.saturating_add(n - k) % n,
            };
            if self.is_enabled(idx) {
                return Some(idx);
            }
        }
        None
    }
}
