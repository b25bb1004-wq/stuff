//! Myers diff algorithm and unified diff formatting.

use alloc::{collections::BTreeSet, format, vec, vec::Vec};

/// Single edit step transforming slice `a` into slice `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Element `a[0]` equals `b[1]`.
    Keep(usize, usize),
    /// Delete element `a[0]`.
    Delete(usize),
    /// Insert element `b[0]`.
    Insert(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffHunk {
    pub base_start: usize,
    pub base_end: usize,
    pub side_start: usize,
    pub side_end: usize,
}

/// Minimal edit script (Myers O(ND), linear-space refinement) turning `a` into `b`.
/// Deletes come before inserts inside each changed region.
pub fn diff<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Edit> {
    let (mut rchg_a, mut rchg_b) = (vec![true; a.len()], vec![true; b.len()]);
    myers_rec(a, b, 0, 0, &mut rchg_a, &mut rchg_b);
    compact_side(a, &mut rchg_a, &rchg_b);
    compact_side(b, &mut rchg_b, &rchg_a);
    edits_from_rchg(&rchg_a, &rchg_b)
}

/// Lines split on `'\n'` keeping the terminator; a last line without `'\n'` is a line too.
pub fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|&b| b == b'\n').collect()
}

/// `git diff -U<context>` style hunks for two texts, with `"--- a/<path>"` / `"+++ b/<path>"`
/// headers and `"\ No newline at end of file"` markers. Empty output when equal.
pub fn unified(path: &str, a: &[u8], b: &[u8], context: usize) -> Vec<u8> {
    let parts = hunks(a, b, context);
    if parts.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend_from_slice(format!("--- a/{path}\n+++ b/{path}\n").as_bytes());
    for part in &parts {
        write_hunk_header(
            &mut out,
            part.old_start,
            part.old_lines,
            part.new_start,
            part.new_lines,
        );
        for (prefix, line) in &part.lines {
            write_diff_line(&mut out, *prefix, line);
        }
    }
    out
}

/// One hunk of a unified diff. The starts are numbered as the header prints them (1-based, or the
/// line before the hunk when the count is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// First old line in the header.
    pub old_start: usize,
    /// Old line count in the header.
    pub old_lines: usize,
    /// First new line in the header.
    pub new_start: usize,
    /// New line count in the header.
    pub new_lines: usize,
    /// Body lines: `b'-'`, `b'+'` or `b' '`, and the line text with its newline when it has one.
    pub lines: Vec<(u8, Vec<u8>)>,
}

/// The hunks `unified` prints for two texts, with `context` lines around each change. Empty when equal.
pub fn hunks(a: &[u8], b: &[u8], context: usize) -> Vec<Hunk> {
    if a == b {
        return Vec::new();
    }
    let a_lines = lines(a);
    let b_lines = lines(b);
    let changes = diff_hunks(&a_lines, &b_lines);

    let mut out = Vec::new();
    let mut idx = 0;
    while idx < changes.len() {
        let first = changes[idx];
        let mut last_idx = idx;
        while last_idx + 1 < changes.len() {
            let gap = changes[last_idx + 1].base_start - changes[last_idx].base_end;
            if gap <= context.saturating_mul(2) {
                last_idx += 1;
            } else {
                break;
            }
        }
        let last = changes[last_idx];

        let lead = first.base_start.min(context);
        let tail = (a_lines.len() - last.base_end).min(context);
        let a_start = first.base_start - lead;
        let b_start = first.side_start - lead;
        let a_end = last.base_end + tail;
        let b_end = last.side_end + tail;
        let a_count = a_end - a_start;
        let b_count = b_end - b_start;

        let mut body = Vec::new();
        let mut a_pos = a_start;
        for h in &changes[idx..=last_idx] {
            while a_pos < h.base_start {
                body.push((b' ', a_lines[a_pos].to_vec()));
                a_pos += 1;
            }
            for line in &a_lines[h.base_start..h.base_end] {
                body.push((b'-', line.to_vec()));
            }
            for line in &b_lines[h.side_start..h.side_end] {
                body.push((b'+', line.to_vec()));
            }
            a_pos = h.base_end;
        }
        while a_pos < a_end {
            body.push((b' ', a_lines[a_pos].to_vec()));
            a_pos += 1;
        }
        out.push(Hunk {
            old_start: header_start(a_start, a_count),
            old_lines: a_count,
            new_start: header_start(b_start, b_count),
            new_lines: b_count,
            lines: body,
        });
        idx = last_idx + 1;
    }
    out
}

