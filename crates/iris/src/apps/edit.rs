//! Text editor application backed by `calliope::Editor` and a shared `hades` filesystem.

use calliope::{Cursor, Editor as CalliopeEditor};
use hades::{Error as FsError, Fs, MemDisk};
use indium::{Event, Key};
use pigment::{Canvas, Rect, Rgba};

use crate::app::{
    fill_bg, grid_dims, paint_cursor_bar, paint_row, App, Effect, SharedFs, CELL_H, CELL_W,
};
use crate::window::CONTENT_BG_COLOR;

const EDIT_FG: Rgba = Rgba(235, 235, 240, 255);
const EDIT_CURSOR: Rgba = Rgba(90, 165, 255, 255);

/// Text editor application editing a file stored on the shared `hades` filesystem.
pub struct Editor {
    fs: SharedFs,
    path: String,
    title_clean: String,
    title_dirty: String,
    inner: CalliopeEditor,
    unsaved: bool,
    scroll_row: usize,
    scroll_col: usize,
    dirty: bool,
    last_size: (u32, u32),
}

impl Editor {
    /// Open `path` from `fs` (starting with an empty buffer if the file does not yet exist).
    pub fn new(fs: SharedFs, path: &str) -> Self {
        let initial = {
            let mut guard = fs.borrow_mut();
            read_file_text(&mut guard, path)
        };
        Self {
            fs,
            path: path.to_owned(),
            title_clean: path.to_owned(),
            title_dirty: format!("{path}*"),
            inner: CalliopeEditor::new(&initial),
            unsaved: false,
            scroll_row: 0,
            scroll_col: 0,
            dirty: true,
            last_size: (0, 0),
        }
    }

    /// Path of the file being edited.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Full text contents of the buffer.
    pub fn text(&self) -> String {
        self.inner.buffer.text()
    }

    /// Whether the buffer has unsaved edits.
    pub fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    /// Current byte offset of the primary cursor.
    pub fn cursor(&self) -> usize {
        self.inner.cursors().first().map_or(0, |c| c.head)
    }

    /// Save the buffer contents to `self.path` on the shared `hades` filesystem.
    pub fn save(&mut self) -> Result<(), FsError> {
        let content = self.inner.buffer.text();
        let bytes = content.as_bytes();
        let mut fs = self.fs.borrow_mut();
        match fs.stat(&self.path) {
            Ok(_) => {}
            Err(FsError::NotFound) => fs.create(&self.path)?,
            Err(e) => return Err(e),
        }
        if !bytes.is_empty() {
            fs.write(&self.path, 0, bytes)?;
        }
        fs.truncate(&self.path, bytes.len() as u64)?;
        self.unsaved = false;
        self.dirty = true;
        Ok(())
    }

    fn set_cursor(&mut self, pos: usize) {
        let clamped = pos.min(self.inner.buffer.len());
        if clamped != self.cursor() {
            self.inner.set_cursors(vec![Cursor {
                anchor: clamped,
                head: clamped,
            }]);
            self.dirty = true;
        }
    }

    fn cursor_row_col(&self) -> (usize, usize) {
        let pos = self.cursor();
        let row = self.inner.buffer.line_of(pos);
        let start = self.inner.buffer.line_start(row).unwrap_or(0);
        let col = self.inner.buffer.slice(start, pos).chars().count();
        (row, col)
    }

    fn line_end(&self, line: usize) -> usize {
        let len = self.inner.buffer.len();
        self.inner
            .buffer
            .line_start(line + 1)
            .map_or(len, |next| next.saturating_sub(1))
    }

    fn offset_at(&self, line: usize, target_col: usize) -> usize {
        let line_count = self.inner.buffer.line_count();
        let clamped_line = line.min(line_count.saturating_sub(1));
        let start = self.inner.buffer.line_start(clamped_line).unwrap_or(0);
        let end = self.line_end(clamped_line);
        let text = self.inner.buffer.slice(start, end);
        let mut byte_off = 0usize;
        for (idx, ch) in text.chars().enumerate() {
            if idx >= target_col {
                break;
            }
            byte_off += ch.len_utf8();
        }
        start.saturating_add(byte_off)
    }

