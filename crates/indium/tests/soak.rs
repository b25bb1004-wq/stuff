//! Seeded soak of indium's layout, events, text, widget state machines and animation.
//! Env: `STRATA_SOAK_SEED` (hex with `0x`, or decimal; default `0x05EE_D79D`) and
//! `STRATA_SOAK_MS` (budget per target in ms; default 300).
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use indium::anim::{ease, tween, Ease, Spring};
use indium::widgets::{FieldState, ListState, MenuItem, MenuState, Nav, ScrollState, TabsState};
use indium::{
    Action, Align, Canvas, Dir, Event, Insets, Justify, Key, Kind, NodeId, Rect, Rgba, Style,
    Theme, Ui,
};

const DEFAULT_SEED: u64 = 0x05EE_D79D;
const DEFAULT_MS: u64 = 300;
const EDGE: [u32; 8] = [0, 1, 2, 7, 1 << 16, 1 << 31, u32::MAX - 1, u32::MAX];
const NAVS: [Nav; 6] = [
    Nav::Up,
    Nav::Down,
    Nav::PageUp,
    Nav::PageDown,
    Nav::Home,
    Nav::End,
];
const KEYS: [Key; 6] = [
    Key::Tab,
    Key::ShiftTab,
    Key::Enter,
    Key::Space,
    Key::Backspace,
    Key::Escape,
];
const EASES: [Ease; 5] = [
    Ease::Linear,
    Ease::InQuad,
    Ease::OutQuad,
    Ease::InOutQuad,
    Ease::OutCubic,
];
const CHARS: [char; 10] = [
    'a',
    'Z',
    ' ',
    '\u{e9}',
    '\u{4e2d}',
    '\u{1F600}',
    '\u{301}',
    '\n',
    '\0',
    '\u{FFFD}',
];

/// Prints the case seed of a failing case (only when the case panics).
struct Replay {
    name: &'static str,
    case_seed: u64,
}

impl Drop for Replay {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "soak FAILED target={} case_seed={:#x}",
                self.name, self.case_seed
            );
        }
    }
}

/// Run `case` on fresh derived streams until `ms` elapses (at least once); asserts each case is under 1 s.
fn soak(name: &'static str, seed: u64, ms: u64, mut case: impl FnMut(&mut Rng)) {
    let salt = name
        .bytes()
        .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    let mut outer = Rng::new(seed ^ salt);
    let start = Instant::now();
    let mut cases = 0u64;
    loop {
        let case_seed = outer.u64();
        let _replay = Replay { name, case_seed };
        let t = Instant::now();
        case(&mut Rng::new(case_seed));
        let took = t.elapsed();
        assert!(took < Duration::from_secs(1), "{name}: case took {took:?}");
        cases += 1;
        if start.elapsed() >= Duration::from_millis(ms) {
            break;
        }
    }
    println!("soak target={name} seed={seed:#x} cases={cases}");
}

fn env_seed() -> u64 {
    let Ok(raw) = std::env::var("STRATA_SOAK_SEED") else {
        return DEFAULT_SEED;
    };
    let t: String = raw.trim().chars().filter(|&c| c != '_').collect();
    let parsed = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => t.parse().ok(),
    };
    parsed.unwrap_or_else(|| panic!("STRATA_SOAK_SEED is not a u64: {raw}"))
}

fn env_ms() -> u64 {
    match std::env::var("STRATA_SOAK_MS") {
        Ok(raw) => raw
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("STRATA_SOAK_MS is not a number: {raw}")),
        Err(_) => DEFAULT_MS,
    }
}

/// Size: an edge value a third of the time, otherwise `0..1200`.
fn size(rng: &mut Rng) -> u32 {
    if rng.chance(1, 3) {
        *rng.pick(&EDGE)
    } else {
        rng.range(0, 1200) as u32
    }
}

/// Any `u64`, with edge values a quarter of the time.
fn wide(rng: &mut Rng) -> u64 {
    match rng.below(4) {
        0 => 0,
        1 => u64::MAX,
        _ => rng.u64(),
    }
}

fn rand_char(rng: &mut Rng) -> char {
    if rng.chance(1, 2) {
        *rng.pick(&CHARS)
    } else {
        char::from_u32(rng.range(0, 0x11_0000) as u32).unwrap_or('?')
    }
}

fn rand_text(rng: &mut Rng, max: usize) -> String {
    let n = rng.below(max + 1);
    (0..n).map(|_| rand_char(rng)).collect()
}

/// Random bytes (lossy UTF-8) half the time, else mutations of a text the crate edited itself.
fn text_input(rng: &mut Rng) -> String {
    if rng.chance(1, 2) {
        return String::from_utf8_lossy(&rng.bytes(512)).into_owned();
    }
    let mut f = FieldState::new(&rand_text(rng, 64));
    for _ in 0..rng.below(20) {
        f.insert(rand_char(rng));
    }
    let mutated = mutate(rng, f.text.as_bytes());
    String::from_utf8_lossy(&mutated).into_owned()
}