/// The number a header prints for a range that starts at 0-based `start` with `count` lines.
fn header_start(start: usize, count: usize) -> usize {
    if count == 0 {
        start
    } else {
        start + 1
    }
}

pub(crate) fn diff_hunks(a: &[&[u8]], b: &[&[u8]]) -> Vec<DiffHunk> {
    let mut rchg_a = vec![true; a.len()];
    let mut rchg_b = vec![true; b.len()];
    let b_set: BTreeSet<&[u8]> = b.iter().copied().collect();
    let a_set: BTreeSet<&[u8]> = a.iter().copied().collect();
    let a_idx: Vec<usize> = a
        .iter()
        .enumerate()
        .filter(|(_, line)| b_set.contains(*line))
        .map(|(i, _)| i)
        .collect();
    let b_idx: Vec<usize> = b
        .iter()
        .enumerate()
        .filter(|(_, line)| a_set.contains(*line))
        .map(|(j, _)| j)
        .collect();
    let a_clean: Vec<&[u8]> = a_idx.iter().map(|&i| a[i]).collect();
    let b_clean: Vec<&[u8]> = b_idx.iter().map(|&j| b[j]).collect();
    let mut c_rchg_a = vec![true; a_clean.len()];
    let mut c_rchg_b = vec![true; b_clean.len()];
    myers_rec(&a_clean, &b_clean, 0, 0, &mut c_rchg_a, &mut c_rchg_b);
    for (ci, &orig_i) in a_idx.iter().enumerate() {
        rchg_a[orig_i] = c_rchg_a[ci];
    }
    for (cj, &orig_j) in b_idx.iter().enumerate() {
        rchg_b[orig_j] = c_rchg_b[cj];
    }
    compact_side(a, &mut rchg_a, &rchg_b);
    compact_side(b, &mut rchg_b, &rchg_a);
    hunks_from_rchg(&rchg_a, &rchg_b)
}

fn edits_from_rchg(rchg_a: &[bool], rchg_b: &[bool]) -> Vec<Edit> {
    let mut i = 0;
    let mut j = 0;
    let mut edits = Vec::with_capacity(rchg_a.len() + rchg_b.len());
    while i < rchg_a.len() || j < rchg_b.len() {
        if i < rchg_a.len() && !rchg_a[i] && j < rchg_b.len() && !rchg_b[j] {
            edits.push(Edit::Keep(i, j));
            i += 1;
            j += 1;
        } else {
            while i < rchg_a.len() && rchg_a[i] {
                edits.push(Edit::Delete(i));
                i += 1;
            }
            while j < rchg_b.len() && rchg_b[j] {
                edits.push(Edit::Insert(j));
                j += 1;
            }
        }
    }
    edits
}

fn hunks_from_rchg(rchg_a: &[bool], rchg_b: &[bool]) -> Vec<DiffHunk> {
    let mut i = 0;
    let mut j = 0;
    let mut hunks = Vec::new();
    while i < rchg_a.len() || j < rchg_b.len() {
        if i < rchg_a.len() && !rchg_a[i] && j < rchg_b.len() && !rchg_b[j] {
            i += 1;
            j += 1;
        } else {
            let base_start = i;
            let side_start = j;
            while i < rchg_a.len() && rchg_a[i] {
                i += 1;
            }
            while j < rchg_b.len() && rchg_b[j] {
                j += 1;
            }
            hunks.push(DiffHunk {
                base_start,
                base_end: i,
                side_start,
                side_end: j,
            });
        }
    }
    hunks
}

fn write_hunk_header(
    out: &mut Vec<u8>,
    a_start: usize,
    a_count: usize,
    b_start: usize,
    b_count: usize,
) {
    out.extend_from_slice(b"@@ -");
    write_range(out, a_start, a_count);
    out.extend_from_slice(b" +");
    write_range(out, b_start, b_count);
    out.extend_from_slice(b" @@\n");
}

