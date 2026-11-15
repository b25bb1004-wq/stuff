//! Multi-cursor editor with a branching undo tree.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::buffer::Buffer;
use crate::cursor::{normalize_cursors, Cursor};

#[derive(Clone, Debug)]
struct SingleEdit {
    pre_offset: usize,
    post_offset: usize,
    deleted: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct UndoNode {
    parent: Option<usize>,
    children: Vec<usize>,
    edits: Vec<SingleEdit>,
    cursors_before: Vec<Cursor>,
    cursors_after: Vec<Cursor>,
}

/// Undo tree: every edit is a node; undo after a new edit keeps the old branch reachable.
#[derive(Clone, Debug)]
pub struct Editor {
    /// Underlying piece-table text buffer.
    pub buffer: Buffer,
    cursors: Vec<Cursor>,
    nodes: Vec<UndoNode>,
    current: usize,
}

impl Editor {
    /// Create a new editor with `text` and a single empty cursor at byte 0.
    pub fn new(text: &str) -> Editor {
        let initial_cursors = vec![Cursor { anchor: 0, head: 0 }];
        let root = UndoNode {
            parent: None,
            children: Vec::new(),
            edits: Vec::new(),
            cursors_before: Vec::new(),
            cursors_after: initial_cursors.clone(),
        };
        Editor {
            buffer: Buffer::new(text),
            cursors: initial_cursors,
            nodes: vec![root],
            current: 0,
        }
    }

    /// Current cursors, sorted by selection start and never overlapping.
    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    /// Replace the cursor list, sorting and merging overlapping or touching selections.
    pub fn set_cursors(&mut self, c: Vec<Cursor>) {
        self.cursors = normalize_cursors(&self.buffer, c);
    }

    /// Replace every selection (or insert at every empty cursor) with `s`, as ONE undo step;
    /// afterwards each cursor is empty and sits just after its inserted text.
    pub fn type_text(&mut self, s: &str) {
        let cursors_before = normalize_cursors(&self.buffer, core::mem::take(&mut self.cursors));
        self.cursors = cursors_before.clone();
        if self.cursors.is_empty() {
            return;
        }
        let any_selection = self.cursors.iter().any(|c| c.start() < c.end());
        if s.is_empty() && !any_selection {
            return;
        }

        let mut edits = Vec::with_capacity(self.cursors.len());
        let mut raw_after = Vec::with_capacity(self.cursors.len());
        let mut shift: isize = 0;

        for &c in &self.cursors {
            let start = c.start();
            let end = c.end();
            let deleted = self.buffer.slice(start, end);
            let post_offset = offset_with_shift(start, shift);
            edits.push(SingleEdit {
                pre_offset: start,
                post_offset,
                deleted,
                inserted: String::from(s),
            });
            let pos = post_offset.saturating_add(s.len());
            raw_after.push(Cursor {
                anchor: pos,
                head: pos,
            });
            shift += s.len() as isize - (end - start) as isize;
        }

        apply_forward(&mut self.buffer, &edits);
        let cursors_after = normalize_cursors(&self.buffer, raw_after);
        self.push_node(edits, cursors_before, cursors_after);
    }

    /// Delete each selection, or for an empty cursor the char before it (backspace), as ONE undo step.
    pub fn backspace(&mut self) {
        let cursors_before = normalize_cursors(&self.buffer, core::mem::take(&mut self.cursors));
        self.cursors = cursors_before.clone();
        if self.cursors.is_empty() {
            return;
        }

        let mut edits = Vec::new();
        let mut raw_after = Vec::with_capacity(self.cursors.len());
        let mut shift: isize = 0;

        for &c in &self.cursors {
            let start = c.start();
            let end = c.end();
            let (del_start, del_end) = if start < end {
                (start, end)
            } else if start > 0 {
                let prev_len = self.buffer.prev_char_len(start);
                (start.saturating_sub(prev_len), start)
            } else {
                (0, 0)
            };

            let post_offset = offset_with_shift(del_start, shift);
            if del_start < del_end {
                let deleted = self.buffer.slice(del_start, del_end);
                edits.push(SingleEdit {
                    pre_offset: del_start,
                    post_offset,
                    deleted,
                    inserted: String::new(),
                });
                shift -= (del_end - del_start) as isize;
            }
            raw_after.push(Cursor {
                anchor: post_offset,
                head: post_offset,
            });
        }

        if edits.is_empty() {
            return;
        }

        apply_forward(&mut self.buffer, &edits);
        let cursors_after = normalize_cursors(&self.buffer, raw_after);
        self.push_node(edits, cursors_before, cursors_after);
    }

