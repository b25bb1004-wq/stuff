//! Leader check for T62: an independent model of every widget rule run against random operation
//! sequences, plus exact ease, tween and spring values.
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
use assay::Rng;
use indium::anim::{ease, tween, Ease, Spring};
use indium::widgets::{FieldState, ListState, MenuItem, MenuState, Nav, ScrollState, TabsState};

fn ri(r: &mut Rng, lo: i64, hi: i64) -> i64 {
    lo + r.below((hi - lo) as usize) as i64
}

const EASES: [Ease; 5] = [
    Ease::Linear,
    Ease::InQuad,
    Ease::OutQuad,
    Ease::InOutQuad,
    Ease::OutCubic,
];
const NAVS: [Nav; 6] = [
    Nav::Up,
    Nav::Down,
    Nav::PageUp,
    Nav::PageDown,
    Nav::Home,
    Nav::End,
];

fn m_ease(e: Ease, t: u32) -> u32 {
    let t = u64::from(t.min(1000));
    let u = 1000 - t;
    (match e {
        Ease::Linear => t,
        Ease::InQuad => t * t / 1000,
        Ease::OutQuad => 1000 - u * u / 1000,
        Ease::InOutQuad if t < 500 => 2 * t * t / 1000,
        Ease::InOutQuad => 1000 - 2 * u * u / 1000,
        Ease::OutCubic => 1000 - u * u * u / 1_000_000,
    }) as u32
}

fn m_tween(from: i32, to: i32, start: u64, dur: u64, now: u64, e: Ease) -> i32 {
    if now <= start {
        return from;
    }
    if dur == 0 || now >= start.saturating_add(dur) {
        return to;
    }
    let t = ((now - start) * 1000 / dur) as u32;
    (i64::from(from) + (i64::from(to) - i64::from(from)) * i64::from(m_ease(e, t)) / 1000) as i32
}

#[test]
fn anim_exact() {
    for e in EASES {
        for t in 0..=1200 {
            let v = ease(e, t);
            assert_eq!(v, m_ease(e, t), "{e:?} t={t}");
            assert!(v <= 1000);
        }
        assert_eq!((ease(e, 0), ease(e, 1000)), (0, 1000), "{e:?} ends");
    }
    let mut r = Rng::new(0x62_0001);
    for _ in 0..20_000 {
        let e = *r.pick(&EASES);
        let (from, to) = (
            ri(&mut r, -100_000, 100_000) as i32,
            ri(&mut r, -100_000, 100_000) as i32,
        );
        let start = r.below(1000) as u64;
        let dur = if r.chance(1, 10) {
            0
        } else {
            r.below(2000) as u64
        };
        let now = r.below(4000) as u64;
        assert_eq!(
            tween(from, to, start, dur, now, e),
            m_tween(from, to, start, dur, now, e),
            "{e:?} {from} {to} {start} {dur} {now}"
        );
    }
    for _ in 0..300 {
        let mut a = Spring {
            pos: ri(&mut r, -1_000_000, 1_000_000),
            vel: ri(&mut r, -10_000, 10_000),
            target: ri(&mut r, -1_000_000, 1_000_000),
            stiffness: ri(&mut r, 0, 400),
            damping: ri(&mut r, 0, 60),
        };
        let mut m = a;
        for _ in 0..400 {
            let dt = r.below(40) as u32;
            a.step(dt);
            let dt = i64::from(dt);
            let f = m.stiffness.saturating_mul(m.target.saturating_sub(m.pos)) / 1000
                - m.damping.saturating_mul(m.vel) / 1000;
            m.vel = m.vel.saturating_add(f.saturating_mul(dt) / 1000);
            m.pos = m.pos.saturating_add(m.vel.saturating_mul(dt) / 1000);
            assert_eq!(a, m);
            assert_eq!(
                a.settled(),
                (m.target - m.pos).abs() <= 1 && m.vel.abs() <= 1
            );
        }
    }
}

// ---- list model ----
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct L {
    len: usize,
    sel: Option<usize>,
    off: usize,
    vis: usize,
}