/// `start` is already the printed number (see `header_start`).
fn write_range(out: &mut Vec<u8>, start: usize, count: usize) {
    if count == 1 {
        out.extend_from_slice(format!("{start}").as_bytes());
    } else {
        out.extend_from_slice(format!("{start},{count}").as_bytes());
    }
}

fn write_diff_line(out: &mut Vec<u8>, prefix: u8, line: &[u8]) {
    out.push(prefix);
    out.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        out.extend_from_slice(b"\n\\ No newline at end of file\n");
    }
}

fn compact_side<T: PartialEq>(a: &[T], rchg: &mut [bool], rchgo: &[bool]) {
    let n = a.len();
    let m = rchgo.len();
    let mut ix = 0usize;
    let mut ixo = 0usize;
    loop {
        while ix < n && !rchg[ix] {
            while ixo < m && rchgo[ixo] {
                ixo += 1;
            }
            ixo += 1;
            ix += 1;
        }
        if ix == n {
            break;
        }
        let mut ixs = ix;
        let mut i = ix;
        while i < n && rchg[i] {
            i += 1;
        }
        let mut has_back;
        let mut i_back = 0;
        loop {
            let grpsize = i - ixs;
            has_back = false;
            while ixs > 0 && a[ixs - 1] == a[i - 1] {
                ixs -= 1;
                i -= 1;
                rchg[ixs] = true;
                rchg[i] = false;
                while ixs > 0 && rchg[ixs - 1] {
                    ixs -= 1;
                }
                ixo -= 1;
                while ixo > 0 && rchgo[ixo - 1] {
                    ixo -= 1;
                }
            }
            if ixo < m && rchgo[ixo] {
                has_back = true;
                i_back = i;
            }
            while i < n && a[ixs] == a[i] {
                rchg[ixs] = false;
                rchg[i] = true;
                ixs += 1;
                i += 1;
                while i < n && rchg[i] {
                    i += 1;
                }
                while ixo < m && rchgo[ixo] {
                    ixo += 1;
                }
                ixo += 1;
                if ixo < m && rchgo[ixo] {
                    has_back = true;
                    i_back = i;
                }
            }
            if grpsize == i - ixs {
                break;
            }
        }
        if has_back {
            while i > i_back {
                ixs -= 1;
                i -= 1;
                rchg[ixs] = true;
                rchg[i] = false;
                ixo -= 1;
                while ixo > 0 && rchgo[ixo - 1] {
                    ixo -= 1;
                }
            }
        }
        ix = i;
    }
}

fn myers_rec<T: PartialEq>(
    a: &[T],
    b: &[T],
    a_off: usize,
    b_off: usize,
    rchg_a: &mut [bool],
    rchg_b: &mut [bool],
) {
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        rchg_a[a_off + prefix] = false;
        rchg_b[b_off + prefix] = false;
        prefix += 1;
    }
    let a_rem = &a[prefix..];
    let b_rem = &b[prefix..];
    let mut suffix = 0;
    while suffix < a_rem.len()
        && suffix < b_rem.len()
        && a_rem[a_rem.len() - 1 - suffix] == b_rem[b_rem.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let a_suf_off = a_off + a.len() - suffix;
    let b_suf_off = b_off + b.len() - suffix;
    for k in 0..suffix {
        rchg_a[a_suf_off + k] = false;
        rchg_b[b_suf_off + k] = false;
    }

    let a_mid = &a_rem[..a_rem.len() - suffix];
    let b_mid = &b_rem[..b_rem.len() - suffix];
    if a_mid.is_empty() || b_mid.is_empty() {
        return;
    }
    let a_mid_off = a_off + prefix;
    let b_mid_off = b_off + prefix;
    let (sx, sy) = find_middle_snake(a_mid, b_mid);
    myers_rec(
        &a_mid[..sx],
        &b_mid[..sy],
        a_mid_off,
        b_mid_off,
        rchg_a,
        rchg_b,
    );
    myers_rec(
        &a_mid[sx..],
        &b_mid[sy..],
        a_mid_off + sx,
        b_mid_off + sy,
        rchg_a,
        rchg_b,
    );
}

