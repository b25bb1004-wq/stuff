//! Unit and property tests for `indium::widgets`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use indium::widgets::{FieldState, ListState, MenuItem, MenuState, Nav, ScrollState, TabsState};

const NAVS: [Nav; 6] = [
    Nav::Up,
    Nav::Down,
    Nav::PageUp,
    Nav::PageDown,
    Nav::Home,
    Nav::End,
];

fn item(label: &str, enabled: bool) -> MenuItem {
    MenuItem {
        label: label.to_string(),
        enabled,
    }
}

fn menu() -> MenuState {
    MenuState {
        items: vec![
            item("A", true),
            item("B", false),
            item("C", true),
            item("D", true),
        ],
        open: false,
        hover: None,
    }
}

// ---- List ----

#[test]
fn list_new_selects_nothing() {
    let l = ListState::new(10, 3);
    assert_eq!((l.selected, l.offset, l.len, l.visible), (None, 0, 10, 3));
}

#[test]
fn list_first_key_from_none() {
    let mut l = ListState::new(10, 3);
    assert!(l.nav(Nav::Down));
    assert_eq!(l.selected, Some(0));
    let mut l = ListState::new(10, 3);
    assert!(l.nav(Nav::End));
    assert_eq!(l.selected, Some(9));
    assert_eq!(l.offset, 7);
    let mut l = ListState::new(10, 3);
    assert!(l.nav(Nav::Up));
    assert_eq!(l.selected, Some(0));
}

#[test]
fn list_empty_never_changes() {
    let mut l = ListState::new(0, 3);
    for n in NAVS {
        assert!(!l.nav(n));
    }
    assert_eq!(l.selected, None);
}

#[test]
fn list_nav_hand_worked() {
    let mut l = ListState::new(10, 3);
    // First Down from no selection lands on row 0, so four presses reach row 3.
    l.nav(Nav::Down);
    l.nav(Nav::Down);
    l.nav(Nav::Down);
    l.nav(Nav::Down);
    assert_eq!((l.selected, l.offset), (Some(3), 1));
    assert!(l.nav(Nav::End));
    assert_eq!((l.selected, l.offset), (Some(9), 7));
    assert!(l.nav(Nav::Home));
    assert_eq!((l.selected, l.offset), (Some(0), 0));
    assert!(l.nav(Nav::PageDown));
    assert_eq!((l.selected, l.offset), (Some(3), 1));
    assert!(l.nav(Nav::PageUp));
    assert_eq!((l.selected, l.offset), (Some(0), 0));
    // Up at the top and End at the bottom report no change.
    assert!(!l.nav(Nav::Up));
    assert!(l.nav(Nav::End));
    assert!(!l.nav(Nav::End));
    assert!(!l.nav(Nav::Down));
}

#[test]
fn list_zero_visible_acts_as_one() {
    let mut l = ListState::new(5, 0);
    assert!(l.nav(Nav::Down));
    assert!(l.nav(Nav::Down));
    assert_eq!((l.selected, l.offset), (Some(1), 1));
    assert!(l.nav(Nav::PageDown));
    assert_eq!(l.selected, Some(2));
}

#[test]
fn list_click_row_rules() {
    let mut l = ListState::new(10, 3);
    assert!(l.click_row(2));
    assert_eq!((l.selected, l.offset), (Some(2), 0));
    assert!(!l.click_row(2));
    // Row beyond the visible area selects without moving the offset.
    assert!(l.click_row(5));
    assert_eq!((l.selected, l.offset), (Some(5), 0));
    // Row past the end is ignored.
    assert!(!l.click_row(20));
    assert_eq!(l.selected, Some(5));
}

#[test]
fn list_set_len_hand_worked() {
    let mut l = ListState::new(10, 3);
    l.selected = Some(8);
    l.offset = 6;
    assert!(l.set_len(5));
    assert_eq!((l.selected, l.offset), (Some(4), 2));
    assert!(!l.set_len(5));
    assert!(l.set_len(0));
    assert_eq!((l.selected, l.offset), (None, 0));
    assert!(l.set_len(3));
    assert_eq!(l.selected, None);
}

