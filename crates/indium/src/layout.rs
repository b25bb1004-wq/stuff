//! Flex-style layout computation for `Ui`.

use pigment::{measure, Rect};

use crate::types::{Align, Dir, Justify, Kind, NodeId};
use crate::Ui;

impl Ui {
    /// Compute layout rectangles for all live nodes within `(width, height)`.
    pub fn layout(&mut self, width: u32, height: u32) {
        if self.nodes.is_empty() || self.nodes[0].is_none() {
            return;
        }

        let mut base_sizes = vec![(0u32, 0u32); self.nodes.len()];
        let mut order = Vec::new();
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.node(id) {
                order.push(id);
                for &child in &node.children {
                    stack.push(child);
                }
            }
        }

        let scale = self.theme.text_scale.max(1);
        for &id in order.iter().rev() {
            base_sizes[id] = self.compute_base_size(id, scale, &base_sizes);
        }

        if let Some(Some(root)) = self.nodes.get_mut(0) {
            root.rect = Some(Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            });
        }

        let mut place_stack = vec![0usize];
        while let Some(id) = place_stack.pop() {
            let (rect, style, children) = match self.node(id) {
                Some(n) if matches!(n.kind, Kind::Box) && !n.children.is_empty() => match n.rect {
                    Some(r) => (r, n.style, n.children.clone()),
                    None => continue,
                },
                _ => continue,
            };

            self.place_children(rect, &style, &children, &base_sizes);
            for &child in &children {
                place_stack.push(child);
            }
        }
    }

    /// Compute `(base_w, base_h)` for `id`, assuming all children already have entries in `base_sizes`.
    fn compute_base_size(&self, id: NodeId, scale: u32, base_sizes: &[(u32, u32)]) -> (u32, u32) {
        let Some(node) = self.node(id) else {
            return (0, 0);
        };

        let (content_w, content_h) = match &node.kind {
            Kind::Label(text) | Kind::Button(text) => measure(text, scale),
            Kind::Field(text) => {
                let (mw, _) = measure(text, scale);
                (
                    mw.max(80u32.saturating_mul(scale)),
                    16u32.saturating_mul(scale),
                )
            }
            Kind::Box => {
                if node.children.is_empty() {
                    (0, 0)
                } else {
                    let n = node.children.len() as u32;
                    let gaps = node.style.gap.saturating_mul(n.saturating_sub(1));
                    let mut main_sum = 0u32;
                    let mut cross_max = 0u32;
                    for &cid in &node.children {
                        let (cw, ch) = base_sizes.get(cid).copied().unwrap_or((0, 0));
                        match node.style.dir {
                            Dir::Row => {
                                main_sum = main_sum.saturating_add(cw);
                                cross_max = cross_max.max(ch);
                            }
                            Dir::Column => {
                                main_sum = main_sum.saturating_add(ch);
                                cross_max = cross_max.max(cw);
                            }
                        }
                    }
                    let total_main = main_sum.saturating_add(gaps);
                    match node.style.dir {
                        Dir::Row => (total_main, cross_max),
                        Dir::Column => (cross_max, total_main),
                    }
                }
            }
        };

        let pad_x = node
            .style
            .padding
            .left
            .saturating_add(node.style.padding.right);
        let pad_y = node
            .style
            .padding
            .top
            .saturating_add(node.style.padding.bottom);
        let w = node
            .style
            .width
            .unwrap_or_else(|| content_w.saturating_add(pad_x))
            .max(node.style.min_w);
        let h = node
            .style
            .height
            .unwrap_or_else(|| content_h.saturating_add(pad_y))
            .max(node.style.min_h);
        (w, h)
    }

    /// Position and size all `children` inside `parent_rect` according to `style`.
    fn place_children(
        &mut self,
        parent_rect: Rect,
        style: &crate::types::Style,
        children: &[NodeId],
        base_sizes: &[(u32, u32)],
    ) {
        let inner_x = parent_rect.x.saturating_add_unsigned(style.padding.left);
        let inner_y = parent_rect.y.saturating_add_unsigned(style.padding.top);
        let inner_w = parent_rect
            .w
            .saturating_sub(style.padding.left)
            .saturating_sub(style.padding.right);
        let inner_h = parent_rect
            .h
            .saturating_sub(style.padding.top)
            .saturating_sub(style.padding.bottom);

        let is_row = matches!(style.dir, Dir::Row);
        let (inner_main_start, inner_main, inner_cross_start, inner_cross) = if is_row {
            (inner_x, inner_w, inner_y, inner_h)
        } else {
            (inner_y, inner_h, inner_x, inner_w)
        };

        let n = children.len();
        let mut sum_base_main: i64 = 0;
        let mut total_grow: u64 = 0;
        for &cid in children {
            let (bw, bh) = base_sizes.get(cid).copied().unwrap_or((0, 0));
            let base_main = if is_row { bw } else { bh };
            sum_base_main = sum_base_main.saturating_add(i64::from(base_main));
            if let Some(c) = self.node(cid) {
                total_grow = total_grow.saturating_add(u64::from(c.style.grow));
            }
        }

        let gaps_total = i64::from(style.gap).saturating_mul((n as i64) - 1);
        let free = i64::from(inner_main)
            .saturating_sub(sum_base_main)
            .saturating_sub(gaps_total);

        let mut extras = vec![0u64; n];
        let (before, gap_add) = if free > 0 && total_grow > 0 {
            let free_u = free as u128;
            let total_g = u128::from(total_grow);
            let mut used: u64 = 0;
            for (i, &cid) in children.iter().enumerate() {
                let g = self.node(cid).map_or(0, |c| c.style.grow);
                if g > 0 {
                    let extra = ((free_u * u128::from(g)) / total_g) as u64;
                    extras[i] = extra;
                    used = used.saturating_add(extra);
                }
            }
            let mut rem = (free as u64).saturating_sub(used);
            for (i, &cid) in children.iter().enumerate() {
                if rem == 0 {
                    break;
                }
                if self.node(cid).is_some_and(|c| c.style.grow > 0) {
                    extras[i] = extras[i].saturating_add(1);
                    rem -= 1;
                }
            }
            (0i64, 0i64)
        } else if free > 0 {
            match style.justify {
                Justify::Start => (0, 0),
                Justify::End => (free, 0),
                Justify::Center => (free / 2, 0),
                Justify::SpaceBetween => {
                    if n <= 1 {
                        (0, 0)
                    } else {
                        (0, free / ((n as i64) - 1))
                    }
                }
            }
        } else {
            (0, 0)
        };

        let mut cursor = i64::from(inner_main_start).saturating_add(before);
        for (i, &cid) in children.iter().enumerate() {
            let (bw, bh) = base_sizes.get(cid).copied().unwrap_or((0, 0));
            let (base_main, base_cross) = if is_row { (bw, bh) } else { (bh, bw) };
            let child_main = u64::from(base_main)
                .saturating_add(extras[i])
                .min(u64::from(u32::MAX)) as u32;

            let has_explicit_cross = self.node(cid).is_some_and(|c| {
                if is_row {
                    c.style.height.is_some()
                } else {
                    c.style.width.is_some()
                }
            });

            let (child_cross_size, cross_offset) = match style.align {
                Align::Stretch => {
                    if has_explicit_cross {
                        (base_cross, 0i64)
                    } else {
                        (inner_cross, 0i64)
                    }
                }
                Align::Start => (base_cross, 0i64),
                Align::Center => {
                    if base_cross >= inner_cross {
                        (base_cross, 0i64)
                    } else {
                        (
                            base_cross,
                            (i64::from(inner_cross) - i64::from(base_cross)) / 2,
                        )
                    }
                }
                Align::End => {
                    if base_cross >= inner_cross {
                        (base_cross, 0i64)
                    } else {
                        (base_cross, i64::from(inner_cross) - i64::from(base_cross))
                    }
                }
            };

            let main_pos = cursor.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            let cross_pos = i64::from(inner_cross_start)
                .saturating_add(cross_offset)
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

            let rect = if is_row {
                Rect {
                    x: main_pos,
                    y: cross_pos,
                    w: child_main,
                    h: child_cross_size,
                }
            } else {
                Rect {
                    x: cross_pos,
                    y: main_pos,
                    w: child_cross_size,
                    h: child_main,
                }
            };

            if let Some(Some(child)) = self.nodes.get_mut(cid) {
                child.rect = Some(rect);
            }

            cursor = cursor
                .saturating_add(i64::from(child_main))
                .saturating_add(i64::from(style.gap))
                .saturating_add(gap_add);
        }
    }
}
