//! Window compositor with floating/tiling layout, input routing, eased animations, and minimal damage.
#![forbid(unsafe_code)]

mod damage;
mod layout;
mod window;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use indium::Event;
use pigment::{Canvas, Rect};

use crate::damage::{
    clip_to_bounds, extract_changed_rects, finalize_damage, intersect_rect, merge_candidates,
    offset_and_clip,
};
use crate::layout::DragState;
use crate::window::{Window, DESKTOP_COLOR};

/// Opaque window identifier returned by [`Compositor::open`].
pub type WinId = u32;

/// Window placement policy managed by [`Compositor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Windows float freely, raise on click, and drag by their title bar.
    Floating,
    /// Master-and-stack tiling: first window left half, rest stacked on the right half.
    Tiling,
}

/// Where an input went after the compositor handled its own shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Event delivered to window `0` with coordinates translated into its content area.
    ToWindow(WinId, Event),
    /// Event consumed by a compositor shortcut, title-bar drag, or window chrome.
    Handled,
    /// Event fell on the desktop background or had no target window.
    Dropped,
}

/// Software window compositor with damage-driven compositing and eased window motion.
pub struct Compositor {
    pub(crate) screen: Canvas,
    pub(crate) layout: Layout,
    pub(crate) windows: BTreeMap<WinId, Window>,
    pub(crate) z_order: Vec<WinId>,
    pub(crate) tiling_order: Vec<WinId>,
    pub(crate) focused: Option<WinId>,
    next_id: WinId,
    last_now_ms: u64,
    pub(crate) drag: Option<DragState>,
    pending_damage: Vec<Rect>,
    z_dirty: bool,
}

