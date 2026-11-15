//! Flex containers: items in a row or a column, grow, justify and cross-axis alignment.

use crate::dom::NodeId;
use crate::style::{Computed, Display, Flex, FlexAlign, FlexDir, FlexJustify, Len};

use super::block::place;
use super::{margin_px, Ctx};

/// A flex item: an element child of a flex container that is not `display: none`.
struct Item {
    id: NodeId,
    st: Computed,
    grow: u32,
}

/// A placed item. Its boxes and text runs are the output ranges `[start, end)` they occupy,
/// so the cross-axis pass can move them.
struct Placed {
    st: Computed,
    boxes: (usize, usize),
    runs: (usize, usize),
    mt: i32,
    mb: i32,
    bh: i32,
}

/// Lay out the items of flex container `id` (style `st`) in a content box at (`cx`, `cy`) of
/// width `cw`. Returns the content height used when the container height is auto.
pub(crate) fn children(ctx: &mut Ctx, id: NodeId, st: Computed, cx: i32, cy: i32, cw: i32) -> i32 {
    let fx = ctx.flex_of(id);
    let items = items(ctx, id);
    let fixed_h = match st.height {
        Len::Px(h) => Some(h),
        Len::Auto => None,
    };
    match fx.dir {
        FlexDir::Row => row(ctx, &items, fx, (cx, cy, cw), fixed_h),
        FlexDir::Column => column(ctx, &items, fx, (cx, cy, cw), fixed_h),
    }
}

/// The flex items of `id`: element children that are not `display: none`, in order.
fn items(ctx: &Ctx, id: NodeId) -> Vec<Item> {
    let Some(node) = ctx.doc.nodes.get(id) else {
        return Vec::new();
    };
    node.children
        .iter()
        .filter_map(|&c| {
            let st = ctx.style(c)?;
            (st.display != Display::None).then(|| Item {
                id: c,
                st,
                grow: ctx.flex_of(c).grow,
            })
        })
        .collect()
}

/// Lay out the items in a row: widths come from the main-axis distribution, then each item
/// is placed and aligned on the cross axis. Returns the cross size.
fn row(
    ctx: &mut Ctx,
    items: &[Item],
    fx: Flex,
    (cx, cy, cw): (i32, i32, i32),
    fixed_h: Option<i32>,
) -> i32 {
    let (shares, off, space) = main_axis(items, fx, Some(cw), base_w);
    let mut x = cx.saturating_add(off);
    let mut placed = Vec::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        let st = it.st;
        let (ml, mr) = (margin_px(st.margin[3]), margin_px(st.margin[1]));
        let (mt, mb) = (margin_px(st.margin[0]), margin_px(st.margin[2]));
        let content = fixed(st.width).saturating_add(share(&shares, i));
        let boxes0 = ctx.boxes.len();
        let runs0 = ctx.runs.len();
        let bh = place(
            ctx,
            it.id,
            x.saturating_add(ml),
            cy.saturating_add(mt),
            content,
            0,
            true,
        );
        placed.push(Placed {
            st,
            boxes: (boxes0, ctx.boxes.len()),
            runs: (runs0, ctx.runs.len()),
            mt,
            mb,
            bh,
        });
        let bw = content.saturating_add(edge_w(&st));
        let outer = ml.saturating_add(bw).saturating_add(mr);
        x = x
            .saturating_add(outer)
            .saturating_add(fx.gap)
            .saturating_add(space);
    }
    let cross = fixed_h.unwrap_or_else(|| placed.iter().map(outer_h).fold(0, i32::max));
    for p in &placed {
        let outer = outer_h(p);
        let (dy, stretch) = match fx.align {
            FlexAlign::Start => (0, None),
            FlexAlign::Center => (cross.saturating_sub(outer).div_euclid(2), None),
            FlexAlign::End => (cross.saturating_sub(outer), None),
            FlexAlign::Stretch if p.st.height == Len::Auto => {
                let h = cross
                    .saturating_sub(p.mt)
                    .saturating_sub(p.mb)
                    .max(edge_h(&p.st));
                (0, Some(h))
            }
            FlexAlign::Stretch => (0, None),
        };
        shift(ctx, p.boxes, p.runs, dy);
        if let Some(h) = stretch {
            if p.boxes.1 > p.boxes.0 {
                if let Some(entry) = ctx.boxes.get_mut(p.boxes.0) {
                    entry.1.h = h;
                }
            }
        }
    }
    cross
}

/// Lay out the items in a column, top to bottom. Returns the content height used.
fn column(
    ctx: &mut Ctx,
    items: &[Item],
    fx: Flex,
    (cx, cy, cw): (i32, i32, i32),
    fixed_h: Option<i32>,
) -> i32 {
    let (shares, off, space) = main_axis(items, fx, fixed_h, base_h);
    let mut y = cy.saturating_add(off);
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            y = y.saturating_add(fx.gap).saturating_add(space);
        }
        let st = it.st;
        let (ml, mr) = (margin_px(st.margin[3]), margin_px(st.margin[1]));
        let (mt, mb) = (margin_px(st.margin[0]), margin_px(st.margin[2]));
        let fill = fx.align == FlexAlign::Stretch && st.width == Len::Auto;
        let content = if fill {
            cw.saturating_sub(ml)
                .saturating_sub(mr)
                .saturating_sub(edge_w(&st))
                .max(0)
        } else {
            fixed(st.width)
        };
        let outer_w = ml
            .saturating_add(content)
            .saturating_add(edge_w(&st))
            .saturating_add(mr);
        let rem = cw.saturating_sub(outer_w);
        let x_off = match fx.align {
            FlexAlign::Start | FlexAlign::Stretch => 0,
            FlexAlign::Center => rem.div_euclid(2),
            FlexAlign::End => rem,
        };
        let bh = place(
            ctx,
            it.id,
            cx.saturating_add(x_off).saturating_add(ml),
            y.saturating_add(mt),
            content,
            share(&shares, i),
            true,
        );
        y = y.saturating_add(mt).saturating_add(bh).saturating_add(mb);
    }
    y.saturating_sub(cy)
}

