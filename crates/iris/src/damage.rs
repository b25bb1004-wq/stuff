//! Screen-space damage clipping, candidate merging, pixel diffing, and tight rect packing.

use pigment::{Canvas, Rect};

/// Tracked screen-space damage rect paired with its exact count of changed pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackedRect {
    pub rect: Rect,
    pub changed: u64,
}

/// Clip `r` to `[bx, bx + bw) x [by, by + bh)`, returning `None` if empty.
pub(crate) fn clip_to_bounds(r: Rect, bx: i32, by: i32, bw: u32, bh: u32) -> Option<Rect> {
    let x0 = i64::from(r.x).max(i64::from(bx));
    let y0 = i64::from(r.y).max(i64::from(by));
    let x1 = (i64::from(r.x) + i64::from(r.w)).min(i64::from(bx) + i64::from(bw));
    let y1 = (i64::from(r.y) + i64::from(r.h)).min(i64::from(by) + i64::from(bh));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(Rect {
        x: i32::try_from(x0).ok()?,
        y: i32::try_from(y0).ok()?,
        w: u32::try_from(x1 - x0).ok()?,
        h: u32::try_from(y1 - y0).ok()?,
    })
}

/// Intersection of two rects, or `None` if disjoint or zero-size.
pub(crate) fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    clip_to_bounds(a, b.x, b.y, b.w, b.h)
}

/// Translate `local` by `(bounds.x, bounds.y)` and clip it to `bounds` in 64-bit arithmetic.
pub(crate) fn offset_and_clip(bounds: Rect, local: Rect) -> Option<Rect> {
    let x0 = i64::from(bounds.x) + i64::from(local.x);
    let y0 = i64::from(bounds.y) + i64::from(local.y);
    let x1 = x0 + i64::from(local.w);
    let y1 = y0 + i64::from(local.h);
    let cx0 = x0.max(i64::from(bounds.x));
    let cy0 = y0.max(i64::from(bounds.y));
    let cx1 = x1.min(i64::from(bounds.x) + i64::from(bounds.w));
    let cy1 = y1.min(i64::from(bounds.y) + i64::from(bounds.h));
    if cx0 >= cx1 || cy0 >= cy1 {
        return None;
    }
    Some(Rect {
        x: i32::try_from(cx0).ok()?,
        y: i32::try_from(cy0).ok()?,
        w: u32::try_from(cx1 - cx0).ok()?,
        h: u32::try_from(cy1 - cy0).ok()?,
    })
}

/// Area of `r` in pixels using 64-bit arithmetic.
pub(crate) fn area(r: Rect) -> u64 {
    u64::from(r.w) * u64::from(r.h)
}

