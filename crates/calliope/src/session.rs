//! Editing session for the terminal editor. It owns the text (through `Editor`, so undo is the
//! same as the rest of calliope), the cursor and selection, search, the internal clipboard and
//! the save state, and it renders one `Screen` per frame. Terminal input arrives as `Ev` values,
//! so every key sequence can be replayed in a test without a console.

use std::path::PathBuf;

use crate::cursor::Cursor;
use crate::file::{self, DecodeError, Eol};
use crate::highlight::Style;
use crate::input::{Ev, Mouse};
use crate::keys::Key;
use crate::screen::Screen;
use crate::syntax::{color_line, Lang, LineState};
use crate::undo::Editor;

const TAB: usize = 4;

/// A question the status line asks instead of normal editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// Ctrl-F: the text to search for.
    Find,
    /// Ctrl-G: the line number to jump to.
    GoTo,
    /// Ctrl-Q with unsaved changes: `y` saves and quits, `n` quits, Esc cancels.
    Quit,
}

/// One open file and everything the editor shows and does with it.
pub struct Session {
    path: Option<PathBuf>,
    eol: Eol,
    lang: Lang,
    editor: Editor,
    // Undo-tree node the text was last saved at (or opened at); dirty means "not there".
    saved: usize,
    version: u64,
    // Derived view of the text, rebuilt whenever `version` moves on.
    cache_version: Option<u64>,
    text: String,
    starts: Vec<usize>,
    // Tokenizer state at the start of each line that has been coloured so far.
    states: Vec<LineState>,
    top: usize,
    left: usize,
    width: usize,
    height: usize,
    prompt: Option<Prompt>,
    input: String,
    query: String,
    clipboard: String,
    message: String,
    drag_anchor: Option<usize>,
    quit: bool,
}