    fn move_left(&mut self) {
        let pos = self.cursor();
        if pos == 0 {
            return;
        }
        let line = self.inner.buffer.line_of(pos);
        let start = self.inner.buffer.line_start(line).unwrap_or(0);
        if pos > start {
            let prefix = self.inner.buffer.slice(start, pos);
            let step = prefix.chars().next_back().map_or(0, char::len_utf8);
            self.set_cursor(pos.saturating_sub(step));
        } else if line > 0 {
            self.set_cursor(start.saturating_sub(1));
        }
    }

    fn move_right(&mut self) {
        let pos = self.cursor();
        let len = self.inner.buffer.len();
        if pos >= len {
            return;
        }
        let line = self.inner.buffer.line_of(pos);
        let next_start = self.inner.buffer.line_start(line + 1).unwrap_or(len);
        if pos < next_start {
            let tail = self.inner.buffer.slice(pos, next_start);
            let step = tail.chars().next().map_or(0, char::len_utf8);
            self.set_cursor(pos.saturating_add(step));
        }
    }

    fn move_vertical(&mut self, delta: isize) {
        let (row, col) = self.cursor_row_col();
        let max_row = self.inner.buffer.line_count().saturating_sub(1);
        let target_row = if delta < 0 {
            row.saturating_sub(delta.unsigned_abs())
        } else {
            row.saturating_add(delta as usize).min(max_row)
        };
        if target_row != row {
            let pos = self.offset_at(target_row, col);
            self.set_cursor(pos);
        }
    }

    fn insert_str(&mut self, s: &str) {
        if !s.is_empty() {
            self.inner.type_text(s);
            self.unsaved = true;
            self.dirty = true;
        }
    }

    fn do_backspace(&mut self) {
        if self.cursor() > 0 {
            self.inner.backspace();
            self.unsaved = true;
            self.dirty = true;
        }
    }

    fn ensure_cursor_visible(&mut self, cols: usize, rows: usize) {
        let (cur_row, cur_col) = self.cursor_row_col();
        if rows > 0 {
            if cur_row < self.scroll_row {
                self.scroll_row = cur_row;
            } else if cur_row >= self.scroll_row.saturating_add(rows) {
                self.scroll_row = cur_row.saturating_sub(rows - 1);
            }
        }
        if cols > 0 {
            if cur_col < self.scroll_col {
                self.scroll_col = cur_col;
            } else if cur_col >= self.scroll_col.saturating_add(cols) {
                self.scroll_col = cur_col.saturating_sub(cols - 1);
            }
        }
    }
}

impl App for Editor {
    fn title(&self) -> &str {
        if self.unsaved {
            &self.title_dirty
        } else {
            &self.title_clean
        }
    }

    fn event(&mut self, ev: Event, _now_ms: u64) -> Effect {
        match ev {
            Event::Key(Key::Space) => self.insert_str(" "),
            Event::Key(Key::Enter) => self.insert_str("\n"),
            Event::Key(Key::Tab) => self.insert_str("    "),
            Event::Key(Key::Backspace) => self.do_backspace(),
            Event::Text('\x13') => {
                let _ = self.save();
            }
            Event::Text('\u{2190}') | Event::Text('\x02') => self.move_left(),
            Event::Text('\u{2192}') | Event::Text('\x06') => self.move_right(),
            Event::Text('\u{2191}') | Event::Text('\x10') => self.move_vertical(-1),
            Event::Text('\u{2193}') | Event::Text('\x0e') => self.move_vertical(1),
            Event::Text('\x08') | Event::Text('\x7f') => self.do_backspace(),
            Event::Text('\n') | Event::Text('\r') => self.insert_str("\n"),
            Event::Text(ch) if !ch.is_control() => {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                self.insert_str(s);
            }
            Event::MouseDown { x, y } if x >= 0 && y >= 0 => {
                let row = self
                    .scroll_row
                    .saturating_add((y as usize) / (CELL_H as usize));
                let col = self
                    .scroll_col
                    .saturating_add((x as usize) / (CELL_W as usize));
                let pos = self.offset_at(row, col);
                self.set_cursor(pos);
            }
            _ => {}
        }
        Effect::None
    }

