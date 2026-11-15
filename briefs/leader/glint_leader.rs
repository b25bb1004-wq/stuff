//! Leader check for T61: its own flex model on random trees, hit testing, the focus and event
//! model on random event sequences with removals, and render pixels in both themes.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
use assay::Rng;
use indium::*;
use std::collections::BTreeMap;

#[derive(Clone)]
struct MNode {
    kind: Kind,
    style: Style,
    kids: Vec<NodeId>,
    parent: Option<NodeId>,
}

struct Model {
    n: BTreeMap<NodeId, MNode>,
    scale: u32,
    rect: BTreeMap<NodeId, Rect>,
    focus: Option<NodeId>,
    pressed: Option<NodeId>,
}

impl Model {
    fn base(&self, id: NodeId) -> (u32, u32) {
        let m = &self.n[&id];
        let s = &m.style;
        let (cw, ch) = match &m.kind {
            Kind::Label(t) | Kind::Button(t) => pigment::measure(t, self.scale),
            Kind::Field(t) => {
                let (w, _) = pigment::measure(t, self.scale);
                (w.max(80 * self.scale), 16 * self.scale)
            }
            Kind::Box => {
                let bs: Vec<(u32, u32)> = m.kids.iter().map(|&k| self.base(k)).collect();
                let gaps = s.gap.saturating_mul(bs.len().saturating_sub(1) as u32);
                let (main, cross): (Vec<u32>, Vec<u32>) = bs
                    .iter()
                    .map(|&(w, h)| if s.dir == Dir::Row { (w, h) } else { (h, w) })
                    .unzip();
                let sm = main.iter().fold(gaps, |a, &b| a.saturating_add(b));
                let cm = cross.into_iter().max().unwrap_or(0);
                if s.dir == Dir::Row {
                    (sm, cm)
                } else {
                    (cm, sm)
                }
            }
        };
        let p = s.padding;
        let w = s
            .width
            .unwrap_or(cw.saturating_add(p.left).saturating_add(p.right))
            .max(s.min_w);
        let h = s
            .height
            .unwrap_or(ch.saturating_add(p.top).saturating_add(p.bottom))
            .max(s.min_h);
        (w, h)
    }

    fn place(&mut self, id: NodeId, r: Rect) {
        self.rect.insert(id, r);
        let m = self.n[&id].clone();
        if m.kids.is_empty() {
            return;
        }
        let s = m.style;
        let p = s.padding;
        let (ix, iy) = (
            r.x.saturating_add(p.left as i32),
            r.y.saturating_add(p.top as i32),
        );
        let iw = r.w.saturating_sub(p.left.saturating_add(p.right));
        let ih = r.h.saturating_sub(p.top.saturating_add(p.bottom));
        let row = s.dir == Dir::Row;
        let (im, ic) = if row { (iw, ih) } else { (ih, iw) };
        let bases: Vec<(u32, u32)> = m
            .kids
            .iter()
            .map(|&k| {
                let (w, h) = self.base(k);
                if row {
                    (w, h)
                } else {
                    (h, w)
                }
            })
            .collect();
        let n = bases.len() as i64;
        let free = i64::from(im)
            - bases.iter().map(|b| i64::from(b.0)).sum::<i64>()
            - i64::from(s.gap) * (n - 1);
        let grows: Vec<i64> = m
            .kids
            .iter()
            .map(|k| i64::from(self.n[k].style.grow))
            .collect();
        let g: i64 = grows.iter().sum();
        let mut extra = vec![0i64; bases.len()];
        let (mut start, mut gap_extra) = (0i64, 0i64);
        if free > 0 && g > 0 {
            for (e, gi) in extra.iter_mut().zip(&grows) {
                *e = free * gi / g;
            }
            let mut left = free - extra.iter().sum::<i64>();
            for (e, gi) in extra.iter_mut().zip(&grows) {
                if left > 0 && *gi > 0 {
                    *e += 1;
                    left -= 1;
                }
            }
        } else if free > 0 {
            match s.justify {
                Justify::Start => {}
                Justify::End => start = free,
                Justify::Center => start = free / 2,
                Justify::SpaceBetween => {
                    if n > 1 {
                        gap_extra = free / (n - 1);
                    }
                }
            }
        }
        let mut pos = start;
        for (i, &k) in m.kids.iter().enumerate() {
            let main = (i64::from(bases[i].0) + extra[i]) as u32;
            let ks = self.n[&k].style;
            let own_cross = if row {
                ks.height.is_some()
            } else {
                ks.width.is_some()
            };
            let base_c = bases[i].1;
            let (coff, csize) = match (s.align, own_cross) {
                (Align::Stretch, false) => (0, ic),
                (Align::Stretch, true) | (Align::Start, _) => (0, base_c),
                (Align::Center, _) => (ic.saturating_sub(base_c) / 2, base_c),
                (Align::End, _) => (ic.saturating_sub(base_c), base_c),
            };
            let mp = pos.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            let cr = if row {
                Rect {
                    x: ix.saturating_add(mp),
                    y: iy.saturating_add(coff as i32),
                    w: main,
                    h: csize,
                }
            } else {
                Rect {
                    x: ix.saturating_add(coff as i32),
                    y: iy.saturating_add(mp),
                    w: csize,
                    h: main,
                }
            };
            self.place(k, cr);
            pos += i64::from(main) + i64::from(s.gap) + gap_extra;
        }
    }

