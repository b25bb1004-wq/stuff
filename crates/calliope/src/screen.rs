//! Screen representation, rendering, tab expansion, scrolling, and ANSI output.

use std::fmt::Write as _;

use crate::highlight::{highlight, Style};
use crate::regex::Regex;

/// Rendered terminal frame with text rows, per-cell styles, and cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Text rows, each of exact char length `width`.
    pub rows: Vec<String>,
    /// Style rows matching `rows` cell-for-cell.
    pub styles: Vec<Vec<Style>>,
    /// Screen cursor `(row, col)`, 0-based.
    pub cursor: (usize, usize),
}

/// Format a `Screen` as a full-screen ANSI escape-sequence redraw.
pub fn ansi(screen: &Screen) -> String {
    let mut out = String::from("\x1b[H");
    for (r, (row, styles)) in screen.rows.iter().zip(screen.styles.iter()).enumerate() {
        if r > 0 {
            out.push_str("\r\n");
        }
        let mut cur_style: Option<Style> = None;
        for (ch, &st) in row.chars().zip(styles.iter()) {
            if cur_style != Some(st) {
                out.push_str(sgr(st));
                cur_style = Some(st);
            }
            out.push(ch);
        }
        out.push_str("\x1b[0m");
    }
    let _ = write!(out, "\x1b[{};{}H", screen.cursor.0 + 1, screen.cursor.1 + 1);
    out
}

fn sgr(st: Style) -> &'static str {
    match st {
        Style::Plain => "\x1b[0m",
        Style::Keyword => "\x1b[35m",
        Style::Type => "\x1b[33m",
        Style::Number => "\x1b[36m",
        Style::String => "\x1b[32m",
        Style::Comment => "\x1b[90m",
        Style::Error => "\x1b[31m",
        Style::Status => "\x1b[7m",
        Style::Match => "\x1b[43;30m",
        Style::Selection => "\x1b[48;5;238m",
    }
}

/// Redraw only the rows that differ from `prev` (every row when there is no comparable previous
/// frame), then place the cursor. Rows are exactly `width` cells, so no clearing is needed.
pub fn ansi_changed(prev: Option<&Screen>, next: &Screen) -> String {
    let prev = prev.filter(|p| p.rows.len() == next.rows.len());
    let mut out = String::from("\x1b[?25l");
    for (r, (row, styles)) in next.rows.iter().zip(next.styles.iter()).enumerate() {
        if prev.is_some_and(|p| p.rows[r] == *row && p.styles[r] == *styles) {
            continue;
        }
        let _ = write!(out, "\x1b[{};1H", r + 1);
        let mut cur_style: Option<Style> = None;
        for (ch, &st) in row.chars().zip(styles.iter()) {
            if cur_style != Some(st) {
                out.push_str(sgr(st));
                cur_style = Some(st);
            }
            out.push(ch);
        }
        out.push_str("\x1b[0m");
    }
    let _ = write!(
        out,
        "\x1b[{};{}H\x1b[?25h",
        next.cursor.0 + 1,
        next.cursor.1 + 1
    );
    out
}

/// Compute visual column of char index `char_col` in `line` (expanding `\t` to multiples of 4).
pub(crate) fn visual_col(line: &str, char_col: usize) -> usize {
    let mut vcol = 0usize;
    for (idx, ch) in line.chars().enumerate() {
        if idx >= char_col {
            break;
        }
        if ch == '\t' {
            vcol += 4 - (vcol % 4);
        } else {
            vcol += 1;
        }
    }
    vcol
}

pub(crate) struct RenderInput<'a> {
    pub text: &'a str,
    pub name: &'a str,
    pub width: usize,
    pub height: usize,
    pub cursor: (usize, usize),
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub last_search: Option<&'a str>,
    pub status_text: &'a str,
    pub status_cursor_col: Option<usize>,
}