fn find_middle_snake<T: PartialEq>(a: &[T], b: &[T]) -> (usize, usize) {
    let n = a.len();
    let m = b.len();
    let dmin = -(m as isize);
    let dmax = n as isize;
    let delta = dmax + dmin;
    let odd = (delta & 1) != 0;
    let off = m as isize + 1;
    let v_len = n + m + 3;
    let mut kvdf = vec![-1isize; v_len];
    let mut kvdb = vec![isize::MAX; v_len];
    let mut fmin = 0isize;
    let mut fmax = 0isize;
    let mut bmin = delta;
    let mut bmax = delta;
    kvdf[off as usize] = 0;
    kvdb[(delta + off) as usize] = n as isize;

    for _d in 1..=(n + m).div_ceil(2) + 1 {
        if fmin > dmin {
            fmin -= 1;
            kvdf[(fmin - 1 + off) as usize] = -1;
        } else {
            fmin += 1;
        }
        if fmax < dmax {
            fmax += 1;
            kvdf[(fmax + 1 + off) as usize] = -1;
        } else {
            fmax -= 1;
        }
        let mut k = fmax;
        while k >= fmin {
            let idx = (k + off) as usize;
            let left = kvdf[idx - 1];
            let up = kvdf[idx + 1];
            let rx = left + 1;
            let right_ok = left >= 0 && rx <= dmax && (rx - k) >= 0 && (rx - k) <= m as isize;
            let down_ok = up >= 0 && up <= dmax && (up - k) >= 0 && (up - k) <= m as isize;
            let x_start = if right_ok && (!down_ok || left >= up) {
                rx
            } else if down_ok {
                up
            } else {
                -1
            };
            if x_start >= 0 {
                let mut x = x_start as usize;
                let mut y = (x_start - k) as usize;
                while x < n && y < m && a[x] == b[y] {
                    x += 1;
                    y += 1;
                }
                kvdf[idx] = x as isize;
                if odd && bmin <= k && k <= bmax && kvdb[idx] <= dmax && (x as isize) >= kvdb[idx] {
                    return (x, y);
                }
            } else {
                kvdf[idx] = -1;
            }
            k -= 2;
        }

        if bmin > dmin {
            bmin -= 1;
            kvdb[(bmin - 1 + off) as usize] = isize::MAX;
        } else {
            bmin += 1;
        }
        if bmax < dmax {
            bmax += 1;
            kvdb[(bmax + 1 + off) as usize] = isize::MAX;
        } else {
            bmax -= 1;
        }
        let mut k = bmax;
        while k >= bmin {
            let idx = (k + off) as usize;
            let left = kvdb[idx - 1];
            let up = kvdb[idx + 1];
            let down_ok = (0..=dmax).contains(&left) && (0..=m as isize).contains(&(left - k));
            let rx = up.saturating_sub(1);
            let right_ok =
                (1..isize::MAX).contains(&up) && rx <= dmax && (0..=m as isize).contains(&(rx - k));
            let x_start = if down_ok && (!right_ok || left < up) {
                left
            } else if right_ok {
                rx
            } else {
                isize::MAX
            };
            if x_start < isize::MAX {
                let mut x = x_start as usize;
                let mut y = (x_start - k) as usize;
                while x > 0 && y > 0 && a[x - 1] == b[y - 1] {
                    x -= 1;
                    y -= 1;
                }
                kvdb[idx] = x as isize;
                if !odd && fmin <= k && k <= fmax && kvdf[idx] >= 0 && kvdf[idx] >= (x as isize) {
                    return (x, y);
                }
            } else {
                kvdb[idx] = isize::MAX;
            }
            k -= 2;
        }
    }

    (n, 0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn lines_splitting_cases() {
        assert!(lines(b"").is_empty());
        assert_eq!(lines(b"a\n"), vec![b"a\n".as_slice()]);
        assert_eq!(lines(b"a\nb"), vec![b"a\n".as_slice(), b"b".as_slice()]);
        assert_eq!(lines(b"\n\n"), vec![b"\n".as_slice(), b"\n".as_slice()]);
    }

    #[test]
    fn deletes_before_inserts_in_changed_region() {
        let edits = diff(b"abc", b"axyc");
        assert_eq!(
            edits,
            vec![
                Edit::Keep(0, 0),
                Edit::Delete(1),
                Edit::Insert(1),
                Edit::Insert(2),
                Edit::Keep(2, 3),
            ]
        );
    }
}
