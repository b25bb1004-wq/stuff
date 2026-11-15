//! Leader check for T66b: own flex cascade and own flex/block box model on generated pages of
//! nested divs (no text, so the T66a inline model is not involved), compared box by box.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::type_complexity,
    clippy::single_match
)]
use assay::Rng;
use odyssey::dom::Document;
use odyssey::layout::{layout, BoxRect};
use odyssey::style::{compute_flex, Flex, FlexAlign, FlexDir, FlexJustify};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
enum D {
    Block,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug)]
enum M {
    Px(i32),
    Auto,
}

struct El {
    class: Option<&'static str>,
    disp: D,
    w: Option<i32>,
    h: Option<i32>,
    margin: [M; 4],
    pad: [i32; 4],
    border: [i32; 4],
    flex_decls: Vec<(String, Option<FlexVal>)>, // text, parsed value (None: invalid)
    kids: Vec<El>,
}

#[derive(Clone, Copy, Debug)]
enum FlexVal {
    Dir(FlexDir),
    Justify(FlexJustify),
    Align(FlexAlign),
    Grow(u32),
    Gap(i32),
}

fn flex_decl(r: &mut Rng) -> (String, Option<FlexVal>) {
    match r.below(6) {
        0 => {
            let (t, v) = *r.pick(&[
                ("row", Some(FlexDir::Row)),
                ("column", Some(FlexDir::Column)),
                ("diagonal", None),
            ]);
            (format!("flex-direction: {t}"), v.map(FlexVal::Dir))
        }
        1 => {
            let (t, v) = *r.pick(&[
                ("flex-start", Some(FlexJustify::Start)),
                ("center", Some(FlexJustify::Center)),
                ("flex-end", Some(FlexJustify::End)),
                ("space-between", Some(FlexJustify::SpaceBetween)),
                ("space-around", None),
            ]);
            (format!("justify-content: {t}"), v.map(FlexVal::Justify))
        }
        2 => {
            let (t, v) = *r.pick(&[
                ("flex-start", Some(FlexAlign::Start)),
                ("center", Some(FlexAlign::Center)),
                ("flex-end", Some(FlexAlign::End)),
                ("stretch", Some(FlexAlign::Stretch)),
                ("baseline", None),
            ]);
            (format!("align-items: {t}"), v.map(FlexVal::Align))
        }
        3 | 4 => {
            if r.chance(1, 8) {
                (format!("flex-grow: {}", *r.pick(&["-1", "x"])), None)
            } else {
                let g = r.below(4) as u32;
                (format!("flex-grow: {g}"), Some(FlexVal::Grow(g)))
            }
        }
        _ => {
            let g = r.below(14) as i32 - 3;
            let t = if g == 0 {
                "0".to_string()
            } else {
                format!("{g}px")
            };
            (format!("gap: {t}"), Some(FlexVal::Gap(g.max(0))))
        }
    }
}

fn el(r: &mut Rng, depth: u32) -> El {
    let px = |r: &mut Rng, hi: usize| r.below(hi) as i32;
    let mut e = El {
        class: if r.chance(1, 2) {
            Some(*r.pick(&["f1", "f2", "f3"]))
        } else {
            None
        },
        disp: match r.below(10) {
            0 => D::None,
            1..=4 => D::Flex,
            _ => D::Block,
        },
        w: if r.chance(1, 2) {
            Some(px(r, 200))
        } else {
            None
        },
        h: if r.chance(1, 3) {
            Some(px(r, 120))
        } else {
            None
        },
        margin: [0; 4].map(|_| {
            if r.chance(1, 8) {
                M::Auto
            } else {
                M::Px(px(r, 12))
            }
        }),
        pad: [0; 4].map(|_| px(r, 8)),
        border: [0; 4].map(|_| px(r, 4)),
        flex_decls: (0..r.below(4)).map(|_| flex_decl(r)).collect(),
        kids: vec![],
    };
    if depth < 3 {
        for _ in 0..r.below(5) {
            e.kids.push(el(r, depth + 1));
        }
    }
    e
}