fn cell_width(x: usize, c: char) -> usize {
    if c == '\t' {
        TAB - x % TAB
    } else {
        1
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    starts
}

impl Session {
    /// Open `bytes` (the file contents, or empty for a new file) for a `width` x `height` screen.
    /// Fails when the bytes are not UTF-8, so the file is never rewritten in a changed form.
    pub fn from_bytes(
        path: Option<PathBuf>,
        bytes: &[u8],
        width: usize,
        height: usize,
    ) -> Result<Session, DecodeError> {
        let (text, eol) = file::decode(bytes)?;
        let lang = Lang::from_path(path.as_deref());
        let mut s = Session {
            path,
            eol,
            lang,
            editor: Editor::new(&text),
            saved: 0,
            version: 0,
            cache_version: None,
            text: String::new(),
            starts: Vec::new(),
            states: vec![LineState::default()],
            top: 0,
            left: 0,
            width: 1,
            height: 2,
            prompt: None,
            input: String::new(),
            query: String::new(),
            clipboard: String::new(),
            message: String::new(),
            drag_anchor: None,
            quit: false,
        };
        s.resize(width, height);
        Ok(s)
    }

    /// Set the screen size in cells; the status line takes the last row.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width.max(1);
        self.height = height.max(2);
        self.ensure_visible();
    }

    /// Screen size as `(width, height)`.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// True once the user has chosen to quit.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// True when the text differs from the last save (or was never saved).
    /// Undoing back to the saved state is clean again; redoing away from it is dirty.
    pub fn modified(&self) -> bool {
        self.editor.current_node() != self.saved
    }

    /// The current text (line endings as the editor holds them: `\n`).
    pub fn text(&self) -> String {
        self.editor.buffer.text()
    }

    /// The line-ending style that a save will write.
    pub fn eol(&self) -> Eol {
        self.eol
    }

    /// The message shown in the status line.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The open prompt, if any.
    pub fn prompt(&self) -> Option<Prompt> {
        self.prompt
    }

    /// Cursor position as 1-based `(line, column)`; the column counts display cells.
    pub fn cursor_line_col(&mut self) -> (usize, usize) {
        self.sync();
        let head = self.cursor().head;
        (self.line_of(head) + 1, self.col_of(head) + 1)
    }

    /// Write the text to the file it was opened from. Returns `false` (with a message) when
    /// there is no file name or the write fails.
    pub fn save(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            self.message = "no file name: start calliope with a FILE".to_string();
            return false;
        };
        let bytes = file::encode(&self.editor.buffer.text(), self.eol).into_bytes();
        match file::save_atomic(&path, &bytes) {
            Ok(()) => {
                self.saved = self.editor.current_node();
                self.message = format!("saved {} bytes", bytes.len());
                true
            }
            Err(e) => {
                self.message = format!("save failed: {e}");
                false
            }
        }
    }

    /// Apply one input event.
    pub fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Key(key) => match self.prompt {
                Some(p) => self.prompt_key(p, key),
                None => self.normal_key(key, false),
            },
            Ev::Shift(key) => {
                if self.prompt.is_none() {
                    self.normal_key(key, true);
                }
            }
            Ev::Mouse(m) => {
                if self.prompt.is_none() {
                    self.mouse(m);
                }
            }
        }
    }

    /// Rows of text above the status line.
    fn text_rows(&self) -> usize {
        self.height - 1
    }

    /// Rebuild the line index and text snapshot if the buffer changed.
    fn sync(&mut self) {
        if self.cache_version == Some(self.version) {
            return;
        }
        self.text = self.editor.buffer.text();
        self.starts = line_starts(&self.text);
        self.cache_version = Some(self.version);
    }

    fn cursor(&self) -> Cursor {
        self.editor
            .cursors()
            .first()
            .copied()
            .unwrap_or(Cursor { anchor: 0, head: 0 })
    }

    fn line_of(&self, off: usize) -> usize {
        self.starts.partition_point(|&s| s <= off).saturating_sub(1)
    }

    /// Text of `line` without its line ending (and without a trailing `\r`).
    fn line_text(&self, line: usize) -> &str {
        let start = self.starts[line];
        let end = match self.starts.get(line + 1) {
            Some(&next) => next - 1,
            None => self.text.len(),
        };
        let raw = &self.text[start..end];
        raw.strip_suffix('\r').unwrap_or(raw)
    }

    /// Display column of byte offset `off` on its line.
    fn col_of(&self, off: usize) -> usize {
        let line = self.line_of(off);
        let start = self.starts[line];
        let mut x = 0;
        for c in self.text[start..off].chars() {
            x += cell_width(x, c);
        }
        x
    }

    /// Byte offset on `line` at display column `dcol` (clamped to the line end).
    fn offset_at(&self, line: usize, dcol: usize) -> usize {
        let text = self.line_text(line);
        let mut x = 0;
        for (b, c) in text.char_indices() {
            let w = cell_width(x, c);
            if x + w > dcol {
                return self.starts[line] + b;
            }
            x += w;
        }
        self.starts[line] + text.len()
    }

    fn prev_boundary(&self, off: usize) -> usize {
        self.text[..off]
            .chars()
            .next_back()
            .map_or(0, |c| off - c.len_utf8())
    }

    fn next_boundary(&self, off: usize) -> usize {
        self.text[off..]
            .chars()
            .next()
            .map_or(self.text.len(), |c| off + c.len_utf8())
    }

    /// Make the cursor visible, scrolling vertically and horizontally as needed.
    fn ensure_visible(&mut self) {
        self.sync();
        let head = self.cursor().head;
        let line = self.line_of(head);
        let rows = self.text_rows();
        if line < self.top {
            self.top = line;
        } else if line >= self.top + rows {
            self.top = line + 1 - rows;
        }
        let col = self.col_of(head);
        if col < self.left {
            self.left = col;
        } else if col >= self.left + self.width {
            self.left = col + 1 - self.width;
        }
    }

    /// Replace the cursor with a caret or selection and scroll it into view.
    fn place(&mut self, anchor: usize, head: usize) {
        self.editor.set_cursors(vec![Cursor { anchor, head }]);
        self.ensure_visible();
    }

    /// Run an editor operation. The line-state cache is cut back at the edited line, so
    /// colouring a large file does not restart after every keystroke. Returns whether the
    /// operation changed the text; only then is the file marked modified.
    fn mutate(&mut self, op: impl FnOnce(&mut Editor) -> bool) -> bool {
        self.sync();
        let before = self.line_of(self.cursor().start());
        let changed = op(&mut self.editor);
        if changed {
            self.version += 1;
            self.sync();
            let after = self.line_of(self.cursor().start());
            self.states.truncate(before.min(after) + 1);
        }
        self.ensure_visible();
        changed
    }

    fn insert(&mut self, s: &str) {
        self.mutate(|e| {
            e.type_text(s);
            true
        });
    }

    fn selection(&self) -> (usize, usize) {
        let c = self.cursor();
        (c.start(), c.end())
    }

    fn normal_key(&mut self, key: Key, extend: bool) {
        self.sync();
        match key {
            Key::Char(c) => {
                let mut buf = [0u8; 4];
                self.insert(c.encode_utf8(&mut buf));
            }
            Key::Enter => self.insert("\n"),
            Key::Tab => self.insert("\t"),
            Key::Backspace => {
                let (s, e) = self.selection();
                if s != e || s > 0 {
                    self.mutate(|ed| {
                        ed.backspace();
                        true
                    });
                }
            }
            Key::Delete => {
                let (s, e) = self.selection();
                if s == e {
                    if e < self.text.len() {
                        let next = self.next_boundary(e);
                        self.place(e, next);
                        self.insert("");
                    }
                } else {
                    self.insert("");
                }
            }
            Key::Left
            | Key::Right
            | Key::Up
            | Key::Down
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown => self.navigate(key, extend),
            Key::Esc => {
                let head = self.cursor().head;
                self.place(head, head);
            }
            Key::Ctrl(c) => self.ctrl(c),
        }
    }

    fn navigate(&mut self, key: Key, extend: bool) {
        self.sync();
        let c = self.cursor();
        let (s, e) = (c.start(), c.end());
        let has_sel = s != e;
        let line = self.line_of(c.head);
        let col = self.col_of(c.head);
        let rows = self.text_rows();
        let last = self.starts.len() - 1;
        let target = match key {
            Key::Left if has_sel && !extend => s,
            Key::Left => self.prev_boundary(c.head),
            Key::Right if has_sel && !extend => e,
            Key::Right => self.next_boundary(c.head),
            Key::Up if line == 0 => 0,
            Key::Up => self.offset_at(line - 1, col),
            Key::Down if line == last => self.text.len(),
            Key::Down => self.offset_at(line + 1, col),
            Key::Home => self.starts[line],
            Key::End => self.starts[line] + self.line_text(line).len(),
            Key::PageUp => self.offset_at(line.saturating_sub(rows), col),
            Key::PageDown => self.offset_at((line + rows).min(last), col),
            _ => c.head,
        };
        let anchor = if extend { c.anchor } else { target };
        self.place(anchor, target);
    }

    fn ctrl(&mut self, c: char) {
        match c {
            's' => {
                self.save();
            }
            'q' => {
                if self.modified() {
                    self.prompt = Some(Prompt::Quit);
                    self.message =
                        "Unsaved changes: y = save and quit, n = quit without saving, Esc = cancel"
                            .to_string();
                } else {
                    self.quit = true;
                }
            }
            'z' => {
                if !self.mutate(|e| e.undo()) {
                    self.message = "nothing to undo".to_string();
                }
            }
            'y' => {
                if !self.mutate(|e| e.redo()) {
                    self.message = "nothing to redo".to_string();
                }
            }
            'c' => {
                self.copy();
            }
            'x' => {
                if self.copy() {
                    self.insert("");
                }
            }
            'v' => {
                if self.clipboard.is_empty() {
                    self.message = "clipboard is empty".to_string();
                } else {
                    let text = self.clipboard.clone();
                    self.insert(&text);
                }
            }
            'f' => self.open_prompt(Prompt::Find),
            'g' => self.open_prompt(Prompt::GoTo),
            'n' => {
                self.search(true);
            }
            'p' => {
                self.search(false);
            }
            _ => {}
        }
    }

    fn open_prompt(&mut self, p: Prompt) {
        self.prompt = Some(p);
        self.input.clear();
        self.message.clear();
    }

    /// Copy the selection to the internal clipboard; `false` when nothing is selected.
    fn copy(&mut self) -> bool {
        self.sync();
        let (s, e) = self.selection();
        if s == e {
            self.message = "nothing selected".to_string();
            return false;
        }
        self.clipboard = self.text[s..e].to_string();
        self.message = format!("copied {} bytes", e - s);
        true
    }

    /// Select the next (or previous) match of the search text, wrapping at the ends.
    fn search(&mut self, forward: bool) -> bool {
        self.sync();
        if self.query.is_empty() {
            self.message = "no search text: press Ctrl-F first".to_string();
            return false;
        }
        let q = self.query.clone();
        let (s, e) = self.selection();
        let found = if forward {
            self.text[e..]
                .find(&q)
                .map(|i| e + i)
                .or_else(|| self.text.find(&q))
        } else {
            self.text[..s].rfind(&q).or_else(|| self.text.rfind(&q))
        };
        match found {
            Some(p) => {
                self.place(p, p + q.len());
                self.message = format!("found \"{q}\"");
                true
            }
            None => {
                self.message = format!("not found: \"{q}\"");
                false
            }
        }
    }

    fn goto(&mut self, text: &str) {
        self.sync();
        let t = text.trim();
        match t.parse::<usize>() {
            Ok(n) if (1..=self.starts.len()).contains(&n) => {
                let off = self.starts[n - 1];
                self.place(off, off);
                self.message = format!("line {n}");
            }
            _ => self.message = format!("no line {t}"),
        }
    }

    fn prompt_key(&mut self, p: Prompt, key: Key) {
        if p == Prompt::Quit {
            match key {
                Key::Char('y' | 'Y') => {
                    self.prompt = None;
                    if self.save() {
                        self.quit = true;
                    }
                }
                Key::Char('n' | 'N') => self.quit = true,
                Key::Esc => {
                    self.prompt = None;
                    self.message.clear();
                }
                _ => {}
            }
            return;
        }
        match key {
            Key::Esc => {
                self.prompt = None;
                self.input.clear();
                self.message.clear();
            }
            Key::Backspace => {
                self.input.pop();
            }
            Key::Char(c) => self.input.push(c),
            Key::Enter => {
                let text = std::mem::take(&mut self.input);
                self.prompt = None;
                match p {
                    Prompt::Find => {
                        self.query = text;
                        self.search(true);
                    }
                    Prompt::GoTo => self.goto(&text),
                    Prompt::Quit => {}
                }
            }
            _ => {}
        }
    }

    fn mouse(&mut self, m: Mouse) {
        self.sync();
        let rows = self.text_rows();
        match m {
            Mouse::Down(r, c) if r < rows => {
                let off = self.point(r, c);
                self.drag_anchor = Some(off);
                self.place(off, off);
            }
            Mouse::Drag(r, c) => {
                if let Some(anchor) = self.drag_anchor {
                    let off = self.point(r.min(rows - 1), c);
                    self.place(anchor, off);
                }
            }
            Mouse::Up => self.drag_anchor = None,
            Mouse::Wheel(d) => self.scroll(d),
            Mouse::Down(..) => {}
        }
    }

    /// Byte offset under text-area cell `(row, col)`.
    fn point(&self, row: usize, col: usize) -> usize {
        let line = (self.top + row).min(self.starts.len() - 1);
        self.offset_at(line, self.left + col)
    }

    /// Scroll the view by `d` lines; the cursor moves only if it would leave the view.
    fn scroll(&mut self, d: isize) {
        self.sync();
        let last = self.starts.len() - 1;
        let rows = self.text_rows();
        self.top = self.top.saturating_add_signed(d).min(last);
        let c = self.cursor();
        let line = self.line_of(c.head);
        let visible = line.clamp(self.top, (self.top + rows - 1).min(last));
        if visible != line {
            let off = self.offset_at(visible, self.col_of(c.head));
            self.place(c.anchor, off);
        }
    }

    fn status_line(&self) -> String {
        if let Some(p) = self.prompt {
            return match p {
                Prompt::Find => format!("Find: {}", self.input),
                Prompt::GoTo => format!("Go to line: {}", self.input),
                Prompt::Quit => self.message.clone(),
            };
        }
        let name = self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "[new file]".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let head = self.cursor().head;
        let line = self.line_of(head) + 1;
        let col = self.col_of(head) + 1;
        let dirty = if self.modified() { " [+]" } else { "" };
        let eol = match self.eol {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
            Eol::Mixed => "mixed",
        };
        format!(
            " {name}{dirty}  {line}:{col}  {} lines  {eol}  {}",
            self.starts.len(),
            self.message
        )
    }

    /// Colour and lay out one frame. Call after every batch of input events.
    pub fn render(&mut self) -> Screen {
        self.sync();
        let rows = self.text_rows();
        let w = self.width;
        let last = self.starts.len();
        let stateful = matches!(self.lang, Lang::Rust | Lang::Markdown);
        if stateful {
            let need = (self.top + rows).min(last);
            while self.states.len() < need {
                let i = self.states.len() - 1;
                let (_, next) = color_line(self.lang, self.line_text(i), self.states[i]);
                self.states.push(next);
            }
        }
        let (sel_s, sel_e) = self.selection();
        let mut out_rows = Vec::with_capacity(self.height);
        let mut out_styles = Vec::with_capacity(self.height);
        for r in 0..rows {
            let line = self.top + r;
            let (row, styles) = if line < last {
                self.render_line(line, sel_s, sel_e, stateful)
            } else {
                (" ".repeat(w), vec![Style::Plain; w])
            };
            out_rows.push(row);
            out_styles.push(styles);
        }
        let status: Vec<char> = self.status_line().chars().collect();
        let status_row: String = (0..w)
            .map(|k| status.get(k).copied().unwrap_or(' '))
            .collect();
        out_rows.push(status_row);
        out_styles.push(vec![Style::Status; w]);

        let cursor = if let Some(p) = self.prompt {
            let label = match p {
                Prompt::Find => "Find: ".chars().count() + self.input.chars().count(),
                Prompt::GoTo => "Go to line: ".chars().count() + self.input.chars().count(),
                Prompt::Quit => self.status_line().chars().count(),
            };
            (self.height - 1, label.min(w - 1))
        } else {
            let head = self.cursor().head;
            let line = self.line_of(head);
            let row = line.saturating_sub(self.top).min(rows - 1);
            let col = self.col_of(head).saturating_sub(self.left).min(w - 1);
            (row, col)
        };
        Screen {
            rows: out_rows,
            styles: out_styles,
            cursor,
        }
    }

    fn render_line(
        &self,
        line: usize,
        sel_s: usize,
        sel_e: usize,
        stateful: bool,
    ) -> (String, Vec<Style>) {
        let text = self.line_text(line);
        let base = self.starts[line];
        let syntax = match self.lang {
            Lang::Plain => Vec::new(),
            lang => {
                let st = if stateful {
                    self.states[line]
                } else {
                    LineState::default()
                };
                color_line(lang, text, st).0
            }
        };
        let mut cells: Vec<(char, Style)> = Vec::with_capacity(text.len());
        let mut x = 0;
        for (ci, (b, c)) in text.char_indices().enumerate() {
            let abs = base + b;
            let selected = abs >= sel_s && abs < sel_e;
            let syn = syntax.get(ci).copied().unwrap_or(Style::Plain);
            let st = if selected { Style::Selection } else { syn };
            let w = cell_width(x, c);
            if c == '\t' {
                for _ in 0..w {
                    cells.push((' ', st));
                }
            } else {
                cells.push((c, st));
            }
            x += w;
        }
        let line_end = base + text.len();
        if sel_s <= line_end && sel_e > line_end && line + 1 < self.starts.len() {
            cells.push((' ', Style::Selection));
        }
        let mut row = String::with_capacity(self.width);
        let mut styles = Vec::with_capacity(self.width);
        for k in 0..self.width {
            let (ch, st) = cells
                .get(self.left + k)
                .copied()
                .unwrap_or((' ', Style::Plain));
            row.push(ch);
            styles.push(st);
        }
        (row, styles)
    }
}