/// Apply 1..5 random bit flips, truncations, splices or repeated chunks to `valid`.
fn mutate(rng: &mut Rng, valid: &[u8]) -> Vec<u8> {
    let mut v = valid.to_vec();
    for _ in 0..rng.range(1, 6) {
        match rng.below(4) {
            0 if !v.is_empty() => {
                let i = rng.below(v.len());
                v[i] ^= 1u8 << rng.below(8);
            }
            1 => {
                let n = rng.below(v.len() + 1);
                v.truncate(n);
            }
            2 => {
                let at = rng.below(v.len() + 1);
                let extra = rng.bytes(32);
                v.splice(at..at, extra);
            }
            3 if !v.is_empty() => {
                let s = rng.below(v.len());
                let l = rng.below(v.len() - s + 1).min(64);
                let chunk = v[s..s + l].to_vec();
                let at = rng.below(v.len() + 1);
                v.splice(at..at, chunk);
            }
            _ => {}
        }
    }
    v
}

fn random_style(rng: &mut Rng) -> Style {
    Style {
        dir: if rng.chance(1, 2) {
            Dir::Row
        } else {
            Dir::Column
        },
        justify: *rng.pick(&[
            Justify::Start,
            Justify::End,
            Justify::Center,
            Justify::SpaceBetween,
        ]),
        align: *rng.pick(&[Align::Stretch, Align::Start, Align::Center, Align::End]),
        gap: size(rng),
        padding: Insets {
            top: size(rng),
            right: size(rng),
            bottom: size(rng),
            left: size(rng),
        },
        grow: size(rng),
        width: if rng.chance(1, 3) {
            Some(size(rng))
        } else {
            None
        },
        height: if rng.chance(1, 3) {
            Some(size(rng))
        } else {
            None
        },
        min_w: size(rng),
        min_h: size(rng),
    }
}

/// Add up to `n` nodes in one of three shapes (random, deep chain, wide). Returns every id added.
fn grow_tree(rng: &mut Rng, ui: &mut Ui, n: usize) -> Vec<NodeId> {
    let shape = rng.below(3);
    let mut boxes = vec![ui.root()];
    let mut all = vec![ui.root()];
    for _ in 0..n {
        let parent = match shape {
            0 => *rng.pick(&boxes),
            1 => boxes.last().copied().unwrap_or(0),
            _ => boxes.first().copied().unwrap_or(0),
        };
        let kind = match rng.below(4) {
            0 => Kind::Box,
            1 => Kind::Label(rand_text(rng, 48)),
            2 => Kind::Button(rand_text(rng, 24)),
            _ => Kind::Field(rand_text(rng, 24)),
        };
        let is_box = matches!(kind, Kind::Box);
        if let Some(id) = ui.add(parent, kind, random_style(rng)) {
            all.push(id);
            if is_box {
                boxes.push(id);
            }
        }
    }
    all
}

fn layout_case(rng: &mut Rng) {
    let scale = *rng.pick(&[0u32, 1, 2, 3, 16, u32::MAX]);
    let mut ui = Ui::new(Theme {
        text_scale: scale,
        ..Theme::light()
    });
    let n = 1 + rng.below(600);
    let ids = grow_tree(rng, &mut ui, n);
    let (w, h) = (size(rng), size(rng));
    ui.layout(w, h);
    assert_eq!(ui.rect(ui.root()), Some(Rect { x: 0, y: 0, w, h }));

    let (cw, ch) = (rng.below(97) as u32, rng.below(97) as u32);
    let mut canvas = Canvas::new(cw, ch, Rgba(0, 0, 0, 255));
    ui.render(&mut canvas);
    for _ in 0..8 {
        let _ = ui.hit(coord(rng), coord(rng));
    }

    let victim = *rng.pick(&ids);
    let _ = ui.remove(victim);
    ui.layout(w, h);
    ui.render(&mut canvas);
}

fn coord(rng: &mut Rng) -> i32 {
    match rng.below(4) {
        0 => i32::MIN,
        1 => i32::MAX,
        _ => (rng.range(0, 2000) as i32) - 100,
    }
}

