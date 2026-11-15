//! Ex command-line execution (`:w`, `:q`, `:e`, `:{n}`, `:s///`, `:%s///`).

use std::path::Path;

use crate::app::App;
use crate::regex::Regex;
use crate::undo::Editor;

impl App {
    pub(crate) fn exec_command(&mut self, raw: &str) {
        let cmd = raw.trim();
        if cmd.is_empty() {
            return;
        }
        if cmd == "q" {
            if self.modified() {
                self.message = Some(String::from("unsaved changes (use :q! to quit)"));
            } else {
                self.quit_requested = true;
            }
            return;
        }
        if cmd == "q!" {
            self.quit_requested = true;
            return;
        }
        if cmd == "w" {
            self.cmd_write(None);
            return;
        }
        if cmd == "wq" {
            if self.cmd_write(None) {
                self.quit_requested = true;
            }
            return;
        }
        if let Some(path) = raw.strip_prefix("w ") {
            self.cmd_write(Some(path));
            return;
        }
        if let Some(path) = raw.strip_prefix("e ") {
            self.cmd_edit(path);
            return;
        }
        if cmd.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = cmd.parse::<usize>() {
                self.goto_line_first_non_blank(n.saturating_sub(1));
                return;
            }
        }
        if let Some(rest) = cmd.strip_prefix("%s/") {
            self.cmd_sub(rest, true, raw);
            return;
        }
        if let Some(rest) = cmd.strip_prefix("s/") {
            self.cmd_sub(rest, false, raw);
            return;
        }
        self.message = Some(format!("unknown command: {raw}"));
    }

    fn cmd_write(&mut self, override_path: Option<&str>) -> bool {
        let target = match override_path {
            Some(p) => p.to_string(),
            None => match &self.name {
                Some(n) => n.clone(),
                None => {
                    self.message = Some(String::from("cannot write: [scratch]"));
                    return false;
                }
            },
        };
        let content = self.editor.buffer.text();
        match std::fs::write(&target, content.as_bytes()) {
            Ok(()) => {
                self.saved_node = Some(self.editor.current_node());
                self.message = Some(format!("wrote {} bytes", content.len()));
                true
            }
            Err(_) => {
                self.message = Some(format!("cannot write: {target}"));
                false
            }
        }
    }

    fn cmd_edit(&mut self, path_str: &str) {
        if self.modified() {
            self.message = Some(String::from("unsaved changes (use :q! to quit)"));
            return;
        }
        let path = Path::new(path_str);
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => {
                self.message = Some(format!("cannot read: {path_str}"));
                return;
            }
        };
        self.name = Some(path.display().to_string());
        self.editor = Editor::new(&text);
        self.saved_node = Some(self.editor.current_node());
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.sticky_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
    }

    fn cmd_sub(&mut self, rest: &str, all_lines: bool, raw: &str) {
        let Some((pattern, rep, global)) = parse_sub_args(rest) else {
            self.message = Some(format!("unknown command: {raw}"));
            return;
        };
        let re = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => {
                self.message = Some(String::from("bad regex"));
                return;
            }
        };

        let (start_line, end_line) = if all_lines {
            (0, self.line_count())
        } else {
            (self.cursor_line, self.cursor_line + 1)
        };

        let mut total_subs = 0usize;
        let mut first_change_byte: Option<usize> = None;
        let mut last_change_end: usize = 0;
        let mut new_lines = Vec::with_capacity(end_line - start_line);

        for line_idx in start_line..end_line {
            let (l_start, l_end) = self.line_byte_range(line_idx);
            let line = self.editor.buffer.slice(l_start, l_end);
            let matches: Vec<(usize, usize)> = if global {
                re.find_iter(&line)
            } else {
                re.find_at(&line, 0)
                    .and_then(|g| g.first().copied().flatten())
                    .into_iter()
                    .collect()
            };

            if matches.is_empty() {
                new_lines.push(line);
                continue;
            }

            if first_change_byte.is_none() {
                first_change_byte = Some(l_start + matches[0].0);
            }
            if let Some(&(_, me)) = matches.last() {
                last_change_end = l_start + me;
            }

            let mut replaced = String::with_capacity(line.len());
            let mut pos = 0usize;
            for (ms, me) in matches {
                replaced.push_str(&line[pos..ms]);
                replaced.push_str(&rep);
                pos = me;
                total_subs += 1;
            }
            replaced.push_str(&line[pos..]);
            new_lines.push(replaced);
        }

        if total_subs == 0 {
            self.message = Some(format!("no match: {pattern}"));
            return;
        }

        let (region_start, _) = self.line_byte_range(start_line);
        let (_, region_end) = self.line_byte_range(end_line - 1);
        let new_region = new_lines.join("\n");

        let first_byte = first_change_byte.unwrap_or(region_start);
        let rel_start = first_byte - region_start;
        let old_tail = region_end - last_change_end;
        let new_end = new_region.len().saturating_sub(old_tail);
        let replaced_slice = &new_region[rel_start..new_end];

        self.editor
            .replace_range(first_byte, last_change_end, replaced_slice);

        let (cl, cc) = self.byte_to_line_col(first_byte);
        self.cursor_line = cl;
        self.cursor_col = cc;
        self.clamp_normal_cursor();
        self.sticky_col = self.cursor_col;
        self.message = Some(format!("{total_subs} substitutions"));
    }
}

fn parse_sub_args(rest: &str) -> Option<(String, String, bool)> {
    let (pattern, after_pat) = scan_part(rest)?;
    let (rep, after_rep) = scan_part(after_pat)?;
    let global = match after_rep {
        "" => false,
        "g" => true,
        _ => return None,
    };
    Some((pattern, rep, global))
}

fn scan_part(s: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((idx, ch)) = chars.next() {
        if ch == '/' {
            return Some((out, &s[idx + 1..]));
        }
        if ch == '\\' {
            if let Some((_, next)) = chars.next() {
                if next == '/' {
                    out.push('/');
                } else {
                    out.push('\\');
                    out.push(next);
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    None
}
