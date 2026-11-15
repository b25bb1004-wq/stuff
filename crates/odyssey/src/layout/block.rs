//! Block boxes: width resolution, vertical stacking and margin collapsing.

use crate::dom::NodeId;
use crate::style::{Computed, Display, Len};

use super::flex;
use super::inline::{self, Item};
use super::{margin_px, BoxRect, Ctx, MAX_DEPTH};

/// Lay out block `id` in a containing block at `cb_x` with width `cb_w`, its border box
/// starting at `border_top`. Returns the border-box height.
pub(crate) fn block(ctx: &mut Ctx, id: NodeId, cb_x: i32, cb_w: i32, border_top: i32) -> i32 {
    let Some(st) = ctx.style(id) else {
        return 0;
    };
    let (ml, content_w) = horizontal(&st, cb_w);
    let report = matches!(st.display, Display::Block | Display::Flex);
    place(
        ctx,
        id,
        cb_x.saturating_add(ml),
        border_top,
        content_w,
        0,
        report,
    )
}

/// Lay out `id` with its border box at (`border_x`, `border_top`) and content width `content_w`,
/// adding `extra_h` to its content height. Its box is reported when `report`. Returns the
/// border-box height.
pub(crate) fn place(
    ctx: &mut Ctx,
    id: NodeId,
    border_x: i32,
    border_top: i32,
    content_w: i32,
    extra_h: i32,
    report: bool,
) -> i32 {
    if ctx.depth >= MAX_DEPTH {
        return 0;
    }
    let Some(st) = ctx.style(id) else {
        return 0;
    };
    let [pt, pr, pb, pl] = st.padding;
    let [bt, br, bb, bl] = st.border;
    let bw = content_w.saturating_add(sum([pl, pr, bl, br]));
    let slot = report.then(|| {
        ctx.boxes.push((
            id,
            BoxRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        ));
        ctx.boxes.len() - 1
    });
    let content_x = border_x.saturating_add(bl).saturating_add(pl);
    let content_y = border_top.saturating_add(bt).saturating_add(pt);
    ctx.depth += 1;
    let auto_h = if st.display == Display::Flex {
        flex::children(ctx, id, st, content_x, content_y, content_w)
    } else {
        children(ctx, id, content_x, content_y, content_w)
    };
    ctx.depth -= 1;
    let content_h = match st.height {
        Len::Px(h) => h,
        // Negative margins can sum below zero; a box never has a negative height.
        Len::Auto => auto_h.max(0),
    }
    .saturating_add(extra_h)
    .max(0);
    let bh = content_h.saturating_add(sum([pt, pb, bt, bb]));
    if let Some(entry) = slot.and_then(|i| ctx.boxes.get_mut(i)) {
        entry.1 = BoxRect {
            x: border_x,
            y: border_top,
            w: bw,
            h: bh,
        };
    }
    bh
}

fn sum(vals: [i32; 4]) -> i32 {
    vals.iter().fold(0, |a, &v| a.saturating_add(v))
}

/// Resolve the left margin and the content width of a block (CSS 2.1 section 10.3.3, simplified).
fn horizontal(st: &Computed, cb_w: i32) -> (i32, i32) {
    let edges = sum([st.padding[1], st.padding[3], st.border[1], st.border[3]]);
    let fixed_l = margin_px(st.margin[3]);
    let fixed_r = margin_px(st.margin[1]);
    let free = cb_w
        .saturating_sub(fixed_l)
        .saturating_sub(fixed_r)
        .saturating_sub(edges)
        .max(0);
    match st.width {
        Len::Auto => (fixed_l, free),
        Len::Px(w) => {
            let slack = cb_w
                .saturating_sub(w)
                .saturating_sub(fixed_l)
                .saturating_sub(fixed_r)
                .saturating_sub(edges)
                .max(0);
            let ml = match (st.margin[3], st.margin[1]) {
                (Len::Auto, Len::Auto) => slack / 2,
                (Len::Auto, _) => slack,
                _ => fixed_l,
            };
            (ml, w)
        }
    }
}

/// Gap between the previous bottom edge and the next top edge: the larger of the two margins,
/// or just the child's margin when there is no previous sibling.
fn gap(pending: Option<i32>, mt: i32) -> i32 {
    match pending {
        None => mt,
        Some(p) => p.max(mt),
    }
}

/// Stack the children of `parent` inside a content box at (`cx`, `cy`) of width `cw`.
/// Returns the content height, bottom margin of the last block included.
fn children(ctx: &mut Ctx, parent: NodeId, cx: i32, cy: i32, cw: i32) -> i32 {
    let items = inline::flatten(ctx, parent);
    if !items.iter().any(|i| matches!(i, Item::Block(_))) {
        return inline::lines(ctx, &items, cx, cy, cw);
    }
    let mut cursor = cy;
    let mut pending: Option<i32> = None;
    let mut run: Vec<Item> = Vec::new();
    for item in items {
        match item {
            Item::Block(child) => {
                flush_run(ctx, &mut run, cx, cw, &mut cursor, &mut pending);
                let Some(st) = ctx.style(child) else {
                    continue;
                };
                let top = cursor.saturating_add(gap(pending, margin_px(st.margin[0])));
                let bh = block(ctx, child, cx, cw, top);
                cursor = top.saturating_add(bh);
                pending = Some(margin_px(st.margin[2]));
            }
            Item::Text { .. } => run.push(item),
        }
    }
    flush_run(ctx, &mut run, cx, cw, &mut cursor, &mut pending);
    cursor
        .saturating_sub(cy)
        .saturating_add(pending.unwrap_or(0))
}

/// Wrap a run of inline items in an anonymous block. A run with no line boxes takes no room
/// and does not separate its neighbours' margins.
fn flush_run(
    ctx: &mut Ctx,
    run: &mut Vec<Item>,
    cx: i32,
    cw: i32,
    cursor: &mut i32,
    pending: &mut Option<i32>,
) {
    if run.is_empty() {
        return;
    }
    let top = cursor.saturating_add(gap(*pending, 0));
    let h = inline::lines(ctx, run.as_slice(), cx, top, cw);
    if h > 0 {
        *cursor = top.saturating_add(h);
        *pending = Some(0);
    }
    run.clear();
}