/// Axis-aligned bounding box of `a` and `b` in screen coordinates (valid when both are screen-clipped).
fn union_bbox(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = i64::from(a.x).min(i64::from(b.x));
    let y0 = i64::from(a.y).min(i64::from(b.y));
    let x1 = (i64::from(a.x) + i64::from(a.w)).max(i64::from(b.x) + i64::from(b.w));
    let y1 = (i64::from(a.y) + i64::from(a.h)).max(i64::from(b.y) + i64::from(b.h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(Rect {
        x: i32::try_from(x0).ok()?,
        y: i32::try_from(y0).ok()?,
        w: u32::try_from(x1 - x0).ok()?,
        h: u32::try_from(y1 - y0).ok()?,
    })
}

fn touches_or_overlaps(a: Rect, b: Rect) -> bool {
    let ax0 = i64::from(a.x);
    let ay0 = i64::from(a.y);
    let ax1 = ax0 + i64::from(a.w);
    let ay1 = ay0 + i64::from(a.h);
    let bx0 = i64::from(b.x);
    let by0 = i64::from(b.y);
    let bx1 = bx0 + i64::from(b.w);
    let by1 = by0 + i64::from(b.h);
    ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1
}

fn overlaps(a: Rect, b: Rect) -> bool {
    let ax0 = i64::from(a.x);
    let ay0 = i64::from(a.y);
    let ax1 = ax0 + i64::from(a.w);
    let ay1 = ay0 + i64::from(a.h);
    let bx0 = i64::from(b.x);
    let by0 = i64::from(b.y);
    let bx1 = bx0 + i64::from(b.w);
    let by1 = by0 + i64::from(b.h);
    ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
}

/// Merge overlapping or touching candidate dirty rects when doing so adds little overhead.
pub(crate) fn merge_candidates(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.retain(|r| r.w > 0 && r.h > 0);
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < rects.len() {
            let mut j = i + 1;
            while j < rects.len() {
                let a = rects[i];
                let b = rects[j];
                if touches_or_overlaps(a, b) {
                    if let Some(u) = union_bbox(a, b) {
                        if area(u) <= area(a).saturating_add(area(b)).saturating_add(1024) {
                            rects[i] = u;
                            rects.swap_remove(j);
                            changed = true;
                            continue;
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }
    rects
}

/// Compare `tile` against `screen` over `origin`, copy changed pixels into `screen`,
/// and return disjoint `TrackedRect`s covering all changed pixels.
pub(crate) fn extract_changed_rects(
    tile: &Canvas,
    screen: &mut Canvas,
    origin: Rect,
) -> Vec<TrackedRect> {
    let mut completed: Vec<TrackedRect> = Vec::new();
    let mut active: Vec<TrackedRect> = Vec::new();

    let tw = tile.width as usize;
    let sw = screen.width as usize;
    let ox = origin.x.max(0) as usize;
    let oy = origin.y.max(0) as usize;
    let ow = origin.w as usize;
    let oh = origin.h as usize;

    for ly in 0..oh {
        let sy = oy.saturating_add(ly);
        let tile_row_start = ly.saturating_mul(tw);
        let screen_row_start = sy.saturating_mul(sw).saturating_add(ox);

        let mut row_runs: Vec<(i32, u32, u64)> = Vec::new();
        let mut run_start: Option<(usize, usize, u64)> = None;

        for lx in 0..ow {
            let ti = tile_row_start.saturating_add(lx);
            let si = screen_row_start.saturating_add(lx);
            let (Some(&tp), Some(sp)) = (tile.pixels.get(ti), screen.pixels.get_mut(si)) else {
                continue;
            };
            if tp == *sp {
                continue;
            }
            *sp = tp;

            match run_start {
                None => {
                    run_start = Some((lx, lx + 1, 1));
                }
                Some((rx0, _rx1, rc)) => {
                    let new_len = (lx + 1 - rx0) as u64;
                    let new_rc = rc + 1;
                    // Only bridge small unchanged gaps on a row when density stays >= 50%.
                    if new_len <= 2 * new_rc {
                        run_start = Some((rx0, lx + 1, new_rc));
                    } else {
                        row_runs.push((origin.x + rx0 as i32, (_rx1 - rx0) as u32, rc));
                        run_start = Some((lx, lx + 1, 1));
                    }
                }
            }
        }
        if let Some((rx0, rx1, rc)) = run_start {
            row_runs.push((origin.x + rx0 as i32, (rx1 - rx0) as u32, rc));
        }

        let cur_y = origin.y + ly as i32;
        let mut next_active: Vec<TrackedRect> = Vec::with_capacity(row_runs.len());
        let mut used = vec![false; active.len()];
        for (rx, rw, rc) in row_runs {
            let mut matched = false;
            for (idx, act) in active.iter().enumerate() {
                if !used[idx]
                    && act.rect.x == rx
                    && act.rect.w == rw
                    && act.rect.y + act.rect.h as i32 == cur_y
                {
                    used[idx] = true;
                    next_active.push(TrackedRect {
                        rect: Rect {
                            x: act.rect.x,
                            y: act.rect.y,
                            w: act.rect.w,
                            h: act.rect.h.saturating_add(1),
                        },
                        changed: act.changed.saturating_add(rc),
                    });
                    matched = true;
                    break;
                }
            }
            if !matched {
                next_active.push(TrackedRect {
                    rect: Rect {
                        x: rx,
                        y: cur_y,
                        w: rw,
                        h: 1,
                    },
                    changed: rc,
                });
            }
        }
        for (idx, act) in active.into_iter().enumerate() {
            if !used[idx] {
                completed.push(act);
            }
        }
        active = next_active;
    }

    completed.extend(active);
    completed
}

/// Coalesce disjoint `TrackedRect`s while preserving disjointness and
/// `area <= 2 * changed + 2048` per returned rect.
pub(crate) fn finalize_damage(mut tracked: Vec<TrackedRect>) -> Vec<Rect> {
    tracked.retain(|t| t.rect.w > 0 && t.rect.h > 0 && t.changed > 0);
    if tracked.is_empty() {
        return Vec::new();
    }

    tracked.sort_by_key(|t| (t.rect.y, t.rect.x));

    // Pass 1: exact zero-waste merges between touching same-span rects.
    let mut did_merge = true;
    while did_merge {
        did_merge = false;
        let mut i = 0;
        while i < tracked.len() {
            let mut j = i + 1;
            let limit = (i + 32).min(tracked.len());
            while j < limit && j < tracked.len() {
                let a = tracked[i];
                let b = tracked[j];
                let exact_v = a.rect.x == b.rect.x
                    && a.rect.w == b.rect.w
                    && (i64::from(a.rect.y) + i64::from(a.rect.h) == i64::from(b.rect.y)
                        || i64::from(b.rect.y) + i64::from(b.rect.h) == i64::from(a.rect.y));
                let exact_h = a.rect.y == b.rect.y
                    && a.rect.h == b.rect.h
                    && (i64::from(a.rect.x) + i64::from(a.rect.w) == i64::from(b.rect.x)
                        || i64::from(b.rect.x) + i64::from(b.rect.w) == i64::from(a.rect.x));
                if exact_v || exact_h {
                    if let Some(u) = union_bbox(a.rect, b.rect) {
                        tracked[i] = TrackedRect {
                            rect: u,
                            changed: a.changed.saturating_add(b.changed),
                        };
                        tracked.remove(j);
                        did_merge = true;
                        continue;
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }

    // Pass 2: bounded coalescing within the 2x + 2048 per-rect budget while keeping disjointness.
    did_merge = true;
    while did_merge {
        did_merge = false;
        let mut i = 0;
        while i < tracked.len() {
            let mut merged_i = false;
            let limit = (i + 24).min(tracked.len());
            let mut j = i + 1;
            while j < limit && j < tracked.len() {
                let Some(mut u) = union_bbox(tracked[i].rect, tracked[j].rect) else {
                    j += 1;
                    continue;
                };
                // Expand `u` to a fixpoint so every overlapping rect is fully contained in `u`.
                loop {
                    let mut grew = false;
                    for t in &tracked {
                        if i64::from(t.rect.y) >= i64::from(u.y) + i64::from(u.h) {
                            break;
                        }
                        if overlaps(u, t.rect) {
                            if let Some(expanded) = union_bbox(u, t.rect) {
                                if expanded != u {
                                    u = expanded;
                                    grew = true;
                                }
                            }
                        }
                    }
                    if !grew {
                        break;
                    }
                }
                let mut sum_changed = 0u64;
                let mut absorbed: Vec<usize> = Vec::new();
                for (k, t) in tracked.iter().enumerate() {
                    if i64::from(t.rect.y) >= i64::from(u.y) + i64::from(u.h) {
                        break;
                    }
                    if overlaps(u, t.rect) {
                        sum_changed = sum_changed.saturating_add(t.changed);
                        absorbed.push(k);
                    }
                }
                if absorbed.len() >= 2
                    && area(u) <= sum_changed.saturating_mul(2).saturating_add(2048)
                {
                    // Replace all absorbed rects with `u`, keeping `tracked` sorted & disjoint.
                    for &idx in absorbed.iter().rev() {
                        tracked.remove(idx);
                    }
                    let insert_pos = tracked
                        .binary_search_by_key(&(u.y, u.x), |t| (t.rect.y, t.rect.x))
                        .unwrap_or_else(|p| p);
                    tracked.insert(
                        insert_pos,
                        TrackedRect {
                            rect: u,
                            changed: sum_changed,
                        },
                    );
                    did_merge = true;
                    merged_i = true;
                    break;
                }
                j += 1;
            }
            if !merged_i {
                i += 1;
            }
        }
    }

    tracked.into_iter().map(|t| t.rect).collect()
}