#[test]
fn list_random_sequences_keep_invariants() {
    forall(400, 0x6001, |rng| {
        let mut l = ListState::new(rng.below(30), rng.below(6));
        for _ in 0..60 {
            match rng.below(4) {
                0 => {
                    let n = NAVS[rng.below(NAVS.len())];
                    l.nav(n);
                }
                1 => {
                    // Keep the click inside the visible rows, where the rule holds.
                    let r = rng.below(l.visible.max(1));
                    l.click_row(r);
                }
                2 => {
                    l.set_len(rng.below(30));
                }
                _ => {
                    l.nav(Nav::Down);
                }
            }
            check_list(&l);
        }
    });
}

fn check_list(l: &ListState) {
    let v = l.visible.max(1);
    match l.selected {
        None => {}
        Some(s) => {
            assert!(s < l.len, "selected {s} out of {}", l.len);
            assert!(l.offset <= s && s < l.offset + v, "offset rule broken");
        }
    }
    if l.len == 0 {
        assert_eq!(l.selected, None);
    }
}

// ---- Scroll ----

#[test]
fn scroll_hand_worked() {
    let mut s = ScrollState {
        content: 100,
        view: 20,
        pos: 0,
    };
    assert_eq!(s.max(), 80);
    assert!(!s.scroll_by(-5));
    assert!(s.scroll_by(30));
    assert_eq!(s.pos, 30);
    assert!(s.scroll_by(100));
    assert_eq!(s.pos, 80);
    assert!(!s.scroll_by(1));
}

#[test]
fn scroll_max_saturates() {
    let s = ScrollState {
        content: 10,
        view: 20,
        pos: 0,
    };
    assert_eq!(s.max(), 0);
}

#[test]
fn scroll_thumb_hand_worked() {
    let mut s = ScrollState {
        content: 100,
        view: 20,
        pos: 0,
    };
    assert_eq!(s.thumb(100), (0, 20));
    s.pos = 80;
    assert_eq!(s.thumb(100), (80, 20));
    s.pos = 40;
    assert_eq!(s.thumb(100), (40, 20));
    // Minimum thumb length of 8 pixels.
    let big = ScrollState {
        content: 1000,
        view: 10,
        pos: 0,
    };
    assert_eq!(big.thumb(100), (0, 8));
    // Content that fits fills the track.
    let fits = ScrollState {
        content: 10,
        view: 20,
        pos: 0,
    };
    assert_eq!(fits.thumb(50), (0, 50));
}

#[test]
fn scroll_random_keeps_pos_in_range() {
    forall(400, 0x6002, |rng| {
        let view = rng.below(200) as u32;
        let content = rng.below(400) as u32;
        let mut s = ScrollState {
            content,
            view,
            pos: 0,
        };
        for _ in 0..40 {
            let d = rng.range(0, 400) as i32 - 200;
            s.scroll_by(d);
            assert!(s.pos <= s.max());
            let track = rng.below(300) as u32;
            let (start, len) = s.thumb(track);
            assert!(len <= track);
            assert!(start + len <= track);
        }
    });
}

// ---- Tabs ----

#[test]
fn tabs_hand_worked() {
    let mut t = TabsState {
        count: 3,
        active: 0,
    };
    assert!(t.next());
    assert!(t.next());
    assert!(t.next());
    assert_eq!(t.active, 0);
    assert!(t.prev());
    assert_eq!(t.active, 2);
    assert!(t.prev());
    assert_eq!(t.active, 1);
    assert!(!t.select(5));
    assert!(!t.select(1));
    assert!(t.select(0));
    assert_eq!(t.active, 0);
}

#[test]
fn tabs_empty_never_changes() {
    let mut t = TabsState {
        count: 0,
        active: 0,
    };
    assert!(!t.next());
    assert!(!t.prev());
    assert!(!t.select(0));
    assert_eq!(t.active, 0);
}