    fn paint(&mut self, c: &mut Canvas) -> Option<Rect> {
        let size = (c.width, c.height);
        if c.width == 0 || c.height == 0 {
            self.dirty = false;
            self.last_size = size;
            return None;
        }
        if !self.dirty && self.last_size == size {
            return None;
        }
        self.dirty = false;
        self.last_size = size;

        fill_bg(c, CONTENT_BG_COLOR);
        let (cols, rows) = grid_dims(c);
        if cols == 0 || rows == 0 {
            return Some(Rect {
                x: 0,
                y: 0,
                w: c.width,
                h: c.height,
            });
        }

        self.ensure_cursor_visible(cols, rows);
        let total_lines = self.inner.buffer.line_count();
        for vis_row in 0..rows {
            let line_idx = self.scroll_row.saturating_add(vis_row);
            if line_idx >= total_lines {
                break;
            }
            let start = self.inner.buffer.line_start(line_idx).unwrap_or(0);
            let end = self.line_end(line_idx);
            let raw = self.inner.buffer.slice(start, end);
            let line_trimmed = raw.strip_suffix('\r').unwrap_or(&raw);
            let visible: String = line_trimmed
                .chars()
                .skip(self.scroll_col)
                .take(cols)
                .collect();
            paint_row(c, vis_row, &visible, EDIT_FG, CONTENT_BG_COLOR, cols);
        }

        let (cur_row, cur_col) = self.cursor_row_col();
        if cur_row >= self.scroll_row && cur_col >= self.scroll_col {
            let vr = cur_row - self.scroll_row;
            let vc = cur_col - self.scroll_col;
            if vr < rows && vc <= cols {
                paint_cursor_bar(c, vr, vc.min(cols.saturating_sub(1)), EDIT_CURSOR);
            }
        }

        Some(Rect {
            x: 0,
            y: 0,
            w: c.width,
            h: c.height,
        })
    }
}

fn read_file_text(fs: &mut Fs<MemDisk>, path: &str) -> String {
    let mut bytes = Vec::new();
    let mut offset = 0u64;
    let mut chunk = [0u8; 4096];
    loop {
        match fs.read(path, offset, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(slice) = chunk.get(..n) {
                    bytes.extend_from_slice(slice);
                }
                offset = offset.saturating_add(n as u64);
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::cell::RefCell;
    use std::rc::Rc;

    use hades::{mkfs, MkfsOptions};

    use super::*;

    fn test_fs() -> SharedFs {
        let mut disk = MemDisk::new(128);
        mkfs(&mut disk, MkfsOptions::default()).unwrap();
        let fs = Fs::mount(disk).unwrap();
        Rc::new(RefCell::new(fs))
    }

    #[test]
    fn type_arrows_backspace_and_save() {
        let fs = test_fs();
        let mut ed = Editor::new(Rc::clone(&fs), "/notes.txt");
        assert_eq!(ed.title(), "/notes.txt");

        for ch in "ab\ncd".chars() {
            ed.event(Event::Text(ch), 0);
        }
        assert_eq!(ed.text(), "ab\ncd");
        assert_eq!(ed.title(), "/notes.txt*");

        // Move up, left, insert 'X', move right, backspace.
        ed.event(Event::Text('\u{2191}'), 1);
        ed.event(Event::Text('\u{2190}'), 2);
        ed.event(Event::Text('X'), 3);
        assert_eq!(ed.text(), "aXb\ncd");

        ed.event(Event::Text('\u{2192}'), 4);
        ed.event(Event::Key(Key::Backspace), 5);
        assert_eq!(ed.text(), "aX\ncd");

        // Ctrl+S saves and clears '*'.
        ed.event(Event::Text('\x13'), 6);
        assert!(!ed.is_unsaved());
        assert_eq!(ed.title(), "/notes.txt");

        let mut buf = [0u8; 32];
        let n = fs.borrow_mut().read("/notes.txt", 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"aX\ncd");
    }
}
