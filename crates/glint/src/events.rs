//! Hit testing, focus management, and input event dispatch for `Ui`.

use pigment::Rect;

use crate::types::{Action, Event, Key, Kind, NodeId};
use crate::Ui;

/// Return whether `rect` contains `(x, y)` with half-open bounds `[x, x + w) x [y, y + h)`.
fn rect_contains(rect: Rect, x: i32, y: i32) -> bool {
    let px = i64::from(x);
    let py = i64::from(y);
    let rx = i64::from(rect.x);
    let ry = i64::from(rect.y);
    px >= rx
        && px < rx.saturating_add(i64::from(rect.w))
        && py >= ry
        && py < ry.saturating_add(i64::from(rect.h))
}

impl Ui {
    /// Return the deepest live node containing `(x, y)`, preferring later siblings, or `None` outside root.
    pub fn hit(&self, x: i32, y: i32) -> Option<NodeId> {
        let root_rect = self.rect(0)?;
        if !rect_contains(root_rect, x, y) {
            return None;
        }

        let mut stack = vec![(0usize, false)];
        while let Some((id, children_done)) = stack.pop() {
            if children_done {
                if let Some(r) = self.rect(id) {
                    if rect_contains(r, x, y) {
                        return Some(id);
                    }
                }
            } else if let Some(node) = self.node(id) {
                stack.push((id, true));
                for &cid in &node.children {
                    stack.push((cid, false));
                }
            }
        }
        None
    }

    /// Deliver an input event and return the resulting actions in order.
    pub fn event(&mut self, ev: Event) -> Vec<Action> {
        let mut actions = Vec::new();
        match ev {
            Event::MouseMove { x, y } => {
                self.hovered = self.hit(x, y).filter(|&id| self.is_button(id));
            }
            Event::MouseDown { x, y } => {
                let hit = self.hit(x, y);
                if let Some(id) = hit {
                    if self.is_button(id) {
                        self.pressed = Some(id);
                        self.set_focus(Some(id), &mut actions);
                    } else if self.is_field(id) {
                        self.pressed = None;
                        self.set_focus(Some(id), &mut actions);
                    } else {
                        self.pressed = None;
                        self.set_focus(None, &mut actions);
                    }
                } else {
                    self.pressed = None;
                    self.set_focus(None, &mut actions);
                }
            }
            Event::MouseUp { x, y } => {
                if let Some(pid) = self.pressed.take() {
                    if self.hit(x, y) == Some(pid) {
                        actions.push(Action::Clicked(pid));
                    }
                }
            }
            Event::Key(key) => self.handle_key(key, &mut actions),
            Event::Text(ch) => {
                if let Some(fid) = self.focused {
                    if let Some(Some(node)) = self.nodes.get_mut(fid) {
                        if let Kind::Field(ref mut text) = node.kind {
                            text.push(ch);
                            actions.push(Action::Changed(fid));
                        }
                    }
                }
            }
        }
        actions
    }

    /// Handle a logical key press and append any resulting actions.
    fn handle_key(&mut self, key: Key, actions: &mut Vec<Action>) {
        match key {
            Key::Tab => self.move_focus(true, actions),
            Key::ShiftTab => self.move_focus(false, actions),
            Key::Escape => self.set_focus(None, actions),
            Key::Enter => {
                if let Some(fid) = self.focused {
                    if self.is_button(fid) {
                        actions.push(Action::Clicked(fid));
                    } else if self.is_field(fid) {
                        actions.push(Action::Submitted(fid));
                    }
                }
            }
            Key::Space => {
                if let Some(fid) = self.focused {
                    if self.is_button(fid) {
                        actions.push(Action::Clicked(fid));
                    }
                }
            }
            Key::Backspace => {
                if let Some(fid) = self.focused {
                    if let Some(Some(node)) = self.nodes.get_mut(fid) {
                        if let Kind::Field(ref mut text) = node.kind {
                            if text.pop().is_some() {
                                actions.push(Action::Changed(fid));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Update `self.focused` and emit `Action::Focused` if it changed.
    fn set_focus(&mut self, new_focus: Option<NodeId>, actions: &mut Vec<Action>) {
        if self.focused != new_focus {
            self.focused = new_focus;
            actions.push(Action::Focused(new_focus));
        }
    }

    /// Move focus forward (`forward = true`) or backward in pre-order among focusable nodes.
    fn move_focus(&mut self, forward: bool, actions: &mut Vec<Action>) {
        let focusables = self.collect_focusables();
        if focusables.is_empty() {
            return;
        }
        let target = match self
            .focused
            .and_then(|cur| focusables.iter().position(|&id| id == cur))
        {
            Some(idx) => {
                let len = focusables.len();
                let next_idx = if forward {
                    (idx + 1) % len
                } else {
                    (idx + len - 1) % len
                };
                focusables[next_idx]
            }
            None => {
                if forward {
                    focusables[0]
                } else {
                    focusables[focusables.len() - 1]
                }
            }
        };
        self.set_focus(Some(target), actions);
    }

    /// Collect all live focusable nodes (`Button` and `Field`) in pre-order.
    fn collect_focusables(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.node(id) {
                if matches!(node.kind, Kind::Button(_) | Kind::Field(_)) {
                    out.push(id);
                }
                for &cid in node.children.iter().rev() {
                    stack.push(cid);
                }
            }
        }
        out
    }

    /// Return whether `id` is a live `Button`.
    fn is_button(&self, id: NodeId) -> bool {
        self.node(id)
            .is_some_and(|n| matches!(n.kind, Kind::Button(_)))
    }

    /// Return whether `id` is a live `Field`.
    fn is_field(&self, id: NodeId) -> bool {
        self.node(id)
            .is_some_and(|n| matches!(n.kind, Kind::Field(_)))
    }
}
