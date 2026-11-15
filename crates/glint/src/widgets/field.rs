//! Single-line text field editing state. Cursor and anchor are char indices.

/// Editable text with a cursor and an optional selection anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldState {
    /// Current text.
    pub text: String,
    /// Cursor position in chars, `0..=char count`.
    pub cursor: usize,
    /// Selection anchor in chars; the selection runs from anchor to cursor.
    pub anchor: Option<usize>,
}

impl FieldState {
    /// New field holding `text`, cursor at the end, no selection.
    pub fn new(text: &str) -> FieldState {
        FieldState {
            text: text.to_string(),
            cursor: text.chars().count(),
            anchor: None,
        }
    }

    /// Insert `c` at the cursor, replacing any selection.
    pub fn insert(&mut self, c: char) {
        self.fix();
        self.delete_selection();
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
        self.anchor = None;
    }

    /// Delete the selection, or the char before the cursor; returns whether the state changed.
    pub fn backspace(&mut self) -> bool {
        self.fix();
        let before = self.clone();
        if !self.delete_selection() && self.cursor > 0 {
            let at = self.byte_at(self.cursor - 1);
            self.text.remove(at);
            self.cursor -= 1;
        }
        self.anchor = None;
        *self != before
    }

    /// Delete the selection, or the char after the cursor; returns whether the state changed.
    pub fn delete(&mut self) -> bool {
        self.fix();
        let before = self.clone();
        if !self.delete_selection() && self.cursor < self.len() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
        self.anchor = None;
        *self != before
    }

    /// Move left one char (or to the selection start); `shift` extends the selection.
    pub fn left(&mut self, shift: bool) -> bool {
        self.fix();
        let target = match self.selection() {
            Some((s, _)) if !shift => s,
            _ => self.cursor.saturating_sub(1),
        };
        self.go(target, shift)
    }

    /// Move right one char (or to the selection end); `shift` extends the selection.
    pub fn right(&mut self, shift: bool) -> bool {
        self.fix();
        let target = match self.selection() {
            Some((_, e)) if !shift => e,
            _ => self.cursor.saturating_add(1).min(self.len()),
        };
        self.go(target, shift)
    }

    /// Move to the start of the text; `shift` extends the selection.
    pub fn home(&mut self, shift: bool) -> bool {
        self.fix();
        self.go(0, shift)
    }

    /// Move to the end of the text; `shift` extends the selection.
    pub fn end(&mut self, shift: bool) -> bool {
        self.fix();
        self.go(self.len(), shift)
    }

    /// Select the whole text, anchor at the start and cursor at the end.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.len();
    }

    /// Selected char range as `(start, end)`, or `None` when nothing is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        match self.anchor {
            Some(a) if a != self.cursor => Some((a.min(self.cursor), a.max(self.cursor))),
            _ => None,
        }
    }

    /// Text of the current selection, empty when nothing is selected.
    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some((s, e)) => self
                .text
                .get(self.byte_at(s)..self.byte_at(e))
                .unwrap_or("")
                .to_string(),
            None => String::new(),
        }
    }

    /// Number of chars in the text.
    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Byte offset of char index `ci`; the text length when `ci` is past the end.
    fn byte_at(&self, ci: usize) -> usize {
        self.text
            .char_indices()
            .nth(ci)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Clamp the cursor and anchor into the text (guards against hand-edited fields).
    fn fix(&mut self) {
        let len = self.len();
        self.cursor = self.cursor.min(len);
        self.anchor = self.anchor.map(|a| a.min(len));
    }

    /// Remove the selected chars, leaving the cursor at the start; returns whether any were removed.
    fn delete_selection(&mut self) -> bool {
        let Some((s, e)) = self.selection() else {
            return false;
        };
        let (bs, be) = (self.byte_at(s), self.byte_at(e));
        self.text.replace_range(bs..be, "");
        self.cursor = s;
        true
    }

    /// Move the cursor to `target`, extending the selection when `shift` is set.
    fn go(&mut self, target: usize, shift: bool) -> bool {
        let before = self.clone();
        if !shift {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = target;
        *self != before
    }
}
