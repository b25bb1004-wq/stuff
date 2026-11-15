//! 2-D software renderer for physics worlds using `pigment`.

use pigment::{Canvas, Rect, Rgba};

use crate::fx::{isqrt, Fx, Vec2};
use crate::phys::{BodyId, Shape, World};

/// Orthographic 2-D camera with world-space centre and pixel scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    /// World-space point mapped to the centre of the canvas.
    pub center: Vec2,
    /// Pixels per world unit.
    pub scale: Fx,
}

/// Project a world-space point `p` into canvas pixel coordinates (world Y up, screen Y down).
pub fn to_screen(cam: &Camera, width: u32, height: u32, p: Vec2) -> (i32, i32) {
    let dx = ((p.x - cam.center.x) * cam.scale).round().to_int();
    let dy = ((p.y - cam.center.y) * cam.scale).round().to_int();
    let sx = i64::from(width / 2).saturating_add(dx);
    let sy = i64::from(height / 2).saturating_sub(dy);
    (
        sx.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        sy.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    )
}

/// Render every live body in `world` onto `canvas` in ascending `BodyId` order.
pub fn draw_world(
    canvas: &mut Canvas,
    cam: &Camera,
    world: &World,
    color: &dyn Fn(BodyId) -> Rgba,
) {
    for id in world.ids() {
        let Some(body) = world.body(id) else {
            continue;
        };
        let col = color(id);
        match body.shape {
            Shape::Aabb { half } => {
                let hx = half.x.max(Fx::ZERO);
                let hy = half.y.max(Fx::ZERO);
                let (x0, y0) = to_screen(
                    cam,
                    canvas.width,
                    canvas.height,
                    Vec2::new(body.pos.x - hx, body.pos.y + hy),
                );
                let (x1, y1) = to_screen(
                    cam,
                    canvas.width,
                    canvas.height,
                    Vec2::new(body.pos.x + hx, body.pos.y - hy),
                );
                let w = (i64::from(x1) - i64::from(x0)).clamp(0, i64::from(u32::MAX)) as u32;
                let h = (i64::from(y1) - i64::from(y0)).clamp(0, i64::from(u32::MAX)) as u32;
                canvas.fill_rect(Rect { x: x0, y: y0, w, h }, col);
            }
            Shape::Circle { r } => {
                if r <= Fx::ZERO || cam.scale <= Fx::ZERO {
                    continue;
                }
                let rad = (r * cam.scale).round().to_int();
                if rad <= 0 {
                    continue;
                }
                let (cx, cy) = to_screen(cam, canvas.width, canvas.height, body.pos);
                draw_disc(canvas, cx, cy, rad, col);
            }
        }
    }
}

/// Fill an integer disc of radius `rad` centred at `(cx, cy)` using clipped horizontal spans.
fn draw_disc(canvas: &mut Canvas, cx: i32, cy: i32, rad: i64, col: Rgba) {
    let y_min = (i64::from(cy).saturating_sub(rad)).max(0);
    let y_max = (i64::from(cy).saturating_add(rad)).min(i64::from(canvas.height) - 1);
    if y_min > y_max {
        return;
    }
    let rad_sq = i128::from(rad) * i128::from(rad);
    let cw = i64::from(canvas.width);
    for y in y_min..=y_max {
        let dy = y - i64::from(cy);
        let rem = rad_sq - i128::from(dy) * i128::from(dy);
        if rem < 0 {
            continue;
        }
        let dx_max = isqrt(rem as u128) as i64;
        let x0 = (i64::from(cx).saturating_sub(dx_max)).max(0);
        let x1 = (i64::from(cx).saturating_add(dx_max).saturating_add(1)).min(cw);
        if x0 < x1 {
            canvas.fill_rect(
                Rect {
                    x: x0 as i32,
                    y: y as i32,
                    w: (x1 - x0) as u32,
                    h: 1,
                },
                col,
            );
        }
    }
}