impl L {
    fn v(&self) -> usize {
        self.vis.max(1)
    }
    fn show(&mut self) {
        if let Some(s) = self.sel {
            if s < self.off {
                self.off = s;
            } else if s >= self.off + self.v() {
                self.off = s + 1 - self.v();
            }
        }
    }
    fn nav(&mut self, n: Nav) -> bool {
        let old = *self;
        if self.len == 0 {
            return false;
        }
        let last = self.len - 1;
        let v = self.v();
        let new = match (self.sel, n) {
            (None, Nav::End) => last,
            (None, _) => 0,
            (Some(s), Nav::Up) => s.saturating_sub(1),
            (Some(s), Nav::Down) => (s + 1).min(last),
            (Some(s), Nav::PageUp) => s.saturating_sub(v),
            (Some(s), Nav::PageDown) => (s + v).min(last),
            (Some(_), Nav::Home) => 0,
            (Some(_), Nav::End) => last,
        };
        if Some(new) != self.sel {
            self.sel = Some(new);
            self.show();
        }
        *self != old
    }
    fn click(&mut self, row: usize) -> bool {
        let old = *self;
        if self.off + row < self.len {
            self.sel = Some(self.off + row);
        }
        *self != old
    }
    fn set_len(&mut self, len: usize) -> bool {
        let old = *self;
        self.len = len;
        self.sel = if len == 0 {
            None
        } else {
            self.sel.map(|s| s.min(len - 1))
        };
        self.off = self.off.min(len.saturating_sub(self.v()));
        self.show();
        *self != old
    }
}

fn same(a: &ListState, m: &L) -> bool {
    a.len == m.len && a.selected == m.sel && a.offset == m.off && a.visible == m.vis
}

#[test]
fn list_scroll_tabs_match_model() {
    let mut r = Rng::new(0x62_0002);
    for case in 0..400 {
        let (len, vis) = (r.below(30), r.below(8));
        let mut a = ListState::new(len, vis);
        let mut m = L {
            len,
            sel: None,
            off: 0,
            vis,
        };
        assert!(same(&a, &m), "new");
        for step in 0..60 {
            let (ga, gm, what) = match r.below(10) {
                0 => {
                    let row = r.below(10);
                    (a.click_row(row), m.click(row), format!("click {row}"))
                }
                1 => {
                    let n = r.below(30);
                    (a.set_len(n), m.set_len(n), format!("set_len {n}"))
                }
                _ => {
                    let n = *r.pick(&NAVS);
                    (a.nav(n), m.nav(n), format!("{n:?}"))
                }
            };
            assert!(
                same(&a, &m) && ga == gm,
                "case {case} step {step} {what}: {a:?} vs {m:?} ({ga} vs {gm})"
            );
        }
    }
    for _ in 0..2000 {
        let mut s = ScrollState {
            content: r.below(5000) as u32,
            view: r.below(1000) as u32,
            pos: 0,
        };
        let max = s.content.saturating_sub(s.view);
        assert_eq!(s.max(), max);
        for _ in 0..10 {
            let d = ri(&mut r, -3000, 3000) as i32;
            let want = (i64::from(s.pos) + i64::from(d)).clamp(0, i64::from(max)) as u32;
            let changed = want != s.pos;
            assert_eq!(s.scroll_by(d), changed);
            assert_eq!(s.pos, want);
            let track = r.below(800) as u32;
            let th = if s.content <= s.view {
                (0, track)
            } else {
                let len = (u64::from(track) * u64::from(s.view) / u64::from(s.content))
                    .max(8)
                    .min(u64::from(track));
                let start = (u64::from(track) - len) * u64::from(s.pos) / u64::from(max);
                (start as u32, len as u32)
            };
            assert_eq!(s.thumb(track), th, "{s:?} track {track}");
        }
    }
    for _ in 0..500 {
        let count = r.below(6);
        let mut t = TabsState { count, active: 0 };
        let mut act = 0usize;
        for _ in 0..30 {
            let (got, want) = match r.below(3) {
                0 => (t.next(), if count == 0 { act } else { (act + 1) % count }),
                1 => (
                    t.prev(),
                    if count == 0 {
                        act
                    } else {
                        (act + count - 1) % count
                    },
                ),
                _ => {
                    let i = r.below(8);
                    (t.select(i), if i < count { i } else { act })
                }
            };
            assert_eq!(got, want != act, "tabs changed flag");
            act = want;
            assert_eq!(t.active, act);
        }
    }
}

