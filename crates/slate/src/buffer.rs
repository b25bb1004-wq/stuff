//! Piece-table text buffer over an immutable original and an append-only add string.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Original,
    Add,
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    source: Source,
    start: usize,
    len: usize,
    newlines: usize,
}

/// Text as a piece table over an immutable original and an append-only add buffer.
/// All positions are byte offsets into the UTF-8 text and must lie on char boundaries.
#[derive(Clone, Debug)]
pub struct Buffer {
    original: String,
    add: String,
    pieces: Vec<Piece>,
    total_len: usize,
    total_newlines: usize,
}

impl Buffer {
    /// Create a new buffer initialized with `text`.
    pub fn new(text: &str) -> Buffer {
        let original = String::from(text);
        let total_len = original.len();
        let total_newlines = count_newlines(original.as_bytes());
        let mut pieces = Vec::new();
        if total_len > 0 {
            pieces.push(Piece {
                source: Source::Original,
                start: 0,
                len: total_len,
                newlines: total_newlines,
            });
        }
        Buffer {
            original,
            add: String::new(),
            pieces,
            total_len,
            total_newlines,
        }
    }

    /// Total length in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// Returns `true` when the buffer contains zero bytes.
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Materialize the full buffer contents as a `String`.
    pub fn text(&self) -> String {
        self.slice(0, self.total_len)
    }

    /// Return the substring in `start..end`; panics if out of range or not on char boundaries.
    pub fn slice(&self, start: usize, end: usize) -> String {
        assert!(start <= end && end <= self.total_len, "slice out of range");
        assert!(
            self.is_char_boundary(start) && self.is_char_boundary(end),
            "slice not on char boundary"
        );
        let mut out = String::with_capacity(end - start);
        if start == end {
            return out;
        }
        let mut offset = 0usize;
        for &p in &self.pieces {
            let p_end = offset + p.len;
            if p_end <= start {
                offset = p_end;
                continue;
            }
            if offset >= end {
                break;
            }
            let lo = start.saturating_sub(offset);
            let hi = (end - offset).min(p.len);
            if let Some(s) = self.piece_str(p).get(lo..hi) {
                out.push_str(s);
            }
            offset = p_end;
        }
        out
    }

    /// Insert `s` at byte offset `at`; panics if `at` is out of range or not a char boundary.
    pub fn insert(&mut self, at: usize, s: &str) {
        assert!(at <= self.total_len, "insert out of range");
        assert!(self.is_char_boundary(at), "insert not on char boundary");
        if s.is_empty() {
            return;
        }
        let add_start = self.add.len();
        self.add.push_str(s);
        let add_len = s.len();
        let add_nl = count_newlines(s.as_bytes());
        let new_piece = Piece {
            source: Source::Add,
            start: add_start,
            len: add_len,
            newlines: add_nl,
        };

        let mut offset = 0usize;
        let mut inserted = false;
        for idx in 0..self.pieces.len() {
            let p = self.pieces[idx];
            if at == offset {
                if idx > 0 {
                    let prev = &mut self.pieces[idx - 1];
                    if prev.source == Source::Add && prev.start + prev.len == add_start {
                        prev.len += add_len;
                        prev.newlines += add_nl;
                        inserted = true;
                        break;
                    }
                }
                self.pieces.insert(idx, new_piece);
                inserted = true;
                break;
            }
            if at < offset + p.len {
                let local = at - offset;
                let left_nl = count_newlines(&self.piece_bytes(p)[..local]);
                let left = Piece {
                    source: p.source,
                    start: p.start,
                    len: local,
                    newlines: left_nl,
                };
                let right = Piece {
                    source: p.source,
                    start: p.start + local,
                    len: p.len - local,
                    newlines: p.newlines - left_nl,
                };
                self.pieces[idx] = left;
                self.pieces.insert(idx + 1, new_piece);
                self.pieces.insert(idx + 2, right);
                inserted = true;
                break;
            }
            offset += p.len;
        }

        if !inserted {
            if let Some(last) = self.pieces.last_mut() {
                if last.source == Source::Add && last.start + last.len == add_start {
                    last.len += add_len;
                    last.newlines += add_nl;
                    inserted = true;
                }
            }
            if !inserted {
                self.pieces.push(new_piece);
            }
        }

        self.total_len += add_len;
        self.total_newlines += add_nl;
    }

    /// Delete byte range `start..end`; panics if out of range or not on char boundaries.
    pub fn delete(&mut self, start: usize, end: usize) {
        assert!(start <= end && end <= self.total_len, "delete out of range");
        assert!(
            self.is_char_boundary(start) && self.is_char_boundary(end),
            "delete not on char boundary"
        );
        if start == end {
            return;
        }
        let mut next: Vec<Piece> = Vec::with_capacity(self.pieces.len() + 1);
        let mut offset = 0usize;
        for &p in &self.pieces {
            let p_end = offset + p.len;
            if p_end <= start || offset >= end {
                push_merged(&mut next, p);
            } else {
                if offset < start {
                    let keep = start - offset;
                    let nl = count_newlines(&self.piece_bytes(p)[..keep]);
                    push_merged(
                        &mut next,
                        Piece {
                            source: p.source,
                            start: p.start,
                            len: keep,
                            newlines: nl,
                        },
                    );
                }
                if p_end > end {
                    let skip = end - offset;
                    let nl = count_newlines(&self.piece_bytes(p)[skip..]);
                    push_merged(
                        &mut next,
                        Piece {
                            source: p.source,
                            start: p.start + skip,
                            len: p.len - skip,
                            newlines: nl,
                        },
                    );
                }
            }
            offset = p_end;
        }
        self.pieces = next;
        self.total_len -= end - start;
        self.total_newlines = self.pieces.iter().map(|p| p.newlines).sum();
    }

