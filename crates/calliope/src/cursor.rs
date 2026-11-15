//! Cursor representation and selection normalization/merging.

use alloc::vec::Vec;

use crate::buffer::Buffer;

/// A selection or caret inside a [`Buffer`]: selection spans `min..max`, empty when equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Byte offset where the selection started.
    pub anchor: usize,
    /// Byte offset where the caret currently sits.
    pub head: usize,
}

impl Cursor {
    /// Start byte offset of the selection (`min(anchor, head)`).
    pub(crate) fn start(self) -> usize {
        self.anchor.min(self.head)
    }

    /// End byte offset of the selection (`max(anchor, head)`).
    pub(crate) fn end(self) -> usize {
        self.anchor.max(self.head)
    }
}

/// Clamp `pos` to `buf.len()` and snap backward to a valid UTF-8 char boundary.
fn clamp_to_boundary(buf: &Buffer, pos: usize) -> usize {
    let mut p = pos.min(buf.len());
    while p > 0 && !buf.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Sort cursors by selection start and merge overlapping or touching selections.
pub(crate) fn normalize_cursors(buf: &Buffer, cursors: Vec<Cursor>) -> Vec<Cursor> {
    let mut clamped: Vec<Cursor> = cursors
        .into_iter()
        .map(|c| Cursor {
            anchor: clamp_to_boundary(buf, c.anchor),
            head: clamp_to_boundary(buf, c.head),
        })
        .collect();
    clamped.sort_by_key(|c| (c.start(), c.end()));

    let mut out: Vec<Cursor> = Vec::with_capacity(clamped.len());
    for c in clamped {
        if let Some(prev) = out.last_mut() {
            let p_start = prev.start();
            let p_end = prev.end();
            let c_start = c.start();
            let c_end = c.end();
            if p_end >= c_start {
                let merged_end = p_end.max(c_end);
                *prev = Cursor {
                    anchor: p_start,
                    head: merged_end,
                };
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use alloc::vec;

    #[test]
    fn merges_overlapping_and_touching() {
        let buf = Buffer::new("abcdefghij");
        let res = normalize_cursors(
            &buf,
            vec![
                Cursor { anchor: 5, head: 2 },
                Cursor { anchor: 5, head: 7 },
                Cursor { anchor: 8, head: 8 },
                Cursor { anchor: 8, head: 8 },
            ],
        );
        assert_eq!(
            res,
            vec![Cursor { anchor: 2, head: 7 }, Cursor { anchor: 8, head: 8 },]
        );
    }
}
