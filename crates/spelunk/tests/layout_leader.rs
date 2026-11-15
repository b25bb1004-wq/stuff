//! Leader check for T66a: its own cascade (own selector matching and specificity on generated
//! rules) and its own block and inline layout model, on 60+ generated pages, plus fuzzing.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::type_complexity
)]
use assay::Rng;
use spelunk::dom::{Data, Document, NodeId};
use spelunk::layout::{layout, BoxRect, TextRun};
use spelunk::style::{compute, Computed, Display, Len};
use std::collections::BTreeMap;

// ---------- generated pages ----------
#[derive(Clone, Debug)]
enum Val {
    Disp(Display),
    L(Len),
    Px(i32),
    Col([u8; 4]),
}

#[derive(Clone, Debug)]
struct Decl {
    text: String,
    important: bool,
    longs: Option<Vec<(&'static str, Val)>>, // None: invalid, ignored
}

#[derive(Clone)]
struct Sel {
    text: String,
    parts: Vec<(
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    )>, // tag, class, id; descendant chain
}

struct El {
    tag: &'static str,
    class: Option<&'static str>,
    id: Option<&'static str>,
    style: Vec<Decl>,
    kids: Vec<Kid>,
}

enum Kid {
    E(El),
    T(String),
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
const MARGIN: [&str; 4] = ["margin-top", "margin-right", "margin-bottom", "margin-left"];
const PADDING: [&str; 4] = [
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
];
const BORDER: [&str; 4] = [
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
];

fn px_text(r: &mut Rng, hi: usize) -> (String, i32) {
    let n = r.below(hi) as i32;
    if n == 0 && r.chance(1, 2) {
        ("0".into(), 0)
    } else {
        (format!("{n}px"), n)
    }
}

fn four<T: Clone>(v: &[T]) -> [T; 4] {
    match v.len() {
        1 => [v[0].clone(), v[0].clone(), v[0].clone(), v[0].clone()],
        2 => [v[0].clone(), v[1].clone(), v[0].clone(), v[1].clone()],
        3 => [v[0].clone(), v[1].clone(), v[2].clone(), v[1].clone()],
        _ => [v[0].clone(), v[1].clone(), v[2].clone(), v[3].clone()],
    }
}

fn color(r: &mut Rng) -> (String, [u8; 4]) {
    match r.below(4) {
        0 => {
            let c = [r.below(16) as u8, r.below(16) as u8, r.below(16) as u8];
            (
                format!("#{:x}{:x}{:x}", c[0], c[1], c[2]),
                [c[0] * 17, c[1] * 17, c[2] * 17, 255],
            )
        }
        1 => {
            let c = [r.below(256) as u8, r.below(256) as u8, r.below(256) as u8];
            (
                format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
                [c[0], c[1], c[2], 255],
            )
        }
        2 => ("transparent".into(), [0, 0, 0, 0]),
        _ => {
            let (n, c) = *r.pick(&[
                ("black", [0, 0, 0, 255]),
                ("white", [255, 255, 255, 255]),
                ("red", [255, 0, 0, 255]),
                ("green", [0, 128, 0, 255]),
                ("blue", [0, 0, 255, 255]),
                ("gray", [128, 128, 128, 255]),
            ]);
            (n.into(), c)
        }
    }
}

fn decl(r: &mut Rng) -> Decl {
    let important = r.chance(1, 8);
    let (text, longs): (String, Option<Vec<(&'static str, Val)>>) = match r.below(14) {
        0 => {
            let (t, d) = *r.pick(&[
                ("block", Display::Block),
                ("inline", Display::Inline),
                ("none", Display::None),
            ]);
            (
                format!("display: {t}"),
                Some(vec![("display", Val::Disp(d))]),
            )
        }
        1 | 2 => {
            let p = *r.pick(&["width", "height"]);
            if r.chance(1, 4) {
                (format!("{p}: auto"), Some(vec![(p, Val::L(Len::Auto))]))
            } else {
                let (t, n) = px_text(r, if p == "width" { 300 } else { 80 });
                (format!("{p}: {t}"), Some(vec![(p, Val::L(Len::Px(n)))]))
            }
        }
        3 | 4 => {
            let k = 1 + r.below(4);
            let vs: Vec<(String, Len)> = (0..k)
                .map(|_| {
                    if r.chance(1, 4) {
                        ("auto".to_string(), Len::Auto)
                    } else {
                        let (t, n) = px_text(r, 30);
                        (t, Len::Px(n))
                    }
                })
                .collect();
            let f = four(&vs);
            let t = vs.iter().map(|v| v.0.clone()).collect::<Vec<_>>().join(" ");
            (
                format!("margin: {t}"),
                Some((0..4).map(|i| (MARGIN[i], Val::L(f[i].1))).collect()),
            )
        }
        5 => {
            let i = r.below(4);
            let (t, n) = px_text(r, 30);
            (
                format!("margin-{}: {t}", SIDES[i]),
                Some(vec![(MARGIN[i], Val::L(Len::Px(n)))]),
            )
        }
        6 | 7 => {
            let border = r.chance(1, 2);
            let k = 1 + r.below(4);
            let vs: Vec<(String, i32)> = (0..k)
                .map(|_| {
                    let n = r.below(if border { 6 } else { 25 }) as i32
                        - if r.chance(1, 8) { 9 } else { 0 };
                    (if n == 0 { "0".into() } else { format!("{n}px") }, n.max(0))
                })
                .collect();
            let f = four(&vs);
            let t = vs.iter().map(|v| v.0.clone()).collect::<Vec<_>>().join(" ");
            let names = if border { BORDER } else { PADDING };
            let prop = if border { "border-width" } else { "padding" };
            (
                format!("{prop}: {t}"),
                Some((0..4).map(|i| (names[i], Val::Px(f[i].1))).collect()),
            )
        }
        8 => {
            let i = r.below(4);
            if r.chance(1, 2) {
                let (t, n) = px_text(r, 25);
                (
                    format!("padding-{}: {t}", SIDES[i]),
                    Some(vec![(PADDING[i], Val::Px(n))]),
                )
            } else {
                let (t, n) = px_text(r, 6);
                (
                    format!("border-{}-width: {t}", SIDES[i]),
                    Some(vec![(BORDER[i], Val::Px(n))]),
                )
            }
        }
        9 => {
            let (t, c) = color(r);
            (format!("color: {t}"), Some(vec![("color", Val::Col(c))]))
        }
        10 => {
            let (t, c) = color(r);
            (
                format!("background-color: {t}"),
                Some(vec![("background-color", Val::Col(c))]),
            )
        }
        11 => {
            let n = 8 + r.below(40) as i32;
            (
                format!("font-size: {n}px"),
                Some(vec![("font-size", Val::Px(n))]),
            )
        }
        12 => {
            let t = *r.pick(&[
                "width: 12em",
                "color: purple",
                "padding: x",
                "margin: 1px 2px 3px 4px 5px",
                "display: grid",
                "font-size: big",
                "height: -",
                "bogus: 3px",
            ]);
            (t.into(), None)
        }
        _ => {
            let (t, n) = px_text(r, 300);
            (
                format!("width: {t}"),
                Some(vec![("width", Val::L(Len::Px(n)))]),
            )
        }
    };
    Decl {
        text: if important {
            format!("{text} !important")
        } else {
            text
        },
        important,
        longs,
    }
}

const CLASSES: [&str; 3] = ["c1", "c2", "c3"];
const IDS: [&str; 4] = ["i1", "i2", "i3", "i4"];

fn sel(r: &mut Rng) -> Sel {
    let tags: [&'static str; 8] = ["div", "p", "span", "section", "li", "h1", "em", "body"];
    let compound = |r: &mut Rng| -> (
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    ) {
        match r.below(5) {
            0 => (Some(*r.pick(&tags)), None, None),
            1 => (None, Some(*r.pick(&CLASSES)), None),
            2 => (None, None, Some(*r.pick(&IDS))),
            3 => (Some(*r.pick(&tags)), Some(*r.pick(&CLASSES)), None),
            _ => (Some(*r.pick(&tags)), None, Some(*r.pick(&IDS))),
        }
    };
    let n = if r.chance(1, 4) { 2 } else { 1 };
    let parts: Vec<_> = (0..n).map(|_| compound(r)).collect();
    let text = parts
        .iter()
        .map(|(t, c, i)| {
            format!(
                "{}{}{}",
                t.unwrap_or(""),
                c.map_or(String::new(), |c| format!(".{c}")),
                i.map_or(String::new(), |i| format!("#{i}"))
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    Sel { text, parts }
}

const WORDS: [&str; 8] = [
    "a",
    "box",
    "of",
    "words",
    "wraps",
    "here",
    "Mm",
    "spelunking",
];

fn text(r: &mut Rng) -> String {
    let mut s: Vec<&str> = (0..1 + r.below(6)).map(|_| *r.pick(&WORDS)).collect();
    if r.chance(1, 3) {
        s.push("");
    }
    s.join(if r.chance(1, 6) { "  \n " } else { " " })
}

fn el(r: &mut Rng, tag: &'static str, depth: u32) -> El {
    let mut e = El {
        tag,
        class: if r.chance(1, 2) {
            Some(*r.pick(&CLASSES))
        } else {
            None
        },
        id: if r.chance(1, 4) {
            Some(*r.pick(&IDS))
        } else {
            None
        },
        style: if r.chance(1, 3) {
            (0..1 + r.below(3)).map(|_| decl(r)).collect()
        } else {
            vec![]
        },
        kids: vec![],
    };
    let inline_only = matches!(tag, "p" | "h1" | "h2" | "span" | "em");
    let n = if depth > 3 { 0 } else { r.below(5) };
    for _ in 0..n {
        let kid = if tag == "ul" {
            Kid::E(el(r, "li", depth + 1))
        } else if inline_only {
            if r.chance(1, 3) {
                {
                    let t = *r.pick(&["span", "em"]);
                    Kid::E(el(r, t, depth + 1))
                }
            } else {
                Kid::T(text(r))
            }
        } else {
            match r.below(9) {
                0 | 1 => Kid::T(text(r)),
                2 => Kid::E(el(r, "span", depth + 1)),
                _ => {
                    let t = *r.pick(&["div", "p", "section", "h1", "h2", "ul", "div"]);
                    Kid::E(el(r, t, depth + 1))
                }
            }
        };
        // no two text nodes in a row, no leading space after a text node
        if let (Kid::T(_), Some(Kid::T(_))) = (&kid, e.kids.last()) {
            continue;
        }
        e.kids.push(kid);
    }
    e
}

fn flatten<'a>(e: &'a El, list: &mut Vec<&'a El>) {
    list.push(e);
    for k in &e.kids {
        if let Kid::E(c) = k {
            flatten(c, list);
        }
    }
}

fn html_of(e: &El, out: &mut String) {
    out.push('<');
    out.push_str(e.tag);
    if let Some(c) = e.class {
        out.push_str(&format!(" class=\"{c}\""));
    }
    if let Some(i) = e.id {
        out.push_str(&format!(" id=\"{i}\""));
    }
    if !e.style.is_empty() {
        let s = e
            .style
            .iter()
            .map(|d| d.text.clone())
            .collect::<Vec<_>>()
            .join("; ");
        out.push_str(&format!(" style=\"{s}\""));
    }
    out.push('>');
    for k in &e.kids {
        match k {
            Kid::E(c) => html_of(c, out),
            Kid::T(t) => out.push_str(t),
        }
    }
    out.push_str(&format!("</{}>", e.tag));
}

// ---------- cascade model ----------
struct Rule {
    sel: Sel,
    decls: Vec<Decl>,
    origin: u8,
}

fn ua_rules() -> Vec<Rule> {
    let mut v = Vec::new();
    let tag = |t: &'static str| Sel {
        text: t.into(),
        parts: vec![(Some(t), None, None)],
    };
    let d = |longs: Vec<(&'static str, Val)>| Decl {
        text: String::new(),
        important: false,
        longs: Some(longs),
    };
    for t in [
        "html",
        "body",
        "div",
        "p",
        "h1",
        "h2",
        "h3",
        "ul",
        "ol",
        "li",
        "section",
        "header",
        "footer",
        "nav",
        "main",
        "article",
        "pre",
        "blockquote",
    ] {
        v.push(Rule {
            sel: tag(t),
            decls: vec![d(vec![("display", Val::Disp(Display::Block))])],
            origin: 0,
        });
    }
    for t in ["head", "style", "script", "title"] {
        v.push(Rule {
            sel: tag(t),
            decls: vec![d(vec![("display", Val::Disp(Display::None))])],
            origin: 0,
        });
    }
    let m = |t: &'static str, a: i32, b: i32| Rule {
        sel: tag(t),
        decls: vec![d((0..4)
            .map(|i| (MARGIN[i], Val::L(Len::Px(if i % 2 == 0 { a } else { b }))))
            .collect())],
        origin: 0,
    };
    v.push(m("body", 8, 8));
    v.push(m("p", 16, 0));
    v.push(Rule {
        sel: tag("h1"),
        decls: vec![d(vec![("font-size", Val::Px(32))])],
        origin: 0,
    });
    v.push(m("h1", 21, 0));
    v.push(Rule {
        sel: tag("h2"),
        decls: vec![d(vec![("font-size", Val::Px(24))])],
        origin: 0,
    });
    v.push(m("h2", 19, 0));
    for t in ["ul", "ol"] {
        v.push(m(t, 16, 0));
        v.push(Rule {
            sel: tag(t),
            decls: vec![d(vec![("padding-left", Val::Px(40))])],
            origin: 0,
        });
    }
    v
}

fn compound_matches(
    doc: &Document,
    id: NodeId,
    c: &(Option<&str>, Option<&str>, Option<&str>),
) -> bool {
    c.0.is_none_or(|t| doc.name(id) == Some(t))
        && c.1.is_none_or(|k| doc.attr(id, "class") == Some(k))
        && c.2.is_none_or(|i| doc.attr(id, "id") == Some(i))
}

fn matches(doc: &Document, id: NodeId, s: &Sel) -> bool {
    let (last, rest) = s.parts.split_last().unwrap();
    if !compound_matches(doc, id, last) {
        return false;
    }
    let mut cur = doc.nodes[id].parent;
    let mut need = rest.iter().rev();
    let mut want = need.next();
    while let (Some(w), Some(p)) = (want, cur) {
        if matches!(doc.nodes[p].data, Data::Element { .. }) && compound_matches(doc, p, w) {
            want = need.next();
        }
        cur = doc.nodes[p].parent;
    }
    want.is_none()
}

fn spec(s: &Sel) -> (u32, u32, u32) {
    s.parts.iter().fold((0, 0, 0), |a, p| {
        (
            a.0 + u32::from(p.2.is_some()),
            a.1 + u32::from(p.1.is_some()),
            a.2 + u32::from(p.0.is_some()),
        )
    })
}

fn model_compute(
    doc: &Document,
    rules: &[Rule],
    attr_style: &BTreeMap<NodeId, Vec<Decl>>,
) -> Vec<Option<Computed>> {
    let mut out: Vec<Option<Computed>> = vec![None; doc.nodes.len()];
    let initial = Computed {
        display: Display::Inline,
        width: Len::Auto,
        height: Len::Auto,
        margin: [Len::Px(0); 4],
        padding: [0; 4],
        border: [0; 4],
        color: [0, 0, 0, 255],
        background: [0, 0, 0, 0],
        font_px: 16,
    };
    for id in doc.elements() {
        // (important, origin, specificity, order) -> value, per longhand
        let mut best: BTreeMap<&str, ((bool, u8, (u32, u32, u32), usize), Val)> = BTreeMap::new();
        let mut order = 0usize;
        let mut consider = |d: &Decl, origin: u8, sp: (u32, u32, u32), order: &mut usize| {
            *order += 1;
            if let Some(ls) = &d.longs {
                for (n, v) in ls {
                    let key = (d.important, origin, sp, *order);
                    if best.get(n).is_none_or(|(k, _)| key > *k) {
                        best.insert(n, (key, v.clone()));
                    }
                }
            }
        };
        for r in rules {
            if matches(doc, id, &r.sel) {
                for d in &r.decls {
                    consider(d, r.origin, spec(&r.sel), &mut order);
                }
            }
        }
        for d in attr_style.get(&id).into_iter().flatten() {
            consider(d, 2, (0, 0, 0), &mut order);
        }
        let parent = doc.nodes[id].parent.and_then(|p| out[p]);
        let mut c = initial;
        if let Some(p) = parent {
            c.color = p.color;
            c.font_px = p.font_px;
        }
        for (n, (_, v)) in best {
            match (n, v) {
                ("display", Val::Disp(d)) => c.display = d,
                ("width", Val::L(l)) => c.width = l,
                ("height", Val::L(l)) => c.height = l,
                ("color", Val::Col(k)) => c.color = k,
                ("background-color", Val::Col(k)) => c.background = k,
                ("font-size", Val::Px(p)) => c.font_px = p,
                (n, Val::L(l)) => c.margin[MARGIN.iter().position(|m| *m == n).unwrap()] = l,
                (n, Val::Px(p)) if n.starts_with("padding") => {
                    c.padding[PADDING.iter().position(|m| *m == n).unwrap()] = p
                }
                (n, Val::Px(p)) => c.border[BORDER.iter().position(|m| *m == n).unwrap()] = p,
                _ => unreachable!(),
            }
        }
        out[id] = Some(c);
    }
    out
}

// ---------- layout model ----------
struct L<'a> {
    doc: &'a Document,
    st: &'a [Option<Computed>],
    boxes: Vec<(NodeId, BoxRect)>,
    runs: Vec<TextRun>,
}

fn px(l: Len) -> i32 {
    match l {
        Len::Px(n) => n,
        Len::Auto => 0,
    }
}

impl L<'_> {
    fn disp(&self, id: NodeId) -> Option<Display> {
        match self.doc.nodes[id].data {
            Data::Element { .. } => Some(self.st[id].unwrap().display),
            Data::Text(_) => Some(Display::Inline),
            _ => None,
        }
    }

    /// Lays out block `id` whose margin edge is at (x, y) in a container `cw` wide; returns the
    /// border-box height.
    fn block(&mut self, id: NodeId, x: i32, y: i32, cw: i32) -> i32 {
        let s = self.st[id].unwrap();
        let (bl, br, bt, bb) = (s.border[3], s.border[1], s.border[0], s.border[2]);
        let (pl, pr, pt, pb) = (s.padding[3], s.padding[1], s.padding[0], s.padding[2]);
        let (mut ml, mut mr) = (px(s.margin[3]), px(s.margin[1]));
        let content_w = match s.width {
            Len::Auto => (cw - ml - mr - bl - br - pl - pr).max(0),
            Len::Px(w) => {
                let slack = (cw - w - bl - br - pl - pr - ml - mr).max(0);
                match (s.margin[3], s.margin[1]) {
                    (Len::Auto, Len::Auto) => {
                        ml = slack / 2;
                        mr = slack - slack / 2;
                    }
                    (Len::Auto, _) => ml = slack,
                    (_, Len::Auto) => mr = slack,
                    _ => {}
                }
                w
            }
        };
        let _ = mr;
        let bx = x + ml;
        let by = y + px(s.margin[0]);
        let idx = self.boxes.len();
        self.boxes.push((
            id,
            BoxRect {
                x: bx,
                y: by,
                w: 0,
                h: 0,
            },
        ));
        let (cx, cy) = (bx + bl + pl, by + bt + pt);
        let kids: Vec<NodeId> = self.doc.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|&k| self.disp(k).is_some_and(|d| d != Display::None))
            .collect();
        let any_block = kids.iter().any(|&k| self.disp(k) == Some(Display::Block));
        let mut cursor = cy;
        let mut prev_mb: Option<i32> = None;
        let mut i = 0;
        while i < kids.len() {
            if any_block && self.disp(kids[i]) == Some(Display::Block) {
                let ks = self.st[kids[i]].unwrap();
                let mt = px(ks.margin[0]);
                let gap = prev_mb.map_or(mt, |p| p.max(mt));
                let h = self.block(kids[i], cx, cursor + gap - mt, content_w);
                cursor += gap + h;
                prev_mb = Some(px(ks.margin[2]));
                i += 1;
            } else {
                let mut j = i;
                while j < kids.len() && self.disp(kids[j]) != Some(Display::Block) {
                    j += 1;
                }
                let gap = prev_mb.unwrap_or(0);
                let before = self.runs.len();
                let h = self.lines(&kids[i..j], cx, cursor + gap, content_w);
                // an anonymous block with no words is not generated: margins collapse through it
                if self.runs.len() > before {
                    cursor += gap + h;
                    prev_mb = Some(0);
                }
                i = j;
            }
        }
        let content_h = match s.height {
            Len::Px(h) => h,
            Len::Auto => cursor + prev_mb.unwrap_or(0) - cy,
        };
        let r = &mut self.boxes[idx].1;
        r.w = content_w + pl + pr + bl + br;
        r.h = content_h + pt + pb + bt + bb;
        r.h
    }

