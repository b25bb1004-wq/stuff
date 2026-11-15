//! Window state, chrome geometry, animation, hit-testing, and rendering.

use glint::anim::{tween, Ease};
use pigment::{Canvas, Rect, Rgba};

/// Width in pixels of the window frame border on each side.
pub(crate) const BORDER_PX: u32 = 1;
/// Height in pixels of the window title bar below the top border.
pub(crate) const TITLE_BAR_PX: u32 = 18;
/// Total horizontal chrome added to client content width (left + right border).
pub(crate) const CHROME_W: u32 = BORDER_PX * 2;
/// Total vertical chrome added to client content height (top border + title bar + bottom border).
pub(crate) const CHROME_H: u32 = BORDER_PX + TITLE_BAR_PX + BORDER_PX;
/// Duration in milliseconds of layout and `move_to` animations.
pub(crate) const ANIM_MS: u64 = 180;

/// Desktop background colour.
pub(crate) const DESKTOP_COLOR: Rgba = Rgba(28, 32, 40, 255);
/// 1 px window border colour.
pub(crate) const BORDER_COLOR: Rgba = Rgba(68, 76, 92, 255);
/// Title bar colour for the focused window.
pub(crate) const TITLE_FOCUSED_COLOR: Rgba = Rgba(64, 118, 210, 255);
/// Title bar colour for unfocused windows.
pub(crate) const TITLE_UNFOCUSED_COLOR: Rgba = Rgba(46, 52, 64, 255);
/// Default client content background colour.
pub(crate) const CONTENT_BG_COLOR: Rgba = Rgba(24, 26, 32, 255);

/// Active rectangular interpolation between two frame rects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnimRect {
    pub from: Rect,
    pub to: Rect,
    pub start_ms: u64,
    pub dur_ms: u64,
}

impl AnimRect {
    /// Evaluate the animated frame rect at `now_ms`.
    pub fn rect_at(&self, now_ms: u64) -> Rect {
        if self.is_done(now_ms) {
            return self.to;
        }
        if now_ms <= self.start_ms {
            return self.from;
        }
        let x = tween(
            self.from.x,
            self.to.x,
            self.start_ms,
            self.dur_ms,
            now_ms,
            Ease::OutCubic,
        );
        let y = tween(
            self.from.y,
            self.to.y,
            self.start_ms,
            self.dur_ms,
            now_ms,
            Ease::OutCubic,
        );
        let fw = i32::try_from(self.from.w).unwrap_or(i32::MAX);
        let tw = i32::try_from(self.to.w).unwrap_or(i32::MAX);
        let fh = i32::try_from(self.from.h).unwrap_or(i32::MAX);
        let th = i32::try_from(self.to.h).unwrap_or(i32::MAX);
        let w = tween(fw, tw, self.start_ms, self.dur_ms, now_ms, Ease::OutCubic).max(0) as u32;
        let h = tween(fh, th, self.start_ms, self.dur_ms, now_ms, Ease::OutCubic).max(0) as u32;
        Rect { x, y, w, h }
    }

    /// True once the animation has reached `to`.
    pub fn is_done(&self, now_ms: u64) -> bool {
        self.dur_ms == 0 || now_ms >= self.start_ms.saturating_add(self.dur_ms)
    }
}

/// Region hit by a pointer coordinate inside a window frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitZone {
    TitleBar,
    Content { cx: i32, cy: i32 },
    Border,
}

/// Internal per-window state.
pub(crate) struct Window {
    pub _title: String,
    pub surface: Canvas,
    pub floating_rect: Rect,
    pub rect: Rect,
    pub anim: Option<AnimRect>,
    pub last_rect: Option<Rect>,
    pub last_z: Option<usize>,
    pub last_focused: bool,
    pub dirty_all: bool,
    pub dirty_rects: Vec<Rect>,
}

impl Window {
    /// Create a new window with client surface `w x h` at floating frame position `(fx, fy)`.
    pub fn new(title: &str, w: u32, h: u32, fx: i32, fy: i32) -> Self {
        let frame_w = w.saturating_add(CHROME_W);
        let frame_h = h.saturating_add(CHROME_H);
        let floating_rect = Rect {
            x: fx,
            y: fy,
            w: frame_w,
            h: frame_h,
        };
        Self {
            _title: title.to_owned(),
            surface: Canvas::new(w, h, CONTENT_BG_COLOR),
            floating_rect,
            rect: floating_rect,
            anim: None,
            last_rect: None,
            last_z: None,
            last_focused: false,
            dirty_all: true,
            dirty_rects: Vec::new(),
        }
    }