fn m_txt(m: M) -> String {
    match m {
        M::Px(0) => "0".into(),
        M::Px(n) => format!("{n}px"),
        M::Auto => "auto".into(),
    }
}

fn html_of(e: &El, out: &mut String) {
    let mut st = vec![format!(
        "display: {}",
        match e.disp {
            D::Block => "block",
            D::Flex => "flex",
            D::None => "none",
        }
    )];
    st.push(format!(
        "width: {}",
        e.w.map_or("auto".into(), |w| format!("{w}px"))
    ));
    st.push(format!(
        "height: {}",
        e.h.map_or("auto".into(), |h| format!("{h}px"))
    ));
    st.push(format!(
        "margin: {}",
        e.margin
            .iter()
            .map(|&m| m_txt(m))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    st.push(format!(
        "padding: {}",
        e.pad
            .iter()
            .map(|&p| format!("{p}px"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    st.push(format!(
        "border-width: {}",
        e.border
            .iter()
            .map(|&p| format!("{p}px"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    st.extend(e.flex_decls.iter().map(|d| d.0.clone()));
    out.push_str("<div");
    if let Some(c) = e.class {
        out.push_str(&format!(" class=\"{c}\""));
    }
    out.push_str(&format!(" style=\"{}\">", st.join("; ")));
    for k in &e.kids {
        html_of(k, out);
    }
    out.push_str("</div>");
}

fn flatten<'a>(e: &'a El, v: &mut Vec<&'a El>) {
    v.push(e);
    for k in &e.kids {
        flatten(k, v);
    }
}

struct Rule {
    class: Option<&'static str>, // None: `div`
    decls: Vec<(String, Option<FlexVal>, bool)>,
}

fn apply(f: &mut Flex, v: FlexVal) {
    match v {
        FlexVal::Dir(d) => f.dir = d,
        FlexVal::Justify(j) => f.justify = j,
        FlexVal::Align(a) => f.align = a,
        FlexVal::Grow(g) => f.grow = g,
        FlexVal::Gap(g) => f.gap = g,
    }
}

fn slot(v: FlexVal) -> u8 {
    match v {
        FlexVal::Dir(_) => 0,
        FlexVal::Justify(_) => 1,
        FlexVal::Align(_) => 2,
        FlexVal::Grow(_) => 3,
        FlexVal::Gap(_) => 4,
    }
}

const INIT: Flex = Flex {
    dir: FlexDir::Row,
    justify: FlexJustify::Start,
    align: FlexAlign::Stretch,
    grow: 0,
    gap: 0,
};

fn model_flex(e: &El, rules: &[Rule]) -> Flex {
    // key: (important, origin, specificity, order)
    let mut best: BTreeMap<u8, ((bool, u8, u8, usize), FlexVal)> = BTreeMap::new();
    let mut order = 0;
    let mut put = |v: Option<FlexVal>, key: (bool, u8, u8, usize)| {
        if let Some(v) = v {
            if best.get(&slot(v)).is_none_or(|(k, _)| key > *k) {
                best.insert(slot(v), (key, v));
            }
        }
    };
    for rl in rules {
        if rl.class.is_none_or(|c| e.class == Some(c)) {
            let spec = if rl.class.is_some() { 10 } else { 1 };
            for (_, v, imp) in &rl.decls {
                order += 1;
                put(*v, (*imp, 1, spec, order));
            }
        }
    }
    for (_, v) in &e.flex_decls {
        order += 1;
        put(*v, (false, 2, 0, order));
    }
    let mut f = INIT;
    for (_, (_, v)) in best {
        apply(&mut f, v);
    }
    f
}

// ---- box model ----
struct Lay<'a> {
    els: &'a [&'a El],
    fx: &'a [Flex],
    idx: BTreeMap<*const El, usize>,
    boxes: Vec<BoxRect>,
}

fn mpx(m: M) -> i32 {
    match m {
        M::Px(n) => n,
        M::Auto => 0,
    }
}

impl Lay<'_> {
    fn f(&self, e: &El) -> Flex {
        self.fx[self.idx[&(e as *const El)]]
    }
    fn hpb(e: &El) -> i32 {
        e.pad[1] + e.pad[3] + e.border[1] + e.border[3]
    }
    fn vpb(e: &El) -> i32 {
        e.pad[0] + e.pad[2] + e.border[0] + e.border[2]
    }
    fn shown(e: &El) -> Vec<&El> {
        e.kids.iter().filter(|k| k.disp != D::None).collect()
    }

    /// Places `e`'s border box at (bx, by) with content width `cw`; `forced` is a border-box
    /// height (stretch). Returns the border-box height.
    fn place(&mut self, e: &El, bx: i32, by: i32, cw: i32, forced: Option<i32>) -> i32 {
        let at = self.boxes.len();
        self.boxes.push(BoxRect {
            x: bx,
            y: by,
            w: cw + Self::hpb(e),
            h: 0,
        });
        let (cx, cy) = (bx + e.border[3] + e.pad[3], by + e.border[0] + e.pad[0]);
        let content_h = if e.disp == D::Flex {
            self.flex(e, cx, cy, cw)
        } else {
            self.blocks(e, cx, cy, cw)
        };
        let h = forced.unwrap_or(e.h.unwrap_or(content_h) + Self::vpb(e));
        self.boxes[at].h = h;
        h
    }

    /// Block child in a block container with containing width `cw` at margin-edge (x, y).
    fn block_child(&mut self, e: &El, x: i32, y: i32, cw: i32) -> i32 {
        let (mut ml, mr) = (mpx(e.margin[3]), mpx(e.margin[1]));
        let content = match e.w {
            None => (cw - ml - mr - Self::hpb(e)).max(0),
            Some(w) => {
                let slack = (cw - w - Self::hpb(e) - ml - mr).max(0);
                match (e.margin[3], e.margin[1]) {
                    (M::Auto, M::Auto) => ml = slack / 2,
                    (M::Auto, _) => ml = slack,
                    _ => {}
                }
                w
            }
        };
        self.place(e, x + ml, y + mpx(e.margin[0]), content, None)
    }

    fn blocks(&mut self, e: &El, cx: i32, cy: i32, cw: i32) -> i32 {
        let mut cur = cy;
        let mut prev: Option<i32> = None;
        for k in Self::shown(e) {
            let mt = mpx(k.margin[0]);
            let gap = prev.map_or(mt, |p| p.max(mt));
            let h = self.block_child(k, cx, cur + gap - mt, cw);
            cur += gap + h;
            prev = Some(mpx(k.margin[2]));
        }
        cur + prev.unwrap_or(0) - cy
    }

    fn measure(&mut self, e: &El, cw: i32) -> i32 {
        let n = self.boxes.len();
        let h = self.place(e, 0, 0, cw, None);
        self.boxes.truncate(n);
        h
    }

    fn flex(&mut self, e: &El, cx: i32, cy: i32, cw: i32) -> i32 {
        let f = self.f(e);
        let items = Self::shown(e);
        let n = items.len() as i32;
        if n == 0 {
            return e.h.unwrap_or(0);
        }
        let gaps = f.gap * (n - 1);
        let sum_grow: u32 = items.iter().map(|k| self.f(k).grow).sum();
        let mh = |k: &El| mpx(k.margin[3]) + mpx(k.margin[1]);
        let mv = |k: &El| mpx(k.margin[0]) + mpx(k.margin[2]);
        let shares = |free: i32, items: &[&El], me: &Self| -> Vec<i32> {
            let mut s: Vec<i32> = items
                .iter()
                .map(|k| (i64::from(free) * i64::from(me.f(k).grow) / i64::from(sum_grow)) as i32)
                .collect();
            let rem = free - s.iter().sum::<i32>();
            if let Some(last) = items.iter().rposition(|k| me.f(k).grow > 0) {
                s[last] += rem;
            }
            s
        };
        let justify = |left: i32| -> (i32, i32) {
            let left = left.max(0);
            match f.justify {
                FlexJustify::Start => (0, 0),
                FlexJustify::Center => (left / 2, 0),
                FlexJustify::End => (left, 0),
                FlexJustify::SpaceBetween if n > 1 => (0, left / (n - 1)),
                FlexJustify::SpaceBetween => (0, 0),
            }
        };
        let off = |space: i32, outer: i32, a: FlexAlign| match a {
            FlexAlign::Center => (space - outer).div_euclid(2),
            FlexAlign::End => space - outer,
            _ => 0,
        };
        if f.dir == FlexDir::Row {
            let base: Vec<i32> = items.iter().map(|k| k.w.unwrap_or(0)).collect();
            let free = cw
                - items
                    .iter()
                    .zip(&base)
                    .map(|(k, b)| mh(k) + Self::hpb(k) + b)
                    .sum::<i32>()
                - gaps;
            let (widths, (start, extra)) = if free > 0 && sum_grow > 0 {
                let s = shares(free, &items, self);
                (
                    base.iter().zip(s).map(|(b, s)| b + s).collect::<Vec<_>>(),
                    (0, 0),
                )
            } else {
                (base.clone(), justify(free))
            };
            let nat: Vec<i32> = items
                .iter()
                .zip(&widths)
                .map(|(k, &w)| self.measure(k, w))
                .collect();
            let cross = e.h.unwrap_or_else(|| {
                items
                    .iter()
                    .zip(&nat)
                    .map(|(k, h)| h + mv(k))
                    .max()
                    .unwrap_or(0)
            });
            let mut x = cx + start;
            for ((k, &w), &h) in items.iter().zip(&widths).zip(&nat) {
                let stretch = f.align == FlexAlign::Stretch && k.h.is_none();
                let (forced, y) = if stretch {
                    (
                        Some((cross - mv(k)).max(Self::vpb(k))),
                        cy + mpx(k.margin[0]),
                    )
                } else {
                    (None, cy + off(cross, h + mv(k), f.align) + mpx(k.margin[0]))
                };
                self.place(k, x + mpx(k.margin[3]), y, w, forced);
                x += mh(k) + Self::hpb(k) + w + f.gap + extra;
            }
            cross
        } else {
            let widths: Vec<i32> = items
                .iter()
                .map(|k| match k.w {
                    None if f.align == FlexAlign::Stretch => (cw - mh(k) - Self::hpb(k)).max(0),
                    w => w.unwrap_or(0),
                })
                .collect();
            let nat: Vec<i32> = items
                .iter()
                .zip(&widths)
                .map(|(k, &w)| self.measure(k, w))
                .collect();
            let outer_nat: i32 = items.iter().zip(&nat).map(|(k, h)| h + mv(k)).sum();
            let (mut heights, mut start, mut extra) = (nat.clone(), 0, 0);
            match e.h {
                Some(fixed) => {
                    let base: i32 = items
                        .iter()
                        .map(|k| mv(k) + Self::vpb(k) + k.h.unwrap_or(0))
                        .sum();
                    let free = fixed - base - gaps;
                    if free > 0 && sum_grow > 0 {
                        for (h, s) in heights.iter_mut().zip(shares(free, &items, self)) {
                            *h += s;
                        }
                    } else {
                        (start, extra) = justify(fixed - outer_nat - gaps);
                    }
                }
                None => {}
            }
            let mut y = cy + start;
            for ((k, &w), &h) in items.iter().zip(&widths).zip(&heights) {
                let x =
                    cx + off(
                        cw,
                        w + Self::hpb(k) + mh(k),
                        if k.w.is_none() && f.align == FlexAlign::Stretch {
                            FlexAlign::Start
                        } else {
                            f.align
                        },
                    ) + mpx(k.margin[3]);
                let forced = (h != nat[items.iter().position(|q| std::ptr::eq(*q, *k)).unwrap()])
                    .then_some(h);
                self.place(k, x, y + mpx(k.margin[0]), w, forced);
                y += h + mv(k) + f.gap + extra;
            }
            e.h.unwrap_or(
                heights
                    .iter()
                    .zip(&items)
                    .map(|(h, k)| h + mv(k))
                    .sum::<i32>()
                    + gaps,
            )
        }
    }
}

#[test]
fn flex_matches_model() {
    let mut r = Rng::new(0x66B_0001);
    let mut flex_items = 0;
    for case in 0..300 {
        let rules: Vec<Rule> = (0..r.below(5))
            .map(|_| Rule {
                class: if r.chance(1, 2) {
                    Some(*r.pick(&["f1", "f2", "f3"]))
                } else {
                    None
                },
                decls: (0..1 + r.below(2))
                    .map(|_| {
                        let (t, v) = flex_decl(&mut r);
                        (t, v, r.chance(1, 6))
                    })
                    .collect(),
            })
            .collect();
        let css: String = rules
            .iter()
            .map(|rl| {
                let d: Vec<String> = rl
                    .decls
                    .iter()
                    .map(|(t, _, i)| {
                        if *i {
                            format!("{t} !important")
                        } else {
                            t.clone()
                        }
                    })
                    .collect();
                format!(
                    "{} {{ {} }}\n",
                    rl.class.map_or("div".into(), |c| format!(".{c}")),
                    d.join("; ")
                )
            })
            .collect();
        let mut root = el(&mut r, 0);
        root.disp = if r.chance(3, 4) { D::Flex } else { D::Block };
        let mut html = format!("<html><head><style>{css}</style></head><body style=\"margin: 0\">");
        html_of(&root, &mut html);
        html.push_str("</body></html>");
        let doc: Document = odyssey::html::parse(&html);
        let mut els = Vec::new();
        flatten(&root, &mut els);
        let ids = doc.elements();
        assert_eq!(
            ids.len(),
            4 + els.len(),
            "case {case}: parser reshaped\n{html}"
        );
        let got_f = compute_flex(&doc);
        let fx: Vec<Flex> = els.iter().map(|e| model_flex(e, &rules)).collect();
        for (k, f) in fx.iter().enumerate() {
            assert_eq!(
                got_f[ids[4 + k]],
                Some(*f),
                "case {case}: flex values of div #{k}\n{html}"
            );
        }
        assert_eq!(got_f[ids[0]], Some(INIT));
        let vw = 100 + r.below(600) as i32;
        let st = odyssey::style::compute(&doc);
        let lay = layout(&doc, &st, vw);
        let mut m = Lay {
            els: &els,
            fx: &fx,
            idx: els
                .iter()
                .enumerate()
                .map(|(i, e)| (*e as *const El, i))
                .collect(),
            boxes: vec![],
        };
        let _ = m.els;
        // html and body are plain blocks at full width; body margin 0
        let mut want = vec![
            BoxRect {
                x: 0,
                y: 0,
                w: vw,
                h: 0,
            },
            BoxRect {
                x: 0,
                y: 0,
                w: vw,
                h: 0,
            },
        ];
        let h = if root.disp == D::None {
            0
        } else {
            m.block_child(&root, 0, 0, vw) + mpx(root.margin[0]) + mpx(root.margin[2])
        };
        want[0].h = h;
        want[1].h = h;
        want.extend(m.boxes);
        let got: Vec<BoxRect> = lay.boxes.iter().map(|b| b.1).collect();
        assert_eq!(got, want, "case {case} vw {vw}\n{html}");
        assert!(lay.runs.is_empty());
        flex_items += els
            .iter()
            .filter(|e| e.disp == D::Flex)
            .map(|e| e.kids.len())
            .sum::<usize>();
    }
    assert!(flex_items > 600, "only {flex_items} flex items generated");
}

#[test]
fn flex_fuzz_never_panics() {
    let mut r = Rng::new(0x66B_0002);
    let bits = [
        "<div style=\"display:flex\">",
        "<div style=\"display:flex;flex-direction:column;height:-5px\">",
        "</div>",
        "<div style=\"flex-grow:4000000000\">",
        "<div style=\"gap:2147483647px;width:2147483647px\">",
        "<div style=\"align-items:center;justify-content:space-between;margin:-9px\">",
        "x y",
        "<span>",
        "<div style=\"display:none\">",
    ];
    for _ in 0..3000 {
        let s: String = (0..r.below(25)).map(|_| *r.pick(&bits)).collect();
        let doc = odyssey::html::parse(&s);
        let st = odyssey::style::compute(&doc);
        let _ = compute_flex(&doc);
        for vw in [-1, 0, 50, i32::MAX] {
            let _ = layout(&doc, &st, vw);
        }
    }
}