    fn collect(&self, id: NodeId, out: &mut Vec<(NodeId, String, u32)>) {
        match &self.doc.nodes[id].data {
            Data::Text(t) => {
                let p = self.doc.nodes[id].parent.unwrap();
                let f = self.st[p].unwrap().font_px;
                let scale = ((f + 8) / 16).max(1) as u32;
                out.push((id, t.clone(), scale));
            }
            Data::Element { .. } if self.st[id].unwrap().display != Display::None => {
                for &k in &self.doc.nodes[id].children {
                    self.collect(k, out);
                }
            }
            _ => {}
        }
    }

    fn lines(&mut self, kids: &[NodeId], cx: i32, cy: i32, cw: i32) -> i32 {
        let mut texts = Vec::new();
        for &k in kids {
            self.collect(k, &mut texts);
        }
        // tokens: (node, word or None for a space, scale), whitespace runs collapsed across the run
        let mut toks: Vec<(NodeId, Option<String>, u32)> = Vec::new();
        for (node, t, scale) in texts {
            let mut word = String::new();
            for ch in t.chars() {
                if ch.is_ascii_whitespace() {
                    if !word.is_empty() {
                        toks.push((node, Some(std::mem::take(&mut word)), scale));
                    }
                    if !matches!(toks.last(), Some((_, None, _))) {
                        toks.push((node, None, scale));
                    }
                } else {
                    word.push(ch);
                }
            }
            if !word.is_empty() {
                toks.push((node, Some(word), scale));
            }
        }
        let (mut y, mut x, mut line_scale, mut on_line) = (cy, 0i32, 0u32, false);
        let mut pending: Option<u32> = None;
        for (node, w, scale) in toks {
            match w {
                None => {
                    if on_line {
                        pending = Some(scale);
                    }
                }
                Some(word) => {
                    let (ww, wh) = pigment::measure(&word, scale);
                    let sp = pending.map_or(0, |s| 8 * s as i32);
                    if on_line && x + sp + ww as i32 > cw {
                        y += 16 * line_scale as i32;
                        x = 0;
                        line_scale = 0;
                    } else {
                        x += sp;
                    }
                    pending = None;
                    self.runs.push(TextRun {
                        node,
                        text: word,
                        rect: BoxRect {
                            x: cx + x,
                            y,
                            w: ww as i32,
                            h: wh as i32,
                        },
                        scale,
                    });
                    x += ww as i32;
                    line_scale = line_scale.max(scale);
                    on_line = true;
                }
            }
        }
        if on_line {
            y += 16 * line_scale as i32;
        }
        y - cy
    }
}

fn model_layout(
    doc: &Document,
    st: &[Option<Computed>],
    vw: i32,
) -> (Vec<(NodeId, BoxRect)>, Vec<TextRun>, i32) {
    let root = doc.elements()[0];
    let mut l = L {
        doc,
        st,
        boxes: vec![],
        runs: vec![],
    };
    let h = l.block(root, 0, 0, vw);
    (l.boxes, l.runs, h)
}

fn page(r: &mut Rng) -> (String, Vec<Rule>, El) {
    let mut rules = ua_rules();
    let mut css = String::new();
    for _ in 0..r.below(10) {
        let s = sel(r);
        let ds: Vec<Decl> = (0..1 + r.below(3)).map(|_| decl(r)).collect();
        css.push_str(&format!(
            "{} {{ {} }}\n",
            s.text,
            ds.iter()
                .map(|d| d.text.clone())
                .collect::<Vec<_>>()
                .join("; ")
        ));
        rules.push(Rule {
            sel: s,
            decls: ds,
            origin: 1,
        });
    }
    let mut body = el(r, "body", 0);
    if body.kids.is_empty() {
        body.kids.push(Kid::E(el(r, "div", 1)));
    }
    let mut html = format!("<html><head><style>{css}</style></head>");
    html_of(&body, &mut html);
    html.push_str("</html>");
    (html, rules, body)
}

#[test]
fn cascade_and_layout_match_model() {
    let mut r = Rng::new(0x66A_0001);
    let (mut words_placed, mut skipped) = (0, 0);
    for case in 0..80 {
        let (html, rules, body) = page(&mut r);
        let mut list = Vec::new();
        flatten(&body, &mut list);
        let doc = spelunk::html::parse(&html);
        let els = doc.elements();
        assert_eq!(
            els.len(),
            3 + list.len(),
            "case {case}: parser reshaped the page\n{html}"
        );
        let mut attr = BTreeMap::new();
        for (k, &e) in list.iter().enumerate() {
            assert_eq!(doc.name(els[3 + k]), Some(e.tag), "case {case}");
            attr.insert(els[3 + k], e.style.clone());
        }
        let want = model_compute(&doc, &rules, &attr);
        let got = compute(&doc);
        for &id in &els {
            assert_eq!(
                got[id],
                want[id],
                "case {case}: computed style of <{}> #{id}\n{html}",
                doc.name(id).unwrap()
            );
        }
        // a block inside an inline is outside the brief: compare the cascade only
        let block_in_inline = els.iter().any(|&id| {
            let mut p = doc.nodes[id].parent;
            want[id].is_some_and(|c| c.display == Display::Block)
                && std::iter::from_fn(|| {
                    let cur = p?;
                    p = doc.nodes[cur].parent;
                    Some(cur)
                })
                .any(|a| want[a].is_some_and(|c| c.display == Display::Inline))
        });
        if block_in_inline {
            skipped += 1;
            continue;
        }
        let vw = 200 + r.below(500) as i32;
        let lay = layout(&doc, &got, vw);
        let (boxes, runs, h) = model_layout(&doc, &want, vw);
        assert_eq!(lay.boxes, boxes, "case {case} vw {vw}: boxes\n{html}");
        assert_eq!(lay.runs.len(), runs.len(), "case {case}: run count\n{html}");
        for (a, b) in lay.runs.iter().zip(&runs) {
            assert_eq!(a, b, "case {case} vw {vw}: run\n{html}");
        }
        assert_eq!(lay.height, h, "case {case}: height");
        words_placed += runs.len();
    }
    assert!(
        words_placed > 500,
        "generator placed only {words_placed} words"
    );
    assert!(skipped <= 20, "{skipped} of 80 pages skipped layout");
}

#[test]
fn fuzz_never_panics() {
    let mut r = Rng::new(0x66A_0002);
    let bits = [
        "<div>",
        "</div>",
        "<p style=\"margin:auto;width:9999999999px\">",
        "<span>",
        "x y  z",
        "<style>p{padding:-5px 1px}div{width:-3px;margin:-2147483648px}</style>",
        "<h1 style=\"font-size:100000px\">",
        "<ul><li>",
        "<b>",
        "&amp;",
        "<section style=\"display:none\">",
        "\u{e9}\u{e9}",
        "<div style=\"border-width:2147483647px\">",
    ];
    for _ in 0..3000 {
        let s: String = (0..r.below(30)).map(|_| *r.pick(&bits)).collect();
        let doc = spelunk::html::parse(&s);
        let st = compute(&doc);
        for vw in [-5, 0, 1, 300, i32::MAX] {
            let _ = layout(&doc, &st, vw);
        }
    }
}