fn events_case(rng: &mut Rng) {
    let mut ui = Ui::new(Theme::dark());
    let n = 1 + rng.below(200);
    let ids = grow_tree(rng, &mut ui, n);
    let (w, h) = (rng.range(0, 1200) as u32, rng.range(0, 900) as u32);
    ui.layout(w, h);
    for _ in 0..rng.below(300) {
        let ev = match rng.below(5) {
            0 => Event::MouseMove {
                x: coord(rng),
                y: coord(rng),
            },
            1 => Event::MouseDown {
                x: coord(rng),
                y: coord(rng),
            },
            2 => Event::MouseUp {
                x: coord(rng),
                y: coord(rng),
            },
            3 => Event::Key(*rng.pick(&KEYS)),
            _ => Event::Text(rand_char(rng)),
        };
        let _actions: Vec<Action> = ui.event(ev);
        if let Some(f) = ui.focus() {
            assert!(ui.text(f).is_some(), "focus on non-widget node {f}");
        }
        if rng.chance(1, 25) {
            let _ = ui.remove(*rng.pick(&ids));
            ui.layout(w, h);
        }
    }
}

fn text_case(rng: &mut Rng) {
    let mut ui = Ui::new(Theme::light());
    let root = ui.root();
    let kind = match rng.below(3) {
        0 => Kind::Label(String::new()),
        1 => Kind::Button(String::new()),
        _ => Kind::Field(String::new()),
    };
    let Some(id) = ui.add(root, kind, Style::default()) else {
        panic!("adding a widget under the root Box failed");
    };
    let s = text_input(rng);
    assert!(ui.set_text(id, &s));
    assert_eq!(ui.text(id), Some(s.as_str()));

    let Some(bx) = ui.add(root, Kind::Box, Style::default()) else {
        panic!("adding a Box under the root failed");
    };
    assert!(!ui.set_text(bx, "x"));
    assert_eq!(ui.text(bx), None);

    let mut f = FieldState::new(&s);
    assert_eq!(f.text, s);
    assert_eq!(f.cursor, s.chars().count());
    for _ in 0..rng.below(40) {
        field_op(rng, &mut f);
        check_field(&f);
    }
    assert!(ui.set_text(id, &f.text));
    assert_eq!(ui.text(id), Some(f.text.as_str()));
}

fn field_op(rng: &mut Rng, f: &mut FieldState) {
    match rng.below(12) {
        0..=2 => f.insert(rand_char(rng)),
        3 => {
            f.backspace();
        }
        4 => {
            f.delete();
        }
        5 => {
            f.left(rng.chance(1, 2));
        }
        6 => {
            f.right(rng.chance(1, 2));
        }
        7 => {
            f.home(rng.chance(1, 2));
        }
        8 => {
            f.end(rng.chance(1, 2));
        }
        9 => f.select_all(),
        10 => {
            let _ = f.selected_text();
        }
        _ => {
            let _ = f.selection();
        }
    }
}

fn check_field(f: &FieldState) {
    let len = f.text.chars().count();
    assert!(f.cursor <= len, "cursor {} past {len} chars", f.cursor);
    if let Some(a) = f.anchor {
        assert!(a <= len, "anchor {a} past {len} chars");
    }
    if let Some((s, e)) = f.selection() {
        assert!(s <= e && e <= len);
        assert_eq!(f.selected_text().chars().count(), e - s);
    }
}

fn list_case(rng: &mut Rng) {
    let len = match rng.below(4) {
        0 => usize::MAX,
        1 => 0,
        _ => rng.below(5000),
    };
    let visible = *rng.pick(&[0usize, 1, 3, 40, usize::MAX]);
    let mut s = ListState::new(len, visible);
    for _ in 0..rng.below(200) {
        match rng.below(4) {
            0 => {
                s.nav(NAVS[rng.below(NAVS.len())]);
            }
            1 => {
                s.click_row(*rng.pick(&[0usize, 3, 4999, usize::MAX]));
            }
            2 => {
                s.set_len(rng.below(3000));
            }
            _ => {
                s.set_len(*rng.pick(&[0usize, 1, usize::MAX]));
            }
        }
        if let Some(sel) = s.selected {
            assert!(sel < s.len, "selected {sel} outside {} rows", s.len);
            assert!(s.offset <= sel, "offset {} past selection {sel}", s.offset);
        }
    }
}

fn scroll_case(rng: &mut Rng) {
    let mut s = ScrollState {
        content: size(rng),
        view: size(rng),
        pos: size(rng),
    };
    for _ in 0..rng.below(100) {
        if rng.chance(2, 3) {
            let d = match rng.below(3) {
                0 => i32::MIN,
                1 => i32::MAX,
                _ => rng.u64() as i32,
            };
            s.scroll_by(d);
            assert!(s.pos <= s.max());
        } else {
            let track = size(rng);
            let (start, len) = s.thumb(track);
            assert!(u64::from(start) + u64::from(len) <= u64::from(track));
        }
    }

    let count = *rng.pick(&[0usize, 1, 5, usize::MAX]);
    let mut t = TabsState {
        count,
        active: rng.u64() as usize,
    };
    for _ in 0..rng.below(100) {
        match rng.below(3) {
            0 => {
                t.next();
                assert!(t.count == 0 || t.active < t.count);
            }
            1 => {
                t.prev();
                assert!(t.count == 0 || t.active < t.count);
            }
            _ => {
                let i = rng.u64() as usize;
                if t.select(i) {
                    assert_eq!(t.active, i);
                }
            }
        }
    }
}