pub(crate) fn build_screen(inp: RenderInput<'_>) -> Screen {
    if inp.height == 0 {
        return Screen {
            rows: Vec::new(),
            styles: Vec::new(),
            cursor: (0, 0),
        };
    }

    let text_rows = inp.height - 1;
    let mut byte_styles = highlight(inp.name, inp.text);
    if let Some(pat) = inp.last_search {
        if !pat.is_empty() {
            if let Ok(re) = Regex::new(pat) {
                for (s, e) in re.find_iter(inp.text) {
                    if s < e && e <= byte_styles.len() {
                        for st in &mut byte_styles[s..e] {
                            *st = Style::Match;
                        }
                    }
                }
            }
        }
    }

    let line_ranges = split_line_ranges(inp.text);
    let mut rows = Vec::with_capacity(inp.height);
    let mut styles = Vec::with_capacity(inp.height);

    for r in 0..text_rows {
        let line_idx = inp.scroll_row + r;
        if let Some(&(l_start, l_end)) = line_ranges.get(line_idx) {
            let line_str = &inp.text[l_start..l_end];
            let line_styles = &byte_styles[l_start..l_end];
            let (row_str, row_st) =
                render_text_line(line_str, line_styles, inp.scroll_col, inp.width);
            rows.push(row_str);
            styles.push(row_st);
        } else {
            rows.push(" ".repeat(inp.width));
            styles.push(vec![Style::Plain; inp.width]);
        }
    }

    let status_row = pad_or_cut(inp.status_text, inp.width);
    rows.push(status_row);
    styles.push(vec![Style::Status; inp.width]);

    let cursor = if let Some(scol) = inp.status_cursor_col {
        (inp.height - 1, scol.min(inp.width.saturating_sub(1)))
    } else {
        let cur_line = inp.cursor.0;
        let vcol = line_ranges
            .get(cur_line)
            .map_or(0, |&(s, e)| visual_col(&inp.text[s..e], inp.cursor.1));
        let row = cur_line
            .saturating_sub(inp.scroll_row)
            .min(text_rows.saturating_sub(1));
        let col = vcol
            .saturating_sub(inp.scroll_col)
            .min(inp.width.saturating_sub(1));
        (row, col)
    };

    Screen {
        rows,
        styles,
        cursor,
    }
}

fn split_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            out.push((start, idx));
            start = idx + 1;
        }
    }
    out.push((start, text.len()));
    out
}

fn render_text_line(
    line: &str,
    line_styles: &[Style],
    scroll_col: usize,
    width: usize,
) -> (String, Vec<Style>) {
    let mut out_str = String::with_capacity(width);
    let mut out_st = Vec::with_capacity(width);
    let mut vcol = 0usize;
    let end_vcol = scroll_col.saturating_add(width);

    for (byte_idx, ch) in line.char_indices() {
        if vcol >= end_vcol {
            break;
        }
        let st = line_styles.get(byte_idx).copied().unwrap_or(Style::Plain);
        if ch == '\t' {
            let spaces = 4 - (vcol % 4);
            for _ in 0..spaces {
                if vcol >= scroll_col && vcol < end_vcol {
                    out_str.push(' ');
                    out_st.push(st);
                }
                vcol += 1;
            }
        } else {
            if vcol >= scroll_col && vcol < end_vcol {
                out_str.push(ch);
                out_st.push(st);
            }
            vcol += 1;
        }
    }

    while out_st.len() < width {
        out_str.push(' ');
        out_st.push(Style::Plain);
    }

    (out_str, out_st)
}

fn pad_or_cut(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(width);
    let mut count = 0usize;
    for ch in s.chars() {
        if count >= width {
            break;
        }
        out.push(ch);
        count += 1;
    }
    while count < width {
        out.push(' ');
        count += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn tab_expansion_and_ansi_format() {
        let screen = build_screen(RenderInput {
            text: "\ta\n",
            name: "",
            width: 10,
            height: 3,
            cursor: (0, 1),
            scroll_row: 0,
            scroll_col: 0,
            last_search: None,
            status_text: "NORMAL",
            status_cursor_col: None,
        });
        assert_eq!(screen.rows[0], "    a     ");
        assert_eq!(screen.rows[1], "          ");
        assert_eq!(screen.rows[2], "NORMAL    ");
        assert_eq!(screen.cursor, (0, 4));
        let a = ansi(&screen);
        assert!(a.starts_with("\x1b[H"));
        assert!(a.ends_with("\x1b[1;5H"));
        assert!(a.contains("\x1b[0m\r\n"));
    }
}
