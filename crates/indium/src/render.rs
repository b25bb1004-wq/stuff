//! Software rendering of a laid-out `Ui` onto a `pigment::Canvas`.

use pigment::{Canvas, Rect};

use crate::types::Kind;
use crate::Ui;

impl Ui {
    /// Render the widget tree onto `canvas` using the configured theme.
    pub fn render(&self, canvas: &mut Canvas) {
        canvas.fill_rect(
            Rect {
                x: 0,
                y: 0,
                w: canvas.width,
                h: canvas.height,
            },
            self.theme.bg,
        );

        let scale = self.theme.text_scale.max(1);
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let Some(node) = self.node(id) else {
                continue;
            };
            if let Some(rect) = node.rect {
                let tx = rect.x.saturating_add_unsigned(node.style.padding.left);
                let ty = rect.y.saturating_add_unsigned(node.style.padding.top);
                let inner_w = rect
                    .w
                    .saturating_sub(node.style.padding.left)
                    .saturating_sub(node.style.padding.right);

                match &node.kind {
                    Kind::Box => {}
                    Kind::Label(text) => {
                        canvas.draw_text(tx, ty, text, self.theme.fg, scale, inner_w);
                    }
                    Kind::Button(text) => {
                        let bg = if self.pressed == Some(id) {
                            self.theme.button_pressed
                        } else if self.hovered == Some(id) {
                            self.theme.button_hover
                        } else {
                            self.theme.button
                        };
                        canvas.fill_rect(rect, bg);
                        canvas.draw_text(tx, ty, text, self.theme.fg, scale, inner_w);
                    }
                    Kind::Field(text) => {
                        canvas.fill_rect(rect, self.theme.field_bg);
                        canvas.draw_text(tx, ty, text, self.theme.fg, scale, inner_w);
                    }
                }
            }

            for &cid in node.children.iter().rev() {
                stack.push(cid);
            }
        }

        if let Some(fid) = self.focused {
            if let Some(rect) = self.rect(fid) {
                canvas.stroke_rect(rect, self.theme.focus_ring);
            }
        }
    }
}