fn menu_case(rng: &mut Rng) {
    let n = rng.below(12);
    let items: Vec<MenuItem> = (0..n)
        .map(|_| MenuItem {
            label: rand_text(rng, 8),
            enabled: rng.chance(2, 3),
        })
        .collect();
    let mut m = MenuState {
        items,
        open: false,
        hover: None,
    };
    for _ in 0..rng.below(100) {
        match rng.below(4) {
            0 => m.open(),
            1 => m.close(),
            2 => {
                let _ = m.key(NAVS[rng.below(NAVS.len())]);
            }
            _ => {
                if let Some(i) = m.enter() {
                    assert!(m.items.get(i).is_some_and(|it| it.enabled));
                    assert!(!m.open);
                }
            }
        }
        if let Some(h) = m.hover {
            assert!(
                m.items.get(h).is_some_and(|it| it.enabled),
                "hover {h} not enabled"
            );
        }
    }
}

fn anim_case(rng: &mut Rng) {
    let e = *rng.pick(&EASES);
    let t = if rng.chance(1, 2) {
        *rng.pick(&EDGE)
    } else {
        rng.u64() as u32
    };
    assert!(ease(e, t) <= 1000);

    let (from, to) = (rng.u64() as i32, rng.u64() as i32);
    let (start, dur, now) = (wide(rng), wide(rng), wide(rng));
    let v = tween(from, to, start, dur, now, e);
    assert!(
        from.min(to) <= v && v <= from.max(to),
        "tween {v} outside {from}..{to}"
    );
    if now <= start {
        assert_eq!(v, from);
    } else if dur == 0 || now >= start.saturating_add(dur) {
        assert_eq!(v, to);
    }
}

fn spring_case(rng: &mut Rng) {
    // Wild state and time steps: must not panic.
    let mut wild = Spring {
        pos: rng.u64() as i64,
        vel: rng.u64() as i64,
        target: rng.u64() as i64,
        stiffness: rng.u64() as i64,
        damping: rng.u64() as i64,
    };
    for _ in 0..50 {
        wild.step(rng.u64() as u32);
        let _ = wild.settled();
    }

    // Sane parameters: must reach the target to within 1 unit.
    let span = |rng: &mut Rng| rng.range(0, 2_000_001) as i64 - 1_000_000;
    let mut s = Spring {
        pos: span(rng),
        vel: 0,
        target: span(rng),
        stiffness: rng.range(1, 1001) as i64,
        damping: rng.range(100, 901) as i64,
    };
    let dt = rng.range(1, 17) as u32;
    // Integer state cannot creep below one unit, so the residual is bounded, not zero.
    // Overdamped extremes (stiffness 1, damping 900, dt 1) creep at the 1-unit nudge: about
    // 1.9M steps across the 2M span, so 1M was too tight (T79 soak, 2026-11-16).
    for _ in 0..4_000_000 {
        if s.settled() {
            break;
        }
        s.step(dt);
    }
    assert!(
        s.settled() && s.pos == s.target,
        "spring did not settle on target: target {} pos {} vel {} stiffness {} damping {} dt {dt}",
        s.target,
        s.pos,
        s.vel,
        s.stiffness,
        s.damping
    );
}

/// Regression for the stall found by the soak: the old integer truncation stopped 318 units short.
#[test]
fn spring_low_stiffness_reaches_target() {
    for (stiffness, damping, dt, target) in [(603, 823, 5, 318), (1, 500, 16, 999)] {
        let mut s = Spring {
            pos: 0,
            vel: 0,
            target,
            stiffness,
            damping,
        };
        for _ in 0..200_000 {
            s.step(dt);
        }
        assert!(
            s.settled() && s.pos == s.target,
            "stiffness {stiffness} stalled at {} of {target}",
            s.pos
        );
    }
}

#[test]
fn soak_layout() {
    soak("layout", env_seed(), env_ms(), layout_case);
}

#[test]
fn soak_events() {
    soak("events", env_seed(), env_ms(), events_case);
}

#[test]
fn soak_text_and_field() {
    soak("text", env_seed(), env_ms(), text_case);
}

#[test]
fn soak_list() {
    soak("list", env_seed(), env_ms(), list_case);
}

#[test]
fn soak_scroll_and_tabs() {
    soak("scroll", env_seed(), env_ms(), scroll_case);
}

#[test]
fn soak_menu() {
    soak("menu", env_seed(), env_ms(), menu_case);
}

#[test]
fn soak_ease_and_tween() {
    soak("anim", env_seed(), env_ms(), anim_case);
}

#[test]
fn soak_spring() {
    soak("spring", env_seed(), env_ms(), spring_case);
}