    /// Current frame rect at `now_ms`.
    pub fn rect_at(&self, now_ms: u64) -> Rect {
        match &self.anim {
            Some(anim) => anim.rect_at(now_ms),
            None => self.rect,
        }
    }

    /// Snap finished animation state at `now_ms`.
    pub fn advance(&mut self, now_ms: u64) {
        if let Some(anim) = self.anim {
            if anim.is_done(now_ms) {
                self.rect = anim.to;
                self.anim = None;
            }
        }
    }

    /// Start an eased 180 ms animation from current rect at `now_ms` to `target`.
    pub fn animate_to(&mut self, target: Rect, now_ms: u64) {
        let cur = self.rect_at(now_ms);
        self.rect = target;
        if cur == target {
            self.anim = None;
        } else {
            self.anim = Some(AnimRect {
                from: cur,
                to: target,
                start_ms: now_ms,
                dur_ms: ANIM_MS,
            });
        }
    }

    /// Immediately move without animation.
    pub fn snap_to(&mut self, target: Rect) {
        self.rect = target;
        self.anim = None;
    }

    /// Resize the client surface, preserving top-left pixels.
    pub fn resize_surface(&mut self, w: u32, h: u32) {
        let mut next = Canvas::new(w, h, CONTENT_BG_COLOR);
        let copy_w = self.surface.width.min(next.width) as usize;
        let copy_h = self.surface.height.min(next.height) as usize;
        let old_w = self.surface.width as usize;
        let new_w = next.width as usize;
        for row in 0..copy_h {
            let src_start = row.saturating_mul(old_w);
            let dst_start = row.saturating_mul(new_w);
            if let (Some(src), Some(dst)) = (
                self.surface
                    .pixels
                    .get(src_start..src_start.saturating_add(copy_w)),
                next.pixels
                    .get_mut(dst_start..dst_start.saturating_add(copy_w)),
            ) {
                dst.copy_from_slice(src);
            }
        }
        self.surface = next;
        self.floating_rect.w = w.saturating_add(CHROME_W);
        self.floating_rect.h = h.saturating_add(CHROME_H);
        self.dirty_all = true;
        self.dirty_rects.clear();
    }

    /// Title bar rect in screen coordinates for `frame`.
    pub fn title_bar_rect(frame: Rect) -> Option<Rect> {
        if frame.w <= CHROME_W || frame.h <= CHROME_W {
            return None;
        }
        Some(Rect {
            x: frame.x.saturating_add(1),
            y: frame.y.saturating_add(1),
            w: frame.w - CHROME_W,
            h: TITLE_BAR_PX.min(frame.h - CHROME_W),
        })
    }

    /// Client content rect in screen coordinates for `frame`.
    pub fn content_rect(frame: Rect) -> Option<Rect> {
        if frame.w <= CHROME_W || frame.h <= CHROME_H {
            return None;
        }
        Some(Rect {
            x: frame.x.saturating_add(1),
            y: frame.y.saturating_add(19),
            w: frame.w - CHROME_W,
            h: frame.h - CHROME_H,
        })
    }