impl Compositor {
    /// Create a compositor with a `width x height` screen filled with the desktop colour.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            screen: Canvas::new(width, height, DESKTOP_COLOR),
            layout: Layout::Floating,
            windows: BTreeMap::new(),
            z_order: Vec::new(),
            tiling_order: Vec::new(),
            focused: None,
            next_id: 1,
            last_now_ms: 0,
            drag: None,
            pending_damage: Vec::new(),
            z_dirty: false,
        }
    }

    /// Open a new window on top and give it focus.
    pub fn open(&mut self, title: &str, w: u32, h: u32, now_ms: u64) -> WinId {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let (fx, fy) = self.next_floating_origin();
        let win = Window::new(title, w, h, fx, fy);
        self.windows.insert(id, win);
        self.z_order.push(id);
        self.tiling_order.push(id);
        self.focused = Some(id);
        self.z_dirty = true;

        if self.layout == Layout::Tiling {
            self.apply_tiling(now_ms, Some(id));
        }
        id
    }

    /// Close window `id`, returning `true` if it existed.
    pub fn close(&mut self, id: WinId, now_ms: u64) -> bool {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        let Some(win) = self.windows.remove(&id) else {
            return false;
        };
        if let Some(last_r) = win.last_rect {
            self.pending_damage.push(last_r);
        }
        if self.drag.is_some_and(|d| d.id == id) {
            self.drag = None;
        }
        self.z_order.retain(|&w| w != id);
        self.tiling_order.retain(|&w| w != id);
        if self.focused == Some(id) {
            self.focused = self.z_order.last().copied();
        }
        if self.layout == Layout::Tiling {
            self.apply_tiling(now_ms, None);
        }
        true
    }

    /// Borrow the client content canvas for `id` and mark the window damaged.
    pub fn surface_mut(&mut self, id: WinId) -> Option<&mut Canvas> {
        let win = self.windows.get_mut(&id)?;
        win.dirty_all = true;
        win.dirty_rects.clear();
        Some(&mut win.surface)
    }

    /// Record partial damage `r` in window content coordinates for `id`.
    pub fn damage(&mut self, id: WinId, r: Rect) {
        if let Some(win) = self.windows.get_mut(&id) {
            if !win.dirty_all {
                win.dirty_rects.push(r);
            }
        }
    }

    /// Switch layout between floating and tiling, animating every window to its new rect.
    pub fn set_layout(&mut self, l: Layout, now_ms: u64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        if self.layout == l {
            return;
        }
        self.layout = l;
        self.drag = None;
        match l {
            Layout::Tiling => self.apply_tiling(now_ms, None),
            Layout::Floating => {
                for &id in &self.tiling_order {
                    if let Some(win) = self.windows.get_mut(&id) {
                        let target = win.floating_rect;
                        win.animate_to(target, now_ms);
                    }
                }
            }
        }
    }

    /// Animate window `id` to `(x, y)` over 180 ms using `Ease::OutCubic`.
    pub fn move_to(&mut self, id: WinId, x: i32, y: i32, now_ms: u64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        if self.drag.is_some_and(|d| d.id == id) {
            self.drag = None;
        }
        if let Some(win) = self.windows.get_mut(&id) {
            win.floating_rect.x = x;
            win.floating_rect.y = y;
            if self.layout == Layout::Floating {
                let target = win.floating_rect;
                win.animate_to(target, now_ms);
            }
        }
    }

    /// Resize window `id`'s client surface to `w x h`.
    pub fn resize(&mut self, id: WinId, w: u32, h: u32, now_ms: u64) {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        if let Some(win) = self.windows.get_mut(&id) {
            win.resize_surface(w, h);
            if self.layout == Layout::Floating {
                if let Some(mut anim) = win.anim {
                    anim.from.w = win.floating_rect.w;
                    anim.from.h = win.floating_rect.h;
                    anim.to.w = win.floating_rect.w;
                    anim.to.h = win.floating_rect.h;
                    win.anim = Some(anim);
                    win.rect = anim.to;
                } else {
                    win.rect = win.floating_rect;
                }
            }
        }
    }

    /// Raise window `id` to the top of the stacking order.
    pub fn raise(&mut self, id: WinId) {
        if let Some(pos) = self.z_order.iter().position(|&w| w == id) {
            if pos + 1 < self.z_order.len() {
                self.z_order.remove(pos);
                self.z_order.push(id);
                self.z_dirty = true;
            }
        }
    }

    /// Currently focused window, if any.
    pub fn focused(&self) -> Option<WinId> {
        self.focused
    }

    /// Window stacking order from bottom to top.
    pub fn order(&self) -> Vec<WinId> {
        self.z_order.clone()
    }

    /// Current (possibly animated) frame rect of window `id`.
    pub fn rect(&self, id: WinId) -> Option<Rect> {
        self.windows.get(&id).map(|w| w.rect_at(self.last_now_ms))
    }

    /// Handle an input event, returning where it was routed.
    pub fn input(&mut self, ev: Event, super_held: bool, now_ms: u64) -> Routed {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        self.route_input(ev, super_held, now_ms)
    }

    /// Advance animations to `now_ms`, composite damaged regions, and return changed screen rects.
    pub fn frame(&mut self, now_ms: u64) -> Vec<Rect> {
        self.last_now_ms = self.last_now_ms.max(now_ms);
        for win in self.windows.values_mut() {
            win.advance(now_ms);
        }

        let candidates = self.collect_candidate_damage(now_ms);
        if candidates.is_empty() {
            self.commit_frame_state(now_ms);
            return Vec::new();
        }

        let merged = merge_candidates(candidates);
        let mut tracked = Vec::new();
        for region in merged {
            let mut tile = Canvas::new(region.w, region.h, DESKTOP_COLOR);
            for &id in &self.z_order {
                if let Some(win) = self.windows.get(&id) {
                    let frame = win.rect_at(now_ms);
                    if intersect_rect(frame, region).is_some() {
                        win.draw_onto(
                            &mut tile,
                            region.x,
                            region.y,
                            frame,
                            self.focused == Some(id),
                        );
                    }
                }
            }
            tracked.extend(extract_changed_rects(&tile, &mut self.screen, region));
        }

        self.commit_frame_state(now_ms);
        finalize_damage(tracked)
    }

    /// Borrow the composited screen canvas.
    pub fn screen(&self) -> &Canvas {
        &self.screen
    }

    fn next_floating_origin(&self) -> (i32, i32) {
        let count = i32::try_from(self.tiling_order.len()).unwrap_or(0);
        let max_x = (i32::try_from(self.screen.width).unwrap_or(i32::MAX) / 2).max(1);
        let max_y = (i32::try_from(self.screen.height).unwrap_or(i32::MAX) / 2).max(1);
        (
            count.saturating_mul(24) % max_x,
            count.saturating_mul(24) % max_y,
        )
    }

    fn collect_candidate_damage(&mut self, now_ms: u64) -> Vec<Rect> {
        let sw = self.screen.width;
        let sh = self.screen.height;
        let mut raw = std::mem::take(&mut self.pending_damage);

        if self.z_dirty {
            for (i, &id_a) in self.z_order.iter().enumerate() {
                let Some(wa) = self.windows.get(&id_a) else {
                    continue;
                };
                let Some(za) = wa.last_z else { continue };
                let ra_cur = wa.rect_at(now_ms);
                for &id_b in self.z_order.iter().skip(i + 1) {
                    let Some(wb) = self.windows.get(&id_b) else {
                        continue;
                    };
                    let Some(zb) = wb.last_z else { continue };
                    if za > zb {
                        if let Some(r) = intersect_rect(ra_cur, wb.rect_at(now_ms)) {
                            raw.push(r);
                        }
                        if let (Some(la), Some(lb)) = (wa.last_rect, wb.last_rect) {
                            if let Some(r) = intersect_rect(la, lb) {
                                raw.push(r);
                            }
                        }
                    }
                }
            }
        }

        for (&id, win) in &self.windows {
            let cur = win.rect_at(now_ms);
            if win.last_rect != Some(cur) {
                raw.push(cur);
                if let Some(prev) = win.last_rect {
                    raw.push(prev);
                }
                continue;
            }
            if win.last_focused != (self.focused == Some(id)) {
                if let Some(tb) = Window::title_bar_rect(cur) {
                    raw.push(tb);
                }
            }
            if win.dirty_all {
                if let Some(cr) = Window::content_rect(cur) {
                    raw.push(cr);
                }
            } else if !win.dirty_rects.is_empty() {
                if let Some(cr) = Window::content_rect(cur) {
                    for &local_r in &win.dirty_rects {
                        if let Some(clipped) = offset_and_clip(cr, local_r) {
                            raw.push(clipped);
                        }
                    }
                }
            }
        }

        raw.into_iter()
            .filter_map(|r| clip_to_bounds(r, 0, 0, sw, sh))
            .collect()
    }

    fn commit_frame_state(&mut self, now_ms: u64) {
        self.z_dirty = false;
        for (z, &id) in self.z_order.iter().enumerate() {
            if let Some(win) = self.windows.get_mut(&id) {
                win.last_rect = Some(win.rect_at(now_ms));
                win.last_z = Some(z);
                win.last_focused = self.focused == Some(id);
                win.dirty_all = false;
                win.dirty_rects.clear();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn full_render(&self, now_ms: u64) -> Canvas {
        let mut canvas = Canvas::new(self.screen.width, self.screen.height, DESKTOP_COLOR);
        for &id in &self.z_order {
            if let Some(win) = self.windows.get(&id) {
                let frame = win.rect_at(now_ms);
                win.draw_onto(&mut canvas, 0, 0, frame, self.focused == Some(id));
            }
        }
        canvas
    }
}
