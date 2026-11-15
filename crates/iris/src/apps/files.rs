//! Directory browser application backed by the shared `hades` filesystem.

use std::rc::Rc;

use hades::{DirEntry, Kind};
use indium::widgets::{ListState, Nav};
use indium::{Event, Key};
use pigment::{Canvas, Rect, Rgba};

use crate::app::{fill_bg, grid_dims, paint_row, App, Effect, SharedFs, CELL_H};
use crate::apps::edit::Editor;
use crate::window::CONTENT_BG_COLOR;

const HEADER_BG: Rgba = Rgba(36, 42, 54, 255);
const HEADER_FG: Rgba = Rgba(150, 195, 255, 255);
const SEL_BG: Rgba = Rgba(52, 86, 148, 255);
const SEL_FG: Rgba = Rgba(255, 255, 255, 255);
const DIR_FG: Rgba = Rgba(140, 200, 255, 255);
const FILE_FG: Rgba = Rgba(225, 228, 235, 255);

/// Directory browser listing entries from the shared `hades` filesystem.
pub struct Files {
    fs: SharedFs,
    cwd: String,
    title: String,
    entries: Vec<DirEntry>,
    list: ListState,
    dirty: bool,
    last_size: (u32, u32),
}

impl Files {
    /// Open a directory browser rooted at `path` on `fs`.
    pub fn new(fs: SharedFs, path: &str) -> Self {
        let cwd = normalize_dir(path);
        let title = format!("files: {cwd}");
        let mut files = Self {
            fs,
            cwd,
            title,
            entries: Vec::new(),
            list: ListState::new(0, 10),
            dirty: true,
            last_size: (0, 0),
        };
        files.refresh();
        files
    }

    /// Current working directory path.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Current visible directory entries (including `..` when not at `/`).
    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    /// Index of the currently selected entry, if any.
    pub fn selected(&self) -> Option<usize> {
        self.list.selected
    }

    /// Name of the currently selected entry, if any.
    pub fn selected_name(&self) -> Option<&str> {
        self.list
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|e| e.name.as_str())
    }

    /// Reload directory entries from the shared filesystem, keeping selection in range.
    pub fn refresh(&mut self) {
        let prev_name = self.selected_name().map(str::to_owned);
        let mut next = Vec::new();
        if self.cwd != "/" {
            next.push(DirEntry {
                name: String::from(".."),
                kind: Kind::Dir,
                inode: 0,
            });
        }
        if let Ok(listed) = self.fs.borrow_mut().list(&self.cwd) {
            next.extend(listed);
        }
        if next != self.entries {
            self.entries = next;
            self.dirty = true;
        }
        let len = self.entries.len();
        if self.list.set_len(len) {
            self.dirty = true;
        }
        if let Some(name) = prev_name {
            if let Some(pos) = self.entries.iter().position(|e| e.name == name) {
                if self.list.selected != Some(pos) {
                    self.list.selected = Some(pos);
                    reveal_selection(&mut self.list);
                    self.dirty = true;
                }
            }
        }
        if self.list.selected.is_none() && len > 0 {
            self.list.selected = Some(0);
            self.dirty = true;
        }
    }

    fn enter_dir(&mut self, dir: String) {
        self.cwd = normalize_dir(&dir);
        self.title = format!("files: {}", self.cwd);
        self.list.selected = None;
        self.list.offset = 0;
        self.refresh();
        self.dirty = true;
    }

    fn activate_selected(&mut self) -> Effect {
        self.refresh();
        let Some(entry) = self
            .list
            .selected
            .and_then(|i| self.entries.get(i))
            .cloned()
        else {
            return Effect::None;
        };
        if entry.name == ".." {
            self.enter_dir(parent_dir(&self.cwd));
            return Effect::None;
        }
        let target = join_path(&self.cwd, &entry.name);
        match entry.kind {
            Kind::Dir => {
                self.enter_dir(target);
                Effect::None
            }
            Kind::File => Effect::Open(Box::new(Editor::new(Rc::clone(&self.fs), &target))),
        }
    }

    fn nav(&mut self, n: Nav) {
        self.refresh();
        if self.list.nav(n) {
            self.dirty = true;
        }
    }
}

impl App for Files {
    fn title(&self) -> &str {
        &self.title
    }

