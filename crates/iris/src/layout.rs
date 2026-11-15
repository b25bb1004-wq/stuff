//! Master-and-stack tiling geometry, focus cycling, and pointer/drag routing.

use indium::{Event, Key};
use pigment::Rect;

use crate::window::{HitZone, Window};
use crate::{Compositor, Layout, Routed, WinId};

/// Active title-bar drag in floating layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DragState {
    pub id: WinId,
    pub offset_x: i32,
    pub offset_y: i32,
}

/// Compute master-and-stack tiling rects for `count` windows on a `width x height` screen.
pub(crate) fn compute_tiling_rects(width: u32, height: u32, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        }];
    }

    let mut out = Vec::with_capacity(count);
    let master_w = width / 2;
    let stack_w = width.saturating_sub(master_w);
    let stack_x = i32::try_from(master_w).unwrap_or(i32::MAX);

    out.push(Rect {
        x: 0,
        y: 0,
        w: master_w,
        h: height,
    });

    let stack_n = (count - 1) as u64;
    let total_h = u64::from(height);
    for j in 0..stack_n {
        let y0 = j.saturating_mul(total_h) / stack_n;
        let y1 = j.saturating_add(1).saturating_mul(total_h) / stack_n;
        out.push(Rect {
            x: stack_x,
            y: i32::try_from(y0).unwrap_or(i32::MAX),
            w: stack_w,
            h: u32::try_from(y1.saturating_sub(y0)).unwrap_or(0),
        });
    }
    out
}

impl Compositor {
    pub(crate) fn apply_tiling(&mut self, now_ms: u64, snap_id: Option<WinId>) {
        let rects = compute_tiling_rects(
            self.screen.width,
            self.screen.height,
            self.tiling_order.len(),
        );
        for (&id, &target) in self.tiling_order.iter().zip(rects.iter()) {
            if let Some(win) = self.windows.get_mut(&id) {
                if snap_id == Some(id) {
                    win.snap_to(target);
                } else {
                    win.animate_to(target, now_ms);
                }
            }
        }
    }

    pub(crate) fn route_input(&mut self, ev: Event, super_held: bool, now_ms: u64) -> Routed {
        if super_held {
            match ev {
                Event::Key(Key::Tab) => {
                    self.cycle_focus(true);
                    return Routed::Handled;
                }
                Event::Key(Key::ShiftTab) => {
                    self.cycle_focus(false);
                    return Routed::Handled;
                }
                Event::Text('[') => {
                    self.shift_tiling_order(-1, now_ms);
                    return Routed::Handled;
                }
                Event::Text(']') => {
                    self.shift_tiling_order(1, now_ms);
                    return Routed::Handled;
                }
                _ => {}
            }
        }

        match ev {
            Event::Key(_) | Event::Text(_) => match self.focused {
                Some(id) => Routed::ToWindow(id, ev),
                None => Routed::Dropped,
            },
            Event::MouseMove { x, y } => self.handle_mouse_move(x, y, now_ms),
            Event::MouseDown { x, y } => self.handle_mouse_down(x, y, now_ms),
            Event::MouseUp { x, y } => self.handle_mouse_up(x, y, now_ms),
        }
    }

    fn cycle_focus(&mut self, forward: bool) {
        let n = self.tiling_order.len();
        if n == 0 {
            self.focused = None;
            return;
        }
        let next_id = match self
            .focused
            .and_then(|fid| self.tiling_order.iter().position(|&id| id == fid))
        {
            Some(idx) => {
                let next_idx = if forward {
                    (idx + 1) % n
                } else {
                    (idx + n - 1) % n
                };
                self.tiling_order.get(next_idx).copied()
            }
            None => {
                if forward {
                    self.tiling_order.first().copied()
                } else {
                    self.tiling_order.last().copied()
                }
            }
        };
        self.focused = next_id;
        if self.layout == Layout::Floating {
            if let Some(id) = next_id {
                self.raise(id);
            }
        }
    }

    fn shift_tiling_order(&mut self, delta: isize, now_ms: u64) {
        let Some(fid) = self.focused else {
            return;
        };
        let Some(pos) = self.tiling_order.iter().position(|&id| id == fid) else {
            return;
        };
        let target_pos = if delta < 0 {
            if pos == 0 {
                return;
            }
            pos - 1
        } else {
            if pos + 1 >= self.tiling_order.len() {
                return;
            }
            pos + 1
        };
        self.tiling_order.swap(pos, target_pos);
        if self.layout == Layout::Tiling {
            self.apply_tiling(now_ms, None);
        }
    }

    fn topmost_hit(&self, x: i32, y: i32, now_ms: u64) -> Option<(WinId, Rect, HitZone)> {
        for &id in self.z_order.iter().rev() {
            if let Some(win) = self.windows.get(&id) {
                let frame = win.rect_at(now_ms);
                if let Some(zone) = Window::hit_test(frame, x, y) {
                    return Some((id, frame, zone));
                }
            }
        }
        None
    }

    fn handle_mouse_down(&mut self, x: i32, y: i32, now_ms: u64) -> Routed {
        let Some((id, frame, zone)) = self.topmost_hit(x, y, now_ms) else {
            return Routed::Dropped;
        };
        self.focused = Some(id);
        if self.layout == Layout::Floating {
            self.raise(id);
        }
        match zone {
            HitZone::Content { cx, cy } => Routed::ToWindow(id, Event::MouseDown { x: cx, y: cy }),
            HitZone::TitleBar => {
                if self.layout == Layout::Floating {
                    if let Some(win) = self.windows.get_mut(&id) {
                        win.snap_to(frame);
                        win.floating_rect = frame;
                    }
                    self.drag = Some(DragState {
                        id,
                        offset_x: x.saturating_sub(frame.x),
                        offset_y: y.saturating_sub(frame.y),
                    });
                }
                Routed::Handled
            }
            HitZone::Border => Routed::Handled,
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32, now_ms: u64) -> Routed {
        if let Some(drag) = self.drag {
            if self.layout == Layout::Floating {
                if let Some(win) = self.windows.get_mut(&drag.id) {
                    win.floating_rect.x = x.saturating_sub(drag.offset_x);
                    win.floating_rect.y = y.saturating_sub(drag.offset_y);
                    let target = win.floating_rect;
                    win.snap_to(target);
                    return Routed::Handled;
                }
            }
            self.drag = None;
        }
        match self.topmost_hit(x, y, now_ms) {
            Some((id, _, HitZone::Content { cx, cy })) => {
                Routed::ToWindow(id, Event::MouseMove { x: cx, y: cy })
            }
            Some(_) => Routed::Handled,
            None => Routed::Dropped,
        }
    }

    fn handle_mouse_up(&mut self, x: i32, y: i32, now_ms: u64) -> Routed {
        if let Some(drag) = self.drag.take() {
            if self.layout == Layout::Floating {
                if let Some(win) = self.windows.get_mut(&drag.id) {
                    win.floating_rect.x = x.saturating_sub(drag.offset_x);
                    win.floating_rect.y = y.saturating_sub(drag.offset_y);
                    let target = win.floating_rect;
                    win.snap_to(target);
                    return Routed::Handled;
                }
            }
        }
        match self.topmost_hit(x, y, now_ms) {
            Some((id, _, HitZone::Content { cx, cy })) => {
                Routed::ToWindow(id, Event::MouseUp { x: cx, y: cy })
            }
            Some(_) => Routed::Handled,
            None => Routed::Dropped,
        }
    }
}
