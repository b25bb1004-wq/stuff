//! Software painting of laid-out documents onto a `pigment::Canvas`.

use pigment::{Canvas, Rect, Rgba};

use crate::dom::{Document, NodeId};
use crate::layout::{BoxRect, Layout};
use crate::style::Computed;

/// Paint box backgrounds, solid borders, and text runs in document order onto `canvas`.
pub fn paint(doc: &Document, styles: &[Option<Computed>], layout: &Layout, canvas: &mut Canvas) {
    for &(id, rect) in &layout.boxes {
        let Some(st) = styles.get(id).copied().flatten() else {
            continue;
        };
        if rect.w <= 0 || rect.h <= 0 {
            continue;
        }
        let w = u32::try_from(rect.w).unwrap_or(0);
        let h = u32::try_from(rect.h).unwrap_or(0);
        let bg = rgba(st.background);
        if bg.3 > 0 {
            canvas.fill_rect(
                Rect {
                    x: rect.x,
                    y: rect.y,
                    w,
                    h,
                },
                bg,
            );
        }
        let border_col = rgba(st.color);
        if border_col.3 > 0 {
            draw_borders(canvas, rect, st.border, border_col);
        }
    }

    for run in &layout.runs {
        let col = text_color(doc, styles, run.node);
        if col.3 == 0 {
            continue;
        }
        canvas.draw_text(
            run.rect.x,
            run.rect.y,
            &run.text,
            col,
            run.scale.max(1),
            u32::MAX,
        );
    }
}

/// Convert `[r, g, b, a]` to `pigment::Rgba`.
fn rgba(c: [u8; 4]) -> Rgba {
    Rgba(c[0], c[1], c[2], c[3])
}

/// Draw the four solid border sides of `rect` without double-blending corners.
fn draw_borders(canvas: &mut Canvas, rect: BoxRect, border: [i32; 4], color: Rgba) {
    let bt = border[0].max(0).min(rect.h);
    let bb = border[2].max(0).min(rect.h.saturating_sub(bt));
    let bl = border[3].max(0).min(rect.w);
    let br = border[1].max(0).min(rect.w.saturating_sub(bl));
    let w = u32::try_from(rect.w).unwrap_or(0);

    if let Ok(bh) = u32::try_from(bt) {
        if bh > 0 {
            canvas.fill_rect(
                Rect {
                    x: rect.x,
                    y: rect.y,
                    w,
                    h: bh,
                },
                color,
            );
        }
    }
    if let Ok(bh) = u32::try_from(bb) {
        if bh > 0 {
            canvas.fill_rect(
                Rect {
                    x: rect.x,
                    y: rect.y.saturating_add(rect.h.saturating_sub(bb)),
                    w,
                    h: bh,
                },
                color,
            );
        }
    }
    let mid_h = u32::try_from(rect.h.saturating_sub(bt).saturating_sub(bb)).unwrap_or(0);
    if mid_h > 0 {
        let mid_y = rect.y.saturating_add(bt);
        if let Ok(bw) = u32::try_from(bl) {
            if bw > 0 {
                canvas.fill_rect(
                    Rect {
                        x: rect.x,
                        y: mid_y,
                        w: bw,
                        h: mid_h,
                    },
                    color,
                );
            }
        }
        if let Ok(bw) = u32::try_from(br) {
            if bw > 0 {
                canvas.fill_rect(
                    Rect {
                        x: rect.x.saturating_add(rect.w.saturating_sub(br)),
                        y: mid_y,
                        w: bw,
                        h: mid_h,
                    },
                    color,
                );
            }
        }
    }
}

/// Resolve the text colour of `text_node` from its parent element's computed style.
fn text_color(doc: &Document, styles: &[Option<Computed>], text_node: NodeId) -> Rgba {
    doc.nodes
        .get(text_node)
        .and_then(|n| n.parent)
        .and_then(|pid| styles.get(pid).copied().flatten())
        .map_or(Rgba(0, 0, 0, 255), |st| rgba(st.color))
}
