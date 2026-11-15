//! Modal editor application state, Insert/Search mode handling, and buffer helpers.

use std::path::Path;

use crate::keys::Key;
use crate::regex::Regex;
use crate::screen::{build_screen, visual_col, RenderInput, Screen};
use crate::undo::Editor;

/// Editor interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigation and normal-mode commands.
    Normal,
    /// Text insertion mode.
    Insert,
    /// Ex command-line mode (`:`).
    Command,
    /// Forward regex search mode (`/`).
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Register {
    Empty,
    Char(String),
    Line(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingOp {
    None,
    G,
    Delete,
    Yank,
}

/// Modal text editor application.
pub struct App {
    pub(crate) editor: Editor,
    pub(crate) name: Option<String>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) mode: Mode,
    pub(crate) cursor_line: usize,
    pub(crate) cursor_col: usize,
    pub(crate) sticky_col: usize,
    pub(crate) scroll_row: usize,
    pub(crate) scroll_col: usize,
    pub(crate) count: Option<usize>,
    pub(crate) pending: PendingOp,
    pub(crate) register: Register,
    pub(crate) saved_node: Option<usize>,
    pub(crate) insert_base: Option<String>,
    pub(crate) prompt_buf: String,
    pub(crate) last_search: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) quit_requested: bool,
}

impl App {
    /// Create a new unnamed scratch buffer.
    pub fn new(width: usize, height: usize) -> App {
        Self::init(None, "", width, height)
    }

    /// Create a buffer with the given `name` and initial `text`.
    pub fn with_text(name: &str, text: &str, width: usize, height: usize) -> App {
        let buf_name = if name.is_empty() {
            None
        } else {
            Some(String::from(name))
        };
        Self::init(buf_name, text, width, height)
    }