#[test]
fn tabs_random_active_in_range() {
    forall(300, 0x6003, |rng| {
        let count = rng.below(8);
        let mut t = TabsState { count, active: 0 };
        for _ in 0..40 {
            match rng.below(3) {
                0 => {
                    t.next();
                }
                1 => {
                    t.prev();
                }
                _ => {
                    t.select(rng.below(10));
                }
            }
            if count > 0 {
                assert!(t.active < count);
            }
        }
    });
}

// ---- Menu ----

#[test]
fn menu_open_close_hover() {
    let mut m = menu();
    m.open();
    assert!(m.open);
    assert_eq!(m.hover, Some(0));
    m.close();
    assert!(!m.open);
    assert_eq!(m.hover, None);
}

#[test]
fn menu_key_skips_disabled_and_wraps() {
    let mut m = menu();
    assert!(!m.key(Nav::Down), "closed menu ignores keys");
    m.open();
    assert!(m.key(Nav::Down));
    assert_eq!(m.hover, Some(2));
    assert!(m.key(Nav::Down));
    assert_eq!(m.hover, Some(3));
    assert!(m.key(Nav::Down));
    assert_eq!(m.hover, Some(0));
    assert!(m.key(Nav::Up));
    assert_eq!(m.hover, Some(3));
    assert!(m.key(Nav::Up));
    assert_eq!(m.hover, Some(2));
    assert!(m.key(Nav::Home));
    assert_eq!(m.hover, Some(0));
    assert!(!m.key(Nav::PageUp));
    assert!(m.key(Nav::PageDown));
    assert_eq!(m.hover, Some(3));
    assert!(!m.key(Nav::End));
}

#[test]
fn menu_from_none_goes_to_edge() {
    let mut m = menu();
    m.open();
    m.hover = None;
    assert!(m.key(Nav::Up));
    assert_eq!(m.hover, Some(3));
    m.hover = None;
    assert!(m.key(Nav::Down));
    assert_eq!(m.hover, Some(0));
}

#[test]
fn menu_enter_rules() {
    let mut m = menu();
    assert_eq!(m.enter(), None, "closed");
    m.open();
    m.hover = Some(1);
    assert_eq!(m.enter(), None, "disabled hover");
    assert!(m.open);
    m.hover = Some(2);
    assert_eq!(m.enter(), Some(2));
    assert!(!m.open);
    assert_eq!(m.hover, None);
}

#[test]
fn menu_all_disabled() {
    let mut m = MenuState {
        items: vec![item("X", false), item("Y", false)],
        open: false,
        hover: None,
    };
    m.open();
    assert_eq!(m.hover, None);
    assert!(!m.key(Nav::Down));
    assert_eq!(m.enter(), None);
}

#[test]
fn menu_random_hover_is_enabled() {
    forall(300, 0x6004, |rng| {
        let n = rng.below(6);
        let items = (0..n).map(|i| item("i", i % 3 != 1)).collect();
        let mut m = MenuState {
            items,
            open: false,
            hover: None,
        };
        for _ in 0..40 {
            match rng.below(4) {
                0 => m.open(),
                1 => m.close(),
                2 => {
                    m.enter();
                }
                _ => {
                    let k = NAVS[rng.below(NAVS.len())];
                    m.key(k);
                }
            }
            if let Some(h) = m.hover {
                assert!(h < n && m.items[h].enabled);
            }
        }
    });
}

// ---- Field ----

fn field(text: &str) -> FieldState {
    FieldState::new(text)
}

#[test]
fn field_new_cursor_at_end() {
    let f = field("abc");
    assert_eq!((f.cursor, f.anchor), (3, None));
    assert_eq!(f.selection(), None);
    assert_eq!(f.selected_text(), "");
}

#[test]
fn field_insert_and_backspace() {
    let mut f = field("abc");
    f.insert('d');
    assert_eq!((f.text.as_str(), f.cursor), ("abcd", 4));
    assert!(f.backspace());
    assert_eq!((f.text.as_str(), f.cursor), ("abc", 3));
    let mut e = field("");
    assert!(!e.backspace());
}

