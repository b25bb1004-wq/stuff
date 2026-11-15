//! Normal-mode key handling, motions, operators, registers, and undo/redo.

use crate::app::{App, Mode, PendingOp, Register};
use crate::keys::Key;

const MAX_COUNT: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Blank,
    Word,
    Punct,
}

fn classify(c: char) -> CharClass {
    if c == ' ' || c == '\t' {
        CharClass::Blank
    } else if c.is_ascii_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

impl App {
    pub(crate) fn handle_normal(&mut self, key: Key) {
        if self.pending != PendingOp::None {
            let pending = self.pending;
            self.pending = PendingOp::None;
            let count = self.count.take();
            match (pending, key) {
                (PendingOp::G, Key::Char('g')) => {
                    let target = count.map_or(0, |n| n.saturating_sub(1));
                    self.goto_line_first_non_blank(target);
                }
                (PendingOp::Delete, Key::Char('d')) => {
                    self.delete_lines(count.unwrap_or(1).max(1));
                }
                (PendingOp::Yank, Key::Char('y')) => {
                    self.yank_lines(count.unwrap_or(1).max(1));
                }
                _ => {}
            }
            return;
        }

        if let Key::Char(d @ '0'..='9') = key {
            if d == '0' && self.count.is_none() {
                self.cursor_col = 0;
                self.sticky_col = 0;
                return;
            }
            let digit = (d as u8 - b'0') as usize;
            let next = self
                .count
                .unwrap_or(0)
                .saturating_mul(10)
                .saturating_add(digit)
                .min(MAX_COUNT);
            self.count = Some(next);
            return;
        }

        let raw_count = self.count.take();
        let count = raw_count.unwrap_or(1).max(1);

        match key {
            Key::Char('h') | Key::Left => {
                self.cursor_col = self.cursor_col.saturating_sub(count);
                self.sticky_col = self.cursor_col;
            }
            Key::Char('l') | Key::Right => {
                let max = self.max_normal_col(self.cursor_line);
                self.cursor_col = self.cursor_col.saturating_add(count).min(max);
                self.sticky_col = self.cursor_col;
            }
            Key::Char('j') | Key::Down => {
                let last = self.line_count().saturating_sub(1);
                self.cursor_line = self.cursor_line.saturating_add(count).min(last);
                self.cursor_col = self.sticky_col.min(self.max_normal_col(self.cursor_line));
            }
            Key::Char('k') | Key::Up => {
                self.cursor_line = self.cursor_line.saturating_sub(count);
                self.cursor_col = self.sticky_col.min(self.max_normal_col(self.cursor_line));
            }
            Key::Home => {
                self.cursor_col = 0;
                self.sticky_col = 0;
            }
            Key::Char('$') | Key::End => {
                self.cursor_col = self.max_normal_col(self.cursor_line);
                self.sticky_col = self.cursor_col;
            }
            Key::PageDown => {
                let step = self.height.saturating_sub(2).saturating_mul(count);
                let last = self.line_count().saturating_sub(1);
                self.cursor_line = self.cursor_line.saturating_add(step).min(last);
                self.cursor_col = self.sticky_col.min(self.max_normal_col(self.cursor_line));
            }
            Key::PageUp => {
                let step = self.height.saturating_sub(2).saturating_mul(count);
                self.cursor_line = self.cursor_line.saturating_sub(step);
                self.cursor_col = self.sticky_col.min(self.max_normal_col(self.cursor_line));
            }
            Key::Char('w') => {
                for _ in 0..count {
                    self.word_forward();
                }
                self.sticky_col = self.cursor_col;
            }
            Key::Char('b') => {
                for _ in 0..count {
                    self.word_backward();
                }
                self.sticky_col = self.cursor_col;
            }
            Key::Char('g') => {
                self.count = raw_count;
                self.pending = PendingOp::G;
            }
            Key::Char('G') => {
                let target = match raw_count {
                    Some(n) => n.saturating_sub(1),
                    None => self.line_count().saturating_sub(1),
                };
                self.goto_line_first_non_blank(target);
            }
            Key::Char('d') => {
                self.count = raw_count;
                self.pending = PendingOp::Delete;
            }
            Key::Char('y') => {
                self.count = raw_count;
                self.pending = PendingOp::Yank;
            }
            Key::Char('x') => self.delete_chars(count),
            Key::Char('p') => self.paste(true, count),
            Key::Char('P') => self.paste(false, count),
            Key::Char('i') => self.enter_insert(self.cursor_line, self.cursor_col),
            Key::Char('a') => {
                let len = self.line_char_len(self.cursor_line);
                let col = if len == 0 {
                    0
                } else {
                    (self.cursor_col + 1).min(len)
                };
                self.enter_insert(self.cursor_line, col);
            }
            Key::Char('I') => {
                let col = self.first_non_blank(self.cursor_line);
                self.enter_insert(self.cursor_line, col);
            }
            Key::Char('A') => {
                let len = self.line_char_len(self.cursor_line);
                self.enter_insert(self.cursor_line, len);
            }
            Key::Char('o') => {
                let line = self.cursor_line;
                self.enter_insert(line, 0);
                let (_, end) = self.line_byte_range(line);
                self.editor.buffer.insert(end, "\n");
                self.cursor_line = line + 1;
                self.cursor_col = 0;
            }
            Key::Char('O') => {
                let line = self.cursor_line;
                self.enter_insert(line, 0);
                let (start, _) = self.line_byte_range(line);
                self.editor.buffer.insert(start, "\n");
                self.cursor_line = line;
                self.cursor_col = 0;
            }
            Key::Char('u') => {
                for _ in 0..count {
                    if self.editor.undo() {
                        if let Some(c) = self.editor.cursors().first() {
                            let (l, col) = self.byte_to_line_col(c.start());
                            self.cursor_line = l;
                            self.cursor_col = col;
                        }
                    }
                }
                self.clamp_normal_cursor();
                self.sticky_col = self.cursor_col;
            }
            Key::Ctrl('r') => {
                for _ in 0..count {
                    if self.editor.redo() {
                        if let Some(c) = self.editor.cursors().first() {
                            let (l, col) = self.byte_to_line_col(c.start());
                            self.cursor_line = l;
                            self.cursor_col = col;
                        }
                    }
                }
                self.clamp_normal_cursor();
                self.sticky_col = self.cursor_col;
            }
            Key::Char('/') => {
                self.prompt_buf.clear();
                self.mode = Mode::Search;
            }
            Key::Char(':') => {
                self.prompt_buf.clear();
                self.mode = Mode::Command;
            }
            Key::Char('n') => {
                if let Some(pat) = self.last_search.clone() {
                    for _ in 0..count {
                        self.run_search(&pat, true, false);
                    }
                }
            }
            Key::Char('N') => {
                if let Some(pat) = self.last_search.clone() {
                    for _ in 0..count {
                        self.run_search(&pat, false, false);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn goto_line_first_non_blank(&mut self, line: usize) {
        let clamped = line.min(self.line_count().saturating_sub(1));
        self.cursor_line = clamped;
        self.cursor_col = self.first_non_blank(clamped);
        self.clamp_normal_cursor();
        self.sticky_col = self.cursor_col;
    }

    fn delete_chars(&mut self, count: usize) {
        let line_len = self.line_char_len(self.cursor_line);
        if line_len == 0 || self.cursor_col >= line_len {
            return;
        }
        let end_col = (self.cursor_col + count).min(line_len);
        let start = self.line_col_to_byte(self.cursor_line, self.cursor_col);
        let end = self.line_col_to_byte(self.cursor_line, end_col);
        let deleted = self.editor.buffer.slice(start, end);
        self.register = Register::Char(deleted);
        self.editor.replace_range(start, end, "");
        self.clamp_normal_cursor();
        self.sticky_col = self.cursor_col;
    }

    fn delete_lines(&mut self, count: usize) {
        let total = self.line_count();
        let first = self.cursor_line;
        let last = (first + count - 1).min(total - 1);
        let (first_start, _) = self.line_byte_range(first);
        let (_, last_end) = self.line_byte_range(last);
        let yanked = self.editor.buffer.slice(first_start, last_end);
        self.register = Register::Line(yanked);

        if first == 0 && last == total - 1 {
            self.editor.replace_range(0, self.editor.buffer.len(), "");
            self.cursor_line = 0;
            self.cursor_col = 0;
        } else if last + 1 < total {
            self.editor.replace_range(first_start, last_end + 1, "");
            self.cursor_line = first;
            self.cursor_col = 0;
        } else {
            // Deleting up to the last line, with at least one line above.
            self.editor.replace_range(first_start - 1, last_end, "");
            self.cursor_line = first - 1;
            self.cursor_col = 0;
        }
        self.clamp_normal_cursor();
        self.sticky_col = self.cursor_col;
    }

    fn yank_lines(&mut self, count: usize) {
        let total = self.line_count();
        let first = self.cursor_line;
        let last = (first + count - 1).min(total - 1);
        let (first_start, _) = self.line_byte_range(first);
        let (_, last_end) = self.line_byte_range(last);
        self.register = Register::Line(self.editor.buffer.slice(first_start, last_end));
    }

    fn paste(&mut self, after: bool, count: usize) {
        match self.register.clone() {
            Register::Empty => {}
            Register::Line(text) => {
                let mut repeated = String::new();
                for i in 0..count {
                    if i > 0 {
                        repeated.push('\n');
                    }
                    repeated.push_str(&text);
                }
                let line = self.cursor_line;
                if after {
                    let (_, end) = self.line_byte_range(line);
                    let ins = format!("\n{repeated}");
                    self.editor.replace_range(end, end, &ins);
                    self.goto_line_first_non_blank(line + 1);
                } else {
                    let (start, _) = self.line_byte_range(line);
                    let ins = format!("{repeated}\n");
                    self.editor.replace_range(start, start, &ins);
                    self.goto_line_first_non_blank(line);
                }
            }
            Register::Char(text) => {
                if text.is_empty() {
                    return;
                }
                let repeated = text.repeat(count);
                let char_count = repeated.chars().count();
                let line_len = self.line_char_len(self.cursor_line);
                let insert_col = if after && line_len > 0 {
                    (self.cursor_col + 1).min(line_len)
                } else {
                    self.cursor_col
                };
                let byte = self.line_col_to_byte(self.cursor_line, insert_col);
                self.editor.replace_range(byte, byte, &repeated);
                self.cursor_col = insert_col + char_count.saturating_sub(1);
                self.clamp_normal_cursor();
                self.sticky_col = self.cursor_col;
            }
        }
    }

    fn word_forward(&mut self) {
        let total = self.line_count();
        let mut line = self.cursor_line;
        let mut col = self.cursor_col;
        let chars: Vec<char> = self.line_str(line).chars().collect();

        if !chars.is_empty() {
            let cur_class = classify(chars[col.min(chars.len() - 1)]);
            if cur_class != CharClass::Blank {
                while col < chars.len() && classify(chars[col]) == cur_class {
                    col += 1;
                }
            }
            while col < chars.len() && classify(chars[col]) == CharClass::Blank {
                col += 1;
            }
            if col < chars.len() {
                self.cursor_line = line;
                self.cursor_col = col;
                return;
            }
        }

        while line + 1 < total {
            line += 1;
            let next_chars: Vec<char> = self.line_str(line).chars().collect();
            if next_chars.is_empty() {
                self.cursor_line = line;
                self.cursor_col = 0;
                return;
            }
            if let Some(pos) = next_chars
                .iter()
                .position(|&c| classify(c) != CharClass::Blank)
            {
                self.cursor_line = line;
                self.cursor_col = pos;
                return;
            }
        }

        let last_line = total.saturating_sub(1);
        self.cursor_line = last_line;
        self.cursor_col = self.max_normal_col(last_line);
    }

    fn word_backward(&mut self) {
        let mut line = self.cursor_line;
        let col = self.cursor_col;

        if col > 0 {
            let chars: Vec<char> = self.line_str(line).chars().collect();
            let mut idx = col - 1;
            while idx > 0 && classify(chars[idx]) == CharClass::Blank {
                idx -= 1;
            }
            if classify(chars[idx]) != CharClass::Blank {
                let target_class = classify(chars[idx]);
                while idx > 0 && classify(chars[idx - 1]) == target_class {
                    idx -= 1;
                }
                self.cursor_line = line;
                self.cursor_col = idx;
                return;
            }
        }

        while line > 0 {
            line -= 1;
            let prev_chars: Vec<char> = self.line_str(line).chars().collect();
            if prev_chars.is_empty() {
                self.cursor_line = line;
                self.cursor_col = 0;
                return;
            }
            let mut idx = prev_chars.len() - 1;
            while idx > 0 && classify(prev_chars[idx]) == CharClass::Blank {
                idx -= 1;
            }
            if classify(prev_chars[idx]) != CharClass::Blank {
                let target_class = classify(prev_chars[idx]);
                while idx > 0 && classify(prev_chars[idx - 1]) == target_class {
                    idx -= 1;
                }
                self.cursor_line = line;
                self.cursor_col = idx;
                return;
            }
        }

        self.cursor_line = 0;
        self.cursor_col = 0;
    }
}