    fn layout(&mut self, w: u32, h: u32) {
        self.rect.clear();
        self.place(0, Rect { x: 0, y: 0, w, h });
    }

    fn contains(r: &Rect, x: i32, y: i32) -> bool {
        i64::from(r.x) <= i64::from(x)
            && i64::from(x) < i64::from(r.x) + i64::from(r.w)
            && i64::from(r.y) <= i64::from(y)
            && i64::from(y) < i64::from(r.y) + i64::from(r.h)
    }

    /// Topmost in paint order (pre-order, later wins) whose rect holds the point; children may
    /// overflow their parent. Outside the root: None.
    fn hit(&self, root: NodeId, x: i32, y: i32) -> Option<NodeId> {
        if !Self::contains(&self.rect[&root], x, y) {
            return None;
        }
        let mut order = Vec::new();
        self.preorder(root, &mut order);
        order
            .into_iter()
            .rev()
            .find(|i| Self::contains(&self.rect[i], x, y))
    }

    fn preorder(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        for &k in &self.n[&id].kids {
            self.preorder(k, out);
        }
    }

    fn focusables(&self) -> Vec<NodeId> {
        let mut all = Vec::new();
        self.preorder(0, &mut all);
        all.into_iter()
            .filter(|i| matches!(self.n[i].kind, Kind::Button(_) | Kind::Field(_)))
            .collect()
    }

    fn set_focus(&mut self, f: Option<NodeId>, out: &mut Vec<Action>) {
        if self.focus != f {
            self.focus = f;
            out.push(Action::Focused(f));
        }
    }