#[test]
fn menu_matches_model() {
    let mut r = Rng::new(0x62_0003);
    for case in 0..500 {
        let n = r.below(7);
        let items: Vec<MenuItem> = (0..n)
            .map(|i| MenuItem {
                label: format!("m{i}"),
                enabled: r.chance(2, 3),
            })
            .collect();
        let en: Vec<usize> = (0..n).filter(|&i| items[i].enabled).collect();
        let mut a = MenuState {
            items: items.clone(),
            open: false,
            hover: None,
        };
        let (mut open, mut hover) = (false, None::<usize>);
        for step in 0..40 {
            match r.below(6) {
                0 => {
                    a.open();
                    open = true;
                    hover = en.first().copied();
                }
                1 => {
                    a.close();
                    open = false;
                    hover = None;
                }
                2 => {
                    let got = a.enter();
                    let want = if open {
                        hover.filter(|&h| items[h].enabled)
                    } else {
                        None
                    };
                    if want.is_some() {
                        open = false;
                        hover = None;
                    }
                    assert_eq!(got, want, "case {case} step {step} enter");
                }
                _ => {
                    let k = *r.pick(&NAVS);
                    let old = (open, hover);
                    if open && !en.is_empty() {
                        let pos = hover.and_then(|h| en.iter().position(|&e| e == h));
                        let l = en.len();
                        hover = Some(match (k, pos) {
                            (Nav::Down, None) | (Nav::Home | Nav::PageUp, _) => en[0],
                            (Nav::Up, None) | (Nav::End | Nav::PageDown, _) => en[l - 1],
                            (Nav::Down, Some(p)) => en[(p + 1) % l],
                            (Nav::Up, Some(p)) => en[(p + l - 1) % l],
                        });
                    }
                    let changed = a.key(k);
                    assert_eq!(
                        changed,
                        (open, hover) != old,
                        "case {case} step {step} {k:?} flag"
                    );
                }
            }
            assert_eq!(
                (a.open, a.hover),
                (open, hover),
                "case {case} step {step}: {items:?}"
            );
        }
    }
}

// ---- field model ----
#[derive(Clone, Debug, PartialEq, Eq)]
struct F {
    t: Vec<char>,
    c: usize,
    a: Option<usize>,
}

impl F {
    fn sel(&self) -> Option<(usize, usize)> {
        self.a
            .filter(|&a| a != self.c)
            .map(|a| (a.min(self.c), a.max(self.c)))
    }
    fn cut(&mut self) -> bool {
        if let Some((s, e)) = self.sel() {
            self.t.drain(s..e);
            self.c = s;
            self.a = None;
            true
        } else {
            false
        }
    }
    fn mv(&mut self, shift: bool, to: impl Fn(&F) -> usize, collapse: Option<bool>) {
        if shift {
            if self.a.is_none() {
                self.a = Some(self.c);
            }
            self.c = to(self);
        } else {
            self.c = match (collapse, self.sel()) {
                (Some(false), Some((s, _))) => s,
                (Some(true), Some((_, e))) => e,
                _ => to(self),
            };
            self.a = None;
        }
    }
}

#[test]
fn field_matches_model() {
    let mut r = Rng::new(0x62_0004);
    let chars = ['a', 'b', ' ', '\u{e9}', '\u{4e2d}', 'z'];
    for case in 0..500 {
        let init: String = (0..r.below(8)).map(|_| *r.pick(&chars)).collect();
        let mut a = FieldState::new(&init);
        let mut m = F {
            t: init.chars().collect(),
            c: init.chars().count(),
            a: None,
        };
        assert_eq!(
            (a.text.clone(), a.cursor, a.anchor),
            (init.clone(), m.c, None),
            "new puts the cursor at the end"
        );
        for step in 0..60 {
            let shift = r.chance(1, 2);
            let op = r.below(9);
            match op {
                0 => {
                    let ch = *r.pick(&chars);
                    a.insert(ch);
                    m.cut();
                    m.t.insert(m.c, ch);
                    m.c += 1;
                    m.a = None;
                }
                1 => {
                    a.backspace();
                    if !m.cut() && m.c > 0 {
                        m.c -= 1;
                        m.t.remove(m.c);
                    }
                    m.a = None;
                }
                2 => {
                    a.delete();
                    if !m.cut() && m.c < m.t.len() {
                        m.t.remove(m.c);
                    }
                    m.a = None;
                }
                3 => {
                    a.left(shift);
                    m.mv(shift, |f| f.c.saturating_sub(1), Some(false));
                }
                4 => {
                    a.right(shift);
                    m.mv(shift, |f| (f.c + 1).min(f.t.len()), Some(true));
                }
                5 => {
                    a.home(shift);
                    m.mv(shift, |_| 0, None);
                }
                6 => {
                    a.end(shift);
                    m.mv(shift, |f| f.t.len(), None);
                }
                _ => {
                    a.select_all();
                    m.a = Some(0);
                    m.c = m.t.len();
                }
            }
            let text: String = m.t.iter().collect();
            assert_eq!(
                (&a.text, a.cursor),
                (&text, m.c),
                "case {case} step {step} op {op} shift {shift}"
            );
            assert_eq!(a.selection(), m.sel(), "case {case} step {step} op {op}");
            let st: String = m
                .sel()
                .map_or(String::new(), |(s, e)| m.t[s..e].iter().collect());
            assert_eq!(a.selected_text(), st);
        }
    }
}
