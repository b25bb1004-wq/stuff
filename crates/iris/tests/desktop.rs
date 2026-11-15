//! Integration tests for Desktop, app routing, golden frame hashes, and random sessions.
#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::rc::Rc;

use assay::{forall, Rng};
use indium::{Event, Key};
use iris::{App, Desktop, Editor, Effect, Files, Layout, Terminal};
use pigment::{glyph, Canvas, Rect, Rgba, GLYPH_H, GLYPH_W};

const GOLDEN_HASH_UNSAVED_EDITOR: u64 = 0x2019_7a7e_ca68_442d;
const GOLDEN_HASH_TERMINAL_ECHO: u64 = 0xa3fd_4fe5_d730_988f;
const GOLDEN_HASH_TILED_THREE: u64 = 0x7cb5_28c4_ebff_2e47;

fn fnv1a_pixels(pixels: &[Rgba]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for p in pixels {
        for b in [p.0, p.1, p.2, p.3] {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn rect_contains(r: Rect, x: i32, y: i32) -> bool {
    let x0 = i64::from(r.x);
    let y0 = i64::from(r.y);
    let x1 = x0 + i64::from(r.w);
    let y1 = y0 + i64::from(r.h);
    let px = i64::from(x);
    let py = i64::from(y);
    px >= x0 && px < x1 && py >= y0 && py < y1
}

fn step_and_verify_damage(desktop: &mut Desktop, now_ms: u64) -> Vec<Rect> {
    let prev = desktop.screen().clone();
    let damage = desktop.frame(now_ms);
    let cur = desktop.screen();
    let w = cur.width as usize;

    for &r in &damage {
        assert!(r.w > 0 && r.h > 0, "empty damage rect: {r:?}");
        assert!(r.x >= 0 && r.y >= 0, "negative damage origin: {r:?}");
        assert!(
            i64::from(r.x) + i64::from(r.w) <= i64::from(cur.width),
            "damage rect exceeds width: {r:?}"
        );
        assert!(
            i64::from(r.y) + i64::from(r.h) <= i64::from(cur.height),
            "damage rect exceeds height: {r:?}"
        );
    }

    let mut changed = 0u64;
    for (idx, (&pa, &pb)) in prev.pixels.iter().zip(cur.pixels.iter()).enumerate() {
        if pa != pb {
            changed += 1;
            let px = (idx % w) as i32;
            let py = (idx / w) as i32;
            assert!(
                damage.iter().any(|&r| rect_contains(r, px, py)),
                "changed pixel ({px}, {py}) not covered by damage {damage:?}"
            );
        }
    }
    if changed == 0 {
        assert!(damage.is_empty(), "expected empty damage, got {damage:?}");
    }
    damage
}

fn cell_matches_char(screen: &Canvas, x0: i32, y0: i32, ch: char, fg: Rgba, bg: Rgba) -> bool {
    let g = glyph(ch);
    for (row, &bits) in g.iter().enumerate() {
        for col in 0..8usize {
            let expected = if (bits >> (7 - col)) & 1 != 0 { fg } else { bg };
            let px = x0 + col as i32;
            let py = y0 + row as i32;
            if screen.get(px, py) != Some(expected) {
                return false;
            }
        }
    }
    true
}

fn find_text_in_rect(screen: &Canvas, frame: Rect, needle: &str, fg: Rgba, bg: Rgba) -> bool {
    let cx0 = frame.x + 1;
    let cy0 = frame.y + 19;
    let cw = frame.w.saturating_sub(2);
    let ch = frame.h.saturating_sub(20);
    let cols = (cw / GLYPH_W) as usize;
    let rows = (ch / GLYPH_H) as usize;
    let chars: Vec<char> = needle.chars().collect();
    if chars.is_empty() || cols < chars.len() {
        return false;
    }
    for r in 0..rows {
        let py = cy0 + (r as i32) * (GLYPH_H as i32);
        for c in 0..=cols - chars.len() {
            let ok = chars.iter().enumerate().all(|(k, &ch_expected)| {
                let px = cx0 + ((c + k) as i32) * (GLYPH_W as i32);
                cell_matches_char(screen, px, py, ch_expected, fg, bg)
            });
            if ok {
                return true;
            }
        }
    }
    false
}

struct SharedApp<T>(Rc<RefCell<T>>);

impl<T: App> App for SharedApp<T> {
    fn title(&self) -> &str {
        // Return a static label when borrowed through SharedApp; inner title checked directly.
        "shared"
    }

    fn event(&mut self, ev: Event, now_ms: u64) -> Effect {
        self.0.borrow_mut().event(ev, now_ms)
    }

    fn paint(&mut self, c: &mut Canvas) -> Option<Rect> {
        self.0.borrow_mut().paint(c)
    }
}

#[test]
fn scripted_session_and_golden_frames() {
    let mut desktop = Desktop::new(640, 400);
    desktop.fs().borrow_mut().create("/notes.txt").unwrap();

    // 1. Launch Files, open /notes.txt, type "hello", Ctrl+S, close.
    let files_id = desktop.launch(Box::new(Files::new(desktop.fs(), "/")), 280, 240, 0);
    step_and_verify_damage(&mut desktop, 0);

    desktop.input(Event::Key(Key::Enter), false, 10);
    let edit_id = desktop.focused().unwrap();
    assert_ne!(edit_id, files_id);
    assert_eq!(desktop.app(edit_id).unwrap().title(), "/notes.txt");

    for (i, ch) in "hello".chars().enumerate() {
        desktop.input(Event::Text(ch), false, 20 + i as u64);
    }
    assert_eq!(desktop.app(edit_id).unwrap().title(), "/notes.txt*");
    step_and_verify_damage(&mut desktop, 30);
    let hash_unsaved_editor = fnv1a_pixels(&desktop.screen().pixels);

    // Save with Ctrl+S and verify title + hades Fs contents.
    desktop.input(Event::Text('\x13'), false, 40);
    assert_eq!(desktop.app(edit_id).unwrap().title(), "/notes.txt");
    step_and_verify_damage(&mut desktop, 40);

    let mut read_buf = [0u8; 32];
    let n = desktop
        .fs()
        .borrow_mut()
        .read("/notes.txt", 0, &mut read_buf)
        .unwrap();
    assert_eq!(&read_buf[..n], b"hello");

    assert!(desktop.close(edit_id, 50));
    step_and_verify_damage(&mut desktop, 50);

    // 2. Launch Terminal via Super+Enter, run `echo hi`, see `hi` in its grid.
    desktop.input(Event::Key(Key::Enter), true, 60);
    let term_id = desktop.focused().unwrap();
    assert_eq!(desktop.app(term_id).unwrap().title(), "terminal");
    for (i, ch) in "echo hi".chars().enumerate() {
        desktop.input(Event::Text(ch), false, 70 + i as u64);
    }
    desktop.input(Event::Key(Key::Enter), false, 80);
    step_and_verify_damage(&mut desktop, 90);
    let hash_terminal_echo = fnv1a_pixels(&desktop.screen().pixels);

    let term_rect = desktop.rect(term_id).unwrap();
    assert!(find_text_in_rect(
        desktop.screen(),
        term_rect,
        "hi",
        Rgba(220, 236, 224, 255),
        Rgba(24, 26, 32, 255)
    ));

    // 3. Tiling layout with 3 windows (Files, Terminal, Editor on /notes.txt).
    let edit2_id = desktop.launch(
        Box::new(Editor::new(desktop.fs(), "/notes.txt")),
        280,
        240,
        100,
    );
    assert_eq!(desktop.order().len(), 3);
    desktop.set_layout(Layout::Tiling, 100);
    step_and_verify_damage(&mut desktop, 150);
    step_and_verify_damage(&mut desktop, 300);
    let hash_tiled_three = fnv1a_pixels(&desktop.screen().pixels);

    assert_eq!(
        desktop.rect(files_id),
        Some(Rect {
            x: 0,
            y: 0,
            w: 320,
            h: 400
        })
    );
    assert_eq!(
        desktop.rect(term_id),
        Some(Rect {
            x: 320,
            y: 0,
            w: 320,
            h: 200
        })
    );
    assert_eq!(
        desktop.rect(edit2_id),
        Some(Rect {
            x: 320,
            y: 200,
            w: 320,
            h: 200
        })
    );

    // Idle frame produces empty damage.
    assert!(step_and_verify_damage(&mut desktop, 310).is_empty());

    assert_eq!(hash_unsaved_editor, GOLDEN_HASH_UNSAVED_EDITOR);
    assert_eq!(hash_terminal_echo, GOLDEN_HASH_TERMINAL_ECHO);
    assert_eq!(hash_tiled_three, GOLDEN_HASH_TILED_THREE);
}

#[test]
fn app_state_assertions_inside_desktop() {
    let mut desktop = Desktop::new(640, 400);
    let fs = desktop.fs();
    fs.borrow_mut().create("/notes.txt").unwrap();

    let ed = Rc::new(RefCell::new(Editor::new(Rc::clone(&fs), "/notes.txt")));
    let term = Rc::new(RefCell::new(Terminal::new()));

    let ed_id = desktop.launch(Box::new(SharedApp(Rc::clone(&ed))), 320, 200, 0);
    let _term_id = desktop.launch(Box::new(SharedApp(Rc::clone(&term))), 320, 200, 1);
    step_and_verify_damage(&mut desktop, 1);

    // Focus editor via Super+Tab, type "hello", save, verify buffer + Fs.
    desktop.input(Event::Key(Key::Tab), true, 5);
    assert_eq!(desktop.focused(), Some(ed_id));
    for ch in "hello".chars() {
        desktop.input(Event::Text(ch), false, 10);
    }
    assert_eq!(ed.borrow().text(), "hello");
    assert!(ed.borrow().is_unsaved());
    desktop.input(Event::Text('\x13'), false, 11);
    assert!(!ed.borrow().is_unsaved());
    step_and_verify_damage(&mut desktop, 12);

    let mut buf = [0u8; 16];
    let n = fs.borrow_mut().read("/notes.txt", 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");

    // Focus terminal via Super+Tab, run `echo hi`, assert terminal lines and grid.
    desktop.input(Event::Key(Key::Tab), true, 15);
    for ch in "echo hi".chars() {
        desktop.input(Event::Text(ch), false, 20);
    }
    desktop.input(Event::Key(Key::Enter), false, 21);
    step_and_verify_damage(&mut desktop, 22);

    assert_eq!(term.borrow().lines(), &["$ echo hi", "hi"]);
    assert!(term.borrow().grid_lines(40, 12).iter().any(|l| l == "hi"));
}

#[test]
fn forall_random_events_over_all_three_apps() {
    forall(40, 0x7640_DE57_0064, |rng: &mut Rng| {
        let sw = rng.range(120, 320) as u32;
        let sh = rng.range(100, 240) as u32;
        let mut desktop = Desktop::new(sw, sh);
        {
            let fs_handle = desktop.fs();
            let mut fs = fs_handle.borrow_mut();
            let _ = fs.mkdir("/dir");
            let _ = fs.create("/a.txt");
            let _ = fs.write("/a.txt", 0, b"abc\ndef");
            let _ = fs.create("/dir/b.txt");
        }

        desktop.launch(Box::new(Files::new(desktop.fs(), "/")), 180, 140, 0);
        desktop.launch(Box::new(Editor::new(desktop.fs(), "/a.txt")), 180, 140, 0);
        desktop.launch(Box::new(Terminal::new()), 180, 140, 0);

        let mut now_ms = 0u64;
        step_and_verify_damage(&mut desktop, now_ms);

        let steps = rng.range(25, 45);
        for _ in 0..steps {
            let ops = rng.range(1, 5);
            for _ in 0..ops {
                let super_held = rng.chance(1, 6);
                let ev = match rng.range(0, 16) {
                    0 => Event::MouseDown {
                        x: rng.range(0, sw as u64 + 20) as i32 - 10,
                        y: rng.range(0, sh as u64 + 20) as i32 - 10,
                    },
                    1 => Event::MouseMove {
                        x: rng.range(0, sw as u64 + 20) as i32 - 10,
                        y: rng.range(0, sh as u64 + 20) as i32 - 10,
                    },
                    2 => Event::MouseUp {
                        x: rng.range(0, sw as u64 + 20) as i32 - 10,
                        y: rng.range(0, sh as u64 + 20) as i32 - 10,
                    },
                    3 => Event::Key(Key::Enter),
                    4 => Event::Key(Key::Backspace),
                    5 => Event::Key(Key::Space),
                    6 => Event::Key(Key::Tab),
                    7 => Event::Key(Key::ShiftTab),
                    8 => Event::Text('\u{2191}'),
                    9 => Event::Text('\u{2193}'),
                    10 => Event::Text('\u{2190}'),
                    11 => Event::Text('\u{2192}'),
                    12 => Event::Text('\x13'),
                    13 => Event::Text(*rng.pick(&['[', ']', 'a', 'e', 'c', 'h', 'o', ' ', 'x'])),
                    14 => {
                        if rng.chance(1, 3) {
                            let layout = if rng.chance(1, 2) {
                                Layout::Tiling
                            } else {
                                Layout::Floating
                            };
                            desktop.set_layout(layout, now_ms);
                        }
                        continue;
                    }
                    _ => {
                        let ch = char::from_u32(rng.range(0x20, 0x7f) as u32).unwrap_or('a');
                        Event::Text(ch)
                    }
                };
                if super_held && matches!(ev, Event::Key(Key::Enter)) && desktop.order().len() >= 6
                {
                    continue;
                }
                let _ = desktop.input(ev, super_held, now_ms);
            }
            now_ms += rng.range(0, 90);
            step_and_verify_damage(&mut desktop, now_ms);
        }
    });
}