    /// Classify a screen coordinate `(px, py)` against `frame`.
    pub fn hit_test(frame: Rect, px: i32, py: i32) -> Option<HitZone> {
        let fx0 = i64::from(frame.x);
        let fy0 = i64::from(frame.y);
        let fx1 = fx0 + i64::from(frame.w);
        let fy1 = fy0 + i64::from(frame.h);
        let x = i64::from(px);
        let y = i64::from(py);
        if x < fx0 || x >= fx1 || y < fy0 || y >= fy1 {
            return None;
        }
        if frame.w > CHROME_W && frame.h > CHROME_H {
            let cx0 = fx0 + 1;
            let cy0 = fy0 + 19;
            let cx1 = fx1 - 1;
            let cy1 = fy1 - 1;
            if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
                let cx = i32::try_from(x - cx0).unwrap_or(i32::MAX);
                let cy = i32::try_from(y - cy0).unwrap_or(i32::MAX);
                return Some(HitZone::Content { cx, cy });
            }
        }
        if frame.w > CHROME_W && frame.h > CHROME_W {
            let tx0 = fx0 + 1;
            let ty0 = fy0 + 1;
            let tx1 = fx1 - 1;
            let ty1 = (fy0 + 19).min(fy1 - 1);
            if x >= tx0 && x < tx1 && y >= ty0 && y < ty1 {
                return Some(HitZone::TitleBar);
            }
        }
        Some(HitZone::Border)
    }

    /// Composite this window onto `dst`, whose top-left is at `(offset_x, offset_y)` on screen.
    pub fn draw_onto(
        &self,
        dst: &mut Canvas,
        offset_x: i32,
        offset_y: i32,
        frame: Rect,
        focused: bool,
    ) {
        if frame.w == 0 || frame.h == 0 || dst.width == 0 || dst.height == 0 {
            return;
        }
        let fx0 = i64::from(frame.x) - i64::from(offset_x);
        let fy0 = i64::from(frame.y) - i64::from(offset_y);
        let fx1 = fx0 + i64::from(frame.w);
        let fy1 = fy0 + i64::from(frame.h);
        let dw = i64::from(dst.width);
        let dh = i64::from(dst.height);
        if fx1 <= 0 || fy1 <= 0 || fx0 >= dw || fy0 >= dh {
            return;
        }

        // 1 px border on all 4 edges without double-blending corners.
        fill_box_clipped(dst, fx0, fy0, fx1, fy0 + 1, BORDER_COLOR);
        if frame.h >= 2 {
            fill_box_clipped(dst, fx0, fy1 - 1, fx1, fy1, BORDER_COLOR);
        }
        if frame.h > 2 {
            fill_box_clipped(dst, fx0, fy0 + 1, fx0 + 1, fy1 - 1, BORDER_COLOR);
            if frame.w >= 2 {
                fill_box_clipped(dst, fx1 - 1, fy0 + 1, fx1, fy1 - 1, BORDER_COLOR);
            }
        }

        // 18 px title bar inside the border.
        if frame.w > CHROME_W && frame.h > CHROME_W {
            let tb_col = if focused {
                TITLE_FOCUSED_COLOR
            } else {
                TITLE_UNFOCUSED_COLOR
            };
            let ty1 = (fy0 + 1 + i64::from(TITLE_BAR_PX)).min(fy1 - 1);
            fill_box_clipped(dst, fx0 + 1, fy0 + 1, fx1 - 1, ty1, tb_col);
        }

        // Content area below the title bar, clipped to content rect.
        if frame.w > CHROME_W && frame.h > CHROME_H {
            let cx0 = fx0 + 1;
            let cy0 = fy0 + 19;
            let cx1 = fx1 - 1;
            let cy1 = fy1 - 1;
            fill_box_clipped(dst, cx0, cy0, cx1, cy1, CONTENT_BG_COLOR);

            let cw = frame.w - CHROME_W;
            let ch = frame.h - CHROME_H;
            if self.surface.width <= cw && self.surface.height <= ch {
                if let (Ok(dx), Ok(dy)) = (i32::try_from(cx0), i32::try_from(cy0)) {
                    dst.draw(&self.surface, dx, dy);
                    return;
                }
            }
            let bx0 = cx0.max(0);
            let by0 = cy0.max(0);
            let bx1 = cx1.min(dw).min(cx0 + i64::from(self.surface.width));
            let by1 = cy1.min(dh).min(cy0 + i64::from(self.surface.height));
            for dy in by0..by1 {
                let sy = (dy - cy0) as i32;
                for dx in bx0..bx1 {
                    let sx = (dx - cx0) as i32;
                    if let Some(p) = self.surface.get(sx, sy) {
                        dst.blend(dx as i32, dy as i32, p);
                    }
                }
            }
        }
    }
}

/// Fill `[x0, x1) x [y0, y1)` on `dst`, clipped to `dst` bounds in 64-bit arithmetic.
fn fill_box_clipped(dst: &mut Canvas, x0: i64, y0: i64, x1: i64, y1: i64, c: Rgba) {
    let cx0 = x0.max(0);
    let cy0 = y0.max(0);
    let cx1 = x1.min(i64::from(dst.width));
    let cy1 = y1.min(i64::from(dst.height));
    if cx0 < cx1 && cy0 < cy1 {
        if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
            i32::try_from(cx0),
            i32::try_from(cy0),
            u32::try_from(cx1 - cx0),
            u32::try_from(cy1 - cy0),
        ) {
            dst.fill_rect(Rect { x, y, w, h }, c);
        }
    }
}
