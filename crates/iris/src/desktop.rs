//! Desktop shell coordinating a [`Compositor`], hosted [`App`] windows, and a shared `hades` filesystem.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use hades::{mkfs, Fs, MemDisk, MkfsOptions};
use indium::{Event, Key};
use pigment::{Canvas, Rect};

use crate::app::{App, Effect, SharedFs, CELL_H, CELL_W};
use crate::apps::term::Terminal;
use crate::{Compositor, Layout, Routed, WinId};

const MEM_DISK_BLOCKS: u64 = 256;

/// Desktop manager binding [`App`] instances to [`Compositor`] windows and a shared [`hades::Fs`].
pub struct Desktop {
    comp: Compositor,
    apps: BTreeMap<WinId, Box<dyn App>>,
    fs: SharedFs,
}

impl Desktop {
    /// Create a desktop of `w x h` pixels with a freshly formatted in-memory `hades` filesystem.
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            comp: Compositor::new(w, h),
            apps: BTreeMap::new(),
            fs: init_mem_fs(),
        }
    }

    /// Borrow a cloned handle to the shared in-memory `hades` filesystem.
    pub fn fs(&self) -> SharedFs {
        Rc::clone(&self.fs)
    }

    /// Launch `app` in a new window of client size `w x h` at `now_ms`.
    pub fn launch(&mut self, app: Box<dyn App>, w: u32, h: u32, now_ms: u64) -> WinId {
        let id = self.comp.open(app.title(), w, h, now_ms);
        self.apps.insert(id, app);
        id
    }

    /// Close window `id` and drop its app, returning `true` if it existed.
    pub fn close(&mut self, id: WinId, now_ms: u64) -> bool {
        self.apps.remove(&id);
        self.comp.close(id, now_ms)
    }

    /// Switch compositor layout between floating and tiling at `now_ms`.
    pub fn set_layout(&mut self, layout: Layout, now_ms: u64) {
        self.comp.set_layout(layout, now_ms);
    }

    /// Currently focused window, if any.
    pub fn focused(&self) -> Option<WinId> {
        self.comp.focused()
    }

    /// Window stacking order from bottom to top.
    pub fn order(&self) -> Vec<WinId> {
        self.comp.order()
    }

    /// Current (possibly animated) frame rect of window `id`.
    pub fn rect(&self, id: WinId) -> Option<Rect> {
        self.comp.rect(id)
    }

    /// Borrow the app instance hosted in window `id`, if present.
    pub fn app(&self, id: WinId) -> Option<&dyn App> {
        self.apps.get(&id).map(Box::as_ref)
    }

    /// Handle an input event, intercepting `Super+Enter` to launch a terminal and routing window events.
    pub fn input(&mut self, ev: Event, super_held: bool, now_ms: u64) -> Routed {
        if super_held && matches!(ev, Event::Key(Key::Enter)) {
            let (w, h) = self.default_window_size();
            let _ = self.launch(Box::new(Terminal::new()), w, h, now_ms);
            return Routed::Handled;
        }

        let routed = self.comp.input(ev, super_held, now_ms);
        if let Routed::ToWindow(id, win_ev) = routed {
            let effect = self
                .apps
                .get_mut(&id)
                .map_or(Effect::None, |app| app.event(win_ev, now_ms));
            match effect {
                Effect::None => {}
                Effect::Close => {
                    self.apps.remove(&id);
                    self.comp.close(id, now_ms);
                }
                Effect::Open(child) => {
                    let (w, h) = self.default_window_size();
                    let _ = self.launch(child, w, h, now_ms);
                }
            }
        }
        routed
    }

    /// Paint dirty apps, forward their damage to the compositor, and composite the frame at `now_ms`.
    pub fn frame(&mut self, now_ms: u64) -> Vec<Rect> {
        for (&id, app) in &mut self.apps {
            let dmg = if let Some(win) = self.comp.windows.get_mut(&id) {
                let title = app.title();
                if win._title != title {
                    win._title = title.to_owned();
                }
                app.paint(&mut win.surface)
            } else {
                None
            };
            if let Some(r) = dmg {
                self.comp.damage(id, r);
            }
        }
        self.comp.frame(now_ms)
    }

    /// Borrow the composited screen canvas.
    pub fn screen(&self) -> &Canvas {
        self.comp.screen()
    }

    fn default_window_size(&self) -> (u32, u32) {
        let sw = self.comp.screen.width;
        let sh = self.comp.screen.height;
        let raw_w = (sw.saturating_mul(2) / 3).min(sw.saturating_sub(4)).max(1);
        let raw_h = (sh.saturating_mul(2) / 3).min(sh.saturating_sub(24)).max(1);
        let w = if raw_w >= CELL_W {
            (raw_w / CELL_W) * CELL_W
        } else {
            raw_w
        };
        let h = if raw_h >= CELL_H {
            (raw_h / CELL_H) * CELL_H
        } else {
            raw_h
        };
        (w, h)
    }
}

fn init_mem_fs() -> SharedFs {
    let mut disk = MemDisk::new(MEM_DISK_BLOCKS);
    let _ = mkfs(&mut disk, MkfsOptions::default());
    match Fs::mount(disk) {
        Ok(fs) => Rc::new(RefCell::new(fs)),
        Err(e) => panic!("hades MemDisk({MEM_DISK_BLOCKS}) mount failed: {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn super_enter_launches_terminal_and_exit_closes() {
        let mut d = Desktop::new(640, 400);
        assert_eq!(d.focused(), None);

        assert_eq!(d.input(Event::Key(Key::Enter), true, 0), Routed::Handled);
        let term_id = d.focused().unwrap();
        assert_eq!(d.app(term_id).unwrap().title(), "terminal");
        assert!(!d.frame(0).is_empty());
        assert!(d.frame(1).is_empty());

        for ch in "exit".chars() {
            d.input(Event::Text(ch), false, 10);
        }
        d.input(Event::Key(Key::Enter), false, 11);
        assert_eq!(d.focused(), None);
        assert!(d.order().is_empty());
        assert!(!d.frame(12).is_empty());
    }
}