/// Distribute the main axis of a container of size `avail` (None when auto). Returns each
/// item's grow share, then the justify offset and the extra spacing between items.
fn main_axis(
    items: &[Item],
    fx: Flex,
    avail: Option<i32>,
    base: fn(&Computed) -> i32,
) -> (Vec<i32>, i32, i32) {
    let n = items.len();
    let Some(avail) = avail else {
        return (vec![0; n], 0, 0);
    };
    let used = items
        .iter()
        .fold(gap_total(fx.gap, n), |a, it| a.saturating_add(base(&it.st)));
    let free = avail.saturating_sub(used);
    let sum = items
        .iter()
        .fold(0u64, |a, it| a.saturating_add(u64::from(it.grow)));
    if free > 0 && sum > 0 {
        let grows: Vec<u32> = items.iter().map(|it| it.grow).collect();
        return (distribute(free, &grows, sum), 0, 0);
    }
    let (off, space) = justify(fx.justify, free.max(0), n);
    (vec![0; n], off, space)
}

/// Share of `free` for each item, floored by its grow factor; the last growing item also
/// takes the remainder. `sum` is the total grow factor and must be above 0.
fn distribute(free: i32, grows: &[u32], sum: u64) -> Vec<i32> {
    let mut shares: Vec<i32> = grows
        .iter()
        .map(|&g| {
            let s = i128::from(free) * i128::from(g) / i128::from(sum);
            i32::try_from(s).unwrap_or(0)
        })
        .collect();
    let used = shares.iter().fold(0i32, |a, &s| a.saturating_add(s));
    if let Some(last) = grows.iter().rposition(|&g| g > 0) {
        if let Some(s) = shares.get_mut(last) {
            *s = s.saturating_add(free.saturating_sub(used));
        }
    }
    shares
}

/// Leading offset and extra spacing between items for `leftover` pixels of free space.
fn justify(j: FlexJustify, leftover: i32, n: usize) -> (i32, i32) {
    match j {
        FlexJustify::Start => (0, 0),
        FlexJustify::Center => (leftover.div_euclid(2), 0),
        FlexJustify::End => (leftover, 0),
        FlexJustify::SpaceBetween => match i32::try_from(n.saturating_sub(1)) {
            Ok(k) if k > 0 => (0, leftover / k),
            _ => (0, 0),
        },
    }
}

/// Total of the gaps between `n` items.
fn gap_total(gap: i32, n: usize) -> i32 {
    let k = i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX);
    gap.saturating_mul(k)
}

/// Grow share of item `i`, or 0.
fn share(shares: &[i32], i: usize) -> i32 {
    shares.get(i).copied().unwrap_or(0)
}

/// Move the boxes and runs of one placed item down (or right) by `dy` pixels.
fn shift(ctx: &mut Ctx, boxes: (usize, usize), runs: (usize, usize), dy: i32) {
    if dy == 0 {
        return;
    }
    if let Some(s) = ctx.boxes.get_mut(boxes.0..boxes.1) {
        for (_, rect) in s {
            rect.y = rect.y.saturating_add(dy);
        }
    }
    if let Some(s) = ctx.runs.get_mut(runs.0..runs.1) {
        for run in s {
            run.rect.y = run.rect.y.saturating_add(dy);
        }
    }
}

/// Outer height of a placed item: top margin, border box and bottom margin.
fn outer_h(p: &Placed) -> i32 {
    p.mt.saturating_add(p.bh).saturating_add(p.mb)
}

/// Outer width before grow: margins, edges and a fixed width (0 for auto).
fn base_w(st: &Computed) -> i32 {
    let m = margin_px(st.margin[1]).saturating_add(margin_px(st.margin[3]));
    m.saturating_add(edge_w(st)).saturating_add(fixed(st.width))
}

/// Outer height before grow: margins, edges and a fixed height (0 for auto).
fn base_h(st: &Computed) -> i32 {
    let m = margin_px(st.margin[0]).saturating_add(margin_px(st.margin[2]));
    m.saturating_add(edge_h(st))
        .saturating_add(fixed(st.height))
}

/// Horizontal padding and border.
fn edge_w(st: &Computed) -> i32 {
    sum4([st.padding[1], st.padding[3], st.border[1], st.border[3]])
}

/// Vertical padding and border.
fn edge_h(st: &Computed) -> i32 {
    sum4([st.padding[0], st.padding[2], st.border[0], st.border[2]])
}

/// A fixed length in pixels, or 0 for auto.
fn fixed(l: Len) -> i32 {
    match l {
        Len::Px(p) => p,
        Len::Auto => 0,
    }
}

fn sum4(v: [i32; 4]) -> i32 {
    v.iter().fold(0, |a, &x| a.saturating_add(x))
}