    /// Open a file at `path`; a missing file opens as an empty, unmodified buffer.
    pub fn open(path: &Path, width: usize, height: usize) -> std::io::Result<App> {
        let name = path.display().to_string();
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::init(Some(name), &text, width, height)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self::init(Some(name), "", width, height))
            }
            Err(e) => Err(e),
        }
    }

    fn init(name: Option<String>, text: &str, width: usize, height: usize) -> App {
        let editor = Editor::new(text);
        let saved_node = Some(editor.current_node());
        App {
            editor,
            name,
            width,
            height,
            mode: Mode::Normal,
            cursor_line: 0,
            cursor_col: 0,
            sticky_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            count: None,
            pending: PendingOp::None,
            register: Register::Empty,
            saved_node,
            insert_base: None,
            prompt_buf: String::new(),
            last_search: None,
            message: None,
            quit_requested: false,
        }
    }

    /// Process a single key event.
    pub fn handle(&mut self, key: Key) {
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::Insert => self.handle_insert(key),
            Mode::Search => self.handle_search(key),
            Mode::Command => self.handle_command(key),
        }
    }

    /// Render the current screen state, advancing scroll and consuming any one-shot message.
    pub fn render(&mut self) -> Screen {
        self.update_scroll();
        let full_text = self.editor.buffer.text();
        let name_str = self.name.as_deref().unwrap_or("");
        let display_name = self.name.as_deref().unwrap_or("[scratch]");
        let one_shot_msg = self.message.take();

        let (status_text, status_cursor_col) = if let Some(msg) = one_shot_msg {
            (msg, None)
        } else {
            match self.mode {
                Mode::Search => {
                    let s = format!("/{}", self.prompt_buf);
                    let col = 1 + self.prompt_buf.chars().count();
                    (s, Some(col))
                }
                Mode::Command => {
                    let s = format!(":{}", self.prompt_buf);
                    let col = 1 + self.prompt_buf.chars().count();
                    (s, Some(col))
                }
                Mode::Normal | Mode::Insert => {
                    let mode_label = if self.mode == Mode::Normal {
                        "NORMAL"
                    } else {
                        "INSERT"
                    };
                    let mod_flag = if self.modified() { " [+]" } else { "" };
                    let s = format!(
                        "{} {}{} {}:{}",
                        mode_label,
                        display_name,
                        mod_flag,
                        self.cursor_line + 1,
                        self.cursor_col + 1
                    );
                    (s, None)
                }
            }
        };

        build_screen(RenderInput {
            text: &full_text,
            name: name_str,
            width: self.width,
            height: self.height,
            cursor: (self.cursor_line, self.cursor_col),
            scroll_row: self.scroll_row,
            scroll_col: self.scroll_col,
            last_search: self.last_search.as_deref(),
            status_text: &status_text,
            status_cursor_col,
        })
    }

    /// Return the full buffer contents as a `String`.
    pub fn text(&self) -> String {
        self.editor.buffer.text()
    }

    /// Return the current cursor `(line, column in chars)`, both 0-based.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_line, self.cursor_col)
    }

    /// Return the current editor mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Return `true` if the buffer has unsaved modifications.
    pub fn modified(&self) -> bool {
        if let Some(base) = &self.insert_base {
            if self.editor.buffer.text() != *base {
                return true;
            }
        }
        self.saved_node != Some(self.editor.current_node())
    }

    /// Return `true` if `:q`, `:q!`, or `:wq` requested exit.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Resize the viewport dimensions.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    fn update_scroll(&mut self) {
        let text_rows = self.height.saturating_sub(1);
        if self.cursor_line < self.scroll_row {
            self.scroll_row = self.cursor_line;
        } else if text_rows > 0 && self.cursor_line >= self.scroll_row + text_rows {
            self.scroll_row = self.cursor_line - text_rows + 1;
        }

        let line_s = self.line_str(self.cursor_line);
        let vcol = visual_col(&line_s, self.cursor_col);
        if vcol < self.scroll_col {
            self.scroll_col = vcol;
        } else if self.width > 0 && vcol >= self.scroll_col + self.width {
            self.scroll_col = vcol - self.width + 1;
        }
    }

    fn handle_insert(&mut self, key: Key) {
        match key {
            Key::Esc => self.exit_insert(),
            Key::Char(c) => {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                let byte = self.line_col_to_byte(self.cursor_line, self.cursor_col);
                self.editor.buffer.insert(byte, s);
                self.cursor_col += 1;
            }
            Key::Enter => {
                let byte = self.line_col_to_byte(self.cursor_line, self.cursor_col);
                self.editor.buffer.insert(byte, "\n");
                self.cursor_line += 1;
                self.cursor_col = 0;
            }
            Key::Tab => {
                let byte = self.line_col_to_byte(self.cursor_line, self.cursor_col);
                self.editor.buffer.insert(byte, "\t");
                self.cursor_col += 1;
            }
            Key::Backspace => {
                if self.cursor_col > 0 {
                    let end = self.line_col_to_byte(self.cursor_line, self.cursor_col);
                    let start = self.line_col_to_byte(self.cursor_line, self.cursor_col - 1);
                    self.editor.buffer.delete(start, end);
                    self.cursor_col -= 1;
                } else if self.cursor_line > 0 {
                    let prev_len = self.line_char_len(self.cursor_line - 1);
                    let byte = self.line_col_to_byte(self.cursor_line, 0);
                    if byte > 0 {
                        self.editor.buffer.delete(byte - 1, byte);
                        self.cursor_line -= 1;
                        self.cursor_col = prev_len;
                    }
                }
            }
            Key::Delete => {
                let line_len = self.line_char_len(self.cursor_line);
                if self.cursor_col < line_len {
                    let start = self.line_col_to_byte(self.cursor_line, self.cursor_col);
                    let end = self.line_col_to_byte(self.cursor_line, self.cursor_col + 1);
                    self.editor.buffer.delete(start, end);
                } else if self.cursor_line + 1 < self.line_count() {
                    let (_, end) = self.line_byte_range(self.cursor_line);
                    if end < self.editor.buffer.len() {
                        self.editor.buffer.delete(end, end + 1);
                    }
                }
            }
            Key::Left => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            Key::Right => {
                let max = self.line_char_len(self.cursor_line);
                self.cursor_col = (self.cursor_col + 1).min(max);
            }
            Key::Up => {
                if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    let max = self.line_char_len(self.cursor_line);
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            Key::Down => {
                if self.cursor_line + 1 < self.line_count() {
                    self.cursor_line += 1;
                    let max = self.line_char_len(self.cursor_line);
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            Key::Home => {
                self.cursor_col = 0;
            }
            Key::End => {
                self.cursor_col = self.line_char_len(self.cursor_line);
            }
            _ => {}
        }
    }

    pub(crate) fn enter_insert(&mut self, line: usize, col: usize) {
        self.insert_base = Some(self.editor.buffer.text());
        self.cursor_line = line;
        self.cursor_col = col;
        self.mode = Mode::Insert;
    }

    fn exit_insert(&mut self) {
        self.mode = Mode::Normal;
        self.cursor_col = self.cursor_col.saturating_sub(1);
        self.clamp_normal_cursor();
        self.sticky_col = self.cursor_col;

        if let Some(base) = self.insert_base.take() {
            let after = self.editor.buffer.text();
            if base != after {
                let (prefix, base_end, after_end) = diff_bounds(&base, &after);
                let base_mid = &base[prefix..base_end];
                let after_mid = &after[prefix..after_end];
                self.editor.buffer.delete(prefix, after_end);
                self.editor.buffer.insert(prefix, base_mid);
                self.editor.replace_range(prefix, base_end, after_mid);
            }
        }
    }

    fn handle_search(&mut self, key: Key) {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.prompt_buf.clear();
            }
            Key::Backspace => {
                if self.prompt_buf.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.prompt_buf.pop();
                }
            }
            Key::Char(c) => {
                self.prompt_buf.push(c);
            }
            Key::Enter => {
                self.mode = Mode::Normal;
                let pat = std::mem::take(&mut self.prompt_buf);
                if !pat.is_empty() {
                    self.run_search(&pat, true, true);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn run_search(&mut self, pat: &str, forward: bool, save_pat: bool) {
        let re = match Regex::new(pat) {
            Ok(r) => r,
            Err(_) => {
                self.message = Some(String::from("bad regex"));
                return;
            }
        };
        if save_pat {
            self.last_search = Some(String::from(pat));
        }
        let text = self.editor.buffer.text();
        let matches = re.find_iter(&text);
        if matches.is_empty() {
            self.message = Some(format!("no match: {pat}"));
            return;
        }
        let cur_byte = self.line_col_to_byte(self.cursor_line, self.cursor_col);
        let target = if forward {
            matches
                .iter()
                .find(|&&(s, _)| s > cur_byte)
                .or_else(|| matches.first())
                .copied()
        } else {
            matches
                .iter()
                .rfind(|&&(s, _)| s < cur_byte)
                .or_else(|| matches.last())
                .copied()
        };
        if let Some((start_byte, _)) = target {
            let (line, col) = self.byte_to_line_col(start_byte);
            self.cursor_line = line;
            self.cursor_col = col;
            self.clamp_normal_cursor();
            self.sticky_col = self.cursor_col;
        }
    }

    fn handle_command(&mut self, key: Key) {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.prompt_buf.clear();
            }
            Key::Backspace => {
                if self.prompt_buf.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.prompt_buf.pop();
                }
            }
            Key::Char(c) => {
                self.prompt_buf.push(c);
            }
            Key::Enter => {
                self.mode = Mode::Normal;
                let cmd = std::mem::take(&mut self.prompt_buf);
                self.exec_command(&cmd);
            }
            _ => {}
        }
    }

    pub(crate) fn line_count(&self) -> usize {
        self.editor.buffer.line_count()
    }

    pub(crate) fn line_byte_range(&self, line: usize) -> (usize, usize) {
        let start = self
            .editor
            .buffer
            .line_start(line)
            .unwrap_or(self.editor.buffer.len());
        let end = if line + 1 < self.line_count() {
            self.editor
                .buffer
                .line_start(line + 1)
                .unwrap_or(self.editor.buffer.len())
                .saturating_sub(1)
        } else {
            self.editor.buffer.len()
        };
        (start, end)
    }

    pub(crate) fn line_str(&self, line: usize) -> String {
        let (s, e) = self.line_byte_range(line);
        self.editor.buffer.slice(s, e)
    }

    pub(crate) fn line_char_len(&self, line: usize) -> usize {
        self.line_str(line).chars().count()
    }

    pub(crate) fn max_normal_col(&self, line: usize) -> usize {
        self.line_char_len(line).saturating_sub(1)
    }

    pub(crate) fn first_non_blank(&self, line: usize) -> usize {
        let s = self.line_str(line);
        s.chars().position(|c| c != ' ' && c != '\t').unwrap_or(0)
    }

    pub(crate) fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let (s, e) = self.line_byte_range(line);
        let line_s = self.editor.buffer.slice(s, e);
        let rel = line_s
            .char_indices()
            .nth(col)
            .map_or(line_s.len(), |(i, _)| i);
        s + rel
    }

    pub(crate) fn byte_to_line_col(&self, byte_off: usize) -> (usize, usize) {
        let clamped = byte_off.min(self.editor.buffer.len());
        let line = self.editor.buffer.line_of(clamped);
        let l_start = self.editor.buffer.line_start(line).unwrap_or(0);
        let prefix = self.editor.buffer.slice(l_start, clamped);
        (line, prefix.chars().count())
    }

    pub(crate) fn clamp_normal_cursor(&mut self) {
        self.cursor_line = self.cursor_line.min(self.line_count().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.max_normal_col(self.cursor_line));
    }
}

fn diff_bounds(a: &str, b: &str) -> (usize, usize, usize) {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let min_len = ab.len().min(bb.len());
    let mut prefix = 0;
    while prefix < min_len && ab[prefix] == bb[prefix] {
        prefix += 1;
    }
    while prefix > 0 && (!a.is_char_boundary(prefix) || !b.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let mut suffix = 0;
    while prefix + suffix < ab.len()
        && prefix + suffix < bb.len()
        && ab[ab.len() - 1 - suffix] == bb[bb.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0
        && (!a.is_char_boundary(a.len() - suffix) || !b.is_char_boundary(b.len() - suffix))
    {
        suffix -= 1;
    }
    (prefix, a.len() - suffix, b.len() - suffix)
}