    fn event(&mut self, ev: Event, _now_ms: u64) -> Effect {
        match ev {
            Event::Key(Key::Enter) | Event::Text('\n') | Event::Text('\r') => {
                return self.activate_selected();
            }
            Event::Key(Key::Backspace) if self.cwd != "/" => {
                self.enter_dir(parent_dir(&self.cwd));
            }
            Event::Text('\u{2191}') | Event::Text('\x10') | Event::Text('k') | Event::Text('w') => {
                self.nav(Nav::Up);
            }
            Event::Text('\u{2193}') | Event::Text('\x0e') | Event::Text('j') | Event::Text('s') => {
                self.nav(Nav::Down);
            }
            Event::MouseDown { y, .. } if y >= CELL_H as i32 => {
                self.refresh();
                let row = ((y as usize) / (CELL_H as usize)).saturating_sub(1);
                if self.list.click_row(row) {
                    self.dirty = true;
                }
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
        let (cols, rows) = grid_dims(c);
        let vis_rows = rows.saturating_sub(1).max(1);
        if self.list.visible != vis_rows {
            self.list.visible = vis_rows;
            reveal_selection(&mut self.list);
            self.dirty = true;
        }
        if !self.dirty && self.last_size == size {
            return None;
        }
        self.dirty = false;
        self.last_size = size;

        fill_bg(c, CONTENT_BG_COLOR);
        if cols == 0 || rows == 0 {
            return Some(Rect {
                x: 0,
                y: 0,
                w: c.width,
                h: c.height,
            });
        }

        let header = format!("DIR {}", self.cwd);
        paint_row(c, 0, &header, HEADER_FG, HEADER_BG, cols);

        for row in 0..rows.saturating_sub(1) {
            let idx = self.list.offset.saturating_add(row);
            let Some(entry) = self.entries.get(idx) else {
                break;
            };
            let selected = self.list.selected == Some(idx);
            let (fg, bg) = if selected {
                (SEL_FG, SEL_BG)
            } else if entry.kind == Kind::Dir {
                (DIR_FG, CONTENT_BG_COLOR)
            } else {
                (FILE_FG, CONTENT_BG_COLOR)
            };
            let prefix = if entry.kind == Kind::Dir {
                "[D] "
            } else {
                "    "
            };
            let line = format!("{prefix}{}", entry.name);
            paint_row(c, row + 1, &line, fg, bg, cols);
        }

        Some(Rect {
            x: 0,
            y: 0,
            w: c.width,
            h: c.height,
        })
    }
}

fn reveal_selection(list: &mut ListState) {
    let Some(sel) = list.selected else {
        return;
    };
    let vis = list.visible.max(1);
    if sel < list.offset {
        list.offset = sel;
    } else if sel >= list.offset.saturating_add(vis) {
        list.offset = sel.saturating_sub(vis - 1);
    }
}

fn normalize_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        String::from("/")
    } else if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    }
}

fn join_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn parent_dir(dir: &str) -> String {
    if dir == "/" {
        return String::from("/");
    }
    match dir.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(idx) => dir[..idx].to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::cell::RefCell;
    use std::rc::Rc;

    use hades::{mkfs, Fs, MemDisk, MkfsOptions};

    use super::*;

    #[test]
    fn navigate_dirs_and_open_file() {
        let mut disk = MemDisk::new(128);
        mkfs(&mut disk, MkfsOptions::default()).unwrap();
        let mut raw_fs = Fs::mount(disk).unwrap();
        raw_fs.mkdir("/docs").unwrap();
        raw_fs.create("/docs/readme.txt").unwrap();
        raw_fs.write("/docs/readme.txt", 0, b"hi").unwrap();
        let fs = Rc::new(RefCell::new(raw_fs));

        let mut files = Files::new(Rc::clone(&fs), "/");
        assert_eq!(files.selected_name(), Some("docs"));
        assert!(matches!(
            files.event(Event::Key(Key::Enter), 0),
            Effect::None
        ));
        assert_eq!(files.cwd(), "/docs");
        assert_eq!(files.selected_name(), Some(".."));

        files.event(Event::Text('\u{2193}'), 1);
        assert_eq!(files.selected_name(), Some("readme.txt"));
        let eff = files.event(Event::Key(Key::Enter), 2);
        assert!(matches!(eff, Effect::Open(_)));
        if let Effect::Open(app) = eff {
            assert_eq!(app.title(), "/docs/readme.txt");
        }
    }
}
