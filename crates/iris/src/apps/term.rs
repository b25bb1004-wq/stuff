//! Terminal emulator application backed by a `krypton::exec::Shell`.

use std::io;

use indium::{Event, Key};
use krypton::exec::Shell;
use pigment::{Canvas, Rect, Rgba};

use crate::app::{fill_bg, grid_dims, paint_cursor_bar, paint_row, App, Effect};
use crate::window::CONTENT_BG_COLOR;

const MAX_HISTORY_LINES: usize = 200;
const PROMPT: &str = "$ ";
const TERM_FG: Rgba = Rgba(220, 236, 224, 255);
const TERM_CURSOR: Rgba = Rgba(110, 225, 145, 255);

/// Interactive terminal application running a `krypton::exec::Shell`.
pub struct Terminal {
    shell: Shell,
    history: Vec<String>,
    input: String,
    dirty: bool,
    last_size: (u32, u32),
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal {
    /// Create a new terminal with an in-memory `krypton` shell and empty scrollback.
    pub fn new() -> Self {
        Self {
            shell: Shell::new(),
            history: Vec::new(),
            input: String::new(),
            dirty: true,
            last_size: (0, 0),
        }
    }

    /// Scrollback output lines (capped at the most recent 200 lines).
    pub fn lines(&self) -> &[String] {
        &self.history
    }

    /// Current command line buffer (excluding the `$ ` prompt).
    pub fn input_line(&self) -> &str {
        &self.input
    }

    /// Visible wrapped rows for a `cols x rows` character grid, including the active prompt.
    pub fn grid_lines(&self, cols: usize, rows: usize) -> Vec<String> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }
        let (wrapped, _) = self.layout_grid(cols);
        let skip = wrapped.len().saturating_sub(rows);
        wrapped.into_iter().skip(skip).collect()
    }

    fn push_history_line(&mut self, line: String) {
        self.history.push(line);
        if self.history.len() > MAX_HISTORY_LINES {
            let excess = self.history.len() - MAX_HISTORY_LINES;
            self.history.drain(0..excess);
        }
    }

    fn append_output_bytes(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        for line in text.lines() {
            self.push_history_line(line.to_owned());
        }
    }

    fn submit_line(&mut self) -> Effect {
        let line = core::mem::take(&mut self.input);
        self.push_history_line(format!("{PROMPT}{line}"));
        if !line.trim().is_empty() {
            let mut empty_in = io::empty();
            let mut out = Vec::new();
            let mut err = Vec::new();
            let _ = self.shell.run(&line, &mut empty_in, &mut out, &mut err);
            self.append_output_bytes(&out);
            self.append_output_bytes(&err);
        }
        self.dirty = true;
        if self.shell.exited().is_some() {
            Effect::Close
        } else {
            Effect::None
        }
    }

    fn layout_grid(&self, cols: usize) -> (Vec<String>, (usize, usize)) {
        let mut out = Vec::new();
        for line in &self.history {
            wrap_into(line, cols, &mut out);
        }
        let prompt_line = format!("{PROMPT}{}", self.input);
        let prompt_chars = prompt_line.chars().count();
        wrap_into(&prompt_line, cols, &mut out);
        let cur_row = out.len().saturating_sub(1);
        let cur_col = if prompt_chars == 0 {
            0
        } else {
            let rem = prompt_chars % cols;
            if rem == 0 {
                cols
            } else {
                rem
            }
        };
        (out, (cur_row, cur_col))
    }
}

impl App for Terminal {
    fn title(&self) -> &str {
        "terminal"
    }

    fn event(&mut self, ev: Event, _now_ms: u64) -> Effect {
        match ev {
            Event::Key(Key::Enter) => self.submit_line(),
            Event::Key(Key::Space) => {
                self.input.push(' ');
                self.dirty = true;
                Effect::None
            }
            Event::Key(Key::Backspace) => {
                if self.input.pop().is_some() {
                    self.dirty = true;
                }
                Effect::None
            }
            Event::Text('\n') | Event::Text('\r') => self.submit_line(),
            Event::Text('\x08') | Event::Text('\x7f') => {
                if self.input.pop().is_some() {
                    self.dirty = true;
                }
                Effect::None
            }
            Event::Text(ch) if !ch.is_control() => {
                self.input.push(ch);
                self.dirty = true;
                Effect::None
            }
            Event::Key(_)
            | Event::Text(_)
            | Event::MouseMove { .. }
            | Event::MouseDown { .. }
            | Event::MouseUp { .. } => Effect::None,
        }
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

        let (wrapped, (cur_abs_row, cur_col)) = self.layout_grid(cols);
        let skip = wrapped.len().saturating_sub(rows);
        for (vis_row, text) in wrapped.iter().skip(skip).enumerate() {
            paint_row(c, vis_row, text, TERM_FG, CONTENT_BG_COLOR, cols);
        }
        let vis_cur_row = cur_abs_row.saturating_sub(skip);
        if vis_cur_row < rows {
            paint_cursor_bar(
                c,
                vis_cur_row,
                cur_col.min(cols.saturating_sub(1)),
                TERM_CURSOR,
            );
        }

        Some(Rect {
            x: 0,
            y: 0,
            w: c.width,
            h: c.height,
        })
    }
}

fn wrap_into(line: &str, cols: usize, out: &mut Vec<String>) {
    if line.is_empty() {
        out.push(String::new());
        return;
    }
    let mut chunk = String::new();
    let mut count = 0usize;
    for ch in line.chars() {
        chunk.push(ch);
        count += 1;
        if count == cols {
            out.push(core::mem::take(&mut chunk));
            count = 0;
        }
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn echo_and_exit_and_scrollback_cap() {
        let mut t = Terminal::new();
        for ch in "echo hi".chars() {
            assert!(matches!(t.event(Event::Text(ch), 0), Effect::None));
        }
        assert!(matches!(t.event(Event::Key(Key::Enter), 1), Effect::None));
        assert_eq!(t.lines(), &["$ echo hi", "hi"]);
        assert!(t.grid_lines(40, 10).iter().any(|l| l == "hi"));

        for i in 0..250 {
            let cmd = format!("echo line{i}");
            for ch in cmd.chars() {
                t.event(Event::Text(ch), 2);
            }
            t.event(Event::Key(Key::Enter), 3);
        }
        assert_eq!(t.lines().len(), 200);

        for ch in "exit".chars() {
            t.event(Event::Text(ch), 4);
        }
        assert!(matches!(t.event(Event::Key(Key::Enter), 5), Effect::Close));
    }
}