    /// Move back one step in the undo tree; returns `false` at the root.
    pub fn undo(&mut self) -> bool {
        let Some(node) = self.nodes.get(self.current) else {
            return false;
        };
        let Some(parent) = node.parent else {
            return false;
        };
        let edits = node.edits.clone();
        let cursors_before = node.cursors_before.clone();
        apply_backward(&mut self.buffer, &edits);
        self.cursors = cursors_before;
        self.current = parent;
        true
    }

    /// Move forward down the most recent child branch; returns `false` if there are no children.
    pub fn redo(&mut self) -> bool {
        let len = self.branches();
        if len == 0 {
            return false;
        }
        self.redo_branch(len - 1)
    }

    /// Number of child branches from the current node in the undo tree.
    pub fn branches(&self) -> usize {
        self.nodes.get(self.current).map_or(0, |n| n.children.len())
    }

    /// Move forward down child branch `i` (`0` = oldest); returns `false` if `i` is out of range.
    pub fn redo_branch(&mut self, i: usize) -> bool {
        let Some(&child_id) = self.nodes.get(self.current).and_then(|n| n.children.get(i)) else {
            return false;
        };
        let Some(child) = self.nodes.get(child_id) else {
            return false;
        };
        let edits = child.edits.clone();
        let cursors_after = child.cursors_after.clone();
        apply_forward(&mut self.buffer, &edits);
        self.cursors = cursors_after;
        self.current = child_id;
        true
    }

    pub(crate) fn current_node(&self) -> usize {
        self.current
    }

    pub(crate) fn replace_range(&mut self, start: usize, end: usize, inserted: &str) -> bool {
        let deleted = self.buffer.slice(start, end);
        if deleted == inserted {
            return false;
        }
        let edits = vec![SingleEdit {
            pre_offset: start,
            post_offset: start,
            deleted,
            inserted: String::from(inserted),
        }];
        let cursors = vec![Cursor {
            anchor: start,
            head: start,
        }];
        apply_forward(&mut self.buffer, &edits);
        self.push_node(edits, cursors.clone(), cursors);
        true
    }

    fn push_node(
        &mut self,
        edits: Vec<SingleEdit>,
        cursors_before: Vec<Cursor>,
        cursors_after: Vec<Cursor>,
    ) {
        let child_id = self.nodes.len();
        self.nodes.push(UndoNode {
            parent: Some(self.current),
            children: Vec::new(),
            edits,
            cursors_before,
            cursors_after: cursors_after.clone(),
        });
        if let Some(cur) = self.nodes.get_mut(self.current) {
            cur.children.push(child_id);
        }
        self.current = child_id;
        self.cursors = cursors_after;
    }
}

fn offset_with_shift(offset: usize, shift: isize) -> usize {
    if shift >= 0 {
        offset.saturating_add(shift as usize)
    } else {
        offset.saturating_sub(shift.unsigned_abs())
    }
}

fn apply_forward(buf: &mut Buffer, edits: &[SingleEdit]) {
    for e in edits.iter().rev() {
        if !e.deleted.is_empty() {
            buf.delete(e.pre_offset, e.pre_offset + e.deleted.len());
        }
        if !e.inserted.is_empty() {
            buf.insert(e.pre_offset, &e.inserted);
        }
    }
}

fn apply_backward(buf: &mut Buffer, edits: &[SingleEdit]) {
    for e in edits.iter().rev() {
        if !e.inserted.is_empty() {
            buf.delete(e.post_offset, e.post_offset + e.inserted.len());
        }
        if !e.deleted.is_empty() {
            buf.insert(e.post_offset, &e.deleted);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn undo_tree_branches() {
        let mut ed = Editor::new("start");
        ed.set_cursors(vec![Cursor { anchor: 5, head: 5 }]);
        ed.type_text(" A");
        assert_eq!(ed.buffer.text(), "start A");

        assert!(ed.undo());
        assert_eq!(ed.buffer.text(), "start");

        ed.type_text(" B");
        assert_eq!(ed.buffer.text(), "start B");

        assert!(ed.undo());
        assert_eq!(ed.branches(), 2);

        assert!(ed.redo_branch(0));
        assert_eq!(ed.buffer.text(), "start A");

        assert!(ed.undo());
        assert!(ed.redo());
        assert_eq!(ed.buffer.text(), "start B");
    }
}