#[test]
fn field_delete_and_home_end() {
    let mut f = field("abc");
    assert!(f.home(false));
    assert_eq!(f.cursor, 0);
    assert!(f.delete());
    assert_eq!((f.text.as_str(), f.cursor), ("bc", 0));
    assert!(!f.left(false));
    assert!(f.end(false));
    assert_eq!(f.cursor, 2);
    assert!(!f.end(false));
    assert!(!f.right(false));
    assert!(!f.delete());
}

#[test]
fn field_shift_selection_and_replace() {
    let mut f = field("abc");
    assert!(f.left(true));
    assert_eq!((f.anchor, f.cursor), (Some(3), 2));
    assert_eq!(f.selection(), Some((2, 3)));
    assert_eq!(f.selected_text(), "c");
    assert!(f.left(true));
    assert_eq!(f.selection(), Some((1, 3)));
    assert_eq!(f.selected_text(), "bc");
    assert!(f.left(true));
    assert_eq!(f.selected_text(), "abc");
    f.insert('X');
    assert_eq!((f.text.as_str(), f.cursor, f.anchor), ("X", 1, None));
}

#[test]
fn field_plain_move_collapses_selection() {
    let mut f = field("abc");
    f.home(true);
    assert_eq!(f.selection(), Some((0, 3)));
    // Plain left goes to the selection start.
    assert!(f.left(false));
    assert_eq!((f.cursor, f.anchor), (0, None));
    f.select_all();
    assert_eq!((f.anchor, f.cursor), (Some(0), 3));
    // Plain right goes to the selection end.
    assert!(f.right(false));
    assert_eq!((f.cursor, f.anchor), (3, None));
}

#[test]
fn field_right_with_shift_from_start() {
    let mut f = field("abc");
    f.home(false);
    assert!(f.right(true));
    assert_eq!((f.anchor, f.cursor), (Some(0), 1));
    assert_eq!(f.selected_text(), "a");
}

#[test]
fn field_delete_selection_and_backspace_selection() {
    let mut f = field("abc");
    f.select_all();
    assert!(f.delete());
    assert_eq!((f.text.as_str(), f.cursor), ("", 0));

    let mut g = field("abc");
    g.select_all();
    assert!(g.backspace());
    assert_eq!(g.text, "");
}

#[test]
fn field_multibyte_chars() {
    let mut f = field("aéb");
    assert_eq!(f.cursor, 3);
    assert!(f.backspace());
    assert_eq!((f.text.as_str(), f.cursor), ("aé", 2));
    f.left(false);
    assert!(f.delete());
    assert_eq!((f.text.as_str(), f.cursor), ("a", 1));
    f.insert('ö');
    assert_eq!(f.text, "aö");
}

#[test]
fn field_random_sequences_keep_invariants() {
    const CHARS: [char; 4] = ['a', 'é', '日', ' '];
    forall(400, 0x6005, |rng| {
        let mut f = field("héllo");
        for _ in 0..80 {
            let shift = rng.chance(1, 2);
            match rng.below(9) {
                0 => f.insert(*rng.pick(&CHARS)),
                1 => {
                    f.backspace();
                }
                2 => {
                    f.delete();
                }
                3 => {
                    f.left(shift);
                }
                4 => {
                    f.right(shift);
                }
                5 => {
                    f.home(shift);
                }
                6 => {
                    f.end(shift);
                }
                7 => f.select_all(),
                _ => {
                    f.left(false);
                }
            }
            check_field(&f);
        }
    });
}

fn check_field(f: &FieldState) {
    let count = f.text.chars().count();
    assert!(f.cursor <= count, "cursor {} > {count}", f.cursor);
    if let Some(a) = f.anchor {
        assert!(a <= count);
    }
    match f.selection() {
        Some((s, e)) => {
            assert!(s < e && e <= count);
            assert_eq!(f.selected_text().chars().count(), e - s);
        }
        None => assert_eq!(f.selected_text(), ""),
    }
}