    fn event(&mut self, ev: Event) -> Vec<Action> {
        let mut out = Vec::new();
        match ev {
            Event::MouseMove { .. } => {}
            Event::MouseDown { x, y } => {
                let h = self.hit(0, x, y);
                match h.map(|i| self.n[&i].kind.clone()) {
                    Some(Kind::Button(_)) => {
                        self.pressed = h;
                        self.set_focus(h, &mut out);
                    }
                    // brief is silent: a second MouseDown without MouseUp releases the press
                    Some(Kind::Field(_)) => {
                        self.pressed = None;
                        self.set_focus(h, &mut out);
                    }
                    _ => {
                        self.pressed = None;
                        self.set_focus(None, &mut out);
                    }
                }
            }
            Event::MouseUp { x, y } => {
                if let Some(p) = self.pressed.take() {
                    if self.hit(0, x, y) == Some(p) {
                        out.push(Action::Clicked(p));
                    }
                }
            }
            Event::Key(Key::Tab) | Event::Key(Key::ShiftTab) => {
                let f = self.focusables();
                if !f.is_empty() {
                    let fwd = ev == Event::Key(Key::Tab);
                    let next = match self.focus.and_then(|c| f.iter().position(|&x| x == c)) {
                        None if fwd => f[0],
                        None => f[f.len() - 1],
                        Some(i) if fwd => f[(i + 1) % f.len()],
                        Some(i) => f[(i + f.len() - 1) % f.len()],
                    };
                    self.set_focus(Some(next), &mut out);
                }
            }
            Event::Key(Key::Escape) => self.set_focus(None, &mut out),
            Event::Key(k) => {
                if let Some(f) = self.focus {
                    match (&mut self.n.get_mut(&f).unwrap().kind, k) {
                        (Kind::Button(_), Key::Enter | Key::Space) => out.push(Action::Clicked(f)),
                        (Kind::Field(_), Key::Enter) => out.push(Action::Submitted(f)),
                        (Kind::Field(t), Key::Backspace) => {
                            if t.pop().is_some() {
                                out.push(Action::Changed(f));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Text(c) => {
                if let Some(f) = self.focus {
                    if let Kind::Field(t) = &mut self.n.get_mut(&f).unwrap().kind {
                        t.push(c);
                        out.push(Action::Changed(f));
                    }
                }
            }
        }
        out
    }

    fn remove(&mut self, id: NodeId) {
        let mut sub = Vec::new();
        self.preorder(id, &mut sub);
        let p = self.n[&id].parent.unwrap();
        self.n.get_mut(&p).unwrap().kids.retain(|&k| k != id);
        for s in sub {
            self.n.remove(&s);
            if self.focus == Some(s) {
                self.focus = None;
            }
            if self.pressed == Some(s) {
                self.pressed = None;
            }
        }
    }
}

fn rand_style(r: &mut Rng, leaf: bool) -> Style {
    let ins = |r: &mut Rng| if r.chance(1, 2) { 0 } else { r.below(6) as u32 };
    Style {
        dir: if r.chance(1, 2) {
            Dir::Row
        } else {
            Dir::Column
        },
        justify: *r.pick(&[
            Justify::Start,
            Justify::Center,
            Justify::End,
            Justify::SpaceBetween,
        ]),
        align: *r.pick(&[Align::Start, Align::Center, Align::End, Align::Stretch]),
        gap: if leaf { 0 } else { r.below(5) as u32 },
        padding: Insets {
            top: ins(r),
            right: ins(r),
            bottom: ins(r),
            left: ins(r),
        },
        grow: if r.chance(1, 3) { r.below(4) as u32 } else { 0 },
        width: if r.chance(1, 5) {
            Some(r.below(120) as u32)
        } else {
            None
        },
        height: if r.chance(1, 5) {
            Some(r.below(60) as u32)
        } else {
            None
        },
        min_w: if r.chance(1, 5) {
            r.below(50) as u32
        } else {
            0
        },
        min_h: if r.chance(1, 5) {
            r.below(30) as u32
        } else {
            0
        },
    }
}

const WORDS: [&str; 6] = ["ok", "Cancel", "a much longer label", "", "Save file", "x"];

fn build(r: &mut Rng, theme: Theme) -> (Ui, Model) {
    let mut ui = Ui::new(theme);
    let mut m = Model {
        n: BTreeMap::new(),
        scale: theme.text_scale,
        rect: BTreeMap::new(),
        focus: None,
        pressed: None,
    };
    let root_style = rand_style(r, false);
    // the root keeps Style::default(); the model mirrors that
    let _ = root_style;
    m.n.insert(
        0,
        MNode {
            kind: Kind::Box,
            style: Style::default(),
            kids: vec![],
            parent: None,
        },
    );
    let mut boxes = vec![0];
    for i in 0..2 + r.below(25) {
        let parent = *r.pick(&boxes);
        let kind = match if i < 2 { 2 } else { r.below(5) } {
            0 | 1 => Kind::Box,
            2 => Kind::Button(r.pick(&WORDS).to_string()),
            3 => Kind::Field(r.pick(&WORDS).to_string()),
            _ => Kind::Label(r.pick(&WORDS).to_string()),
        };
        let leaf = kind != Kind::Box;
        let st = rand_style(r, leaf);
        let id = ui.add(parent, kind.clone(), st).unwrap();
        m.n.insert(
            id,
            MNode {
                kind: kind.clone(),
                style: st,
                kids: vec![],
                parent: Some(parent),
            },
        );
        m.n.get_mut(&parent).unwrap().kids.push(id);
        if !leaf {
            boxes.push(id);
        }
    }
    (ui, m)
}

fn compare(ui: &Ui, m: &Model, ctx: &str) {
    for (&id, r) in &m.rect {
        assert_eq!(
            ui.rect(id),
            Some(*r),
            "{ctx}: node {id} ({:?})",
            m.n[&id].kind
        );
    }
}

#[test]
fn flex_matches_model_on_random_trees() {
    let mut r = Rng::new(0x61_0001);
    for case in 0..400 {
        let theme = if r.chance(1, 2) {
            Theme::light()
        } else {
            Theme::dark()
        };
        let (mut ui, mut m) = build(&mut r, theme);
        let (w, h) = (r.below(400) as u32, r.below(300) as u32);
        ui.layout(w, h);
        m.layout(w, h);
        compare(&ui, &m, &format!("case {case} {w}x{h}"));
        for _ in 0..40 {
            let (x, y) = (r.below(440) as i32 - 20, r.below(340) as i32 - 20);
            assert_eq!(ui.hit(x, y), m.hit(0, x, y), "case {case}: hit ({x}, {y})");
        }
    }
}

fn center(m: &Model, id: NodeId) -> (i32, i32) {
    let r = m.rect[&id];
    (r.x + (r.w / 2) as i32, r.y + (r.h / 2) as i32)
}

#[test]
fn events_match_model() {
    let mut r = Rng::new(0x61_0002);
    for case in 0..200 {
        let (mut ui, mut m) = build(&mut r, Theme::light());
        let (w, h) = (300, 200);
        for step in 0..60 {
            ui.layout(w, h);
            m.layout(w, h);
            compare(&ui, &m, &format!("case {case} step {step}"));
            let ids: Vec<NodeId> = m.n.keys().copied().collect();
            let target = *r.pick(&ids);
            let (cx, cy) = center(&m, target);
            let ev = match r.below(12) {
                0 => Event::MouseDown { x: cx, y: cy },
                1 => Event::MouseUp { x: cx, y: cy },
                2 => Event::MouseMove { x: cx, y: cy },
                3 | 4 => Event::Key(Key::Tab),
                5 => Event::Key(Key::ShiftTab),
                6 => Event::Key(*r.pick(&[Key::Enter, Key::Space, Key::Backspace, Key::Escape])),
                7 | 8 => Event::Text(*r.pick(&['a', 'Z', ' ', '9'])),
                9 if target != 0 => {
                    assert!(ui.remove(target));
                    m.remove(target);
                    assert_eq!(ui.focus(), m.focus, "case {case}: focus after remove");
                    continue;
                }
                10 => Event::MouseDown { x: -5, y: 3 },
                _ => Event::MouseUp {
                    x: r.below(300) as i32,
                    y: r.below(200) as i32,
                },
            };
            let want = m.event(ev);
            assert_eq!(ui.event(ev), want, "case {case} step {step}: {ev:?}");
            assert_eq!(ui.focus(), m.focus, "case {case} step {step}: focus");
            for (&id, n) in &m.n {
                if let Kind::Field(t) | Kind::Label(t) | Kind::Button(t) = &n.kind {
                    assert_eq!(ui.text(id), Some(t.as_str()), "case {case}: text of {id}");
                }
            }
        }
        assert!(!ui.remove(0), "root cannot be removed");
        assert!(ui.add(9999, Kind::Box, Style::default()).is_none());
        assert!(ui.children(9999).is_empty() && ui.rect(9999).is_none() && !ui.set_text(9999, "x"));
    }
}

#[test]
fn render_pixels_both_themes() {
    for theme in [Theme::light(), Theme::dark()] {
        let mut ui = Ui::new(theme);
        let pad = Insets {
            top: 4,
            right: 4,
            bottom: 4,
            left: 4,
        };
        let row = ui
            .add(
                0,
                Kind::Box,
                Style {
                    dir: Dir::Row,
                    padding: pad,
                    gap: 6,
                    align: Align::Start,
                    ..Style::default()
                },
            )
            .unwrap();
        let b = ui
            .add(
                row,
                Kind::Button("Go".into()),
                Style {
                    padding: pad,
                    ..Style::default()
                },
            )
            .unwrap();
        let f = ui
            .add(
                row,
                Kind::Field(String::new()),
                Style {
                    padding: pad,
                    ..Style::default()
                },
            )
            .unwrap();
        ui.layout(200, 80);
        let br = ui.rect(b).unwrap();
        let fr = ui.rect(f).unwrap();
        let px = |ui: &Ui, x: i32, y: i32| {
            let mut c = Canvas::new(200, 80, Rgba(9, 9, 9, 255));
            ui.render(&mut c);
            c.get(x, y).unwrap()
        };
        assert_eq!(px(&ui, 1, 1), theme.bg);
        assert_eq!(px(&ui, 199, 79), theme.bg);
        assert_eq!(px(&ui, br.x + 1, br.y + 1), theme.button);
        assert_eq!(px(&ui, fr.x + 1, fr.y + 1), theme.field_bg);
        let (cx, cy) = (br.x + 2, br.y + 2);
        ui.event(Event::MouseMove { x: cx, y: cy });
        assert_eq!(px(&ui, br.x + 1, br.y + 1), theme.button_hover);
        ui.event(Event::MouseDown { x: cx, y: cy });
        assert_eq!(px(&ui, br.x + 1, br.y + 1), theme.button_pressed);
        assert_eq!(
            px(&ui, br.x, br.y),
            theme.focus_ring,
            "focus ring on the rect edge"
        );
        assert_eq!(
            ui.event(Event::MouseUp { x: cx, y: cy }),
            vec![Action::Clicked(b)]
        );
        ui.event(Event::Key(Key::Tab));
        assert_eq!(ui.focus(), Some(f));
        assert_eq!(px(&ui, fr.x, fr.y + 1), theme.focus_ring);
        assert_eq!(
            px(&ui, br.x, br.y),
            theme.button_hover,
            "old focus ring gone"
        );
        ui.event(Event::Text('W'));
        let c = {
            let mut c = Canvas::new(200, 80, Rgba(9, 9, 9, 255));
            ui.render(&mut c);
            c
        };
        let fg = (0..fr.h as i32)
            .flat_map(|y| (0..fr.w as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| c.get(fr.x + x, fr.y + y) == Some(theme.fg))
            .count();
        assert!(fg > 0, "typed text is drawn in fg");
    }
}