    /// Number of lines in the buffer (number of `'\n'` bytes plus 1).
    pub fn line_count(&self) -> usize {
        self.total_newlines + 1
    }

    /// Byte offset where 0-based `line` starts, or `None` if `line >= self.line_count()`.
    pub fn line_start(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return Some(0);
        }
        if line > self.total_newlines {
            return None;
        }
        let mut offset = 0usize;
        let mut seen = 0usize;
        for &p in &self.pieces {
            if seen + p.newlines < line {
                seen += p.newlines;
                offset += p.len;
                continue;
            }
            let bytes = self.piece_bytes(p);
            for (idx, &b) in bytes.iter().enumerate() {
                if b == b'\n' {
                    seen += 1;
                    if seen == line {
                        return Some(offset + idx + 1);
                    }
                }
            }
            offset += p.len;
        }
        None
    }

    /// 0-based line index containing byte offset `at` (`at == len` allowed).
    pub fn line_of(&self, at: usize) -> usize {
        assert!(at <= self.total_len, "line_of out of range");
        let mut offset = 0usize;
        let mut nl = 0usize;
        for &p in &self.pieces {
            if offset + p.len <= at {
                nl += p.newlines;
                offset += p.len;
            } else {
                let local = at - offset;
                nl += count_newlines(&self.piece_bytes(p)[..local]);
                break;
            }
        }
        nl
    }

    /// Current number of pieces in the piece table.
    pub fn pieces(&self) -> usize {
        self.pieces.len()
    }

    /// Check whether `at` lies on a UTF-8 char boundary in the buffer.
    pub(crate) fn is_char_boundary(&self, at: usize) -> bool {
        if at == 0 || at == self.total_len {
            return true;
        }
        if at > self.total_len {
            return false;
        }
        let mut offset = 0usize;
        for &p in &self.pieces {
            if at == offset {
                return true;
            }
            if at < offset + p.len {
                let local = at - offset;
                return self.piece_str(p).is_char_boundary(local);
            }
            offset += p.len;
        }
        false
    }

    /// Return the byte length of the UTF-8 char ending at `at` (0 if `at == 0`).
    pub(crate) fn prev_char_len(&self, at: usize) -> usize {
        if at == 0 || at > self.total_len {
            return 0;
        }
        let mut offset = 0usize;
        for &p in &self.pieces {
            if at > offset && at <= offset + p.len {
                let local = at - offset;
                let s = &self.piece_str(p)[..local];
                return s.chars().next_back().map_or(0, char::len_utf8);
            }
            offset += p.len;
        }
        0
    }

    fn piece_str(&self, p: Piece) -> &str {
        let buf = match p.source {
            Source::Original => self.original.as_str(),
            Source::Add => self.add.as_str(),
        };
        buf.get(p.start..p.start + p.len).unwrap_or("")
    }

    fn piece_bytes(&self, p: Piece) -> &[u8] {
        self.piece_str(p).as_bytes()
    }
}

fn push_merged(pieces: &mut Vec<Piece>, p: Piece) {
    if p.len == 0 {
        return;
    }
    if let Some(last) = pieces.last_mut() {
        if last.source == p.source && last.start + last.len == p.start {
            last.len += p.len;
            last.newlines += p.newlines;
            return;
        }
    }
    pieces.push(p);
}

fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn basic_ops_and_lines() {
        let mut b = Buffer::new("hello\nworld");
        assert_eq!(b.len(), 11);
        assert!(!b.is_empty());
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line_start(0), Some(0));
        assert_eq!(b.line_start(1), Some(6));
        assert_eq!(b.line_start(2), None);
        assert_eq!(b.line_of(0), 0);
        assert_eq!(b.line_of(5), 0);
        assert_eq!(b.line_of(6), 1);
        assert_eq!(b.line_of(11), 1);

        b.insert(5, ",\n");
        assert_eq!(b.text(), "hello,\n\nworld");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line_start(1), Some(7));
        assert_eq!(b.line_start(2), Some(8));

        b.delete(5, 8);
        assert_eq!(b.text(), "helloworld");
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn typing_merges_pieces() {
        let mut b = Buffer::new("abc");
        for _ in 0..1000 {
            let len = b.len();
            b.insert(len, "x");
        }
        assert!(b.pieces() <= 3, "pieces = {}", b.pieces());
        assert_eq!(b.len(), 1003);
    }
}
